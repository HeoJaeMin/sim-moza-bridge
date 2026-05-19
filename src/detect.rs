use crate::f1::constants::{F1_25_PACKET_FORMAT, packet_id};
use crate::f1::header::parse_packet_header;
use crate::games::{GameProfile, resolve_game_profile};

pub fn detect_game_profile_from_packet(packet: &[u8]) -> Option<GameProfile> {
    let header = parse_packet_header(packet)?;
    let is_known_f1_packet = header.packet_format == F1_25_PACKET_FORMAT
        && header.game_year == 25
        && header.packet_id >= packet_id::MOTION
        && header.packet_id <= packet_id::LAP_POSITIONS;

    if is_known_f1_packet {
        return resolve_game_profile("f1-25").ok();
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::f1::constants::PACKET_HEADER_SIZE;

    fn make_header_packet(packet_format: u16, game_year: u8, packet_id: u8) -> Vec<u8> {
        let mut packet = vec![0_u8; PACKET_HEADER_SIZE];
        packet[0..2].copy_from_slice(&packet_format.to_le_bytes());
        packet[2] = game_year;
        packet[6] = packet_id;
        packet
    }

    #[test]
    fn recognizes_f1_25_packets() {
        let packet = make_header_packet(F1_25_PACKET_FORMAT, 25, packet_id::CAR_DAMAGE);
        assert_eq!(
            detect_game_profile_from_packet(&packet).unwrap().id,
            "f1-25"
        );
    }

    #[test]
    fn ignores_unknown_packets() {
        assert!(detect_game_profile_from_packet(&[1, 2, 3]).is_none());
        assert!(
            detect_game_profile_from_packet(&make_header_packet(2024, 24, packet_id::CAR_DAMAGE))
                .is_none()
        );
        assert!(
            detect_game_profile_from_packet(&make_header_packet(F1_25_PACKET_FORMAT, 25, 99))
                .is_none()
        );
    }
}
