use super::constants::PACKET_HEADER_SIZE;
use super::header::parse_packet_header;
use crate::telemetry::SessionSample;

pub const SESSION_MIN_PACKET_SIZE: usize = PACKET_HEADER_SIZE + 11;

fn read_u16_le(packet: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes(
        packet[offset..offset + 2]
            .try_into()
            .expect("valid u16 offset"),
    )
}

pub fn parse_session_sample(packet: &[u8]) -> Result<SessionSample, String> {
    let header = parse_packet_header(packet)
        .ok_or_else(|| "packet is too short for F1 header".to_owned())?;

    if packet.len() < SESSION_MIN_PACKET_SIZE {
        return Err("packet is too short for F1 session data".to_owned());
    }

    Ok(SessionSample {
        session_time: header.session_time,
        frame_identifier: header.frame_identifier,
        total_laps: packet[PACKET_HEADER_SIZE + 3],
        track_length_m: read_u16_le(packet, PACKET_HEADER_SIZE + 4),
        session_type: packet[PACKET_HEADER_SIZE + 6],
        track_id: packet[PACKET_HEADER_SIZE + 7] as i8,
        track_temp_c: packet[PACKET_HEADER_SIZE + 1] as i8,
        air_temp_c: packet[PACKET_HEADER_SIZE + 2] as i8,
        session_time_left_s: read_u16_le(packet, PACKET_HEADER_SIZE + 9),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::f1::constants::{F1_25_PACKET_FORMAT, packet_id};

    #[test]
    fn parses_session_sample() {
        let mut packet = vec![0_u8; SESSION_MIN_PACKET_SIZE];
        packet[0..2].copy_from_slice(&F1_25_PACKET_FORMAT.to_le_bytes());
        packet[2] = 25;
        packet[6] = packet_id::SESSION;
        packet[15..19].copy_from_slice(&7.25_f32.to_le_bytes());
        packet[19..23].copy_from_slice(&44_u32.to_le_bytes());
        packet[PACKET_HEADER_SIZE + 1] = 31;
        packet[PACKET_HEADER_SIZE + 2] = 22;
        packet[PACKET_HEADER_SIZE + 3] = 58;
        packet[PACKET_HEADER_SIZE + 4..PACKET_HEADER_SIZE + 6]
            .copy_from_slice(&5412_u16.to_le_bytes());
        packet[PACKET_HEADER_SIZE + 6] = 15;
        packet[PACKET_HEADER_SIZE + 7] = 7;
        packet[PACKET_HEADER_SIZE + 9..PACKET_HEADER_SIZE + 11]
            .copy_from_slice(&1200_u16.to_le_bytes());

        assert_eq!(
            parse_session_sample(&packet).unwrap(),
            SessionSample {
                session_time: 7.25,
                frame_identifier: 44,
                total_laps: 58,
                track_length_m: 5412,
                session_type: 15,
                track_id: 7,
                track_temp_c: 31,
                air_temp_c: 22,
                session_time_left_s: 1200,
            }
        );
    }
}
