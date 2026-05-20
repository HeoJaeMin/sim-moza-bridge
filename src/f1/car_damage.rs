use super::constants::{F1_24_PACKET_FORMAT, MAX_CARS, PACKET_HEADER_SIZE};
use super::header::parse_packet_header;
use crate::telemetry::{DamageSample, WheelValuesF32, WheelValuesU8};

pub const CAR_DAMAGE_DATA_SIZE: usize = 46;
pub const CAR_DAMAGE_PACKET_SIZE: usize = PACKET_HEADER_SIZE + MAX_CARS * CAR_DAMAGE_DATA_SIZE;
pub const F1_24_CAR_DAMAGE_DATA_SIZE: usize = 42;
pub const F1_24_CAR_DAMAGE_PACKET_SIZE: usize =
    PACKET_HEADER_SIZE + MAX_CARS * F1_24_CAR_DAMAGE_DATA_SIZE;

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TyreWearByCorner {
    pub fl: f32,
    pub fr: f32,
    pub rl: f32,
    pub rr: f32,
}

pub fn is_car_damage_packet_size(packet: &[u8]) -> bool {
    packet.len() == CAR_DAMAGE_PACKET_SIZE
}

pub fn car_damage_offset(car_index: usize) -> Result<usize, String> {
    if car_index >= MAX_CARS {
        return Err(format!("car_index must be between 0 and {}", MAX_CARS - 1));
    }

    Ok(PACKET_HEADER_SIZE + car_index * CAR_DAMAGE_DATA_SIZE)
}

fn read_f32_le(packet: &[u8], offset: usize) -> f32 {
    f32::from_le_bytes(
        packet[offset..offset + 4]
            .try_into()
            .expect("valid f32 offset"),
    )
}

fn read_u8_wheels(packet: &[u8], offset: usize) -> WheelValuesU8 {
    WheelValuesU8 {
        rl: packet[offset],
        rr: packet[offset + 1],
        fl: packet[offset + 2],
        fr: packet[offset + 3],
    }
}

fn write_f32_le(packet: &mut [u8], offset: usize, value: f32) {
    packet[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

#[allow(dead_code)]
pub fn read_f1_tyre_wear(packet: &[u8], car_index: usize) -> Result<TyreWearByCorner, String> {
    let base = car_damage_offset(car_index)?;
    if packet.len() < base + 16 {
        return Err("packet is too short for F1 tyre wear data".to_owned());
    }

    Ok(TyreWearByCorner {
        rl: read_f32_le(packet, base),
        rr: read_f32_le(packet, base + 4),
        fl: read_f32_le(packet, base + 8),
        fr: read_f32_le(packet, base + 12),
    })
}

pub fn parse_player_damage_sample(packet: &[u8]) -> Result<DamageSample, String> {
    let header = parse_packet_header(packet)
        .ok_or_else(|| "packet is too short for F1 header".to_owned())?;
    let car_index = header.player_car_index as usize;
    let base = car_damage_offset(car_index)?;

    if packet.len() < base + CAR_DAMAGE_DATA_SIZE || !is_car_damage_packet_size(packet) {
        return Err("packet is too short for F1 car damage data".to_owned());
    }

    Ok(DamageSample {
        session_time: header.session_time,
        frame_identifier: header.frame_identifier,
        player_car_index: header.player_car_index,
        tyre_wear: WheelValuesF32 {
            rl: read_f32_le(packet, base),
            rr: read_f32_le(packet, base + 4),
            fl: read_f32_le(packet, base + 8),
            fr: read_f32_le(packet, base + 12),
        },
        tyre_damage: read_u8_wheels(packet, base + 16),
        tyre_blisters: read_u8_wheels(packet, base + 24),
        front_left_wing_damage: packet[base + 28],
        front_right_wing_damage: packet[base + 29],
        rear_wing_damage: packet[base + 30],
        gearbox_damage: packet[base + 36],
        engine_damage: packet[base + 37],
    })
}

pub fn rewrite_tyre_wear_to_moza_named_order(
    packet: &mut [u8],
    car_index: usize,
) -> Result<(), String> {
    let base = car_damage_offset(car_index)?;
    if packet.len() < base + 16 {
        return Err("packet is too short for F1 tyre wear data".to_owned());
    }

    let raw_rl = read_f32_le(packet, base);
    let raw_rr = read_f32_le(packet, base + 4);
    let raw_fl = read_f32_le(packet, base + 8);
    let raw_fr = read_f32_le(packet, base + 12);

    write_f32_le(packet, base, raw_fl);
    write_f32_le(packet, base + 4, raw_fr);
    write_f32_le(packet, base + 8, raw_rl);
    write_f32_le(packet, base + 12, raw_rr);
    Ok(())
}

pub fn rewrite_all_tyre_wear_to_moza_named_order(packet: &mut [u8]) -> bool {
    if !is_car_damage_packet_size(packet) {
        return false;
    }

    for car_index in 0..MAX_CARS {
        rewrite_tyre_wear_to_moza_named_order(packet, car_index)
            .expect("packet size was already validated");
    }
    true
}

pub fn to_f1_24_car_damage_compat_packet(packet: &[u8]) -> Option<Vec<u8>> {
    if !is_car_damage_packet_size(packet) {
        return None;
    }

    let mut compat = Vec::with_capacity(F1_24_CAR_DAMAGE_PACKET_SIZE);
    compat.extend_from_slice(&packet[..PACKET_HEADER_SIZE]);
    compat[0..2].copy_from_slice(&F1_24_PACKET_FORMAT.to_le_bytes());
    compat[2] = 24;

    for car_index in 0..MAX_CARS {
        let base = PACKET_HEADER_SIZE + car_index * CAR_DAMAGE_DATA_SIZE;
        compat.extend_from_slice(&packet[base..base + 24]);
        compat.extend_from_slice(&packet[base + 28..base + CAR_DAMAGE_DATA_SIZE]);
    }

    debug_assert_eq!(compat.len(), F1_24_CAR_DAMAGE_PACKET_SIZE);
    Some(compat)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::f1::constants::F1_25_PACKET_FORMAT;

    fn make_car_damage_packet() -> Vec<u8> {
        vec![0_u8; CAR_DAMAGE_PACKET_SIZE]
    }

    fn write_wear(packet: &mut [u8], car_index: usize, wear: [f32; 4]) {
        let base = car_damage_offset(car_index).unwrap();
        write_f32_le(packet, base, wear[0]);
        write_f32_le(packet, base + 4, wear[1]);
        write_f32_le(packet, base + 8, wear[2]);
        write_f32_le(packet, base + 12, wear[3]);
    }

    fn read_raw_wear(packet: &[u8], car_index: usize) -> [f32; 4] {
        let base = car_damage_offset(car_index).unwrap();
        [
            read_f32_le(packet, base),
            read_f32_le(packet, base + 4),
            read_f32_le(packet, base + 8),
            read_f32_le(packet, base + 12),
        ]
    }

    #[test]
    fn maps_f1_tyre_order_to_named_corners() {
        let mut packet = make_car_damage_packet();
        write_wear(&mut packet, 0, [11.0, 22.0, 33.0, 44.0]);

        assert_eq!(
            read_f1_tyre_wear(&packet, 0).unwrap(),
            TyreWearByCorner {
                rl: 11.0,
                rr: 22.0,
                fl: 33.0,
                fr: 44.0,
            }
        );
    }

    #[test]
    fn parses_player_damage_sample() {
        let mut packet = make_car_damage_packet();
        packet[0..2].copy_from_slice(&crate::f1::constants::F1_25_PACKET_FORMAT.to_le_bytes());
        packet[2] = 25;
        packet[6] = crate::f1::constants::packet_id::CAR_DAMAGE;
        packet[15..19].copy_from_slice(&8.0_f32.to_le_bytes());
        packet[19..23].copy_from_slice(&90_u32.to_le_bytes());
        packet[27] = 2;

        write_wear(&mut packet, 2, [10.0, 20.0, 30.0, 40.0]);
        let base = car_damage_offset(2).unwrap();
        packet[base + 16..base + 20].copy_from_slice(&[1, 2, 3, 4]);
        packet[base + 24..base + 28].copy_from_slice(&[5, 6, 7, 8]);
        packet[base + 28] = 9;
        packet[base + 29] = 10;
        packet[base + 30] = 11;
        packet[base + 36] = 12;
        packet[base + 37] = 13;

        let sample = parse_player_damage_sample(&packet).unwrap();

        assert_eq!(sample.player_car_index, 2);
        assert_eq!(sample.tyre_wear.rl, 10.0);
        assert_eq!(sample.tyre_wear.fr, 40.0);
        assert_eq!(sample.tyre_damage.fl, 3);
        assert_eq!(sample.tyre_blisters.rr, 6);
        assert_eq!(sample.front_left_wing_damage, 9);
        assert_eq!(sample.rear_wing_damage, 11);
        assert_eq!(sample.gearbox_damage, 12);
        assert_eq!(sample.engine_damage, 13);
    }

    #[test]
    fn rewrites_one_car_to_dashboard_order() {
        let mut packet = make_car_damage_packet();
        write_wear(&mut packet, 2, [11.0, 22.0, 33.0, 44.0]);

        rewrite_tyre_wear_to_moza_named_order(&mut packet, 2).unwrap();

        assert_eq!(read_raw_wear(&packet, 2), [33.0, 44.0, 11.0, 22.0]);
    }

    #[test]
    fn rewrites_all_cars_and_validates_packet_size() {
        let mut packet = make_car_damage_packet();
        write_wear(&mut packet, 0, [10.0, 20.0, 30.0, 40.0]);
        write_wear(&mut packet, 21, [1.0, 2.0, 3.0, 4.0]);

        assert!(rewrite_all_tyre_wear_to_moza_named_order(&mut packet));
        assert_eq!(read_raw_wear(&packet, 0), [30.0, 40.0, 10.0, 20.0]);
        assert_eq!(read_raw_wear(&packet, 21), [3.0, 4.0, 1.0, 2.0]);
        assert!(!rewrite_all_tyre_wear_to_moza_named_order(&mut vec![
            0_u8;
            29
        ]));
    }

    #[test]
    fn rejects_invalid_car_indexes() {
        assert!(car_damage_offset(22).is_err());
    }

    #[test]
    fn converts_f1_25_car_damage_to_f1_24_compat_layout() {
        let mut packet = make_car_damage_packet();
        packet[0..2].copy_from_slice(&F1_25_PACKET_FORMAT.to_le_bytes());
        packet[2] = 25;

        let base = car_damage_offset(0).unwrap();
        write_wear(&mut packet, 0, [10.0, 20.0, 30.0, 40.0]);
        packet[base + 16..base + 20].copy_from_slice(&[1, 2, 3, 4]);
        packet[base + 20..base + 24].copy_from_slice(&[5, 6, 7, 8]);
        packet[base + 24..base + 28].copy_from_slice(&[90, 91, 92, 93]);
        packet[base + 28] = 9;
        packet[base + 29] = 10;

        let compat = to_f1_24_car_damage_compat_packet(&packet).unwrap();

        assert_eq!(compat.len(), F1_24_CAR_DAMAGE_PACKET_SIZE);
        assert_eq!(u16::from_le_bytes(compat[0..2].try_into().unwrap()), 2024);
        assert_eq!(compat[2], 24);
        assert_eq!(read_f32_le(&compat, PACKET_HEADER_SIZE), 10.0);
        assert_eq!(read_f32_le(&compat, PACKET_HEADER_SIZE + 12), 40.0);
        assert_eq!(
            &compat[PACKET_HEADER_SIZE + 16..PACKET_HEADER_SIZE + 20],
            &[1, 2, 3, 4]
        );
        assert_eq!(
            &compat[PACKET_HEADER_SIZE + 20..PACKET_HEADER_SIZE + 24],
            &[5, 6, 7, 8]
        );
        assert_eq!(compat[PACKET_HEADER_SIZE + 24], 9);
        assert_eq!(compat[PACKET_HEADER_SIZE + 25], 10);
        assert!(to_f1_24_car_damage_compat_packet(&vec![0_u8; 29]).is_none());
    }
}
