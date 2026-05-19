use super::constants::{MAX_CARS, PACKET_HEADER_SIZE};

pub const CAR_DAMAGE_DATA_SIZE: usize = 46;
pub const CAR_DAMAGE_PACKET_SIZE: usize = PACKET_HEADER_SIZE + MAX_CARS * CAR_DAMAGE_DATA_SIZE;

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

#[cfg(test)]
mod tests {
    use super::*;

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
}
