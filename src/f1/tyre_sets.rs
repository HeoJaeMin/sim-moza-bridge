use super::constants::PACKET_HEADER_SIZE;
use super::header::parse_packet_header;
use crate::telemetry::{TyreSetInfo, TyreSetsSample};

pub const TYRE_SET_COUNT: usize = 20;
pub const TYRE_SET_DATA_SIZE: usize = 10;
pub const TYRE_SETS_PACKET_SIZE: usize =
    PACKET_HEADER_SIZE + 1 + TYRE_SET_COUNT * TYRE_SET_DATA_SIZE + 1;

pub fn parse_player_tyre_sets_sample(packet: &[u8]) -> Result<Option<TyreSetsSample>, String> {
    let header = parse_packet_header(packet)
        .ok_or_else(|| "packet is too short for F1 header".to_owned())?;
    if packet.len() != TYRE_SETS_PACKET_SIZE {
        return Err(format!(
            "invalid F1 tyre sets packet size: expected {TYRE_SETS_PACKET_SIZE}, got {}",
            packet.len()
        ));
    }
    let car_index = packet[PACKET_HEADER_SIZE];
    if car_index != header.player_car_index {
        return Ok(None);
    }

    let sets_base = PACKET_HEADER_SIZE + 1;
    let fitted_index = packet[sets_base + TYRE_SET_COUNT * TYRE_SET_DATA_SIZE];
    let sets = (0..TYRE_SET_COUNT)
        .map(|index| {
            let base = sets_base + index * TYRE_SET_DATA_SIZE;
            TyreSetInfo {
                index: index as u8,
                actual_tyre_compound: packet[base],
                visual_tyre_compound: packet[base + 1],
                wear_percent: packet[base + 2],
                available: packet[base + 3] != 0,
                recommended_session: packet[base + 4],
                life_span_laps: packet[base + 5],
                usable_life_laps: packet[base + 6],
                lap_delta_ms: i16::from_le_bytes([packet[base + 7], packet[base + 8]]),
                fitted: packet[base + 9] != 0,
            }
        })
        .collect();

    Ok(Some(TyreSetsSample {
        packet_format: header.packet_format,
        session_time: header.session_time,
        frame_identifier: header.frame_identifier,
        player_car_index: header.player_car_index,
        fitted_index,
        sets,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::f1::constants::{F1_25_2026_SEASON_PACKET_FORMAT, packet_id};

    #[test]
    fn parses_player_tyre_set_inventory() {
        let mut packet = vec![0_u8; TYRE_SETS_PACKET_SIZE];
        packet[0..2].copy_from_slice(&F1_25_2026_SEASON_PACKET_FORMAT.to_le_bytes());
        packet[2] = 25;
        packet[6] = packet_id::TYRE_SETS;
        packet[15..19].copy_from_slice(&12.0_f32.to_le_bytes());
        packet[19..23].copy_from_slice(&99_u32.to_le_bytes());
        packet[27] = 17;
        packet[PACKET_HEADER_SIZE] = 17;
        let base = PACKET_HEADER_SIZE + 1 + 4 * TYRE_SET_DATA_SIZE;
        packet[base..base + 10].copy_from_slice(&[18, 17, 3, 1, 1, 20, 18, 0xD4, 0xFE, 1]);
        packet[PACKET_HEADER_SIZE + 1 + TYRE_SET_COUNT * TYRE_SET_DATA_SIZE] = 4;

        let sample = parse_player_tyre_sets_sample(&packet).unwrap().unwrap();
        assert_eq!(sample.player_car_index, 17);
        assert_eq!(sample.fitted_index, 4);
        assert_eq!(sample.sets[4].visual_tyre_compound, 17);
        assert_eq!(sample.sets[4].wear_percent, 3);
        assert_eq!(sample.sets[4].lap_delta_ms, -300);
        assert!(sample.sets[4].available);
        assert!(sample.sets[4].fitted);
    }

    #[test]
    fn ignores_other_cars_tyre_sets() {
        let mut packet = vec![0_u8; TYRE_SETS_PACKET_SIZE];
        packet[0..2].copy_from_slice(&F1_25_2026_SEASON_PACKET_FORMAT.to_le_bytes());
        packet[2] = 25;
        packet[6] = packet_id::TYRE_SETS;
        packet[27] = 17;
        packet[PACKET_HEADER_SIZE] = 3;
        assert!(parse_player_tyre_sets_sample(&packet).unwrap().is_none());
    }
}
