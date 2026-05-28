#![cfg_attr(not(windows), allow(dead_code))]

use crate::config::BridgeConfig;
use crate::hud::HudHandle;
use crate::telemetry::{
    DamageSample, InputSample, LapSample, StatusSample, TelemetryUpdate, WheelValuesF32,
    WheelValuesU8, WheelValuesU16,
};

pub(crate) const LMU_MAPPING_NAME: &str = "LMU_Data";
const LMU_MAX_VEHICLES: usize = 104;
const LMU_TELEMETRY_OFFSET: usize = 128_464;
const LMU_PLAYER_INDEX_OFFSET: usize = LMU_TELEMETRY_OFFSET + 1;
const LMU_PLAYER_HAS_VEHICLE_OFFSET: usize = LMU_TELEMETRY_OFFSET + 2;
const LMU_TELEM_INFO_OFFSET: usize = LMU_TELEMETRY_OFFSET + 4;
const LMU_TELEM_INFO_SIZE: usize = 1_888;
pub(crate) const LMU_VIEW_SIZE: usize =
    LMU_TELEM_INFO_OFFSET + LMU_TELEM_INFO_SIZE * LMU_MAX_VEHICLES;

const LOCAL_VEL_OFFSET: usize = 184;
const GEAR_OFFSET: usize = 352;
const ENGINE_RPM_OFFSET: usize = 356;
const ENGINE_WATER_TEMP_OFFSET: usize = 364;
const THROTTLE_OFFSET: usize = 388;
const BRAKE_OFFSET: usize = 396;
const STEERING_OFFSET: usize = 404;
const CLUTCH_OFFSET: usize = 412;
const FUEL_OFFSET: usize = 524;
const ENGINE_MAX_RPM_OFFSET: usize = 532;
const CURRENT_SECTOR_OFFSET: usize = 600;
const SPEED_LIMITER_OFFSET: usize = 604;
const MAX_GEARS_OFFSET: usize = 605;
const FRONT_TYRE_COMPOUND_OFFSET: usize = 606;
const REAR_TYRE_COMPOUND_OFFSET: usize = 607;
const FUEL_CAPACITY_OFFSET: usize = 608;
const REAR_BRAKE_BIAS_OFFSET: usize = 664;
const GAP_CAR_AHEAD_OFFSET: usize = 780;
const GAP_CAR_BEHIND_OFFSET: usize = 784;
const GAP_PLACE_AHEAD_OFFSET: usize = 788;
const GAP_PLACE_BEHIND_OFFSET: usize = 792;
const WHEELS_OFFSET: usize = 848;
const WHEEL_SIZE: usize = 260;
const WHEEL_BRAKE_TEMP_OFFSET: usize = 24;
const WHEEL_PRESSURE_OFFSET: usize = 120;
const WHEEL_TEMP_OFFSET: usize = 128;
const WHEEL_WEAR_OFFSET: usize = 152;
const WHEEL_INNER_TEMP_OFFSET: usize = 212;

#[cfg_attr(any(target_os = "macos", target_os = "windows"), allow(dead_code))]
pub fn start_lmu_adapter(config: BridgeConfig) -> Result<(), String> {
    #[cfg(not(windows))]
    {
        let _ = config;
        return Err(format!(
            "Le Mans Ultimate adapter requires Windows shared memory ({LMU_MAPPING_NAME}); run this on the game PC with LMU shared memory enabled"
        ));
    }

    #[cfg(windows)]
    {
        super::run_shared_memory_adapter(
            config,
            "Le Mans Ultimate",
            LMU_MAPPING_NAME,
            LMU_VIEW_SIZE,
            parse_lmu_update,
        )
    }
}

pub fn start_lmu_adapter_with_hud(
    config: BridgeConfig,
    hud: Option<HudHandle>,
) -> Result<(), String> {
    #[cfg(not(windows))]
    {
        let _ = config;
        let _ = hud;
        return Err(format!(
            "Le Mans Ultimate adapter requires Windows shared memory ({LMU_MAPPING_NAME}); run this on the game PC with LMU shared memory enabled"
        ));
    }

    #[cfg(windows)]
    {
        super::run_shared_memory_adapter_with_hud(
            config,
            "Le Mans Ultimate",
            LMU_MAPPING_NAME,
            LMU_VIEW_SIZE,
            parse_lmu_update,
            hud,
        )
    }
}

pub(crate) fn parse_lmu_update(
    snapshot: &[u8],
    frame_identifier: u32,
) -> Result<Option<TelemetryUpdate>, String> {
    if snapshot.len() < LMU_VIEW_SIZE {
        return Err("LMU shared-memory snapshot is too short".to_owned());
    }
    if snapshot[LMU_PLAYER_HAS_VEHICLE_OFFSET] == 0 {
        return Ok(None);
    }

    let player_index = snapshot[LMU_PLAYER_INDEX_OFFSET] as usize;
    if player_index >= LMU_MAX_VEHICLES {
        return Ok(None);
    }

    let base = LMU_TELEM_INFO_OFFSET + player_index * LMU_TELEM_INFO_SIZE;
    let session_time = finite_f64(read_f64_le(snapshot, base + 12)?) as f32;
    let lap_number = read_i32_le(snapshot, base + 20)?.max(0) as u8;
    let lap_start = finite_f64(read_f64_le(snapshot, base + 24)?);
    let local_vel_x = finite_f64(read_f64_le(snapshot, base + LOCAL_VEL_OFFSET)?);
    let local_vel_y = finite_f64(read_f64_le(snapshot, base + LOCAL_VEL_OFFSET + 8)?);
    let local_vel_z = finite_f64(read_f64_le(snapshot, base + LOCAL_VEL_OFFSET + 16)?);
    let speed_kmh = (local_vel_x.mul_add(local_vel_x, local_vel_y * local_vel_y)
        + local_vel_z * local_vel_z)
        .sqrt()
        * 3.6;
    let raw_gear = read_i32_le(snapshot, base + GEAR_OFFSET)?;
    let rpm = finite_f64(read_f64_le(snapshot, base + ENGINE_RPM_OFFSET)?);
    let fuel = finite_f64(read_f64_le(snapshot, base + FUEL_OFFSET)?);
    let fuel_capacity = finite_f64(read_f64_le(snapshot, base + FUEL_CAPACITY_OFFSET)?);
    let rear_brake_bias = finite_f64(read_f64_le(snapshot, base + REAR_BRAKE_BIAS_OFFSET)?);
    let current_lap_time_ms = match seconds_to_ms(session_time as f64 - lap_start) {
        Some(value) => value,
        None => 0,
    };

    Ok(Some(TelemetryUpdate {
        input: Some(InputSample {
            session_time,
            frame_identifier,
            player_car_index: player_index as u8,
            throttle: clamp_unit(read_f64_le(snapshot, base + THROTTLE_OFFSET)? as f32),
            steer: (read_f64_le(snapshot, base + STEERING_OFFSET)? as f32).clamp(-1.0, 1.0),
            brake: clamp_unit(read_f64_le(snapshot, base + BRAKE_OFFSET)? as f32),
            clutch: percent_u8(read_f64_le(snapshot, base + CLUTCH_OFFSET)?),
            speed_kmh: clamp_u16(speed_kmh),
            gear: raw_gear.clamp(-1, 12) as i8,
            rpm: clamp_u16(rpm),
            drs: false,
            rev_lights_percent: 0,
            rev_lights_bit_value: 0,
            brake_temps_c: read_brake_temps(snapshot, base)?,
            tyre_surface_temps_c: read_surface_temps(snapshot, base)?,
            tyre_inner_temps_c: read_inner_temps(snapshot, base)?,
            engine_temp_c: clamp_u16(read_f64_le(snapshot, base + ENGINE_WATER_TEMP_OFFSET)?),
            tyre_pressures_psi: read_pressures(snapshot, base)?,
        }),
        lap: Some(LapSample {
            session_time,
            frame_identifier,
            player_car_index: player_index as u8,
            last_lap_time_ms: 0,
            current_lap_time_ms,
            lap_distance_m: 0.0,
            total_distance_m: 0.0,
            car_position: 0,
            current_lap_num: lap_number,
            pit_status: 0,
            sector: read_u8(snapshot, base + CURRENT_SECTOR_OFFSET)?,
            current_lap_invalid: false,
            driver_status: 0,
            result_status: 0,
            delta_to_car_in_front_ms: seconds_to_ms(first_valid_gap(snapshot, base, true)?),
            delta_to_car_behind_ms: seconds_to_ms(first_valid_gap(snapshot, base, false)?),
            delta_to_race_leader_ms: None,
            sector1_time_ms: None,
            sector2_time_ms: None,
        }),
        damage: Some(DamageSample {
            session_time,
            frame_identifier,
            player_car_index: player_index as u8,
            tyre_wear: read_tyre_wear(snapshot, base)?,
            tyre_damage: zero_u8_wheels(),
            tyre_blisters: zero_u8_wheels(),
            front_left_wing_damage: 0,
            front_right_wing_damage: 0,
            rear_wing_damage: 0,
            gearbox_damage: 0,
            engine_damage: 0,
        }),
        status: Some(StatusSample {
            session_time,
            frame_identifier,
            player_car_index: player_index as u8,
            traction_control: 0,
            anti_lock_brakes: 0,
            front_brake_bias: rear_bias_to_front_percent(rear_brake_bias),
            fuel_in_tank: fuel as f32,
            fuel_capacity: fuel_capacity as f32,
            fuel_remaining_laps: 0.0,
            max_rpm: clamp_u16(read_f64_le(snapshot, base + ENGINE_MAX_RPM_OFFSET)?),
            idle_rpm: 0,
            max_gears: read_u8(snapshot, base + MAX_GEARS_OFFSET)?,
            drs_allowed: false,
            drs_activation_distance_m: 0,
            pit_limiter_active: read_u8(snapshot, base + SPEED_LIMITER_OFFSET)? != 0,
            actual_tyre_compound: read_u8(snapshot, base + FRONT_TYRE_COMPOUND_OFFSET)?,
            visual_tyre_compound: read_u8(snapshot, base + REAR_TYRE_COMPOUND_OFFSET)?,
            tyres_age_laps: 0,
            ers_store_energy: 0.0,
            ers_deploy_mode: 0,
            ers_deployed_this_lap: 0.0,
        }),
        ..TelemetryUpdate::default()
    }))
}

fn first_valid_gap(snapshot: &[u8], base: usize, ahead: bool) -> Result<f64, String> {
    let offsets = if ahead {
        [GAP_CAR_AHEAD_OFFSET, GAP_PLACE_AHEAD_OFFSET]
    } else {
        [GAP_CAR_BEHIND_OFFSET, GAP_PLACE_BEHIND_OFFSET]
    };

    for offset in offsets {
        let gap = read_f32_le(snapshot, base + offset)? as f64;
        if gap.is_finite() && gap >= 0.0 {
            return Ok(gap);
        }
    }
    Ok(-1.0)
}

fn read_brake_temps(snapshot: &[u8], base: usize) -> Result<WheelValuesU16, String> {
    read_wheel_u16(snapshot, base, WHEEL_BRAKE_TEMP_OFFSET, |value| value)
}

fn read_surface_temps(snapshot: &[u8], base: usize) -> Result<WheelValuesU8, String> {
    read_wheel_u8(snapshot, base, WHEEL_TEMP_OFFSET, |value| {
        average_tyre_temp_c(value)
    })
}

fn read_inner_temps(snapshot: &[u8], base: usize) -> Result<WheelValuesU8, String> {
    read_wheel_u8(snapshot, base, WHEEL_INNER_TEMP_OFFSET, |value| {
        average_tyre_temp_c(value)
    })
}

fn read_pressures(snapshot: &[u8], base: usize) -> Result<WheelValuesF32, String> {
    Ok(WheelValuesF32 {
        fl: pressure_to_psi(read_wheel_f64(snapshot, base, 0, WHEEL_PRESSURE_OFFSET)?),
        fr: pressure_to_psi(read_wheel_f64(snapshot, base, 1, WHEEL_PRESSURE_OFFSET)?),
        rl: pressure_to_psi(read_wheel_f64(snapshot, base, 2, WHEEL_PRESSURE_OFFSET)?),
        rr: pressure_to_psi(read_wheel_f64(snapshot, base, 3, WHEEL_PRESSURE_OFFSET)?),
    })
}

fn read_tyre_wear(snapshot: &[u8], base: usize) -> Result<WheelValuesF32, String> {
    Ok(WheelValuesF32 {
        fl: wear_to_percent(read_wheel_f64(snapshot, base, 0, WHEEL_WEAR_OFFSET)?),
        fr: wear_to_percent(read_wheel_f64(snapshot, base, 1, WHEEL_WEAR_OFFSET)?),
        rl: wear_to_percent(read_wheel_f64(snapshot, base, 2, WHEEL_WEAR_OFFSET)?),
        rr: wear_to_percent(read_wheel_f64(snapshot, base, 3, WHEEL_WEAR_OFFSET)?),
    })
}

fn read_wheel_u16<F>(
    snapshot: &[u8],
    base: usize,
    field_offset: usize,
    convert: F,
) -> Result<WheelValuesU16, String>
where
    F: Fn(f64) -> f64,
{
    Ok(WheelValuesU16 {
        fl: clamp_u16(convert(read_wheel_f64(snapshot, base, 0, field_offset)?)),
        fr: clamp_u16(convert(read_wheel_f64(snapshot, base, 1, field_offset)?)),
        rl: clamp_u16(convert(read_wheel_f64(snapshot, base, 2, field_offset)?)),
        rr: clamp_u16(convert(read_wheel_f64(snapshot, base, 3, field_offset)?)),
    })
}

fn read_wheel_u8<F>(
    snapshot: &[u8],
    base: usize,
    field_offset: usize,
    convert: F,
) -> Result<WheelValuesU8, String>
where
    F: Fn([f64; 3]) -> f64,
{
    Ok(WheelValuesU8 {
        fl: clamp_u8(convert(read_wheel_f64_triplet(
            snapshot,
            base,
            0,
            field_offset,
        )?)),
        fr: clamp_u8(convert(read_wheel_f64_triplet(
            snapshot,
            base,
            1,
            field_offset,
        )?)),
        rl: clamp_u8(convert(read_wheel_f64_triplet(
            snapshot,
            base,
            2,
            field_offset,
        )?)),
        rr: clamp_u8(convert(read_wheel_f64_triplet(
            snapshot,
            base,
            3,
            field_offset,
        )?)),
    })
}

fn read_wheel_f64(
    snapshot: &[u8],
    base: usize,
    wheel_index: usize,
    field_offset: usize,
) -> Result<f64, String> {
    read_f64_le(
        snapshot,
        base + WHEELS_OFFSET + wheel_index * WHEEL_SIZE + field_offset,
    )
}

fn read_wheel_f64_triplet(
    snapshot: &[u8],
    base: usize,
    wheel_index: usize,
    field_offset: usize,
) -> Result<[f64; 3], String> {
    Ok([
        read_wheel_f64(snapshot, base, wheel_index, field_offset)?,
        read_wheel_f64(snapshot, base, wheel_index, field_offset + 8)?,
        read_wheel_f64(snapshot, base, wheel_index, field_offset + 16)?,
    ])
}

fn read_u8(bytes: &[u8], offset: usize) -> Result<u8, String> {
    bytes
        .get(offset)
        .copied()
        .ok_or_else(|| format!("snapshot is too short for u8 at {offset}"))
}

fn read_i32_le(bytes: &[u8], offset: usize) -> Result<i32, String> {
    bytes
        .get(offset..offset + 4)
        .and_then(|value| value.try_into().ok())
        .map(i32::from_le_bytes)
        .ok_or_else(|| format!("snapshot is too short for i32 at {offset}"))
}

fn read_f32_le(bytes: &[u8], offset: usize) -> Result<f32, String> {
    bytes
        .get(offset..offset + 4)
        .and_then(|value| value.try_into().ok())
        .map(f32::from_le_bytes)
        .ok_or_else(|| format!("snapshot is too short for f32 at {offset}"))
}

fn read_f64_le(bytes: &[u8], offset: usize) -> Result<f64, String> {
    bytes
        .get(offset..offset + 8)
        .and_then(|value| value.try_into().ok())
        .map(f64::from_le_bytes)
        .ok_or_else(|| format!("snapshot is too short for f64 at {offset}"))
}

fn average_tyre_temp_c(values: [f64; 3]) -> f64 {
    let average = (finite_f64(values[0]) + finite_f64(values[1]) + finite_f64(values[2])) / 3.0;
    if average > 180.0 {
        average - 273.15
    } else {
        average
    }
}

fn pressure_to_psi(value: f64) -> f32 {
    let pressure = finite_f64(value);
    if pressure > 1_000.0 {
        (pressure * 0.000_145_037_74) as f32
    } else if pressure > 80.0 {
        (pressure * 0.145_037_74) as f32
    } else {
        pressure as f32
    }
}

fn wear_to_percent(value: f64) -> f32 {
    let wear = finite_f64(value);
    if wear <= 1.0 {
        (wear * 100.0).clamp(0.0, 100.0) as f32
    } else {
        wear.clamp(0.0, 100.0) as f32
    }
}

fn seconds_to_ms(value: f64) -> Option<u32> {
    if value.is_finite() && value >= 0.0 {
        Some((value * 1_000.0).round().min(u32::MAX as f64) as u32)
    } else {
        None
    }
}

fn rear_bias_to_front_percent(rear_bias: f64) -> u8 {
    let rear_percent = if rear_bias <= 1.0 {
        rear_bias * 100.0
    } else {
        rear_bias
    };
    clamp_u8(100.0 - rear_percent)
}

fn percent_u8(value: f64) -> u8 {
    let percent = if value <= 1.0 { value * 100.0 } else { value };
    clamp_u8(percent)
}

fn clamp_unit(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

fn clamp_u8(value: f64) -> u8 {
    if !value.is_finite() || value <= 0.0 {
        0
    } else {
        value.round().min(u8::MAX as f64) as u8
    }
}

fn clamp_u16(value: f64) -> u16 {
    if !value.is_finite() || value <= 0.0 {
        0
    } else {
        value.round().min(u16::MAX as f64) as u16
    }
}

fn finite_f64(value: f64) -> f64 {
    if value.is_finite() { value } else { 0.0 }
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
    fn parses_minimal_lmu_telemetry_snapshot() {
        let mut snapshot = vec![0_u8; LMU_VIEW_SIZE];
        let base = LMU_TELEM_INFO_OFFSET;
        snapshot[LMU_PLAYER_HAS_VEHICLE_OFFSET] = 1;
        snapshot[LMU_PLAYER_INDEX_OFFSET] = 0;
        write_f64_le(&mut snapshot, base + 12, 123.4);
        write_i32_le(&mut snapshot, base + 20, 7);
        write_f64_le(&mut snapshot, base + 24, 100.0);
        write_f64_le(&mut snapshot, base + LOCAL_VEL_OFFSET, 10.0);
        write_f64_le(&mut snapshot, base + LOCAL_VEL_OFFSET + 8, 0.0);
        write_f64_le(&mut snapshot, base + LOCAL_VEL_OFFSET + 16, 20.0);
        write_i32_le(&mut snapshot, base + GEAR_OFFSET, 4);
        write_f64_le(&mut snapshot, base + ENGINE_RPM_OFFSET, 7_450.0);
        write_f64_le(&mut snapshot, base + ENGINE_WATER_TEMP_OFFSET, 92.0);
        write_f64_le(&mut snapshot, base + THROTTLE_OFFSET, 0.81);
        write_f64_le(&mut snapshot, base + BRAKE_OFFSET, 0.23);
        write_f64_le(&mut snapshot, base + STEERING_OFFSET, -0.15);
        write_f64_le(&mut snapshot, base + CLUTCH_OFFSET, 0.4);
        write_f64_le(&mut snapshot, base + FUEL_OFFSET, 34.0);
        write_f64_le(&mut snapshot, base + ENGINE_MAX_RPM_OFFSET, 9_000.0);
        write_f64_le(&mut snapshot, base + FUEL_CAPACITY_OFFSET, 100.0);
        write_f64_le(&mut snapshot, base + REAR_BRAKE_BIAS_OFFSET, 0.44);
        write_f32_le(&mut snapshot, base + GAP_CAR_AHEAD_OFFSET, 1.25);
        write_f32_le(&mut snapshot, base + GAP_CAR_BEHIND_OFFSET, 2.5);
        snapshot[base + SPEED_LIMITER_OFFSET] = 1;
        snapshot[base + MAX_GEARS_OFFSET] = 8;

        for wheel in 0..4 {
            let wheel_base = base + WHEELS_OFFSET + wheel * WHEEL_SIZE;
            write_f64_le(&mut snapshot, wheel_base + WHEEL_BRAKE_TEMP_OFFSET, 420.0);
            write_f64_le(&mut snapshot, wheel_base + WHEEL_PRESSURE_OFFSET, 180.0);
            write_f64_le(&mut snapshot, wheel_base + WHEEL_TEMP_OFFSET, 333.15);
            write_f64_le(&mut snapshot, wheel_base + WHEEL_TEMP_OFFSET + 8, 334.15);
            write_f64_le(&mut snapshot, wheel_base + WHEEL_TEMP_OFFSET + 16, 335.15);
            write_f64_le(&mut snapshot, wheel_base + WHEEL_WEAR_OFFSET, 0.12);
            write_f64_le(&mut snapshot, wheel_base + WHEEL_INNER_TEMP_OFFSET, 343.15);
            write_f64_le(
                &mut snapshot,
                wheel_base + WHEEL_INNER_TEMP_OFFSET + 8,
                344.15,
            );
            write_f64_le(
                &mut snapshot,
                wheel_base + WHEEL_INNER_TEMP_OFFSET + 16,
                345.15,
            );
        }

        let update = parse_lmu_update(&snapshot, 77).unwrap().unwrap();
        let input = update.input.unwrap();
        let lap = update.lap.unwrap();
        let status = update.status.unwrap();
        let damage = update.damage.unwrap();

        assert_eq!(input.frame_identifier, 77);
        assert_eq!(input.gear, 4);
        assert_eq!(input.rpm, 7450);
        assert_eq!(input.speed_kmh, 80);
        assert_eq!(input.engine_temp_c, 92);
        assert_eq!(input.brake_temps_c.fl, 420);
        assert_eq!(input.tyre_surface_temps_c.fl, 61);
        assert_eq!(input.tyre_inner_temps_c.fl, 71);
        assert!((input.throttle - 0.81).abs() < 0.001);
        assert!((input.brake - 0.23).abs() < 0.001);
        assert!((input.tyre_pressures_psi.fl - 26.106).abs() < 0.01);
        assert_eq!(lap.current_lap_num, 7);
        assert_eq!(lap.current_lap_time_ms, 23_400);
        assert_eq!(lap.delta_to_car_in_front_ms, Some(1_250));
        assert_eq!(lap.delta_to_car_behind_ms, Some(2_500));
        assert_eq!(status.front_brake_bias, 56);
        assert_eq!(status.fuel_in_tank, 34.0);
        assert!(status.pit_limiter_active);
        assert_eq!(damage.tyre_wear.fl, 12.0);
    }

    fn write_i32_le(bytes: &mut [u8], offset: usize, value: i32) {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn write_f32_le(bytes: &mut [u8], offset: usize, value: f32) {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn write_f64_le(bytes: &mut [u8], offset: usize, value: f64) {
        bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
}
