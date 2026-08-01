use super::constants::{
    F1_25_2026_SEASON_PACKET_FORMAT, MAX_CARS, PACKET_HEADER_SIZE, max_cars_for_format,
};
use super::header::parse_packet_header;
use crate::telemetry::{InputSample, WheelValuesF32, WheelValuesU8, WheelValuesU16};

pub const CAR_TELEMETRY_DATA_SIZE: usize = 60;
pub const CAR_TELEMETRY_DATA_SIZE_2026: usize = 59;
pub const CAR_TELEMETRY_PACKET_EXTRA_SIZE: usize = 3;
pub const CAR_TELEMETRY_PACKET_SIZE: usize =
    PACKET_HEADER_SIZE + MAX_CARS * CAR_TELEMETRY_DATA_SIZE + CAR_TELEMETRY_PACKET_EXTRA_SIZE;

pub fn car_telemetry_offset(car_index: usize) -> Result<usize, String> {
    car_telemetry_offset_for(car_index, MAX_CARS, CAR_TELEMETRY_DATA_SIZE)
}

fn car_telemetry_offset_for(
    car_index: usize,
    max_cars: usize,
    data_size: usize,
) -> Result<usize, String> {
    if car_index >= max_cars {
        return Err(format!("car_index must be between 0 and {}", max_cars - 1));
    }

    Ok(PACKET_HEADER_SIZE + car_index * data_size)
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

fn read_i8(packet: &[u8], offset: usize) -> i8 {
    packet[offset] as i8
}

fn read_u8_wheels(packet: &[u8], offset: usize) -> WheelValuesU8 {
    WheelValuesU8 {
        rl: packet[offset],
        rr: packet[offset + 1],
        fl: packet[offset + 2],
        fr: packet[offset + 3],
    }
}

fn read_u16_wheels(packet: &[u8], offset: usize) -> WheelValuesU16 {
    WheelValuesU16 {
        rl: read_u16_le(packet, offset),
        rr: read_u16_le(packet, offset + 2),
        fl: read_u16_le(packet, offset + 4),
        fr: read_u16_le(packet, offset + 6),
    }
}

fn read_f32_wheels(packet: &[u8], offset: usize) -> WheelValuesF32 {
    WheelValuesF32 {
        rl: read_f32_le(packet, offset),
        rr: read_f32_le(packet, offset + 4),
        fl: read_f32_le(packet, offset + 8),
        fr: read_f32_le(packet, offset + 12),
    }
}

pub fn parse_player_input_sample(packet: &[u8]) -> Result<InputSample, String> {
    let header = parse_packet_header(packet)
        .ok_or_else(|| "packet is too short for F1 header".to_owned())?;
    let max_cars = max_cars_for_format(header.packet_format)
        .ok_or_else(|| format!("unsupported F1 packet format {}", header.packet_format))?;
    let data_size = if header.packet_format == F1_25_2026_SEASON_PACKET_FORMAT {
        CAR_TELEMETRY_DATA_SIZE_2026
    } else {
        CAR_TELEMETRY_DATA_SIZE
    };
    let car_index = header.player_car_index as usize;
    let base = car_telemetry_offset_for(car_index, max_cars, data_size)?;
    let packet_size = PACKET_HEADER_SIZE + max_cars * data_size + CAR_TELEMETRY_PACKET_EXTRA_SIZE;

    if packet.len() < base + data_size || packet.len() < packet_size {
        return Err("packet is too short for F1 car telemetry data".to_owned());
    }
    let (engine_temp_c, tyre_pressure_offset) =
        if header.packet_format == F1_25_2026_SEASON_PACKET_FORMAT {
            (packet[base + 38] as u16, base + 39)
        } else {
            (read_u16_le(packet, base + 38), base + 40)
        };

    Ok(InputSample {
        session_time: header.session_time,
        frame_identifier: header.frame_identifier,
        player_car_index: header.player_car_index,
        speed_kmh: read_u16_le(packet, base),
        throttle: read_f32_le(packet, base + 2),
        steer: read_f32_le(packet, base + 6),
        brake: read_f32_le(packet, base + 10),
        clutch: packet[base + 14],
        gear: read_i8(packet, base + 15),
        rpm: read_u16_le(packet, base + 16),
        drs: packet[base + 18] != 0,
        rev_lights_percent: packet[base + 19],
        rev_lights_bit_value: read_u16_le(packet, base + 20),
        brake_temps_c: read_u16_wheels(packet, base + 22),
        tyre_surface_temps_c: read_u8_wheels(packet, base + 30),
        tyre_inner_temps_c: read_u8_wheels(packet, base + 34),
        engine_temp_c,
        tyre_pressures_psi: read_f32_wheels(packet, tyre_pressure_offset),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::f1::constants::{
        F1_25_2026_SEASON_PACKET_FORMAT, F1_25_PACKET_FORMAT, MAX_CARS_2026, packet_id,
    };

    #[test]
    fn parses_player_input_sample() {
        let mut packet = vec![0_u8; CAR_TELEMETRY_PACKET_SIZE];
        packet[0..2].copy_from_slice(&F1_25_PACKET_FORMAT.to_le_bytes());
        packet[2] = 25;
        packet[6] = packet_id::CAR_TELEMETRY;
        packet[15..19].copy_from_slice(&12.5_f32.to_le_bytes());
        packet[19..23].copy_from_slice(&77_u32.to_le_bytes());
        packet[27] = 2;

        let base = car_telemetry_offset(2).unwrap();
        packet[base..base + 2].copy_from_slice(&286_u16.to_le_bytes());
        packet[base + 2..base + 6].copy_from_slice(&0.75_f32.to_le_bytes());
        packet[base + 6..base + 10].copy_from_slice(&(-0.125_f32).to_le_bytes());
        packet[base + 10..base + 14].copy_from_slice(&0.25_f32.to_le_bytes());
        packet[base + 14] = 9_u8;
        packet[base + 15] = 7_u8;
        packet[base + 16..base + 18].copy_from_slice(&11750_u16.to_le_bytes());
        packet[base + 18] = 1;
        packet[base + 19] = 83;
        packet[base + 20..base + 22].copy_from_slice(&0b101_u16.to_le_bytes());
        packet[base + 22..base + 24].copy_from_slice(&410_u16.to_le_bytes());
        packet[base + 24..base + 26].copy_from_slice(&411_u16.to_le_bytes());
        packet[base + 26..base + 28].copy_from_slice(&412_u16.to_le_bytes());
        packet[base + 28..base + 30].copy_from_slice(&413_u16.to_le_bytes());
        packet[base + 30..base + 34].copy_from_slice(&[91, 92, 93, 94]);
        packet[base + 34..base + 38].copy_from_slice(&[101, 102, 103, 104]);
        packet[base + 38..base + 40].copy_from_slice(&104_u16.to_le_bytes());
        packet[base + 40..base + 44].copy_from_slice(&21.1_f32.to_le_bytes());
        packet[base + 44..base + 48].copy_from_slice(&21.2_f32.to_le_bytes());
        packet[base + 48..base + 52].copy_from_slice(&22.1_f32.to_le_bytes());
        packet[base + 52..base + 56].copy_from_slice(&22.2_f32.to_le_bytes());

        assert_eq!(
            parse_player_input_sample(&packet).unwrap(),
            InputSample {
                session_time: 12.5,
                frame_identifier: 77,
                player_car_index: 2,
                throttle: 0.75,
                steer: -0.125,
                brake: 0.25,
                clutch: 9,
                speed_kmh: 286,
                gear: 7,
                rpm: 11750,
                drs: true,
                rev_lights_percent: 83,
                rev_lights_bit_value: 0b101,
                brake_temps_c: WheelValuesU16 {
                    rl: 410,
                    rr: 411,
                    fl: 412,
                    fr: 413,
                },
                tyre_surface_temps_c: WheelValuesU8 {
                    rl: 91,
                    rr: 92,
                    fl: 93,
                    fr: 94,
                },
                tyre_inner_temps_c: WheelValuesU8 {
                    rl: 101,
                    rr: 102,
                    fl: 103,
                    fr: 104,
                },
                engine_temp_c: 104,
                tyre_pressures_psi: WheelValuesF32 {
                    rl: 21.1,
                    rr: 21.2,
                    fl: 22.1,
                    fr: 22.2,
                },
            }
        );
    }

    #[test]
    fn rejects_short_packets() {
        assert!(parse_player_input_sample(&[1, 2, 3]).is_err());
    }

    #[test]
    fn rejects_packets_missing_packet_level_tail() {
        let packet = vec![0_u8; CAR_TELEMETRY_PACKET_SIZE - 1];

        assert!(parse_player_input_sample(&packet).is_err());
    }

    #[test]
    fn parses_2026_season_car_telemetry_layout() {
        let packet_size = PACKET_HEADER_SIZE
            + MAX_CARS_2026 * CAR_TELEMETRY_DATA_SIZE_2026
            + CAR_TELEMETRY_PACKET_EXTRA_SIZE;
        let mut packet = vec![0_u8; packet_size];
        packet[0..2].copy_from_slice(&F1_25_2026_SEASON_PACKET_FORMAT.to_le_bytes());
        packet[2] = 25;
        packet[6] = packet_id::CAR_TELEMETRY;
        packet[15..19].copy_from_slice(&25.0_f32.to_le_bytes());
        packet[27] = 23;
        let base =
            car_telemetry_offset_for(23, MAX_CARS_2026, CAR_TELEMETRY_DATA_SIZE_2026).unwrap();
        packet[base..base + 2].copy_from_slice(&321_u16.to_le_bytes());
        packet[base + 2..base + 6].copy_from_slice(&0.95_f32.to_le_bytes());
        packet[base + 15] = 8;
        packet[base + 16..base + 18].copy_from_slice(&12_345_u16.to_le_bytes());
        packet[base + 38] = 107;
        packet[base + 39..base + 43].copy_from_slice(&21.1_f32.to_le_bytes());
        packet[base + 43..base + 47].copy_from_slice(&21.2_f32.to_le_bytes());
        packet[base + 47..base + 51].copy_from_slice(&22.1_f32.to_le_bytes());
        packet[base + 51..base + 55].copy_from_slice(&22.2_f32.to_le_bytes());

        let sample = parse_player_input_sample(&packet).unwrap();
        assert_eq!(sample.player_car_index, 23);
        assert_eq!(sample.speed_kmh, 321);
        assert_eq!(sample.rpm, 12_345);
        assert_eq!(sample.engine_temp_c, 107);
        assert_eq!(sample.tyre_pressures_psi.fr, 22.2);
    }
}
