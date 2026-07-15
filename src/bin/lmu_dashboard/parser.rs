#![cfg_attr(not(windows), allow(dead_code))]

use std::collections::HashMap;

use crate::model::{
    ImpactState, ParsedFrame, Point3, SessionState, VehicleState, VehicleTelemetry,
};

pub const LMU_VIEW_SIZE: usize = 324_820;
const GENERIC_GAME_VERSION_OFFSET: usize = 64;
const SCORING_OFFSET: usize = 1_632;
const SCORING_INFO_SIZE: usize = 548;
const VEHICLE_SCORING_OFFSET: usize = SCORING_OFFSET + SCORING_INFO_SIZE + 12;
const VEHICLE_SCORING_SIZE: usize = 584;
const TELEMETRY_OFFSET: usize = 128_464;
const TELEMETRY_INFO_OFFSET: usize = TELEMETRY_OFFSET + 4;
const TELEMETRY_INFO_SIZE: usize = 1_888;
const MAX_VEHICLES: usize = 104;

const TELEMETRY_ACTIVE_VEHICLES_OFFSET: usize = TELEMETRY_OFFSET;
const TELEMETRY_PLAYER_INDEX_OFFSET: usize = TELEMETRY_OFFSET + 1;
const TELEMETRY_PLAYER_HAS_VEHICLE_OFFSET: usize = TELEMETRY_OFFSET + 2;

#[derive(Clone, Copy, Debug)]
struct RawTelemetry {
    id: i32,
    elapsed_time_s: f64,
    lap_number: i32,
    lap_start_s: f64,
    position: Point3,
    local_velocity: Point3,
    local_acceleration: Point3,
    gear: i32,
    rpm: f64,
    throttle: f64,
    brake: f64,
    steer: f64,
    clutch: f64,
    lap_invalidated: bool,
    impact: ImpactState,
}

pub fn parse_lmu_snapshot(snapshot: &[u8]) -> Result<ParsedFrame, String> {
    if snapshot.len() < LMU_VIEW_SIZE {
        return Err(format!(
            "LMU shared-memory snapshot is too short: expected {LMU_VIEW_SIZE}, got {}",
            snapshot.len()
        ));
    }

    let game_version = read_i32(snapshot, GENERIC_GAME_VERSION_OFFSET)?;
    let num_vehicles = read_i32(snapshot, SCORING_OFFSET + 104)?;
    if !(0..=MAX_VEHICLES as i32).contains(&num_vehicles) {
        return Err(format!(
            "LMU scoring vehicle count is invalid ({num_vehicles}); the shared-memory layout may have changed"
        ));
    }

    let track_length_m = finite_or_zero(read_f64(snapshot, SCORING_OFFSET + 88)?);
    let mut session = SessionState {
        id: String::new(),
        game_version,
        track_name: read_string(snapshot, SCORING_OFFSET, 64)?,
        session_type: session_type(read_i32(snapshot, SCORING_OFFSET + 64)?).to_owned(),
        current_time_s: finite_or_zero(read_f64(snapshot, SCORING_OFFSET + 68)?),
        time_remaining_s: finite_or_zero(read_f32(snapshot, SCORING_OFFSET + 340)? as f64),
        max_laps: read_i32(snapshot, SCORING_OFFSET + 84)?,
        track_length_m,
        game_phase: read_u8(snapshot, SCORING_OFFSET + 108)?,
        ambient_temp_c: finite_or_zero(read_f64(snapshot, SCORING_OFFSET + 228)?),
        track_temp_c: finite_or_zero(read_f64(snapshot, SCORING_OFFSET + 236)?),
        raining: finite_or_zero(read_f64(snapshot, SCORING_OFFSET + 220)?).clamp(0.0, 1.0),
    };

    if session.track_name.is_empty() {
        session.track_name = "Waiting for track".to_owned();
    }

    let telemetry_count = read_u8(snapshot, TELEMETRY_ACTIVE_VEHICLES_OFFSET)? as usize;
    if telemetry_count > MAX_VEHICLES {
        return Err(format!(
            "LMU telemetry vehicle count is invalid ({telemetry_count}); the shared-memory layout may have changed"
        ));
    }

    let mut telemetry = HashMap::with_capacity(telemetry_count);
    let mut telemetry_by_index = Vec::with_capacity(telemetry_count);
    let mut impacts = Vec::new();
    for index in 0..telemetry_count {
        let raw = parse_telemetry(snapshot, index)?;
        if raw.id < 0 {
            continue;
        }
        if raw.impact.event_time_s > 0.0 && raw.impact.magnitude > 0.0 {
            impacts.push(raw.impact);
        }
        telemetry.insert(raw.id, raw);
        telemetry_by_index.push((index, raw));
    }

    let mut vehicles = Vec::with_capacity(num_vehicles as usize);
    for index in 0..num_vehicles as usize {
        let base = VEHICLE_SCORING_OFFSET + index * VEHICLE_SCORING_SIZE;
        let id = read_i32(snapshot, base)?;
        let local_velocity = telemetry
            .get(&id)
            .map(|value| value.local_velocity)
            .unwrap_or_else(|| read_point3(snapshot, base + 288).unwrap_or_default());
        let speed_kmh = vector_length(local_velocity) * 3.6;
        vehicles.push(VehicleState {
            id,
            steam_id: read_u64(snapshot, base + 536)?,
            driver_name: read_string(snapshot, base + 4, 32)?,
            vehicle_name: read_string(snapshot, base + 36, 64)?,
            class_name: read_string(snapshot, base + 200, 32)?,
            position: read_u8(snapshot, base + 199)?,
            completed_laps: read_i16(snapshot, base + 100)?,
            lap_distance_m: finite_or_zero(read_f64(snapshot, base + 104)?),
            best_lap_time_s: positive_finite(read_f64(snapshot, base + 144)?),
            last_lap_time_s: positive_finite(read_f64(snapshot, base + 168)?),
            interval_s: positive_or_zero(read_f64(snapshot, base + 232)?),
            gap_to_leader_s: positive_or_zero(read_f64(snapshot, base + 244)?),
            laps_behind_next: read_i32(snapshot, base + 240)?,
            laps_behind_leader: read_i32(snapshot, base + 252)?,
            in_pits: read_u8(snapshot, base + 198)? != 0,
            pit_state: read_u8(snapshot, base + 457)?,
            is_player: read_u8(snapshot, base + 196)? != 0,
            world: read_point3(snapshot, base + 264)?.xz(),
            speed_kmh,
        });
    }
    vehicles.sort_by_key(|vehicle| {
        if vehicle.position == 0 {
            u8::MAX
        } else {
            vehicle.position
        }
    });

    let telemetry_by_index = telemetry_by_index
        .into_iter()
        .map(|(index, raw)| (index, detailed_telemetry(raw, &vehicles)))
        .collect::<Vec<_>>();
    let player = parse_player(snapshot, &telemetry_by_index, &vehicles)?;
    let telemetry = telemetry_by_index
        .into_iter()
        .map(|(_, telemetry)| telemetry)
        .collect();

    Ok(ParsedFrame {
        session,
        vehicles,
        telemetry,
        player,
        impacts,
    })
}

fn parse_player(
    snapshot: &[u8],
    telemetry: &[(usize, VehicleTelemetry)],
    vehicles: &[VehicleState],
) -> Result<Option<VehicleTelemetry>, String> {
    if read_u8(snapshot, TELEMETRY_PLAYER_HAS_VEHICLE_OFFSET)? == 0 {
        return Ok(None);
    }

    let player_index = read_u8(snapshot, TELEMETRY_PLAYER_INDEX_OFFSET)? as usize;
    let raw = telemetry
        .iter()
        .find(|(index, _)| *index == player_index)
        .map(|(_, telemetry)| telemetry.clone())
        .or_else(|| {
            let player_id = vehicles.iter().find(|vehicle| vehicle.is_player)?.id;
            telemetry
                .iter()
                .find(|(_, telemetry)| telemetry.vehicle_id == player_id)
                .map(|(_, telemetry)| telemetry.clone())
        });
    Ok(raw)
}

fn detailed_telemetry(raw: RawTelemetry, vehicles: &[VehicleState]) -> VehicleTelemetry {
    let scoring = vehicles.iter().find(|vehicle| vehicle.id == raw.id);
    VehicleTelemetry {
        vehicle_id: raw.id,
        lap_number: raw.lap_number,
        lap_distance_m: scoring.map_or(0.0, |vehicle| vehicle.lap_distance_m),
        lap_elapsed_s: (raw.elapsed_time_s - raw.lap_start_s).max(0.0),
        session_time_s: raw.elapsed_time_s,
        speed_kmh: vector_length(raw.local_velocity) * 3.6,
        rpm: raw.rpm,
        gear: raw.gear,
        throttle: raw.throttle.clamp(0.0, 1.0),
        brake: raw.brake.clamp(0.0, 1.0),
        steer: raw.steer.clamp(-1.0, 1.0),
        clutch: raw.clutch.clamp(0.0, 1.0),
        lateral_g: raw.local_acceleration.x / 9.806_65,
        longitudinal_g: raw.local_acceleration.z / 9.806_65,
        world: raw.position.xz(),
        lap_invalidated: raw.lap_invalidated,
    }
}

fn parse_telemetry(snapshot: &[u8], index: usize) -> Result<RawTelemetry, String> {
    let base = TELEMETRY_INFO_OFFSET + index * TELEMETRY_INFO_SIZE;
    let id = read_i32(snapshot, base)?;
    Ok(RawTelemetry {
        id,
        elapsed_time_s: finite_or_zero(read_f64(snapshot, base + 12)?),
        lap_number: read_i32(snapshot, base + 20)?,
        lap_start_s: finite_or_zero(read_f64(snapshot, base + 24)?),
        position: read_point3(snapshot, base + 160)?,
        local_velocity: read_point3(snapshot, base + 184)?,
        local_acceleration: read_point3(snapshot, base + 208)?,
        gear: read_i32(snapshot, base + 352)?,
        rpm: finite_or_zero(read_f64(snapshot, base + 356)?),
        throttle: finite_or_zero(read_f64(snapshot, base + 388)?),
        brake: finite_or_zero(read_f64(snapshot, base + 396)?),
        steer: finite_or_zero(read_f64(snapshot, base + 404)?),
        clutch: finite_or_zero(read_f64(snapshot, base + 412)?),
        lap_invalidated: read_u8(snapshot, base + 745)? != 0,
        impact: ImpactState {
            vehicle_id: id,
            event_time_s: finite_or_zero(read_f64(snapshot, base + 552)?),
            magnitude: finite_or_zero(read_f64(snapshot, base + 560)?),
            position: read_point3(snapshot, base + 568)?,
        },
    })
}

fn session_type(value: i32) -> &'static str {
    match value {
        0 => "Test Day",
        1..=4 => "Practice",
        5..=8 => "Qualifying",
        9 => "Warmup",
        10..=13 => "Race",
        _ => "Unknown",
    }
}

fn positive_finite(value: f64) -> Option<f64> {
    (value.is_finite() && value > 0.0).then_some(value)
}

fn positive_or_zero(value: f64) -> Option<f64> {
    (value.is_finite() && value >= 0.0).then_some(value)
}

fn finite_or_zero(value: f64) -> f64 {
    if value.is_finite() { value } else { 0.0 }
}

fn vector_length(value: Point3) -> f64 {
    value
        .x
        .mul_add(value.x, value.y.mul_add(value.y, value.z * value.z))
        .sqrt()
}

fn read_point3(bytes: &[u8], offset: usize) -> Result<Point3, String> {
    Ok(Point3 {
        x: finite_or_zero(read_f64(bytes, offset)?),
        y: finite_or_zero(read_f64(bytes, offset + 8)?),
        z: finite_or_zero(read_f64(bytes, offset + 16)?),
    })
}

fn read_string(bytes: &[u8], offset: usize, length: usize) -> Result<String, String> {
    let raw = bytes
        .get(offset..offset + length)
        .ok_or_else(|| format!("snapshot is too short for string at {offset}"))?;
    let end = raw.iter().position(|byte| *byte == 0).unwrap_or(raw.len());
    Ok(String::from_utf8_lossy(&raw[..end]).trim().to_owned())
}

fn read_u8(bytes: &[u8], offset: usize) -> Result<u8, String> {
    bytes
        .get(offset)
        .copied()
        .ok_or_else(|| format!("snapshot is too short for u8 at {offset}"))
}

fn read_i16(bytes: &[u8], offset: usize) -> Result<i16, String> {
    read_array(bytes, offset).map(i16::from_le_bytes)
}

fn read_i32(bytes: &[u8], offset: usize) -> Result<i32, String> {
    read_array(bytes, offset).map(i32::from_le_bytes)
}

fn read_u64(bytes: &[u8], offset: usize) -> Result<u64, String> {
    read_array(bytes, offset).map(u64::from_le_bytes)
}

fn read_f32(bytes: &[u8], offset: usize) -> Result<f32, String> {
    read_array(bytes, offset).map(f32::from_le_bytes)
}

fn read_f64(bytes: &[u8], offset: usize) -> Result<f64, String> {
    read_array(bytes, offset).map(f64::from_le_bytes)
}

fn read_array<const N: usize>(bytes: &[u8], offset: usize) -> Result<[u8; N], String> {
    bytes
        .get(offset..offset + N)
        .and_then(|value| value.try_into().ok())
        .ok_or_else(|| format!("snapshot is too short for {N} bytes at {offset}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_session_grid_player_and_impacts() {
        let mut snapshot = vec![0_u8; LMU_VIEW_SIZE];
        write_i32(&mut snapshot, GENERIC_GAME_VERSION_OFFSET, 13);
        write_string(&mut snapshot, SCORING_OFFSET, 64, "Le Mans 2024");
        write_i32(&mut snapshot, SCORING_OFFSET + 64, 10);
        write_f64(&mut snapshot, SCORING_OFFSET + 68, 125.5);
        write_i32(&mut snapshot, SCORING_OFFSET + 84, 50);
        write_f64(&mut snapshot, SCORING_OFFSET + 88, 13_626.0);
        write_i32(&mut snapshot, SCORING_OFFSET + 104, 2);
        snapshot[SCORING_OFFSET + 108] = 5;

        let alice = VEHICLE_SCORING_OFFSET;
        write_i32(&mut snapshot, alice, 7);
        write_string(&mut snapshot, alice + 4, 32, "Alice Driver");
        write_string(&mut snapshot, alice + 36, 64, "Hypercar A");
        write_i16(&mut snapshot, alice + 100, 3);
        write_f64(&mut snapshot, alice + 104, 4_200.0);
        write_f64(&mut snapshot, alice + 144, 210.25);
        snapshot[alice + 196] = 1;
        snapshot[alice + 199] = 1;
        write_string(&mut snapshot, alice + 200, 32, "Hypercar");
        write_f64(&mut snapshot, alice + 232, 0.0);
        write_f64(&mut snapshot, alice + 244, 0.0);
        write_point3(&mut snapshot, alice + 264, 100.0, 2.0, -40.0);

        let bob = VEHICLE_SCORING_OFFSET + VEHICLE_SCORING_SIZE;
        write_i32(&mut snapshot, bob, 8);
        write_string(&mut snapshot, bob + 4, 32, "Bob Driver");
        write_i16(&mut snapshot, bob + 100, 3);
        write_f64(&mut snapshot, bob + 104, 4_000.0);
        snapshot[bob + 199] = 2;
        write_string(&mut snapshot, bob + 200, 32, "Hypercar");
        write_f64(&mut snapshot, bob + 232, 1.234);
        write_f64(&mut snapshot, bob + 244, 1.234);
        write_point3(&mut snapshot, bob + 264, 96.0, 2.0, -42.0);

        snapshot[TELEMETRY_ACTIVE_VEHICLES_OFFSET] = 2;
        snapshot[TELEMETRY_PLAYER_INDEX_OFFSET] = 0;
        snapshot[TELEMETRY_PLAYER_HAS_VEHICLE_OFFSET] = 1;
        let telemetry = TELEMETRY_INFO_OFFSET;
        write_i32(&mut snapshot, telemetry, 7);
        write_f64(&mut snapshot, telemetry + 12, 125.5);
        write_i32(&mut snapshot, telemetry + 20, 4);
        write_f64(&mut snapshot, telemetry + 24, 100.0);
        write_point3(&mut snapshot, telemetry + 160, 100.0, 2.0, -40.0);
        write_point3(&mut snapshot, telemetry + 184, 0.0, 0.0, 80.0);
        write_point3(&mut snapshot, telemetry + 208, 4.0, 0.0, -2.0);
        write_i32(&mut snapshot, telemetry + 352, 6);
        write_f64(&mut snapshot, telemetry + 356, 9_500.0);
        write_f64(&mut snapshot, telemetry + 388, 0.8);
        write_f64(&mut snapshot, telemetry + 396, 0.1);
        write_f64(&mut snapshot, telemetry + 404, -0.2);
        write_f64(&mut snapshot, telemetry + 552, 124.8);
        write_f64(&mut snapshot, telemetry + 560, 3.5);
        write_point3(&mut snapshot, telemetry + 568, 98.0, 2.0, -41.0);

        let second_telemetry = TELEMETRY_INFO_OFFSET + TELEMETRY_INFO_SIZE;
        write_i32(&mut snapshot, second_telemetry, 8);

        let parsed = parse_lmu_snapshot(&snapshot).unwrap();
        assert_eq!(parsed.session.track_name, "Le Mans 2024");
        assert_eq!(parsed.session.session_type, "Race");
        assert_eq!(parsed.vehicles.len(), 2);
        assert_eq!(parsed.vehicles[1].driver_name, "Bob Driver");
        assert_eq!(parsed.vehicles[1].interval_s, Some(1.234));
        assert_eq!(parsed.telemetry.len(), 2);
        assert_eq!(parsed.telemetry[1].vehicle_id, 8);
        assert_eq!(parsed.player.as_ref().unwrap().lap_number, 4);
        assert_eq!(parsed.player.as_ref().unwrap().lap_distance_m, 4_200.0);
        assert!((parsed.player.as_ref().unwrap().speed_kmh - 288.0).abs() < 0.001);
        assert_eq!(parsed.impacts.len(), 1);
        assert_eq!(parsed.impacts[0].vehicle_id, 7);
    }

    #[test]
    fn rejects_short_or_implausible_snapshots() {
        assert!(
            parse_lmu_snapshot(&[0; 64])
                .unwrap_err()
                .contains("too short")
        );

        let mut snapshot = vec![0_u8; LMU_VIEW_SIZE];
        write_i32(&mut snapshot, SCORING_OFFSET + 104, 105);
        assert!(
            parse_lmu_snapshot(&snapshot)
                .unwrap_err()
                .contains("vehicle count")
        );
    }

    fn write_string(bytes: &mut [u8], offset: usize, length: usize, value: &str) {
        let encoded = value.as_bytes();
        let count = encoded.len().min(length.saturating_sub(1));
        bytes[offset..offset + count].copy_from_slice(&encoded[..count]);
    }

    fn write_i16(bytes: &mut [u8], offset: usize, value: i16) {
        bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }

    fn write_i32(bytes: &mut [u8], offset: usize, value: i32) {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn write_f64(bytes: &mut [u8], offset: usize, value: f64) {
        bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }

    fn write_point3(bytes: &mut [u8], offset: usize, x: f64, y: f64, z: f64) {
        write_f64(bytes, offset, x);
        write_f64(bytes, offset + 8, y);
        write_f64(bytes, offset + 16, z);
    }
}
