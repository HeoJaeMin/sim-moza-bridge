use super::constants::PACKET_HEADER_SIZE;
use super::header::parse_packet_header;
use crate::telemetry::{MarshalZoneSample, SessionSample, WeatherForecastSample};

const MAX_MARSHAL_ZONES: usize = 21;
const NUM_MARSHAL_ZONES_OFFSET: usize = PACKET_HEADER_SIZE + 18;
const MARSHAL_ZONES_OFFSET: usize = PACKET_HEADER_SIZE + 19;
const MARSHAL_ZONE_SIZE: usize = 5;
const SAFETY_CAR_STATUS_OFFSET: usize =
    MARSHAL_ZONES_OFFSET + MAX_MARSHAL_ZONES * MARSHAL_ZONE_SIZE;
const NUM_WEATHER_FORECAST_SAMPLES_OFFSET: usize = SAFETY_CAR_STATUS_OFFSET + 2;
const WEATHER_FORECAST_SAMPLES_OFFSET: usize = SAFETY_CAR_STATUS_OFFSET + 3;
const WEATHER_FORECAST_SAMPLE_SIZE: usize = 8;
const MAX_WEATHER_FORECAST_SAMPLES: usize = 64;
const FORECAST_ACCURACY_OFFSET: usize =
    WEATHER_FORECAST_SAMPLES_OFFSET + MAX_WEATHER_FORECAST_SAMPLES * WEATHER_FORECAST_SAMPLE_SIZE;
const PIT_STOP_WINDOW_IDEAL_LAP_OFFSET: usize = FORECAST_ACCURACY_OFFSET + 14;
const PIT_STOP_WINDOW_LATEST_LAP_OFFSET: usize = PIT_STOP_WINDOW_IDEAL_LAP_OFFSET + 1;
const PIT_STOP_REJOIN_POSITION_OFFSET: usize = PIT_STOP_WINDOW_IDEAL_LAP_OFFSET + 2;
pub const SESSION_MIN_PACKET_SIZE: usize = SAFETY_CAR_STATUS_OFFSET + 1;

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

fn read_marshal_zones(packet: &[u8]) -> Vec<MarshalZoneSample> {
    let count = (packet[NUM_MARSHAL_ZONES_OFFSET] as usize).min(MAX_MARSHAL_ZONES);
    (0..count)
        .map(|index| {
            let base = MARSHAL_ZONES_OFFSET + index * MARSHAL_ZONE_SIZE;
            MarshalZoneSample {
                start: read_f32_le(packet, base),
                flag: packet[base + 4] as i8,
            }
        })
        .collect()
}

fn read_weather_forecast_samples(packet: &[u8]) -> Vec<WeatherForecastSample> {
    let count = packet
        .get(NUM_WEATHER_FORECAST_SAMPLES_OFFSET)
        .copied()
        .unwrap_or(0) as usize;
    (0..count.min(MAX_WEATHER_FORECAST_SAMPLES))
        .filter_map(|index| {
            let base = WEATHER_FORECAST_SAMPLES_OFFSET + index * WEATHER_FORECAST_SAMPLE_SIZE;
            let bytes = packet.get(base..base + WEATHER_FORECAST_SAMPLE_SIZE)?;
            Some(WeatherForecastSample {
                session_type: bytes[0],
                time_offset_min: bytes[1],
                weather: bytes[2],
                track_temp_c: bytes[3] as i8,
                track_temp_change: bytes[4] as i8,
                air_temp_c: bytes[5] as i8,
                air_temp_change: bytes[6] as i8,
                rain_percentage: bytes[7],
            })
        })
        .collect()
}

fn optional_u8(packet: &[u8], offset: usize) -> Option<u8> {
    packet
        .get(offset)
        .copied()
        .filter(|value| !matches!(value, 0 | 255))
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
        overall_frame_identifier: Some(header.overall_frame_identifier),
        weather: packet[PACKET_HEADER_SIZE],
        total_laps: packet[PACKET_HEADER_SIZE + 3],
        track_length_m: read_u16_le(packet, PACKET_HEADER_SIZE + 4),
        session_type: packet[PACKET_HEADER_SIZE + 6],
        track_id: packet[PACKET_HEADER_SIZE + 7] as i8,
        track_temp_c: packet[PACKET_HEADER_SIZE + 1] as i8,
        air_temp_c: packet[PACKET_HEADER_SIZE + 2] as i8,
        session_time_left_s: read_u16_le(packet, PACKET_HEADER_SIZE + 9),
        pit_speed_limit_kmh: packet[PACKET_HEADER_SIZE + 13],
        safety_car_status: packet[SAFETY_CAR_STATUS_OFFSET],
        marshal_zones: read_marshal_zones(packet),
        weather_forecast_samples: read_weather_forecast_samples(packet),
        pit_stop_window_ideal_lap: optional_u8(packet, PIT_STOP_WINDOW_IDEAL_LAP_OFFSET),
        pit_stop_window_latest_lap: optional_u8(packet, PIT_STOP_WINDOW_LATEST_LAP_OFFSET),
        pit_stop_rejoin_position: optional_u8(packet, PIT_STOP_REJOIN_POSITION_OFFSET),
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
        packet[23..27].copy_from_slice(&45_u32.to_le_bytes());
        packet[PACKET_HEADER_SIZE + 1] = 31;
        packet[PACKET_HEADER_SIZE + 2] = 22;
        packet[PACKET_HEADER_SIZE + 3] = 58;
        packet[PACKET_HEADER_SIZE + 4..PACKET_HEADER_SIZE + 6]
            .copy_from_slice(&5412_u16.to_le_bytes());
        packet[PACKET_HEADER_SIZE + 6] = 15;
        packet[PACKET_HEADER_SIZE + 7] = 7;
        packet[PACKET_HEADER_SIZE + 9..PACKET_HEADER_SIZE + 11]
            .copy_from_slice(&1200_u16.to_le_bytes());
        packet[NUM_MARSHAL_ZONES_OFFSET] = 2;
        packet[MARSHAL_ZONES_OFFSET..MARSHAL_ZONES_OFFSET + 4]
            .copy_from_slice(&0.25_f32.to_le_bytes());
        packet[MARSHAL_ZONES_OFFSET + 4] = 1;
        let second_zone = MARSHAL_ZONES_OFFSET + MARSHAL_ZONE_SIZE;
        packet[second_zone..second_zone + 4].copy_from_slice(&0.5_f32.to_le_bytes());
        packet[second_zone + 4] = 3;

        assert_eq!(
            parse_session_sample(&packet).unwrap(),
            SessionSample {
                session_time: 7.25,
                frame_identifier: 44,
                overall_frame_identifier: Some(45),
                weather: 0,
                total_laps: 58,
                track_length_m: 5412,
                session_type: 15,
                track_id: 7,
                track_temp_c: 31,
                air_temp_c: 22,
                session_time_left_s: 1200,
                pit_speed_limit_kmh: 0,
                safety_car_status: 0,
                marshal_zones: vec![
                    MarshalZoneSample {
                        start: 0.25,
                        flag: 1,
                    },
                    MarshalZoneSample {
                        start: 0.5,
                        flag: 3,
                    },
                ],
                weather_forecast_samples: Vec::new(),
                pit_stop_window_ideal_lap: None,
                pit_stop_window_latest_lap: None,
                pit_stop_rejoin_position: None,
            }
        );
    }

    #[test]
    fn parses_race_control_weather_and_pit_strategy_fields() {
        let mut packet = vec![0_u8; PIT_STOP_REJOIN_POSITION_OFFSET + 1];
        packet[0..2].copy_from_slice(&F1_25_PACKET_FORMAT.to_le_bytes());
        packet[2] = 25;
        packet[6] = packet_id::SESSION;
        packet[PACKET_HEADER_SIZE] = 2;
        packet[PACKET_HEADER_SIZE + 3] = 57;
        packet[PACKET_HEADER_SIZE + 6] = 15;
        packet[PACKET_HEADER_SIZE + 13] = 80;
        packet[SAFETY_CAR_STATUS_OFFSET] = 2;
        packet[NUM_WEATHER_FORECAST_SAMPLES_OFFSET] = 1;
        let forecast = WEATHER_FORECAST_SAMPLES_OFFSET;
        packet[forecast] = 15;
        packet[forecast + 1] = 5;
        packet[forecast + 2] = 3;
        packet[forecast + 3] = 28;
        packet[forecast + 4] = 1;
        packet[forecast + 5] = 22;
        packet[forecast + 6] = 1;
        packet[forecast + 7] = 75;
        packet[PIT_STOP_WINDOW_IDEAL_LAP_OFFSET] = 18;
        packet[PIT_STOP_WINDOW_LATEST_LAP_OFFSET] = 21;
        packet[PIT_STOP_REJOIN_POSITION_OFFSET] = 6;

        let sample = parse_session_sample(&packet).unwrap();
        assert_eq!(sample.weather, 2);
        assert_eq!(sample.pit_speed_limit_kmh, 80);
        assert_eq!(sample.safety_car_status, 2);
        assert_eq!(sample.pit_stop_window_ideal_lap, Some(18));
        assert_eq!(sample.pit_stop_window_latest_lap, Some(21));
        assert_eq!(sample.pit_stop_rejoin_position, Some(6));
        assert_eq!(sample.weather_forecast_samples.len(), 1);
        assert_eq!(sample.weather_forecast_samples[0].rain_percentage, 75);
    }

    #[test]
    fn rejects_session_without_safety_car_status() {
        let packet = vec![0_u8; SAFETY_CAR_STATUS_OFFSET];
        assert!(parse_session_sample(&packet).is_err());
    }
}
