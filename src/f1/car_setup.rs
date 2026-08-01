use super::constants::{MAX_CARS, PACKET_HEADER_SIZE, max_cars_for_format};
use super::header::parse_packet_header;
use crate::telemetry::{CarSetupSample, WheelValuesF32};

pub const CAR_SETUP_DATA_SIZE: usize = 50;
pub const CAR_SETUP_PACKET_SIZE: usize = PACKET_HEADER_SIZE + MAX_CARS * CAR_SETUP_DATA_SIZE + 4;

fn car_setup_offset(car_index: usize, max_cars: usize) -> Result<usize, String> {
    if car_index >= max_cars {
        return Err(format!("car_index must be between 0 and {}", max_cars - 1));
    }
    Ok(PACKET_HEADER_SIZE + car_index * CAR_SETUP_DATA_SIZE)
}

fn read_f32_le(packet: &[u8], offset: usize) -> f32 {
    f32::from_le_bytes(
        packet[offset..offset + 4]
            .try_into()
            .expect("validated f32 offset"),
    )
}

pub fn parse_player_setup_sample(packet: &[u8]) -> Result<CarSetupSample, String> {
    let header = parse_packet_header(packet)
        .ok_or_else(|| "packet is too short for F1 header".to_owned())?;
    let max_cars = max_cars_for_format(header.packet_format)
        .ok_or_else(|| format!("unsupported F1 packet format {}", header.packet_format))?;
    let packet_size = PACKET_HEADER_SIZE + max_cars * CAR_SETUP_DATA_SIZE + 4;
    if packet.len() != packet_size {
        return Err(format!(
            "invalid F1 car setup packet size: expected {packet_size}, got {}",
            packet.len()
        ));
    }

    let base = car_setup_offset(header.player_car_index as usize, max_cars)?;
    let next_front_wing_offset = PACKET_HEADER_SIZE + max_cars * CAR_SETUP_DATA_SIZE;
    Ok(CarSetupSample {
        packet_format: header.packet_format,
        session_time: header.session_time,
        frame_identifier: header.frame_identifier,
        player_car_index: header.player_car_index,
        front_wing: packet[base],
        rear_wing: packet[base + 1],
        on_throttle_differential_percent: packet[base + 2],
        off_throttle_differential_percent: packet[base + 3],
        front_camber: read_f32_le(packet, base + 4),
        rear_camber: read_f32_le(packet, base + 8),
        front_toe: read_f32_le(packet, base + 12),
        rear_toe: read_f32_le(packet, base + 16),
        front_suspension: packet[base + 20],
        rear_suspension: packet[base + 21],
        front_anti_roll_bar: packet[base + 22],
        rear_anti_roll_bar: packet[base + 23],
        front_ride_height: packet[base + 24],
        rear_ride_height: packet[base + 25],
        brake_pressure_percent: packet[base + 26],
        brake_bias_percent: packet[base + 27],
        engine_braking_percent: packet[base + 28],
        tyre_pressures_psi: WheelValuesF32 {
            rl: read_f32_le(packet, base + 29),
            rr: read_f32_le(packet, base + 33),
            fl: read_f32_le(packet, base + 37),
            fr: read_f32_le(packet, base + 41),
        },
        ballast: packet[base + 45],
        fuel_load_kg: read_f32_le(packet, base + 46),
        next_front_wing: read_f32_le(packet, next_front_wing_offset),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::f1::constants::{
        F1_25_2026_SEASON_PACKET_FORMAT, F1_25_PACKET_FORMAT, MAX_CARS_2026, packet_id,
    };

    fn packet_for(format: u16, max_cars: usize, player: usize) -> Vec<u8> {
        let mut packet = vec![0_u8; PACKET_HEADER_SIZE + max_cars * CAR_SETUP_DATA_SIZE + 4];
        packet[0..2].copy_from_slice(&format.to_le_bytes());
        packet[2] = 25;
        packet[6] = packet_id::CAR_SETUPS;
        packet[15..19].copy_from_slice(&42.5_f32.to_le_bytes());
        packet[19..23].copy_from_slice(&1234_u32.to_le_bytes());
        packet[27] = player as u8;
        packet
    }

    fn write_sample(packet: &mut [u8], max_cars: usize, player: usize) {
        let base = car_setup_offset(player, max_cars).unwrap();
        packet[base] = 21;
        packet[base + 1] = 18;
        packet[base + 2] = 65;
        packet[base + 3] = 50;
        packet[base + 4..base + 8].copy_from_slice(&(-3.5_f32).to_le_bytes());
        packet[base + 8..base + 12].copy_from_slice(&(-2.0_f32).to_le_bytes());
        packet[base + 12..base + 16].copy_from_slice(&0.05_f32.to_le_bytes());
        packet[base + 16..base + 20].copy_from_slice(&0.2_f32.to_le_bytes());
        packet[base + 20..base + 29].copy_from_slice(&[12, 8, 10, 6, 22, 48, 100, 56, 40]);
        packet[base + 29..base + 33].copy_from_slice(&21.0_f32.to_le_bytes());
        packet[base + 33..base + 37].copy_from_slice(&21.1_f32.to_le_bytes());
        packet[base + 37..base + 41].copy_from_slice(&23.2_f32.to_le_bytes());
        packet[base + 41..base + 45].copy_from_slice(&23.3_f32.to_le_bytes());
        packet[base + 45] = 5;
        packet[base + 46..base + 50].copy_from_slice(&30.0_f32.to_le_bytes());
        let next = PACKET_HEADER_SIZE + max_cars * CAR_SETUP_DATA_SIZE;
        packet[next..next + 4].copy_from_slice(&22.0_f32.to_le_bytes());
    }

    #[test]
    fn parses_2025_player_setup() {
        let mut packet = packet_for(F1_25_PACKET_FORMAT, MAX_CARS, 3);
        write_sample(&mut packet, MAX_CARS, 3);
        let sample = parse_player_setup_sample(&packet).unwrap();
        assert_eq!(sample.front_wing, 21);
        assert_eq!(sample.on_throttle_differential_percent, 65);
        assert_eq!(sample.tyre_pressures_psi.fr, 23.3);
        assert_eq!(sample.next_front_wing, 22.0);
    }

    #[test]
    fn parses_2026_player_setup_for_24_cars() {
        let mut packet = packet_for(F1_25_2026_SEASON_PACKET_FORMAT, MAX_CARS_2026, 23);
        write_sample(&mut packet, MAX_CARS_2026, 23);
        let sample = parse_player_setup_sample(&packet).unwrap();
        assert_eq!(sample.packet_format, F1_25_2026_SEASON_PACKET_FORMAT);
        assert_eq!(sample.player_car_index, 23);
        assert_eq!(sample.rear_ride_height, 48);
        assert_eq!(sample.fuel_load_kg, 30.0);
    }
}
