use super::constants::{MAX_CARS, PACKET_HEADER_SIZE, max_cars_for_format};
use super::header::parse_packet_header;
use crate::telemetry::{LapSample, RaceOrderCarSample, RaceOrderSample};

pub const LAP_DATA_SIZE: usize = 57;
pub const LAP_DATA_PACKET_SIZE: usize = PACKET_HEADER_SIZE + MAX_CARS * LAP_DATA_SIZE + 2;

pub fn lap_data_offset(car_index: usize) -> Result<usize, String> {
    lap_data_offset_for(car_index, MAX_CARS)
}

fn lap_data_offset_for(car_index: usize, max_cars: usize) -> Result<usize, String> {
    if car_index >= max_cars {
        return Err(format!("car_index must be between 0 and {}", max_cars - 1));
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
    if minutes == 255 || ms >= 60_000 {
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

fn read_active_car_at_position(packet: &[u8], position: u8, max_cars: usize) -> Option<u8> {
    if position == 0 || position > max_cars as u8 {
        return None;
    }

    let mut matched_index = None;
    for car_index in 0..max_cars {
        let base = lap_data_offset_for(car_index, max_cars).ok()?;
        let pit_status = packet[base + 34];
        let driver_status = packet[base + 44];
        let result_status = packet[base + 45];
        let is_active_on_track =
            pit_status == 0 && matches!(driver_status, 1 | 4) && result_status == 2;
        if packet[base + 32] == position && is_active_on_track {
            if matched_index.is_some() {
                return None;
            }
            matched_index = Some(car_index as u8);
        }
    }

    matched_index
}

fn read_car_behind(packet: &[u8], player_position: u8, max_cars: usize) -> Option<(u32, u8)> {
    let behind_index =
        read_active_car_at_position(packet, player_position.checked_add(1)?, max_cars)?;
    let base = lap_data_offset_for(behind_index as usize, max_cars).ok()?;
    read_delta_ms(packet, base + 14, base + 16).map(|gap| (gap, behind_index))
}

fn read_safety_car_delta_s(packet: &[u8], base: usize) -> Option<f32> {
    let delta = read_f32_le(packet, base + 28);
    delta.is_finite().then_some(delta)
}

pub fn parse_player_lap_sample(packet: &[u8]) -> Result<LapSample, String> {
    let header = parse_packet_header(packet)
        .ok_or_else(|| "packet is too short for F1 header".to_owned())?;
    let max_cars = max_cars_for_format(header.packet_format)
        .ok_or_else(|| format!("unsupported F1 packet format {}", header.packet_format))?;
    let car_index = header.player_car_index as usize;
    let base = lap_data_offset_for(car_index, max_cars)?;
    let packet_size = PACKET_HEADER_SIZE + max_cars * LAP_DATA_SIZE + 2;

    if packet.len() < base + LAP_DATA_SIZE || packet.len() < packet_size {
        return Err("packet is too short for F1 lap data".to_owned());
    }

    let car_position = packet[base + 32];

    let car_behind = read_car_behind(packet, car_position, max_cars);
    let car_in_front_index = car_position
        .checked_sub(1)
        .and_then(|position| read_active_car_at_position(packet, position, max_cars));

    Ok(LapSample {
        session_time: header.session_time,
        frame_identifier: header.frame_identifier,
        overall_frame_identifier: Some(header.overall_frame_identifier),
        player_car_index: header.player_car_index,
        last_lap_time_ms: read_u32_le(packet, base),
        current_lap_time_ms: read_u32_le(packet, base + 4),
        delta_to_car_in_front_ms: read_delta_ms(packet, base + 14, base + 16),
        car_in_front_index,
        delta_to_car_behind_ms: car_behind.map(|(gap_ms, _)| gap_ms),
        car_behind_index: car_behind.map(|(_, index)| index),
        delta_to_race_leader_ms: read_delta_ms(packet, base + 17, base + 19),
        safety_car_delta_s: read_safety_car_delta_s(packet, base),
        lap_distance_m: read_f32_le(packet, base + 20),
        total_distance_m: read_f32_le(packet, base + 24),
        car_position,
        current_lap_num: packet[base + 33],
        pit_status: packet[base + 34],
        num_pit_stops: packet[base + 35],
        sector: packet[base + 36],
        current_lap_invalid: packet[base + 37] != 0,
        driver_status: packet[base + 44],
        result_status: packet[base + 45],
        sector1_time_ms: read_time_ms(packet, base + 8, base + 10),
        sector2_time_ms: read_time_ms(packet, base + 11, base + 13),
    })
}

pub fn parse_race_order_sample(packet: &[u8]) -> Result<RaceOrderSample, String> {
    let header = parse_packet_header(packet)
        .ok_or_else(|| "packet is too short for F1 header".to_owned())?;
    let max_cars = max_cars_for_format(header.packet_format)
        .ok_or_else(|| format!("unsupported F1 packet format {}", header.packet_format))?;
    let packet_size = PACKET_HEADER_SIZE + max_cars * LAP_DATA_SIZE + 2;
    if packet.len() < packet_size {
        return Err("packet is too short for F1 lap data".to_owned());
    }

    let cars = (0..max_cars)
        .filter_map(|car_index| {
            let base = lap_data_offset_for(car_index, max_cars).ok()?;
            let car_position = packet[base + 32];
            let result_status = packet[base + 45];
            if car_position == 0 && result_status == 0 {
                return None;
            }
            Some(RaceOrderCarSample {
                car_index: car_index as u8,
                car_position,
                current_lap_num: packet[base + 33],
                lap_distance_m: read_f32_le(packet, base + 20),
                total_distance_m: read_f32_le(packet, base + 24),
                last_lap_time_ms: read_u32_le(packet, base),
                current_lap_time_ms: read_u32_le(packet, base + 4),
                delta_to_car_in_front_ms: read_delta_ms(packet, base + 14, base + 16),
                delta_to_race_leader_ms: read_delta_ms(packet, base + 17, base + 19),
                safety_car_delta_s: read_safety_car_delta_s(packet, base),
                pit_status: packet[base + 34],
                num_pit_stops: packet[base + 35],
                driver_status: packet[base + 44],
                result_status,
            })
        })
        .collect();

    Ok(RaceOrderSample {
        session_time: header.session_time,
        frame_identifier: header.frame_identifier,
        overall_frame_identifier: Some(header.overall_frame_identifier),
        player_car_index: header.player_car_index,
        cars,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::f1::constants::{
        F1_25_2026_SEASON_PACKET_FORMAT, F1_25_PACKET_FORMAT, MAX_CARS_2026, packet_id,
    };

    #[test]
    fn parses_player_lap_sample() {
        let mut packet = vec![0_u8; LAP_DATA_PACKET_SIZE];
        packet[0..2].copy_from_slice(&F1_25_PACKET_FORMAT.to_le_bytes());
        packet[2] = 25;
        packet[6] = packet_id::LAP_DATA;
        packet[15..19].copy_from_slice(&31.5_f32.to_le_bytes());
        packet[19..23].copy_from_slice(&123_u32.to_le_bytes());
        packet[23..27].copy_from_slice(&124_u32.to_le_bytes());
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
        packet[behind_base + 44] = 4;
        packet[behind_base + 45] = 2;

        assert_eq!(
            parse_player_lap_sample(&packet).unwrap(),
            LapSample {
                session_time: 31.5,
                frame_identifier: 123,
                overall_frame_identifier: Some(124),
                player_car_index: 1,
                last_lap_time_ms: 81_234,
                current_lap_time_ms: 42_345,
                lap_distance_m: 1234.5,
                total_distance_m: 9234.5,
                car_position: 3,
                current_lap_num: 7,
                pit_status: 1,
                num_pit_stops: 0,
                sector: 2,
                current_lap_invalid: true,
                driver_status: 4,
                result_status: 2,
                delta_to_car_in_front_ms: Some(60_456),
                car_in_front_index: None,
                delta_to_car_behind_ms: Some(234),
                car_behind_index: Some(4),
                delta_to_race_leader_ms: Some(789),
                safety_car_delta_s: Some(0.0),
                sector1_time_ms: Some(11_111),
                sector2_time_ms: Some(82_222),
            }
        );

        let order = parse_race_order_sample(&packet).unwrap();
        assert_eq!(order.player_car_index, 1);
        assert_eq!(order.cars.len(), 2);
        assert!(order.cars.iter().any(|car| {
            car.car_index == 4 && car.car_position == 4 && car.delta_to_car_in_front_ms == Some(234)
        }));
    }

    #[test]
    fn ignores_inactive_duplicate_position_when_deriving_behind_gap() {
        let mut packet = vec![0_u8; LAP_DATA_PACKET_SIZE];
        packet[0..2].copy_from_slice(&F1_25_PACKET_FORMAT.to_le_bytes());
        packet[2] = 25;
        packet[6] = packet_id::LAP_DATA;
        packet[27] = 0;

        let player_base = lap_data_offset(0).unwrap();
        packet[player_base + 32] = 1;
        packet[player_base + 33] = 10;
        packet[player_base + 44] = 4;
        packet[player_base + 45] = 2;

        let stale_base = lap_data_offset(1).unwrap();
        packet[stale_base + 14..stale_base + 16].copy_from_slice(&50_u16.to_le_bytes());
        packet[stale_base + 32] = 2;
        packet[stale_base + 44] = 0;
        packet[stale_base + 45] = 1;

        let active_base = lap_data_offset(2).unwrap();
        packet[active_base + 14..active_base + 16].copy_from_slice(&20_900_u16.to_le_bytes());
        packet[active_base + 32] = 2;
        packet[active_base + 44] = 4;
        packet[active_base + 45] = 2;

        let sample = parse_player_lap_sample(&packet).unwrap();
        assert_eq!(sample.delta_to_car_behind_ms, Some(20_900));
        assert_eq!(sample.car_behind_index, Some(2));
    }

    #[test]
    fn rejects_ambiguous_active_duplicate_position_when_deriving_behind_gap() {
        let mut packet = vec![0_u8; LAP_DATA_PACKET_SIZE];
        packet[0..2].copy_from_slice(&F1_25_PACKET_FORMAT.to_le_bytes());
        packet[2] = 25;
        packet[6] = packet_id::LAP_DATA;
        packet[27] = 0;

        let player_base = lap_data_offset(0).unwrap();
        packet[player_base + 32] = 1;
        packet[player_base + 33] = 10;
        packet[player_base + 44] = 4;
        packet[player_base + 45] = 2;

        for (car_index, gap_ms) in [(1, 50_u16), (2, 20_900_u16)] {
            let base = lap_data_offset(car_index).unwrap();
            packet[base + 14..base + 16].copy_from_slice(&gap_ms.to_le_bytes());
            packet[base + 32] = 2;
            packet[base + 44] = 4;
            packet[base + 45] = 2;
        }

        let sample = parse_player_lap_sample(&packet).unwrap();
        assert_eq!(sample.delta_to_car_behind_ms, None);
        assert_eq!(sample.car_behind_index, None);
    }

    #[test]
    fn rejects_wrapped_delta_millisecond_component() {
        let mut packet = vec![0_u8; LAP_DATA_PACKET_SIZE];
        packet[0..2].copy_from_slice(&F1_25_PACKET_FORMAT.to_le_bytes());
        packet[2] = 25;
        packet[6] = packet_id::LAP_DATA;
        packet[27] = 0;

        let player_base = lap_data_offset(0).unwrap();
        packet[player_base + 14..player_base + 16].copy_from_slice(&65_519_u16.to_le_bytes());
        packet[player_base + 16] = 0;
        packet[player_base + 32] = 1;
        packet[player_base + 44] = 4;
        packet[player_base + 45] = 2;

        let sample = parse_player_lap_sample(&packet).unwrap();
        assert_eq!(sample.delta_to_car_in_front_ms, None);
    }

    #[test]
    fn rejects_short_packets() {
        assert!(parse_player_lap_sample(&[1, 2, 3]).is_err());
    }

    #[test]
    fn parses_2026_season_lap_packet_with_24_cars() {
        let packet_size = PACKET_HEADER_SIZE + MAX_CARS_2026 * LAP_DATA_SIZE + 2;
        let mut packet = vec![0_u8; packet_size];
        packet[0..2].copy_from_slice(&F1_25_2026_SEASON_PACKET_FORMAT.to_le_bytes());
        packet[2] = 25;
        packet[6] = packet_id::LAP_DATA;
        packet[15..19].copy_from_slice(&55.0_f32.to_le_bytes());
        packet[19..23].copy_from_slice(&999_u32.to_le_bytes());
        packet[27] = 23;
        let base = lap_data_offset_for(23, MAX_CARS_2026).unwrap();
        packet[base..base + 4].copy_from_slice(&88_765_u32.to_le_bytes());
        packet[base + 4..base + 8].copy_from_slice(&12_345_u32.to_le_bytes());
        packet[base + 20..base + 24].copy_from_slice(&1_234.5_f32.to_le_bytes());
        packet[base + 32] = 7;
        packet[base + 33] = 4;
        packet[base + 36] = 1;
        packet[base + 44] = 4;
        packet[base + 45] = 2;

        let sample = parse_player_lap_sample(&packet).unwrap();
        assert_eq!(sample.player_car_index, 23);
        assert_eq!(sample.last_lap_time_ms, 88_765);
        assert_eq!(sample.current_lap_num, 4);
        assert_eq!(sample.car_position, 7);
        assert_eq!(sample.lap_distance_m, 1_234.5);
    }
}
