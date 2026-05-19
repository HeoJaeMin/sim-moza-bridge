use super::constants::{MAX_CARS, PACKET_HEADER_SIZE};
use super::header::parse_packet_header;
use crate::telemetry::InputSample;

pub const CAR_TELEMETRY_DATA_SIZE: usize = 60;
pub const CAR_TELEMETRY_MIN_PACKET_SIZE: usize =
    PACKET_HEADER_SIZE + MAX_CARS * CAR_TELEMETRY_DATA_SIZE;

pub fn car_telemetry_offset(car_index: usize) -> Result<usize, String> {
    if car_index >= MAX_CARS {
        return Err(format!("car_index must be between 0 and {}", MAX_CARS - 1));
    }

    Ok(PACKET_HEADER_SIZE + car_index * CAR_TELEMETRY_DATA_SIZE)
}

fn read_u16_le(packet: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes(
        packet[offset..offset + 2]
            .try_into()
            .expect("valid u16 offset"),
    )
}

fn read_f32_le(packet: &[u8], offset: usize) -> f32 {
    f32::from_le_bytes(
        packet[offset..offset + 4]
            .try_into()
            .expect("valid f32 offset"),
    )
}

pub fn parse_player_input_sample(packet: &[u8]) -> Result<InputSample, String> {
    let header = parse_packet_header(packet)
        .ok_or_else(|| "packet is too short for F1 header".to_owned())?;
    let car_index = header.player_car_index as usize;
    let base = car_telemetry_offset(car_index)?;

    if packet.len() < base + 18 || packet.len() < CAR_TELEMETRY_MIN_PACKET_SIZE {
        return Err("packet is too short for F1 car telemetry data".to_owned());
    }

    Ok(InputSample {
        session_time: header.session_time,
        frame_identifier: header.frame_identifier,
        player_car_index: header.player_car_index,
        speed_kmh: read_u16_le(packet, base),
        throttle: read_f32_le(packet, base + 2),
        brake: read_f32_le(packet, base + 10),
        gear: packet[base + 15] as i8,
        rpm: read_u16_le(packet, base + 16),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::f1::constants::{F1_25_PACKET_FORMAT, packet_id};

    #[test]
    fn parses_player_input_sample() {
        let mut packet = vec![0_u8; CAR_TELEMETRY_MIN_PACKET_SIZE + 3];
        packet[0..2].copy_from_slice(&F1_25_PACKET_FORMAT.to_le_bytes());
        packet[2] = 25;
        packet[6] = packet_id::CAR_TELEMETRY;
        packet[15..19].copy_from_slice(&12.5_f32.to_le_bytes());
        packet[19..23].copy_from_slice(&77_u32.to_le_bytes());
        packet[27] = 2;

        let base = car_telemetry_offset(2).unwrap();
        packet[base..base + 2].copy_from_slice(&286_u16.to_le_bytes());
        packet[base + 2..base + 6].copy_from_slice(&0.75_f32.to_le_bytes());
        packet[base + 10..base + 14].copy_from_slice(&0.25_f32.to_le_bytes());
        packet[base + 15] = 7_u8;
        packet[base + 16..base + 18].copy_from_slice(&11750_u16.to_le_bytes());

        assert_eq!(
            parse_player_input_sample(&packet).unwrap(),
            InputSample {
                session_time: 12.5,
                frame_identifier: 77,
                player_car_index: 2,
                throttle: 0.75,
                brake: 0.25,
                speed_kmh: 286,
                gear: 7,
                rpm: 11750,
            }
        );
    }

    #[test]
    fn rejects_short_packets() {
        assert!(parse_player_input_sample(&[1, 2, 3]).is_err());
    }
}
