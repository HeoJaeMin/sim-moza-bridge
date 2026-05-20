use super::constants::{MAX_CARS, PACKET_HEADER_SIZE};
use super::header::parse_packet_header;
use crate::telemetry::LapSample;

pub const LAP_DATA_SIZE: usize = 57;
pub const LAP_DATA_PACKET_SIZE: usize = PACKET_HEADER_SIZE + MAX_CARS * LAP_DATA_SIZE + 2;

pub fn lap_data_offset(car_index: usize) -> Result<usize, String> {
    if car_index >= MAX_CARS {
        return Err(format!("car_index must be between 0 and {}", MAX_CARS - 1));
    }

    Ok(PACKET_HEADER_SIZE + car_index * LAP_DATA_SIZE)
}

fn read_u16_le(packet: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes(
        packet[offset..offset + 2]
            .try_into()
            .expect("valid u16 offset"),
    )
}

fn read_u32_le(packet: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(
        packet[offset..offset + 4]
            .try_into()
            .expect("valid u32 offset"),
    )
}

fn read_f32_le(packet: &[u8], offset: usize) -> f32 {
    f32::from_le_bytes(
        packet[offset..offset + 4]
            .try_into()
            .expect("valid f32 offset"),
    )
}

fn read_delta_ms(packet: &[u8], ms_offset: usize, min_offset: usize) -> Option<u32> {
    let ms = read_u16_le(packet, ms_offset) as u32;
    let minutes = packet[min_offset] as u32;
    if minutes == 255 {
        return None;
    }
    Some(minutes * 60_000 + ms)
}

fn read_time_ms(packet: &[u8], ms_offset: usize, min_offset: usize) -> Option<u32> {
    let ms = read_u16_le(packet, ms_offset) as u32;
    let minutes = packet[min_offset] as u32;
    if minutes == 255 && ms == u16::MAX as u32 {
        return None;
    }
    Some(minutes * 60_000 + ms)
}

fn read_delta_to_car_behind_ms(packet: &[u8], player_position: u8) -> Option<u32> {
    if player_position == 0 {
        return None;
    }

    let behind_position = player_position.checked_add(1)?;
    if behind_position > MAX_CARS as u8 {
        return None;
    }

    for car_index in 0..MAX_CARS {
        let base = lap_data_offset(car_index).ok()?;
        if packet[base + 32] == behind_position {
            return read_delta_ms(packet, base + 14, base + 16);
        }
    }

    None
}

pub fn parse_player_lap_sample(packet: &[u8]) -> Result<LapSample, String> {
    let header = parse_packet_header(packet)
        .ok_or_else(|| "packet is too short for F1 header".to_owned())?;
    let car_index = header.player_car_index as usize;
    let base = lap_data_offset(car_index)?;

    if packet.len() < base + LAP_DATA_SIZE || packet.len() < LAP_DATA_PACKET_SIZE {
        return Err("packet is too short for F1 lap data".to_owned());
    }

    let car_position = packet[base + 32];

    Ok(LapSample {
        session_time: header.session_time,
        frame_identifier: header.frame_identifier,
        player_car_index: header.player_car_index,
        last_lap_time_ms: read_u32_le(packet, base),
        current_lap_time_ms: read_u32_le(packet, base + 4),
        delta_to_car_in_front_ms: read_delta_ms(packet, base + 14, base + 16),
        delta_to_car_behind_ms: read_delta_to_car_behind_ms(packet, car_position),
        delta_to_race_leader_ms: read_delta_ms(packet, base + 17, base + 19),
        lap_distance_m: read_f32_le(packet, base + 20),
        total_distance_m: read_f32_le(packet, base + 24),
        car_position,
        current_lap_num: packet[base + 33],
        pit_status: packet[base + 34],
        sector: packet[base + 36],
        current_lap_invalid: packet[base + 37] != 0,
        driver_status: packet[base + 44],
        result_status: packet[base + 45],
        sector1_time_ms: read_time_ms(packet, base + 8, base + 10),
        sector2_time_ms: read_time_ms(packet, base + 11, base + 13),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::f1::constants::{F1_25_PACKET_FORMAT, packet_id};

    #[test]
    fn parses_player_lap_sample() {
        let mut packet = vec![0_u8; LAP_DATA_PACKET_SIZE];
        packet[0..2].copy_from_slice(&F1_25_PACKET_FORMAT.to_le_bytes());
        packet[2] = 25;
        packet[6] = packet_id::LAP_DATA;
        packet[15..19].copy_from_slice(&31.5_f32.to_le_bytes());
        packet[19..23].copy_from_slice(&123_u32.to_le_bytes());
        packet[27] = 1;

        let base = lap_data_offset(1).unwrap();
        packet[base..base + 4].copy_from_slice(&81_234_u32.to_le_bytes());
        packet[base + 4..base + 8].copy_from_slice(&42_345_u32.to_le_bytes());
        packet[base + 8..base + 10].copy_from_slice(&11_111_u16.to_le_bytes());
        packet[base + 10] = 0;
        packet[base + 11..base + 13].copy_from_slice(&22_222_u16.to_le_bytes());
        packet[base + 13] = 1;
        packet[base + 14..base + 16].copy_from_slice(&456_u16.to_le_bytes());
        packet[base + 16] = 1;
        packet[base + 17..base + 19].copy_from_slice(&789_u16.to_le_bytes());
        packet[base + 19] = 0;
        packet[base + 20..base + 24].copy_from_slice(&1234.5_f32.to_le_bytes());
        packet[base + 24..base + 28].copy_from_slice(&9234.5_f32.to_le_bytes());
        packet[base + 32] = 3;
        packet[base + 33] = 7;
        packet[base + 34] = 1;
        packet[base + 36] = 2;
        packet[base + 37] = 1;
        packet[base + 44] = 4;
        packet[base + 45] = 2;
        let behind_base = lap_data_offset(4).unwrap();
        packet[behind_base + 14..behind_base + 16].copy_from_slice(&234_u16.to_le_bytes());
        packet[behind_base + 16] = 0;
        packet[behind_base + 32] = 4;

        assert_eq!(
            parse_player_lap_sample(&packet).unwrap(),
            LapSample {
                session_time: 31.5,
                frame_identifier: 123,
                player_car_index: 1,
                last_lap_time_ms: 81_234,
                current_lap_time_ms: 42_345,
                lap_distance_m: 1234.5,
                total_distance_m: 9234.5,
                car_position: 3,
                current_lap_num: 7,
                pit_status: 1,
                sector: 2,
                current_lap_invalid: true,
                driver_status: 4,
                result_status: 2,
                delta_to_car_in_front_ms: Some(60_456),
                delta_to_car_behind_ms: Some(234),
                delta_to_race_leader_ms: Some(789),
                sector1_time_ms: Some(11_111),
                sector2_time_ms: Some(82_222),
            }
        );
    }

    #[test]
    fn rejects_short_packets() {
        assert!(parse_player_lap_sample(&[1, 2, 3]).is_err());
    }
}
