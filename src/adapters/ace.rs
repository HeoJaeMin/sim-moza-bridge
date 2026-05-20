#![cfg_attr(not(windows), allow(dead_code))]

use crate::config::BridgeConfig;
use crate::telemetry::{
    InputSample, StatusSample, TelemetryUpdate, WheelValuesF32, WheelValuesU8, WheelValuesU16,
};

pub(crate) const ACE_MAPPING_NAME: &str = "Local\\acevo_pmf_physics";
const ACE_PHYSICS_MIN_SIZE: usize = 32;

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

fn parse_ace_update(
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
            brake_temps_c: zero_u16_wheels(),
            tyre_surface_temps_c: zero_u8_wheels(),
            tyre_inner_temps_c: zero_u8_wheels(),
            engine_temp_c: 0,
            tyre_pressures_psi: zero_f32_wheels(),
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

fn clamp_u16(value: f32) -> u16 {
    if !value.is_finite() || value <= 0.0 {
        0
    } else {
        value.round().min(u16::MAX as f32) as u16
    }
}

fn zero_f32_wheels() -> WheelValuesF32 {
    WheelValuesF32 {
        rl: 0.0,
        rr: 0.0,
        fl: 0.0,
        fr: 0.0,
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

fn zero_u16_wheels() -> WheelValuesU16 {
    WheelValuesU16 {
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
        let status = update.status.unwrap();

        assert_eq!(input.frame_identifier, 42);
        assert_eq!(input.speed_kmh, 238);
        assert_eq!(input.gear, 3);
        assert_eq!(input.rpm, 7200);
        assert!((input.throttle - 0.72).abs() < 0.001);
        assert!((input.brake - 0.35).abs() < 0.001);
        assert!((input.steer + 0.2).abs() < 0.001);
        assert!((status.fuel_in_tank - 48.5).abs() < 0.001);
    }

    fn write_f32_le(bytes: &mut [u8], offset: usize, value: f32) {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn write_i32_le(bytes: &mut [u8], offset: usize, value: i32) {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
}
