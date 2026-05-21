#![cfg_attr(not(windows), allow(dead_code))]

use crate::config::BridgeConfig;
use crate::telemetry::{
    DamageSample, InputSample, StatusSample, TelemetryUpdate, WheelValuesF32, WheelValuesU8,
    WheelValuesU16,
};

pub(crate) const ACE_MAPPING_NAME: &str = "Local\\acevo_pmf_physics";
const WHEEL_PRESSURE_OFFSET: usize = 88;
const TYRE_CORE_TEMP_OFFSET: usize = 152;
const BRAKE_TEMP_OFFSET: usize = 348;
const TYRE_TEMP_INNER_OFFSET: usize = 368;
const TYRE_TEMP_MIDDLE_OFFSET: usize = 384;
const TYRE_TEMP_OUTER_OFFSET: usize = 400;
const UNKNOWN_TYRE_WEAR_PERCENT: f32 = -1.0;
pub(crate) const ACE_PHYSICS_MIN_SIZE: usize = TYRE_TEMP_OUTER_OFFSET + 16;

pub fn start_ace_adapter(config: BridgeConfig) -> Result<(), String> {
    #[cfg(not(windows))]
    {
        let _ = config;
        return Err(format!(
            "Assetto Corsa EVO adapter requires Windows shared memory ({ACE_MAPPING_NAME}); run this on the game PC"
        ));
    }

    #[cfg(windows)]
    {
        super::run_shared_memory_adapter(
            config,
            "Assetto Corsa EVO",
            ACE_MAPPING_NAME,
            ACE_PHYSICS_MIN_SIZE,
            parse_ace_update,
        )
    }
}

pub(crate) fn parse_ace_update(
    snapshot: &[u8],
    frame_identifier: u32,
) -> Result<Option<TelemetryUpdate>, String> {
    if snapshot.len() < ACE_PHYSICS_MIN_SIZE {
        return Err("ACE physics snapshot is too short".to_owned());
    }

    let fuel_in_tank = read_f32_le(snapshot, 12)?;
    let raw_gear = read_i32_le(snapshot, 16)?;
    let rpm = read_i32_le(snapshot, 20)?;

    Ok(Some(TelemetryUpdate {
        input: Some(InputSample {
            session_time: 0.0,
            frame_identifier,
            player_car_index: 0,
            throttle: clamp_unit(read_f32_le(snapshot, 4)?),
            steer: read_f32_le(snapshot, 24)?.clamp(-1.0, 1.0),
            brake: clamp_unit(read_f32_le(snapshot, 8)?),
            clutch: 0,
            speed_kmh: clamp_u16(read_f32_le(snapshot, 28)?),
            gear: ace_gear(raw_gear),
            rpm: clamp_u16(rpm as f32),
            drs: false,
            rev_lights_percent: 0,
            rev_lights_bit_value: 0,
            brake_temps_c: read_brake_temps(snapshot)?,
            tyre_surface_temps_c: read_surface_temps(snapshot)?,
            tyre_inner_temps_c: read_inner_temps(snapshot)?,
            engine_temp_c: 0,
            tyre_pressures_psi: read_pressures(snapshot)?,
        }),
        damage: Some(DamageSample {
            session_time: 0.0,
            frame_identifier,
            player_car_index: 0,
            tyre_wear: unknown_tyre_wear_wheels(),
            tyre_damage: zero_u8_wheels(),
            tyre_blisters: zero_u8_wheels(),
            front_left_wing_damage: 0,
            front_right_wing_damage: 0,
            rear_wing_damage: 0,
            gearbox_damage: 0,
            engine_damage: 0,
        }),
        status: Some(StatusSample {
            session_time: 0.0,
            frame_identifier,
            player_car_index: 0,
            traction_control: 0,
            anti_lock_brakes: 0,
            front_brake_bias: 0,
            fuel_in_tank: finite_or_zero(fuel_in_tank),
            fuel_capacity: 0.0,
            fuel_remaining_laps: 0.0,
            max_rpm: 0,
            idle_rpm: 0,
            max_gears: 0,
            drs_allowed: false,
            drs_activation_distance_m: 0,
            pit_limiter_active: false,
            actual_tyre_compound: 0,
            visual_tyre_compound: 0,
            tyres_age_laps: 0,
            ers_store_energy: 0.0,
            ers_deploy_mode: 0,
            ers_deployed_this_lap: 0.0,
        }),
        ..TelemetryUpdate::default()
    }))
}

fn read_f32_le(bytes: &[u8], offset: usize) -> Result<f32, String> {
    bytes
        .get(offset..offset + 4)
        .and_then(|value| value.try_into().ok())
        .map(f32::from_le_bytes)
        .ok_or_else(|| format!("snapshot is too short for f32 at {offset}"))
}

fn read_i32_le(bytes: &[u8], offset: usize) -> Result<i32, String> {
    bytes
        .get(offset..offset + 4)
        .and_then(|value| value.try_into().ok())
        .map(i32::from_le_bytes)
        .ok_or_else(|| format!("snapshot is too short for i32 at {offset}"))
}

fn ace_gear(raw_gear: i32) -> i8 {
    (raw_gear - 1).clamp(-1, 12) as i8
}

fn read_brake_temps(bytes: &[u8]) -> Result<WheelValuesU16, String> {
    read_wheel_u16(bytes, BRAKE_TEMP_OFFSET, |value| value)
}

fn read_surface_temps(bytes: &[u8]) -> Result<WheelValuesU8, String> {
    read_wheel_u8_by_index(|wheel| {
        let inner = read_f32_le(bytes, TYRE_TEMP_INNER_OFFSET + wheel * 4)?;
        let middle = read_f32_le(bytes, TYRE_TEMP_MIDDLE_OFFSET + wheel * 4)?;
        let outer = read_f32_le(bytes, TYRE_TEMP_OUTER_OFFSET + wheel * 4)?;
        Ok((inner + middle + outer) / 3.0)
    })
}

fn read_inner_temps(bytes: &[u8]) -> Result<WheelValuesU8, String> {
    read_wheel_u8(bytes, TYRE_CORE_TEMP_OFFSET, |value| value)
}

fn read_pressures(bytes: &[u8]) -> Result<WheelValuesF32, String> {
    read_wheel_f32(bytes, WHEEL_PRESSURE_OFFSET, finite_nonnegative)
}

fn read_wheel_f32<F>(bytes: &[u8], offset: usize, convert: F) -> Result<WheelValuesF32, String>
where
    F: Fn(f32) -> f32,
{
    Ok(WheelValuesF32 {
        fl: convert(read_f32_le(bytes, offset)?),
        fr: convert(read_f32_le(bytes, offset + 4)?),
        rl: convert(read_f32_le(bytes, offset + 8)?),
        rr: convert(read_f32_le(bytes, offset + 12)?),
    })
}

fn read_wheel_u8<F>(bytes: &[u8], offset: usize, convert: F) -> Result<WheelValuesU8, String>
where
    F: Fn(f32) -> f32,
{
    read_wheel_u8_by_index(|wheel| read_f32_le(bytes, offset + wheel * 4).map(&convert))
}

fn read_wheel_u8_by_index<F>(read: F) -> Result<WheelValuesU8, String>
where
    F: Fn(usize) -> Result<f32, String>,
{
    Ok(WheelValuesU8 {
        fl: clamp_u8(read(0)?),
        fr: clamp_u8(read(1)?),
        rl: clamp_u8(read(2)?),
        rr: clamp_u8(read(3)?),
    })
}

fn read_wheel_u16<F>(bytes: &[u8], offset: usize, convert: F) -> Result<WheelValuesU16, String>
where
    F: Fn(f32) -> f32,
{
    Ok(WheelValuesU16 {
        fl: clamp_u16(convert(read_f32_le(bytes, offset)?)),
        fr: clamp_u16(convert(read_f32_le(bytes, offset + 4)?)),
        rl: clamp_u16(convert(read_f32_le(bytes, offset + 8)?)),
        rr: clamp_u16(convert(read_f32_le(bytes, offset + 12)?)),
    })
}

fn clamp_unit(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

fn finite_or_zero(value: f32) -> f32 {
    if value.is_finite() { value } else { 0.0 }
}

fn finite_nonnegative(value: f32) -> f32 {
    if value.is_finite() && value > 0.0 {
        value
    } else {
        0.0
    }
}

fn unknown_tyre_wear_wheels() -> WheelValuesF32 {
    WheelValuesF32 {
        rl: UNKNOWN_TYRE_WEAR_PERCENT,
        rr: UNKNOWN_TYRE_WEAR_PERCENT,
        fl: UNKNOWN_TYRE_WEAR_PERCENT,
        fr: UNKNOWN_TYRE_WEAR_PERCENT,
    }
}

fn clamp_u8(value: f32) -> u8 {
    if !value.is_finite() || value <= 0.0 {
        0
    } else {
        value.round().min(u8::MAX as f32) as u8
    }
}

fn clamp_u16(value: f32) -> u16 {
    if !value.is_finite() || value <= 0.0 {
        0
    } else {
        value.round().min(u16::MAX as f32) as u16
    }
}

fn zero_u8_wheels() -> WheelValuesU8 {
    WheelValuesU8 {
        rl: 0,
        rr: 0,
        fl: 0,
        fr: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal_ace_physics_snapshot() {
        let mut snapshot = vec![0_u8; ACE_PHYSICS_MIN_SIZE];
        write_f32_le(&mut snapshot, 4, 0.72);
        write_f32_le(&mut snapshot, 8, 0.35);
        write_f32_le(&mut snapshot, 12, 48.5);
        write_i32_le(&mut snapshot, 16, 4);
        write_i32_le(&mut snapshot, 20, 7200);
        write_f32_le(&mut snapshot, 24, -0.2);
        write_f32_le(&mut snapshot, 28, 238.4);

        let update = parse_ace_update(&snapshot, 42).unwrap().unwrap();
        let input = update.input.unwrap();
        let damage = update.damage.unwrap();
        let status = update.status.unwrap();

        assert_eq!(input.frame_identifier, 42);
        assert_eq!(input.speed_kmh, 238);
        assert_eq!(input.gear, 3);
        assert_eq!(input.rpm, 7200);
        assert!((input.throttle - 0.72).abs() < 0.001);
        assert!((input.brake - 0.35).abs() < 0.001);
        assert!((input.steer + 0.2).abs() < 0.001);
        assert_eq!(input.brake_temps_c.fl, 0);
        assert_eq!(input.tyre_surface_temps_c.fl, 0);
        assert_eq!(input.tyre_inner_temps_c.fl, 0);
        assert_eq!(input.tyre_pressures_psi.fl, 0.0);
        assert_eq!(damage.tyre_wear.fl, UNKNOWN_TYRE_WEAR_PERCENT);
        assert!((status.fuel_in_tank - 48.5).abs() < 0.001);
    }

    #[test]
    fn parses_ace_tyre_channels() {
        let mut snapshot = vec![0_u8; ACE_PHYSICS_MIN_SIZE];
        write_wheel_f32(
            &mut snapshot,
            WHEEL_PRESSURE_OFFSET,
            [25.1, 25.2, 24.8, 24.9],
        );
        write_wheel_f32(
            &mut snapshot,
            TYRE_CORE_TEMP_OFFSET,
            [82.0, 83.0, 79.0, 80.0],
        );
        write_wheel_f32(
            &mut snapshot,
            BRAKE_TEMP_OFFSET,
            [421.0, 419.0, 372.0, 368.0],
        );
        write_wheel_f32(
            &mut snapshot,
            TYRE_TEMP_INNER_OFFSET,
            [72.0, 73.0, 70.0, 71.0],
        );
        write_wheel_f32(
            &mut snapshot,
            TYRE_TEMP_MIDDLE_OFFSET,
            [74.0, 75.0, 72.0, 73.0],
        );
        write_wheel_f32(
            &mut snapshot,
            TYRE_TEMP_OUTER_OFFSET,
            [76.0, 77.0, 74.0, 75.0],
        );

        let update = parse_ace_update(&snapshot, 7).unwrap().unwrap();
        let input = update.input.unwrap();
        let damage = update.damage.unwrap();

        assert!((input.tyre_pressures_psi.fl - 25.1).abs() < 0.001);
        assert!((input.tyre_pressures_psi.fr - 25.2).abs() < 0.001);
        assert!((input.tyre_pressures_psi.rl - 24.8).abs() < 0.001);
        assert!((input.tyre_pressures_psi.rr - 24.9).abs() < 0.001);
        assert_eq!(input.tyre_inner_temps_c.fl, 82);
        assert_eq!(input.tyre_surface_temps_c.fl, 74);
        assert_eq!(input.brake_temps_c.fl, 421);
        assert_eq!(damage.tyre_wear.fl, UNKNOWN_TYRE_WEAR_PERCENT);
        assert_eq!(damage.tyre_wear.fr, UNKNOWN_TYRE_WEAR_PERCENT);
        assert_eq!(damage.tyre_wear.rl, UNKNOWN_TYRE_WEAR_PERCENT);
        assert_eq!(damage.tyre_wear.rr, UNKNOWN_TYRE_WEAR_PERCENT);
    }

    fn write_f32_le(bytes: &mut [u8], offset: usize, value: f32) {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn write_i32_le(bytes: &mut [u8], offset: usize, value: i32) {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn write_wheel_f32(bytes: &mut [u8], offset: usize, values: [f32; 4]) {
        for (index, value) in values.into_iter().enumerate() {
            write_f32_le(bytes, offset + index * 4, value);
        }
    }
}
