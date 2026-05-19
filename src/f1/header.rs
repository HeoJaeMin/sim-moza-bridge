use super::constants::PACKET_HEADER_SIZE;

#[derive(Clone, Debug, PartialEq)]
pub struct PacketHeader {
    pub packet_format: u16,
    pub game_year: u8,
    pub game_major_version: u8,
    pub game_minor_version: u8,
    pub packet_version: u8,
    pub packet_id: u8,
    pub session_uid: u64,
    pub session_time: f32,
    pub frame_identifier: u32,
    pub overall_frame_identifier: u32,
    pub player_car_index: u8,
    pub secondary_player_car_index: u8,
}

pub fn parse_packet_header(packet: &[u8]) -> Option<PacketHeader> {
    if packet.len() < PACKET_HEADER_SIZE {
        return None;
    }

    Some(PacketHeader {
        packet_format: u16::from_le_bytes(packet[0..2].try_into().ok()?),
        game_year: packet[2],
        game_major_version: packet[3],
        game_minor_version: packet[4],
        packet_version: packet[5],
        packet_id: packet[6],
        session_uid: u64::from_le_bytes(packet[7..15].try_into().ok()?),
        session_time: f32::from_le_bytes(packet[15..19].try_into().ok()?),
        frame_identifier: u32::from_le_bytes(packet[19..23].try_into().ok()?),
        overall_frame_identifier: u32::from_le_bytes(packet[23..27].try_into().ok()?),
        player_car_index: packet[27],
        secondary_player_car_index: packet[28],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::f1::constants::{F1_25_PACKET_FORMAT, packet_id};

    #[test]
    fn parses_header() {
        let mut packet = vec![0_u8; PACKET_HEADER_SIZE];
        packet[0..2].copy_from_slice(&F1_25_PACKET_FORMAT.to_le_bytes());
        packet[2] = 25;
        packet[3] = 1;
        packet[4] = 7;
        packet[5] = 1;
        packet[6] = packet_id::CAR_DAMAGE;
        packet[7..15].copy_from_slice(&1234_u64.to_le_bytes());
        packet[15..19].copy_from_slice(&42.5_f32.to_le_bytes());
        packet[19..23].copy_from_slice(&100_u32.to_le_bytes());
        packet[23..27].copy_from_slice(&101_u32.to_le_bytes());
        packet[27] = 3;
        packet[28] = 255;

        assert_eq!(
            parse_packet_header(&packet),
            Some(PacketHeader {
                packet_format: 2025,
                game_year: 25,
                game_major_version: 1,
                game_minor_version: 7,
                packet_version: 1,
                packet_id: packet_id::CAR_DAMAGE,
                session_uid: 1234,
                session_time: 42.5,
                frame_identifier: 100,
                overall_frame_identifier: 101,
                player_car_index: 3,
                secondary_player_car_index: 255,
            })
        );
    }

    #[test]
    fn rejects_short_packets() {
        assert!(parse_packet_header(&vec![0_u8; PACKET_HEADER_SIZE - 1]).is_none());
    }
}
