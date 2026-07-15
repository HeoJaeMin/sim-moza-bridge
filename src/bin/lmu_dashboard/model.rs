use serde::{Deserialize, Serialize};

use crate::telemetry_quality::TraceQuality;

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct Point2 {
    pub x: f64,
    pub z: f64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Point3 {
    pub fn distance_to(self, other: Self) -> f64 {
        let dx = self.x - other.x;
        let dy = self.y - other.y;
        let dz = self.z - other.z;
        dx.mul_add(dx, dy.mul_add(dy, dz * dz)).sqrt()
    }

    pub fn xz(self) -> Point2 {
        Point2 {
            x: self.x,
            z: self.z,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct SessionState {
    pub id: String,
    pub game_version: i32,
    pub track_name: String,
    pub session_type: String,
    pub current_time_s: f64,
    pub time_remaining_s: f64,
    pub max_laps: i32,
    pub track_length_m: f64,
    pub game_phase: u8,
    pub ambient_temp_c: f64,
    pub track_temp_c: f64,
    pub raining: f64,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct VehicleState {
    pub id: i32,
    pub steam_id: u64,
    pub driver_name: String,
    pub vehicle_name: String,
    pub class_name: String,
    pub position: u8,
    pub completed_laps: i16,
    pub lap_distance_m: f64,
    pub best_lap_time_s: Option<f64>,
    pub last_lap_time_s: Option<f64>,
    pub interval_s: Option<f64>,
    pub gap_to_leader_s: Option<f64>,
    pub laps_behind_next: i32,
    pub laps_behind_leader: i32,
    pub in_pits: bool,
    pub pit_state: u8,
    pub is_player: bool,
    pub world: Point2,
    pub speed_kmh: f64,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ImpactState {
    pub vehicle_id: i32,
    pub event_time_s: f64,
    pub magnitude: f64,
    pub position: Point3,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct VehicleTelemetry {
    pub vehicle_id: i32,
    pub lap_number: i32,
    pub lap_distance_m: f64,
    pub lap_elapsed_s: f64,
    pub session_time_s: f64,
    pub speed_kmh: f64,
    pub rpm: f64,
    pub gear: i32,
    pub throttle: f64,
    pub brake: f64,
    pub steer: f64,
    pub clutch: f64,
    pub lateral_g: f64,
    pub longitudinal_g: f64,
    pub world: Point2,
    pub lap_invalidated: bool,
}

#[derive(Clone, Debug, Default)]
pub struct ParsedFrame {
    pub session: SessionState,
    pub vehicles: Vec<VehicleState>,
    pub telemetry: Vec<VehicleTelemetry>,
    pub player: Option<VehicleTelemetry>,
    pub impacts: Vec<ImpactState>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct TrackPoint {
    pub lap_distance_m: f64,
    pub x: f64,
    pub z: f64,
    pub samples: u32,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct TelemetryPoint {
    pub session_time_s: f64,
    pub lap_elapsed_s: f64,
    pub lap_distance_m: f64,
    pub x: f64,
    pub z: f64,
    pub speed_kmh: f64,
    pub rpm: f64,
    pub gear: i32,
    pub throttle: f64,
    pub brake: f64,
    pub steer: f64,
    pub clutch: f64,
    pub lateral_g: f64,
    pub longitudinal_g: f64,
}

impl From<&VehicleTelemetry> for TelemetryPoint {
    fn from(sample: &VehicleTelemetry) -> Self {
        Self {
            session_time_s: sample.session_time_s,
            lap_elapsed_s: sample.lap_elapsed_s,
            lap_distance_m: sample.lap_distance_m,
            x: sample.world.x,
            z: sample.world.z,
            speed_kmh: sample.speed_kmh,
            rpm: sample.rpm,
            gear: sample.gear,
            throttle: sample.throttle,
            brake: sample.brake,
            steer: sample.steer,
            clutch: sample.clutch,
            lateral_g: sample.lateral_g,
            longitudinal_g: sample.longitudinal_g,
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct LapSummary {
    pub id: String,
    pub session_id: String,
    pub track_name: String,
    #[serde(default)]
    pub session_type: String,
    #[serde(default)]
    pub vehicle_id: i32,
    #[serde(default)]
    pub driver_name: String,
    #[serde(default)]
    pub class_name: String,
    #[serde(default)]
    pub is_player: bool,
    #[serde(default)]
    pub overall_position: u8,
    #[serde(default)]
    pub class_position: u8,
    pub lap_number: i32,
    pub lap_time_ms: u32,
    pub valid: bool,
    #[serde(default)]
    pub quality: TraceQuality,
    pub sample_count: usize,
    pub created_at_unix_ms: u64,
    pub completed: bool,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct SavedLap {
    pub summary: LapSummary,
    pub samples: Vec<TelemetryPoint>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct CurrentLapInfo {
    pub lap_number: i32,
    pub lap_elapsed_s: f64,
    pub sample_count: usize,
    pub invalid: bool,
    pub quality: TraceQuality,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct CaptureHealth {
    pub state: String,
    pub sample_rate_hz: f64,
    pub accepted_frames: u64,
    pub rejected_frames: u64,
    pub duplicate_frames: u64,
    pub invalid_session_frames: u64,
    pub last_frame_age_ms: u64,
    pub session_resumed: bool,
    pub current_quality: TraceQuality,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ContactConfidence {
    Confirmed,
    Probable,
    #[default]
    Unresolved,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct ContactParticipant {
    pub vehicle_id: i32,
    pub driver_name: String,
    pub class_name: String,
    pub position: u8,
    pub lap_number: i16,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct ContactEvent {
    pub id: String,
    pub session_id: String,
    pub track_name: String,
    pub session_time_s: f64,
    pub car_a: ContactParticipant,
    pub car_b: Option<ContactParticipant>,
    pub magnitude_a: f64,
    pub magnitude_b: Option<f64>,
    pub position: Point2,
    pub confidence: ContactConfidence,
    pub created_at_unix_ms: u64,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct LiveSnapshot {
    pub connected: bool,
    pub source: String,
    pub warning: Option<String>,
    pub session: Option<SessionState>,
    pub vehicles: Vec<VehicleState>,
    pub player: Option<VehicleTelemetry>,
    pub track_points: Vec<TrackPoint>,
    pub recent_contacts: Vec<ContactEvent>,
    pub current_lap: Option<CurrentLapInfo>,
    pub capture: CaptureHealth,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct TraceResponse {
    pub summary: Option<LapSummary>,
    pub samples: Vec<TelemetryPoint>,
}
