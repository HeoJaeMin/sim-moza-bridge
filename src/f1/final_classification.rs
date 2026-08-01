use super::constants::{PACKET_HEADER_SIZE, max_cars_for_format};
use super::header::parse_packet_header;
use crate::telemetry::FinalClassificationSample;

pub const FINAL_CLASSIFICATION_DATA_SIZE: usize = 46;

fn read_u32_le(packet: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(
        packet[offset..offset + 4]
            .try_into()
            .expect("valid u32 offset"),
    )
}

fn read_f64_le(packet: &[u8], offset: usize) -> f64 {
    f64::from_le_bytes(
        packet[offset..offset + 8]
            .try_into()
            .expect("valid f64 offset"),
    )
}

pub fn parse_player_final_classification_sample(
    packet: &[u8],
) -> Result<FinalClassificationSample, String> {
    let header = parse_packet_header(packet)
        .ok_or_else(|| "packet is too short for F1 header".to_owned())?;
    let max_cars = max_cars_for_format(header.packet_format)
        .ok_or_else(|| format!("unsupported F1 packet format {}", header.packet_format))?;
    let car_index = header.player_car_index as usize;
    if car_index >= max_cars {
        return Err(format!("car_index must be between 0 and {}", max_cars - 1));
    }

    let base = PACKET_HEADER_SIZE + 1 + car_index * FINAL_CLASSIFICATION_DATA_SIZE;
    let packet_size = PACKET_HEADER_SIZE + 1 + max_cars * FINAL_CLASSIFICATION_DATA_SIZE;
    if packet.len() < base + FINAL_CLASSIFICATION_DATA_SIZE || packet.len() < packet_size {
        return Err("packet is too short for F1 final classification data".to_owned());
    }

    Ok(FinalClassificationSample {
        session_time: header.session_time,
        frame_identifier: header.frame_identifier,
        player_car_index: header.player_car_index,
        position: packet[base],
        num_laps: packet[base + 1],
        grid_position: packet[base + 2],
        points: packet[base + 3],
        num_pit_stops: packet[base + 4],
        result_status: packet[base + 5],
        result_reason: packet[base + 6],
        best_lap_time_ms: read_u32_le(packet, base + 7),
        total_race_time_s: read_f64_le(packet, base + 11),
        penalties_time_s: packet[base + 19],
        num_penalties: packet[base + 20],
        num_tyre_stints: packet[base + 21],
        tyre_stints_actual: packet[base + 22..base + 30]
            .try_into()
            .expect("validated actual tyre stint offset"),
        tyre_stints_visual: packet[base + 30..base + 38]
            .try_into()
            .expect("validated visual tyre stint offset"),
        tyre_stints_end_laps: packet[base + 38..base + 46]
            .try_into()
            .expect("validated tyre stint end offset"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::f1::constants::{F1_25_2026_SEASON_PACKET_FORMAT, MAX_CARS_2026, packet_id};

    #[test]
    fn parses_2026_player_final_classification() {
        let mut packet =
            vec![0_u8; PACKET_HEADER_SIZE + 1 + MAX_CARS_2026 * FINAL_CLASSIFICATION_DATA_SIZE];
        packet[0..2].copy_from_slice(&F1_25_2026_SEASON_PACKET_FORMAT.to_le_bytes());
        packet[2] = 25;
        packet[6] = packet_id::FINAL_CLASSIFICATION;
        packet[15..19].copy_from_slice(&5_400.0_f32.to_le_bytes());
        packet[19..23].copy_from_slice(&99_000_u32.to_le_bytes());
        packet[27] = 23;
        packet[PACKET_HEADER_SIZE] = 24;

        let base = PACKET_HEADER_SIZE + 1 + 23 * FINAL_CLASSIFICATION_DATA_SIZE;
        packet[base] = 1;
        packet[base + 1] = 53;
        packet[base + 2] = 4;
        packet[base + 3] = 25;
        packet[base + 4] = 1;
        packet[base + 5] = 3;
        packet[base + 6] = 2;
        packet[base + 7..base + 11].copy_from_slice(&91_234_u32.to_le_bytes());
        packet[base + 11..base + 19].copy_from_slice(&5_321.125_f64.to_le_bytes());
        packet[base + 19] = 5;
        packet[base + 20] = 1;
        packet[base + 21] = 2;
        packet[base + 22..base + 30].copy_from_slice(&[16, 18, 0, 0, 0, 0, 0, 0]);
        packet[base + 30..base + 38].copy_from_slice(&[16, 18, 0, 0, 0, 0, 0, 0]);
        packet[base + 38..base + 46].copy_from_slice(&[20, 53, 0, 0, 0, 0, 0, 0]);

        let sample = parse_player_final_classification_sample(&packet).unwrap();
        assert_eq!(sample.position, 1);
        assert_eq!(sample.num_laps, 53);
        assert_eq!(sample.result_status, 3);
        assert_eq!(sample.result_reason, 2);
        assert_eq!(sample.total_race_time_s, 5_321.125);
        assert_eq!(sample.tyre_stints_end_laps[..2], [20, 53]);
    }
}
