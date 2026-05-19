use super::constants::{MAX_CARS, PACKET_HEADER_SIZE};
use super::header::parse_packet_header;
use crate::telemetry::StatusSample;

pub const CAR_STATUS_DATA_SIZE: usize = 55;
pub const CAR_STATUS_PACKET_SIZE: usize = PACKET_HEADER_SIZE + MAX_CARS * CAR_STATUS_DATA_SIZE;

pub fn car_status_offset(car_index: usize) -> Result<usize, String> {
    if car_index >= MAX_CARS {
        return Err(format!("car_index must be between 0 and {}", MAX_CARS - 1));
    }

    Ok(PACKET_HEADER_SIZE + car_index * CAR_STATUS_DATA_SIZE)
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

pub fn parse_player_status_sample(packet: &[u8]) -> Result<StatusSample, String> {
    let header = parse_packet_header(packet)
        .ok_or_else(|| "packet is too short for F1 header".to_owned())?;
    let car_index = header.player_car_index as usize;
    let base = car_status_offset(car_index)?;

    if packet.len() < base + CAR_STATUS_DATA_SIZE || packet.len() < CAR_STATUS_PACKET_SIZE {
        return Err("packet is too short for F1 car status data".to_owned());
    }

    Ok(StatusSample {
        session_time: header.session_time,
        frame_identifier: header.frame_identifier,
        player_car_index: header.player_car_index,
        traction_control: packet[base],
        anti_lock_brakes: packet[base + 1],
        front_brake_bias: packet[base + 3],
        fuel_in_tank: read_f32_le(packet, base + 5),
        fuel_capacity: read_f32_le(packet, base + 9),
        fuel_remaining_laps: read_f32_le(packet, base + 13),
        max_rpm: read_u16_le(packet, base + 17),
        idle_rpm: read_u16_le(packet, base + 19),
        max_gears: packet[base + 21],
        drs_allowed: packet[base + 22] != 0,
        actual_tyre_compound: packet[base + 25],
        visual_tyre_compound: packet[base + 26],
        tyres_age_laps: packet[base + 27],
        ers_store_energy: read_f32_le(packet, base + 37),
        ers_deploy_mode: packet[base + 41],
        ers_deployed_this_lap: read_f32_le(packet, base + 50),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::f1::constants::{F1_25_PACKET_FORMAT, packet_id};

    #[test]
    fn parses_player_status_sample() {
        let mut packet = vec![0_u8; CAR_STATUS_PACKET_SIZE];
        packet[0..2].copy_from_slice(&F1_25_PACKET_FORMAT.to_le_bytes());
        packet[2] = 25;
        packet[6] = packet_id::CAR_STATUS;
        packet[15..19].copy_from_slice(&13.0_f32.to_le_bytes());
        packet[19..23].copy_from_slice(&300_u32.to_le_bytes());
        packet[27] = 3;

        let base = car_status_offset(3).unwrap();
        packet[base] = 2;
        packet[base + 1] = 1;
        packet[base + 3] = 56;
        packet[base + 5..base + 9].copy_from_slice(&42.5_f32.to_le_bytes());
        packet[base + 9..base + 13].copy_from_slice(&110.0_f32.to_le_bytes());
        packet[base + 13..base + 17].copy_from_slice(&5.5_f32.to_le_bytes());
        packet[base + 17..base + 19].copy_from_slice(&12_500_u16.to_le_bytes());
        packet[base + 19..base + 21].copy_from_slice(&4_000_u16.to_le_bytes());
        packet[base + 21] = 8;
        packet[base + 22] = 1;
        packet[base + 25] = 18;
        packet[base + 26] = 17;
        packet[base + 27] = 4;
        packet[base + 37..base + 41].copy_from_slice(&2_000_000.0_f32.to_le_bytes());
        packet[base + 41] = 3;
        packet[base + 50..base + 54].copy_from_slice(&800_000.0_f32.to_le_bytes());

        assert_eq!(
            parse_player_status_sample(&packet).unwrap(),
            StatusSample {
                session_time: 13.0,
                frame_identifier: 300,
                player_car_index: 3,
                traction_control: 2,
                anti_lock_brakes: 1,
                front_brake_bias: 56,
                fuel_in_tank: 42.5,
                fuel_capacity: 110.0,
                fuel_remaining_laps: 5.5,
                max_rpm: 12_500,
                idle_rpm: 4_000,
                max_gears: 8,
                drs_allowed: true,
                actual_tyre_compound: 18,
                visual_tyre_compound: 17,
                tyres_age_laps: 4,
                ers_store_energy: 2_000_000.0,
                ers_deploy_mode: 3,
                ers_deployed_this_lap: 800_000.0,
            }
        );
    }
}
