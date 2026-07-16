#![cfg_attr(not(windows), allow(dead_code))]

use std::fs::{self, File, OpenOptions, create_dir_all};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::config::BridgeConfig;
use crate::hud::HudHandle;
use crate::telemetry::{
    DamageSample, InputSample, LapSample, SessionSample, StatusSample, TelemetryUpdate,
    WheelValuesF32, WheelValuesU8, WheelValuesU16,
};
use crate::telemetry_core::{
    AnalysisConfidence, AnalysisConfidenceLevel, AnalysisLimitation, CaptureCounters,
    SessionIdentity,
};
use crate::telemetry_quality::{
    QualityReason, QualitySample, TraceQuality, TraceQualityStatus, assess_trace,
};

#[cfg(windows)]
use crate::logging::{TelemetryRecorder, print_enabled_outputs};
#[cfg(windows)]
use std::fs::metadata;

pub(crate) const ACR_PHYSICS_MAPPING_NAME: &str = "Local\\acpmf_physics";
pub(crate) const ACR_GRAPHICS_MAPPING_NAME: &str = "Local\\acpmf_graphics";
pub(crate) const ACR_STATIC_MAPPING_NAME: &str = "Local\\acpmf_static";
pub(crate) const ACR_PHYSICS_SIZE: usize = 800;
pub(crate) const ACR_GRAPHICS_SIZE: usize = 1_588;
pub(crate) const ACR_STATIC_SIZE: usize = 784;

const UNKNOWN_TYRE_WEAR_PERCENT: f32 = -1.0;
const STAGE_RESET_MIN_PREVIOUS_DISTANCE_M: f32 = 250.0;
const STAGE_RESET_MIN_DROP_M: f32 = 150.0;
const MAX_LIVE_MAPPING_SKEW: Duration = Duration::from_millis(30);
const ARCHIVE_CHANNEL_CAPACITY: usize = 2_048;
const ARCHIVE_SCHEMA_VERSION: u8 = 1;
const RAW_ARCHIVE_PREFIX: &str = "acr-raw-";
const ANALYSIS_ARCHIVE_PREFIX: &str = "acr-analysis-";
#[cfg(not(test))]
const ARCHIVE_FLUSH_INTERVAL: Duration = Duration::from_secs(1);
#[cfg(test)]
const ARCHIVE_FLUSH_INTERVAL: Duration = Duration::from_millis(25);

#[cfg(windows)]
const POLL_INTERVAL: Duration = Duration::from_millis(10);
#[cfg(windows)]
const STATIC_REFRESH_INTERVAL: Duration = Duration::from_secs(1);
#[cfg(windows)]
const WARNING_INTERVAL: Duration = Duration::from_secs(2);
#[cfg(windows)]
const PACKET_ID_MARKERS: [super::shared_memory::StabilityMarker; 1] =
    [super::shared_memory::StabilityMarker::new(0, 4)];
#[cfg(windows)]
const NO_STABILITY_MARKERS: [super::shared_memory::StabilityMarker; 0] = [];

const PACKET_ID_OFFSET: usize = 0;
const THROTTLE_OFFSET: usize = 4;
const BRAKE_OFFSET: usize = 8;
const FUEL_OFFSET: usize = 12;
const GEAR_OFFSET: usize = 16;
const RPM_OFFSET: usize = 20;
const STEER_OFFSET: usize = 24;
const SPEED_OFFSET: usize = 28;
const VELOCITY_OFFSET: usize = 32;
const G_FORCE_OFFSET: usize = 44;
const WHEEL_SLIP_OFFSET: usize = 56;
const WHEEL_LOAD_OFFSET: usize = 72;
const TYRE_PRESSURE_OFFSET: usize = 88;
const WHEEL_ANGULAR_SPEED_OFFSET: usize = 104;
const TYRE_WEAR_OFFSET: usize = 120;
const TYRE_CORE_TEMP_OFFSET: usize = 152;
const SUSPENSION_TRAVEL_OFFSET: usize = 184;
const TC_OFFSET: usize = 204;
const HEADING_OFFSET: usize = 208;
const PITCH_OFFSET: usize = 212;
const ROLL_OFFSET: usize = 216;
const CAR_DAMAGE_OFFSET: usize = 224;
const PIT_LIMITER_OFFSET: usize = 248;
const ABS_OFFSET: usize = 252;
const AIR_TEMP_OFFSET: usize = 288;
const ROAD_TEMP_OFFSET: usize = 292;
const LOCAL_ANGULAR_VELOCITY_OFFSET: usize = 296;
const FINAL_FF_OFFSET: usize = 308;
const BRAKE_TEMP_OFFSET: usize = 348;
const CLUTCH_OFFSET: usize = 364;
const TYRE_TEMP_INNER_OFFSET: usize = 368;
const TYRE_TEMP_MIDDLE_OFFSET: usize = 384;
const TYRE_TEMP_OUTER_OFFSET: usize = 400;
const BRAKE_BIAS_OFFSET: usize = 564;
const LOCAL_VELOCITY_OFFSET: usize = 568;
const CURRENT_MAX_RPM_OFFSET: usize = 588;
const SLIP_RATIO_OFFSET: usize = 640;
const SLIP_ANGLE_OFFSET: usize = 656;
const TC_IN_ACTION_OFFSET: usize = 672;
const ABS_IN_ACTION_OFFSET: usize = 676;
const SUSPENSION_DAMAGE_OFFSET: usize = 680;
const WATER_TEMP_OFFSET: usize = 712;
const BRAKE_PRESSURE_OFFSET: usize = 716;
const IGNITION_ON_OFFSET: usize = 772;
const ENGINE_RUNNING_OFFSET: usize = 780;

const GRAPHICS_PACKET_ID_OFFSET: usize = 0;
const GRAPHICS_STATUS_OFFSET: usize = 4;
const GRAPHICS_SESSION_TYPE_OFFSET: usize = 8;
const GRAPHICS_COMPLETED_LAPS_OFFSET: usize = 132;
const GRAPHICS_POSITION_OFFSET: usize = 136;
const GRAPHICS_CURRENT_TIME_OFFSET: usize = 140;
const GRAPHICS_LAST_TIME_OFFSET: usize = 144;
const GRAPHICS_SESSION_TIME_LEFT_OFFSET: usize = 152;
const GRAPHICS_DISTANCE_OFFSET: usize = 156;
const GRAPHICS_IN_PIT_OFFSET: usize = 160;
const GRAPHICS_SECTOR_OFFSET: usize = 164;

const STATIC_CAR_MODEL_OFFSET: usize = 68;
const STATIC_TRACK_OFFSET: usize = 134;
const STATIC_MAX_RPM_OFFSET: usize = 412;
const STATIC_MAX_FUEL_OFFSET: usize = 416;
const STATIC_TRACK_LENGTH_OFFSET: usize = 520;
const STATIC_UTF16_CHARS: usize = 33;

#[derive(Clone, Debug, PartialEq)]
struct AcrPhysicsSnapshot {
    packet_id: i32,
    throttle: f32,
    brake: f32,
    fuel: f32,
    raw_gear: i32,
    rpm: i32,
    steer: f32,
    speed_kmh: f32,
    velocity: [f32; 3],
    g_force: [f32; 3],
    wheel_slip: [f32; 4],
    wheel_load: [f32; 4],
    tyre_pressure: [f32; 4],
    wheel_angular_speed: [f32; 4],
    tyre_wear: [f32; 4],
    tyre_core_temp: [f32; 4],
    suspension_travel: [f32; 4],
    tc: f32,
    heading: f32,
    pitch: f32,
    roll: f32,
    car_damage: [f32; 5],
    pit_limiter_on: bool,
    abs: f32,
    air_temp: f32,
    road_temp: f32,
    local_angular_velocity: [f32; 3],
    final_ff: f32,
    brake_temp: [f32; 4],
    clutch: f32,
    tyre_temp_inner: [f32; 4],
    tyre_temp_middle: [f32; 4],
    tyre_temp_outer: [f32; 4],
    brake_bias: f32,
    local_velocity: [f32; 3],
    current_max_rpm: i32,
    slip_ratio: [f32; 4],
    slip_angle: [f32; 4],
    tc_in_action: bool,
    abs_in_action: bool,
    suspension_damage: [f32; 4],
    water_temp: f32,
    brake_pressure: [f32; 4],
    ignition_on: bool,
    engine_running: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
struct AcrGraphicsSnapshot {
    packet_id: i32,
    status: i32,
    session_type: i32,
    completed_laps: i32,
    position: i32,
    current_time_ms: i32,
    last_time_ms: i32,
    session_time_left_s: f32,
    distance_m: f32,
    in_pit: bool,
    sector: i32,
}

#[derive(Clone, Debug, Default, PartialEq)]
struct AcrStaticSnapshot {
    car_model: String,
    track: String,
    max_rpm: i32,
    max_fuel: f32,
    track_length_m: f32,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum AcrStageState {
    #[default]
    Idle,
    Countdown,
    Running,
    Finished,
    Aborted,
    Recovery,
}

impl AcrStageState {
    fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Countdown => "countdown",
            Self::Running => "running",
            Self::Finished => "finished",
            Self::Aborted => "aborted",
            Self::Recovery => "recovery",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum AcrStageEvent {
    #[default]
    None,
    Started,
    Finished,
    Aborted,
    Recovery,
    ResultScreen,
    NextAttempt,
}

impl AcrStageEvent {
    fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Started => "started",
            Self::Finished => "finished",
            Self::Aborted => "aborted",
            Self::Recovery => "recovery",
            Self::ResultScreen => "result_screen",
            Self::NextAttempt => "next_attempt",
        }
    }

    fn is_terminal(self) -> bool {
        matches!(self, Self::Finished | Self::Aborted | Self::Recovery)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct StageObservation {
    stage_number: u8,
    elapsed_s: f32,
    reset: bool,
    state: AcrStageState,
    event: AcrStageEvent,
    official_time_ms: Option<u32>,
    max_distance_m: f32,
    attempt_invalid: bool,
    result_screen_entered: bool,
}

struct AcrStageTracker {
    stage_number: u8,
    started_at: Instant,
    last_distance_m: Option<f32>,
    max_distance_m: f32,
    state: AcrStageState,
    last_official_time_ms: i32,
    last_completed_laps: i32,
    last_status: i32,
    manual_finish_distance_m: Option<f32>,
    manual_finish_awaiting_official: bool,
    attempt_invalid: bool,
}

impl AcrStageTracker {
    fn new(now: Instant, manual_finish_distance_m: Option<f32>) -> Self {
        Self {
            stage_number: 1,
            started_at: now,
            last_distance_m: None,
            max_distance_m: 0.0,
            state: AcrStageState::Idle,
            last_official_time_ms: 0,
            last_completed_laps: 0,
            last_status: 0,
            manual_finish_distance_m: manual_finish_distance_m
                .filter(|distance| distance.is_finite() && *distance >= 250.0),
            manual_finish_awaiting_official: false,
            attempt_invalid: false,
        }
    }

    fn reset_context(&mut self, now: Instant, manual_finish_distance_m: Option<f32>) {
        self.stage_number = 1;
        self.started_at = now;
        self.last_distance_m = None;
        self.max_distance_m = 0.0;
        self.state = AcrStageState::Idle;
        self.last_official_time_ms = 0;
        self.last_completed_laps = 0;
        self.last_status = 0;
        self.manual_finish_distance_m =
            manual_finish_distance_m.filter(|distance| distance.is_finite() && *distance >= 250.0);
        self.manual_finish_awaiting_official = false;
        self.attempt_invalid = false;
    }

    fn has_new_official_finish(&self, graphics: &AcrGraphicsSnapshot) -> bool {
        (self.state == AcrStageState::Running
            || (self.state == AcrStageState::Finished && self.manual_finish_awaiting_official))
            && graphics.last_time_ms > 0
            && (graphics.last_time_ms != self.last_official_time_ms
                || graphics.completed_laps > self.last_completed_laps)
    }

    fn observe(
        &mut self,
        graphics: &AcrGraphicsSnapshot,
        speed_kmh: f32,
        engine_running: bool,
        now: Instant,
    ) -> StageObservation {
        let distance_m = finite_nonnegative(graphics.distance_m);
        let reset = self.last_distance_m.is_some_and(|previous| {
            previous >= STAGE_RESET_MIN_PREVIOUS_DISTANCE_M
                && previous - distance_m >= STAGE_RESET_MIN_DROP_M
        });
        let official_time_ms = (graphics.last_time_ms > 0
            && (graphics.last_time_ms != self.last_official_time_ms
                || graphics.completed_laps > self.last_completed_laps))
            .then_some(graphics.last_time_ms as u32);
        // Assetto Corsa exposes a concrete session kind only while a driving session is
        // active. Treat status=live with an unknown/menu session as non-driving data so it
        // cannot create an archived attempt.
        let live = graphics.status == 2 && (0..=6).contains(&graphics.session_type);
        let result_screen_entered = graphics.status == 3 && self.last_status != 3;
        let moving = speed_kmh.is_finite() && speed_kmh > 3.0;
        let progressed = self
            .last_distance_m
            .is_some_and(|previous| distance_m > previous + 0.5);
        let mut event = AcrStageEvent::None;

        match self.state {
            AcrStageState::Idle => {
                if live && engine_running {
                    self.state = AcrStageState::Countdown;
                    self.started_at = now;
                }
            }
            AcrStageState::Countdown => {
                if live && (moving || progressed || distance_m > 2.0) {
                    self.state = AcrStageState::Running;
                    self.started_at = now;
                    self.max_distance_m = distance_m;
                    self.attempt_invalid = false;
                    event = AcrStageEvent::Started;
                } else if !live && graphics.status != 3 {
                    self.state = AcrStageState::Idle;
                }
            }
            AcrStageState::Running => {
                let manual_finish = self
                    .manual_finish_distance_m
                    .is_some_and(|finish| distance_m >= finish * 0.995);
                if official_time_ms.is_some() {
                    // Official game completion wins when completion and a distance reset arrive
                    // in the same snapshot.
                    self.manual_finish_awaiting_official = false;
                    self.state = AcrStageState::Finished;
                    event = AcrStageEvent::Finished;
                } else if manual_finish {
                    self.manual_finish_awaiting_official = true;
                    self.state = AcrStageState::Finished;
                    event = AcrStageEvent::Finished;
                } else if reset {
                    self.attempt_invalid = true;
                    if distance_m > 100.0
                        && self.max_distance_m - distance_m > STAGE_RESET_MIN_DROP_M
                    {
                        self.state = AcrStageState::Recovery;
                        event = AcrStageEvent::Recovery;
                    } else {
                        self.state = AcrStageState::Aborted;
                        event = AcrStageEvent::Aborted;
                    }
                } else if !live {
                    self.attempt_invalid = true;
                    self.state = AcrStageState::Aborted;
                    event = AcrStageEvent::Aborted;
                }
            }
            AcrStageState::Finished | AcrStageState::Aborted | AcrStageState::Recovery => {
                let late_official_finish = self.state == AcrStageState::Finished
                    && self.manual_finish_awaiting_official
                    && official_time_ms.is_some();
                if late_official_finish {
                    self.manual_finish_awaiting_official = false;
                    event = AcrStageEvent::Finished;
                } else {
                    let finished_at_low_distance = self.state == AcrStageState::Finished
                        && self.max_distance_m >= STAGE_RESET_MIN_PREVIOUS_DISTANCE_M
                        && distance_m < self.max_distance_m * 0.25;
                    let failed_attempt_restarted_near_start = self.state != AcrStageState::Finished
                        && engine_running
                        && distance_m <= 100.0;
                    let next_attempt = live
                        && (reset
                            || finished_at_low_distance
                            || failed_attempt_restarted_near_start);
                    if next_attempt {
                        self.stage_number = self.stage_number.saturating_add(1).max(1);
                        self.started_at = now;
                        self.max_distance_m = distance_m;
                        self.manual_finish_awaiting_official = false;
                        self.attempt_invalid = false;
                        self.state = AcrStageState::Countdown;
                        event = AcrStageEvent::NextAttempt;
                    }
                }
            }
        }

        self.max_distance_m = self.max_distance_m.max(distance_m);
        self.last_distance_m = Some(distance_m);
        self.last_official_time_ms = graphics.last_time_ms;
        self.last_completed_laps = graphics.completed_laps;
        self.last_status = graphics.status;

        let elapsed_s = if let Some(official_time_ms) = official_time_ms {
            official_time_ms as f32 / 1_000.0
        } else if graphics.current_time_ms > 0
            && matches!(self.state, AcrStageState::Running | AcrStageState::Finished)
        {
            graphics.current_time_ms as f32 / 1_000.0
        } else {
            now.duration_since(self.started_at).as_secs_f32()
        };

        StageObservation {
            stage_number: self.stage_number,
            elapsed_s,
            reset,
            state: self.state,
            event,
            official_time_ms,
            max_distance_m: self.max_distance_m,
            attempt_invalid: self.attempt_invalid,
            result_screen_entered,
        }
    }
}

fn select_acr_finish_distance(
    explicit_manual_finish_distance_m: Option<f32>,
    _static_track_length_m: f32,
) -> Option<f32> {
    // Static length can describe the course for analysis, but it is not finish evidence.
    explicit_manual_finish_distance_m
}

#[derive(Default)]
struct AcrFrameValidator {
    previous: Option<(AcrPhysicsSnapshot, AcrGraphicsSnapshot, Instant)>,
    rejected_frames: u64,
}

impl AcrFrameValidator {
    fn reset(&mut self) {
        self.previous = None;
    }

    fn validate(
        &mut self,
        physics: &AcrPhysicsSnapshot,
        graphics: &AcrGraphicsSnapshot,
        statics: &AcrStaticSnapshot,
        now: Instant,
    ) -> Result<(), String> {
        let max_rpm = physics.current_max_rpm.max(statics.max_rpm).max(10_000) as f32;
        let scalar_values = [
            physics.throttle,
            physics.brake,
            physics.steer,
            physics.speed_kmh,
            physics.rpm as f32,
            graphics.distance_m,
        ];
        let arrays_are_finite = physics
            .g_force
            .iter()
            .chain(physics.velocity.iter())
            .chain(physics.local_velocity.iter())
            .chain(physics.wheel_slip.iter())
            .chain(physics.slip_ratio.iter())
            .chain(physics.slip_angle.iter())
            .all(|value| value.is_finite());
        let plausible = scalar_values.iter().all(|value| value.is_finite())
            && arrays_are_finite
            && (-0.05..=1.05).contains(&physics.throttle)
            && (-0.05..=1.05).contains(&physics.brake)
            && physics.steer.abs() <= 2.0
            && (0.0..=450.0).contains(&physics.speed_kmh)
            && physics.rpm >= 0
            && physics.rpm as f32 <= max_rpm * 1.6
            && (0..=13).contains(&physics.raw_gear)
            && physics.g_force.iter().all(|value| value.abs() <= 25.0)
            && graphics.distance_m >= -50.0
            && graphics.distance_m <= 100_000.0;
        if !plausible {
            self.rejected_frames = self.rejected_frames.saturating_add(1);
            return Err(format!(
                "rejected implausible ACR frame packet={} speed={:.1} rpm={} distance={:.1}",
                physics.packet_id, physics.speed_kmh, physics.rpm, graphics.distance_m
            ));
        }

        if let Some((previous, _, timestamp)) = &self.previous {
            let elapsed = now.duration_since(*timestamp).as_secs_f32();
            let speed_jump = (physics.speed_kmh - previous.speed_kmh).abs();
            let rpm_jump = (physics.rpm - previous.rpm).abs();
            let g_force_jump = physics
                .g_force
                .iter()
                .zip(previous.g_force.iter())
                .map(|(current, previous)| (current - previous).abs())
                .fold(0.0_f32, f32::max);
            if elapsed <= 0.1
                && (speed_jump > 120.0
                    || (physics.raw_gear == previous.raw_gear && rpm_jump > 10_000)
                    || g_force_jump > 12.0)
            {
                self.rejected_frames = self.rejected_frames.saturating_add(1);
                return Err(format!(
                    "rejected discontinuous ACR frame packet={} speed_jump={speed_jump:.1} rpm_jump={rpm_jump} g_jump={g_force_jump:.1}",
                    physics.packet_id
                ));
            }
        }

        self.previous = Some((physics.clone(), graphics.clone(), now));
        Ok(())
    }
}

fn observe_validated_stage(
    tracker: &mut AcrStageTracker,
    validator: &mut AcrFrameValidator,
    physics: &AcrPhysicsSnapshot,
    graphics: &AcrGraphicsSnapshot,
    statics: &AcrStaticSnapshot,
    now: Instant,
) -> Result<(StageObservation, bool), String> {
    if graphics.status == 2 {
        if let Err(error) = validator.validate(physics, graphics, statics, now) {
            if tracker.has_new_official_finish(graphics) {
                return Ok((
                    tracker.observe(graphics, physics.speed_kmh, physics.engine_running, now),
                    false,
                ));
            }
            return Err(error);
        }
    } else {
        // Official terminal state may arrive after live physics has stopped updating.
        validator.reset();
    }
    Ok((
        tracker.observe(graphics, physics.speed_kmh, physics.engine_running, now),
        true,
    ))
}

#[derive(Default)]
struct AcrGraphicsFreshness {
    last_physics_packet_id: Option<i32>,
    last_graphics_packet_id: Option<i32>,
    last_physics_change_at: Option<Instant>,
    last_graphics_change_at: Option<Instant>,
}

impl AcrGraphicsFreshness {
    fn observe(
        &mut self,
        physics_packet_id: i32,
        graphics: &AcrGraphicsSnapshot,
        now: Instant,
    ) -> bool {
        let physics_changed = self.last_physics_packet_id != Some(physics_packet_id);
        let graphics_changed = self.last_graphics_packet_id != Some(graphics.packet_id);
        if physics_changed {
            self.last_physics_packet_id = Some(physics_packet_id);
            self.last_physics_change_at = Some(now);
        }
        if graphics_changed {
            self.last_graphics_packet_id = Some(graphics.packet_id);
            self.last_graphics_change_at = Some(now);
        }
        if graphics.status != 2 || (!physics_changed && !graphics_changed) {
            return true;
        }
        self.last_physics_change_at
            .zip(self.last_graphics_change_at)
            .is_some_and(|(physics_at, graphics_at)| {
                let skew = if physics_at >= graphics_at {
                    physics_at.duration_since(graphics_at)
                } else {
                    graphics_at.duration_since(physics_at)
                };
                skew <= MAX_LIVE_MAPPING_SKEW
            })
    }

    fn reset(&mut self) {
        self.last_physics_packet_id = None;
        self.last_graphics_packet_id = None;
        self.last_physics_change_at = None;
        self.last_graphics_change_at = None;
    }
}

struct AcrCaptureGate {
    interval: Duration,
    last_capture_at: Option<Instant>,
}

impl AcrCaptureGate {
    fn new(rate_hz: u16) -> Self {
        let rate_hz = rate_hz.clamp(20, 50) as u64;
        Self {
            interval: Duration::from_nanos(1_000_000_000 / rate_hz),
            last_capture_at: None,
        }
    }

    fn should_capture(&mut self, event: AcrStageEvent, now: Instant) -> bool {
        let due = self
            .last_capture_at
            .is_none_or(|last| now.duration_since(last) >= self.interval);
        if due || event != AcrStageEvent::None {
            self.last_capture_at = Some(now);
            true
        } else {
            false
        }
    }

    fn reset(&mut self) {
        self.last_capture_at = None;
    }
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
struct AcrSessionContext {
    identity: SessionIdentity,
}

impl AcrSessionContext {
    fn from_static(statics: &AcrStaticSnapshot) -> Option<Self> {
        let track = statics.track.trim();
        let car = statics.car_model.trim();
        (!track.is_empty() && !car.is_empty()).then(|| Self {
            identity: SessionIdentity::new("acr", track, "stage").with_vehicle(car),
        })
    }

    fn slug(&self) -> String {
        self.identity.storage_slug()
    }

    fn track(&self) -> &str {
        self.identity.track_name()
    }

    fn car(&self) -> &str {
        self.identity.vehicle_name().unwrap_or_default()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum AcrAttemptOutcome {
    Finished,
    Aborted,
    Recovery,
}

impl AcrAttemptOutcome {
    fn from_event(event: AcrStageEvent) -> Option<Self> {
        match event {
            AcrStageEvent::Finished => Some(Self::Finished),
            AcrStageEvent::Aborted => Some(Self::Aborted),
            AcrStageEvent::Recovery => Some(Self::Recovery),
            AcrStageEvent::None
            | AcrStageEvent::Started
            | AcrStageEvent::ResultScreen
            | AcrStageEvent::NextAttempt => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Finished => "finished",
            Self::Aborted => "aborted",
            Self::Recovery => "recovery",
        }
    }
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
struct AcrTracePoint {
    elapsed_s: f32,
    distance_m: f32,
    speed_kmh: f32,
    rpm: i32,
    max_rpm: i32,
    gear: i8,
    throttle: f32,
    brake: f32,
    steer: f32,
    peak_wheel_slip: f32,
}

impl AcrTracePoint {
    fn from_snapshots(
        physics: &AcrPhysicsSnapshot,
        graphics: &AcrGraphicsSnapshot,
        statics: &AcrStaticSnapshot,
        stage: StageObservation,
    ) -> Self {
        let peak_wheel_slip = physics
            .slip_ratio
            .iter()
            .chain(physics.wheel_slip.iter())
            .filter(|value| value.is_finite())
            .map(|value| value.abs())
            .fold(0.0_f32, f32::max);
        Self {
            elapsed_s: stage.elapsed_s,
            distance_m: finite_nonnegative(graphics.distance_m),
            speed_kmh: finite_nonnegative(physics.speed_kmh),
            rpm: physics.rpm.max(0),
            max_rpm: physics.current_max_rpm.max(statics.max_rpm).max(1),
            gear: acr_gear(physics.raw_gear),
            throttle: clamp_unit(physics.throttle),
            brake: clamp_unit(physics.brake),
            steer: finite_or_zero(physics.steer).clamp(-1.0, 1.0),
            peak_wheel_slip,
        }
    }
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
struct AcrAttemptTrace {
    stage_number: u8,
    track: String,
    car: String,
    outcome: AcrAttemptOutcome,
    elapsed_s: f32,
    official_time_ms: Option<u32>,
    max_distance_m: f32,
    track_length_m: f32,
    invalid: bool,
    validator_drops: u64,
    archive_backpressure_drops: u64,
    points: Vec<AcrTracePoint>,
}

#[derive(Clone, Debug, Default, serde::Serialize)]
struct AcrHabitMetrics {
    brake_applications: usize,
    brake_throttle_overlap_samples: usize,
    abrupt_throttle_transitions: usize,
    abrupt_steering_transitions: usize,
    gear_hunts: usize,
    recovery_attempts_in_history: usize,
}

#[derive(Clone, Debug, serde::Serialize)]
struct AcrHabitSummary {
    metrics: AcrHabitMetrics,
    repeated_habits: Vec<String>,
}

#[derive(Clone, Debug, serde::Serialize)]
struct AcrAttemptComparison {
    baseline_stage: u8,
    baseline_outcome: AcrAttemptOutcome,
    common_distance_m: f32,
    latest_time_s: f32,
    baseline_time_s: f32,
    delta_s: f32,
    latest_sample_count: usize,
    baseline_sample_count: usize,
    confidence: f32,
    confidence_level: AnalysisConfidenceLevel,
    confidence_reasons: Vec<AnalysisLimitation>,
}

#[derive(Clone, Debug, Default, serde::Serialize)]
struct AcrLearningTrend {
    completed_attempts: usize,
    delta_to_previous_completion_s: Option<f32>,
    delta_to_first_completion_s: Option<f32>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum AcrOverrevClassification {
    None,
    MechanicalRiskUnverified,
    TechniqueGainWithMechanicalRisk,
    MechanicalRiskWithTimeLoss,
    MechanicalRiskNeutral,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum AcrEngineBrakingClassification {
    None,
    Unverified,
    ControlledGain,
    TimeLoss,
    Neutral,
}

#[derive(Clone, Debug, serde::Serialize)]
struct AcrEngineBrakingAssessment {
    engine_braking_detected: bool,
    event_count: usize,
    classification: AcrEngineBrakingClassification,
    entry_segment_delta_s: Option<f32>,
    next_segment_delta_s: Option<f32>,
    exit_speed_delta_kmh: Option<f32>,
    gear_continuity: bool,
    peak_wheel_slip: f32,
}

impl Default for AcrEngineBrakingAssessment {
    fn default() -> Self {
        Self {
            engine_braking_detected: false,
            event_count: 0,
            classification: AcrEngineBrakingClassification::None,
            entry_segment_delta_s: None,
            next_segment_delta_s: None,
            exit_speed_delta_kmh: None,
            gear_continuity: true,
            peak_wheel_slip: 0.0,
        }
    }
}

#[derive(Clone, Debug, serde::Serialize)]
struct AcrOverrevAssessment {
    event_count: usize,
    classification: AcrOverrevClassification,
    mechanical_risk: bool,
    technique_gain: bool,
    segment_delta_s: Option<f32>,
    next_segment_delta_s: Option<f32>,
    exit_speed_delta_kmh: Option<f32>,
    gear_continuity: bool,
    peak_wheel_slip: f32,
}

impl Default for AcrOverrevAssessment {
    fn default() -> Self {
        Self {
            event_count: 0,
            classification: AcrOverrevClassification::None,
            mechanical_risk: false,
            technique_gain: false,
            segment_delta_s: None,
            next_segment_delta_s: None,
            exit_speed_delta_kmh: None,
            gear_continuity: true,
            peak_wheel_slip: 0.0,
        }
    }
}

#[derive(Clone, Debug, serde::Serialize)]
struct AcrStageReport {
    schema_version: u8,
    stage_number: u8,
    track: String,
    car: String,
    outcome: AcrAttemptOutcome,
    elapsed_s: f32,
    official_time_ms: Option<u32>,
    max_distance_m: f32,
    invalid: bool,
    archive_backpressure_drops: u64,
    quality: TraceQuality,
    quality_evidence: Vec<String>,
    target_time_s: Option<f32>,
    target_delta_s: Option<f32>,
    comparisons: Vec<AcrAttemptComparison>,
    learning_trend: AcrLearningTrend,
    habits: AcrHabitSummary,
    overrev: AcrOverrevAssessment,
    engine_braking: AcrEngineBrakingAssessment,
    trace: AcrAttemptTrace,
}

#[derive(Default)]
struct AcrStageAnalyzer {
    target_time_s: Option<f32>,
    current_stage_number: Option<u8>,
    current_points: Vec<AcrTracePoint>,
    current_validator_drop_start: u64,
    current_archive_drop_start: u64,
    provisional_finish_history_index: Option<usize>,
    history: Vec<AcrAttemptTrace>,
}

impl AcrStageAnalyzer {
    fn new(target_time_s: Option<f32>) -> Self {
        Self {
            target_time_s: target_time_s.filter(|time| time.is_finite() && *time > 0.0),
            ..Self::default()
        }
    }

    fn reset_context(&mut self) {
        self.current_stage_number = None;
        self.current_points.clear();
        self.current_validator_drop_start = 0;
        self.current_archive_drop_start = 0;
        self.provisional_finish_history_index = None;
        self.history.clear();
    }

    fn restore_history(&mut self, mut history: Vec<AcrAttemptTrace>) {
        if history.len() > 32 {
            history.drain(..history.len() - 32);
        }
        self.provisional_finish_history_index = None;
        self.history = history;
    }

    fn ingest(
        &mut self,
        physics: &AcrPhysicsSnapshot,
        graphics: &AcrGraphicsSnapshot,
        statics: &AcrStaticSnapshot,
        stage: StageObservation,
        include_sample: bool,
        dropped_frames: CaptureCounters,
    ) -> Option<AcrStageReport> {
        if stage.event == AcrStageEvent::Started {
            self.provisional_finish_history_index = None;
            self.current_stage_number = Some(stage.stage_number);
            self.current_points.clear();
            self.current_validator_drop_start = dropped_frames.rejected_frames;
            self.current_archive_drop_start = dropped_frames.persistence_dropped_frames;
        }
        if stage.event == AcrStageEvent::NextAttempt {
            self.provisional_finish_history_index = None;
        }
        if self.current_stage_number.is_none() && stage.state == AcrStageState::Running {
            self.current_stage_number = Some(stage.stage_number);
            self.current_validator_drop_start = dropped_frames.rejected_frames;
            self.current_archive_drop_start = dropped_frames.persistence_dropped_frames;
        }

        let terminal = AcrAttemptOutcome::from_event(stage.event);
        if include_sample
            && self.current_stage_number == Some(stage.stage_number)
            && (stage.state == AcrStageState::Running || stage.event == AcrStageEvent::Finished)
        {
            self.current_points.push(AcrTracePoint::from_snapshots(
                physics, graphics, statics, stage,
            ));
        }

        let outcome = terminal?;
        let elapsed_s = stage
            .official_time_ms
            .map(|time| time as f32 / 1_000.0)
            .unwrap_or(stage.elapsed_s);
        // A manual finish is provisional until the game publishes its official result. Replace
        // that same history slot so coaching and persisted reports still represent one attempt.
        let provisional_finish_index =
            if outcome == AcrAttemptOutcome::Finished && stage.official_time_ms.is_some() {
                self.provisional_finish_history_index.filter(|index| {
                    self.history.get(*index).is_some_and(|attempt| {
                        attempt.stage_number == stage.stage_number
                            && attempt.outcome == AcrAttemptOutcome::Finished
                            && attempt.official_time_ms.is_none()
                    })
                })
            } else {
                None
            };
        if let Some(index) = provisional_finish_index {
            let mut attempt = self.history.remove(index);
            attempt.elapsed_s = elapsed_s;
            attempt.official_time_ms = stage.official_time_ms;
            attempt.max_distance_m = attempt.max_distance_m.max(stage.max_distance_m);
            attempt.track_length_m = finite_nonnegative(statics.track_length_m);
            attempt.invalid = stage.attempt_invalid;
            let report = self.build_report(&attempt);
            self.history.insert(index, attempt);
            self.provisional_finish_history_index = None;
            return Some(report);
        }
        let attempt = AcrAttemptTrace {
            stage_number: stage.stage_number,
            track: statics.track.clone(),
            car: statics.car_model.clone(),
            outcome,
            elapsed_s,
            official_time_ms: stage.official_time_ms,
            max_distance_m: self
                .current_points
                .iter()
                .map(|point| point.distance_m)
                .fold(stage.max_distance_m, f32::max),
            track_length_m: finite_nonnegative(statics.track_length_m),
            invalid: stage.attempt_invalid,
            validator_drops: dropped_frames
                .rejected_frames
                .saturating_sub(self.current_validator_drop_start),
            archive_backpressure_drops: dropped_frames
                .persistence_dropped_frames
                .saturating_sub(self.current_archive_drop_start),
            points: std::mem::take(&mut self.current_points),
        };
        self.current_stage_number = None;
        let report = self.build_report(&attempt);
        let provisional_finish =
            outcome == AcrAttemptOutcome::Finished && stage.official_time_ms.is_none();
        self.history.push(attempt);
        if self.history.len() > 32 {
            self.history.remove(0);
        }
        self.provisional_finish_history_index = if provisional_finish {
            self.history.len().checked_sub(1)
        } else {
            None
        };
        Some(report)
    }

    fn build_report(&self, attempt: &AcrAttemptTrace) -> AcrStageReport {
        let mut comparisons = Vec::new();
        if let Some(previous_completion) = self
            .history
            .iter()
            .rev()
            .find(|previous| attempt_is_reference_usable(previous))
            && let Some(comparison) = compare_attempts(attempt, previous_completion)
        {
            comparisons.push(comparison);
        }
        if let Some(previous_failure) = self.history.iter().rev().find(|previous| {
            matches!(
                previous.outcome,
                AcrAttemptOutcome::Aborted | AcrAttemptOutcome::Recovery
            )
        }) && let Some(comparison) = compare_attempts(attempt, previous_failure)
        {
            comparisons.push(comparison);
        }

        let completed = self
            .history
            .iter()
            .filter(|previous| attempt_is_reference_usable(previous))
            .collect::<Vec<_>>();
        let current_reference_usable = attempt_is_reference_usable(attempt);
        let learning_trend = if current_reference_usable {
            AcrLearningTrend {
                completed_attempts: completed.len() + 1,
                delta_to_previous_completion_s: completed
                    .last()
                    .map(|previous| attempt.elapsed_s - previous.elapsed_s),
                delta_to_first_completion_s: completed
                    .first()
                    .map(|first| attempt.elapsed_s - first.elapsed_s),
            }
        } else {
            AcrLearningTrend {
                completed_attempts: completed.len(),
                ..AcrLearningTrend::default()
            }
        };

        let metrics = habit_metrics(attempt, &self.history);
        let repeated_habits = repeated_habits(attempt, &metrics, &self.history);
        let baseline = self
            .history
            .iter()
            .rev()
            .find(|previous| attempt_is_reference_usable(previous));
        let (quality, quality_evidence) = assess_attempt_quality(attempt);

        AcrStageReport {
            schema_version: ARCHIVE_SCHEMA_VERSION,
            stage_number: attempt.stage_number,
            track: attempt.track.clone(),
            car: attempt.car.clone(),
            outcome: attempt.outcome,
            elapsed_s: attempt.elapsed_s,
            official_time_ms: attempt.official_time_ms,
            max_distance_m: attempt.max_distance_m,
            invalid: attempt.invalid,
            archive_backpressure_drops: attempt.archive_backpressure_drops,
            quality,
            quality_evidence,
            target_time_s: self.target_time_s,
            target_delta_s: (attempt.outcome == AcrAttemptOutcome::Finished)
                .then(|| self.target_time_s.map(|target| attempt.elapsed_s - target))
                .flatten(),
            comparisons,
            learning_trend,
            habits: AcrHabitSummary {
                metrics,
                repeated_habits,
            },
            overrev: assess_overrev(attempt, baseline),
            engine_braking: assess_engine_braking(attempt, baseline),
            trace: attempt.clone(),
        }
    }
}

fn attempt_is_reference_usable(attempt: &AcrAttemptTrace) -> bool {
    attempt.outcome == AcrAttemptOutcome::Finished
        && assess_attempt_quality(attempt).0.is_reference_usable()
}

fn compare_attempts(
    latest: &AcrAttemptTrace,
    baseline: &AcrAttemptTrace,
) -> Option<AcrAttemptComparison> {
    let latest_max = trace_max_distance(&latest.points);
    let baseline_max = trace_max_distance(&baseline.points);
    let common_distance_m = latest_max.min(baseline_max);
    if common_distance_m <= 0.0 {
        return None;
    }
    let latest_time_s = elapsed_at_distance(&latest.points, common_distance_m)?;
    let baseline_time_s = elapsed_at_distance(&baseline.points, common_distance_m)?;
    let confidence = comparison_confidence(latest, baseline, common_distance_m);
    Some(AcrAttemptComparison {
        baseline_stage: baseline.stage_number,
        baseline_outcome: baseline.outcome,
        common_distance_m,
        latest_time_s,
        baseline_time_s,
        delta_s: latest_time_s - baseline_time_s,
        latest_sample_count: latest.points.len(),
        baseline_sample_count: baseline.points.len(),
        confidence: confidence.score,
        confidence_level: confidence.level,
        confidence_reasons: confidence.limitations,
    })
}

fn comparison_confidence(
    latest: &AcrAttemptTrace,
    baseline: &AcrAttemptTrace,
    common_distance_m: f32,
) -> AnalysisConfidence {
    let longest_distance = trace_max_distance(&latest.points)
        .max(trace_max_distance(&baseline.points))
        .max(1.0);
    let coverage = (common_distance_m / longest_distance).clamp(0.0, 1.0);
    let samples = latest.points.len().min(baseline.points.len()) as f32;
    let sample_factor = (samples / 50.0).clamp(0.0, 1.0);
    let validity_factor = if latest.invalid || baseline.invalid {
        0.65
    } else {
        1.0
    };
    let mut limitations = Vec::new();
    if latest.points.len().min(baseline.points.len()) < 50 {
        limitations.push(AnalysisLimitation::LimitedCommonSamples);
    }
    if common_distance_m / longest_distance < 0.8 {
        limitations.push(AnalysisLimitation::PartialCommonDistance);
    }
    if latest.invalid || baseline.invalid {
        limitations.push(AnalysisLimitation::ComparisonIncludesFailedOrInvalidAttempt);
    }
    if latest.validator_drops > 0 || baseline.validator_drops > 0 {
        limitations.push(AnalysisLimitation::ValidatorDropsPresent);
    }
    if latest.archive_backpressure_drops > 0 || baseline.archive_backpressure_drops > 0 {
        limitations.push(AnalysisLimitation::ArchiveBackpressureDropsPresent);
    }
    AnalysisConfidence::from_score(
        (coverage * 0.55 + sample_factor * 0.45) * validity_factor,
        limitations,
    )
}

fn assess_attempt_quality(attempt: &AcrAttemptTrace) -> (TraceQuality, Vec<String>) {
    let completed = attempt.outcome == AcrAttemptOutcome::Finished;
    let track_length_m = if attempt.track_length_m >= 250.0 {
        attempt.track_length_m
    } else {
        attempt.max_distance_m
    };
    let mut samples = attempt
        .points
        .iter()
        .map(|point| QualitySample {
            session_time_s: f64::from(point.elapsed_s),
            elapsed_s: f64::from(point.elapsed_s),
            distance_m: f64::from(point.distance_m),
            speed_kmh: f64::from(point.speed_kmh),
            rpm: f64::from(point.rpm),
            gear: i32::from(point.gear),
            lateral_g: 0.0,
            longitudinal_g: 0.0,
        })
        .collect::<Vec<_>>();

    // Some result screens reset stage distance in the same frame as the official finish.
    // Preserve the tracker's last observed maximum without treating that reset as telemetry.
    if completed
        && let Some(last) = samples.last_mut()
        && f64::from(attempt.max_distance_m) > last.distance_m + 1.0
    {
        if f64::from(attempt.elapsed_s) > last.elapsed_s + f64::EPSILON {
            let mut endpoint = *last;
            endpoint.session_time_s = f64::from(attempt.elapsed_s);
            endpoint.elapsed_s = f64::from(attempt.elapsed_s);
            endpoint.distance_m = f64::from(attempt.max_distance_m);
            samples.push(endpoint);
        } else {
            last.distance_m = f64::from(attempt.max_distance_m);
        }
    }

    let mut quality = assess_trace(
        &samples,
        f64::from(track_length_m),
        attempt.official_time_ms,
        attempt.invalid,
        completed,
    );
    let mut evidence = Vec::new();
    if attempt.invalid {
        evidence.push("attempt_marked_failed_or_invalid".to_owned());
    }
    if attempt.points.len() < 20 {
        degrade_quality(
            &mut quality,
            QualityReason::SparseSamples,
            25,
            TraceQualityStatus::Partial,
        );
        evidence.push(format!("limited_samples:{}", attempt.points.len()));
    }
    if quality.coverage_ratio < 0.9 {
        evidence.push(format!("stage_coverage:{:.3}", quality.coverage_ratio));
    }
    if attempt.validator_drops > 0 {
        degrade_quality(
            &mut quality,
            QualityReason::SampleGap,
            (attempt.validator_drops.saturating_mul(2)).min(20) as u8,
            TraceQualityStatus::Partial,
        );
        evidence.push(format!("validator_drops:{}", attempt.validator_drops));
    }
    if attempt.archive_backpressure_drops > 0 {
        degrade_quality(
            &mut quality,
            QualityReason::SampleGap,
            (attempt.archive_backpressure_drops.saturating_mul(2)).min(20) as u8,
            TraceQualityStatus::Partial,
        );
        evidence.push(format!(
            "archive_backpressure_drops:{}",
            attempt.archive_backpressure_drops
        ));
    }
    if evidence.is_empty() {
        evidence.push("complete_continuous_trace".to_owned());
    }
    (quality, evidence)
}

fn degrade_quality(
    quality: &mut TraceQuality,
    reason: QualityReason,
    penalty: u8,
    status: TraceQualityStatus,
) {
    if !quality.reasons.contains(&reason) {
        quality.reasons.push(reason);
        quality.score = quality.score.saturating_sub(penalty);
    }
    if quality.status != TraceQualityStatus::Rejected
        && status == TraceQualityStatus::Partial
        && quality.status != TraceQualityStatus::Partial
    {
        quality.status = TraceQualityStatus::Partial;
    }
}

fn trace_max_distance(points: &[AcrTracePoint]) -> f32 {
    points
        .iter()
        .map(|point| point.distance_m)
        .fold(0.0_f32, f32::max)
}

fn elapsed_at_distance(points: &[AcrTracePoint], distance_m: f32) -> Option<f32> {
    let first = points.first()?;
    if distance_m <= first.distance_m {
        return Some(first.elapsed_s);
    }
    for pair in points.windows(2) {
        let (previous, current) = (&pair[0], &pair[1]);
        if current.distance_m >= distance_m && current.distance_m > previous.distance_m {
            let ratio =
                (distance_m - previous.distance_m) / (current.distance_m - previous.distance_m);
            return Some(previous.elapsed_s + ratio * (current.elapsed_s - previous.elapsed_s));
        }
    }
    points
        .iter()
        .rev()
        .find(|point| point.distance_m <= distance_m + 0.5)
        .map(|point| point.elapsed_s)
}

fn speed_at_distance(points: &[AcrTracePoint], distance_m: f32) -> Option<f32> {
    points
        .iter()
        .min_by(|left, right| {
            (left.distance_m - distance_m)
                .abs()
                .total_cmp(&(right.distance_m - distance_m).abs())
        })
        .map(|point| point.speed_kmh)
}

fn duration_between(points: &[AcrTracePoint], start_m: f32, end_m: f32) -> Option<f32> {
    Some(elapsed_at_distance(points, end_m)? - elapsed_at_distance(points, start_m)?)
}

fn habit_metrics(attempt: &AcrAttemptTrace, history: &[AcrAttemptTrace]) -> AcrHabitMetrics {
    let mut metrics = AcrHabitMetrics {
        recovery_attempts_in_history: history
            .iter()
            .filter(|previous| previous.outcome == AcrAttemptOutcome::Recovery)
            .count()
            + usize::from(attempt.outcome == AcrAttemptOutcome::Recovery),
        ..AcrHabitMetrics::default()
    };
    for pair in attempt.points.windows(2) {
        let (previous, current) = (&pair[0], &pair[1]);
        metrics.brake_applications += usize::from(previous.brake <= 0.1 && current.brake >= 0.5);
        metrics.brake_throttle_overlap_samples +=
            usize::from(current.brake >= 0.2 && current.throttle >= 0.2);
        metrics.abrupt_throttle_transitions +=
            usize::from((current.throttle - previous.throttle).abs() >= 0.55);
        metrics.abrupt_steering_transitions +=
            usize::from((current.steer - previous.steer).abs() >= 0.65);
    }
    for triple in attempt.points.windows(3) {
        metrics.gear_hunts += usize::from(
            triple[0].gear == triple[2].gear
                && triple[0].gear != triple[1].gear
                && triple[0].gear > 0,
        );
    }
    metrics
}

fn repeated_habits(
    attempt: &AcrAttemptTrace,
    metrics: &AcrHabitMetrics,
    history: &[AcrAttemptTrace],
) -> Vec<String> {
    let mut findings = Vec::new();
    if attempt_is_reference_usable(attempt) {
        findings.extend(repeated_technique_habits(metrics, history));
    }
    // Recovery frequency is an outcome diagnostic, not a technique comparison. Keep it
    // available even when the current or prior attempts are not usable quality references.
    if metrics.recovery_attempts_in_history >= 2 {
        findings.push("repeated_recovery".to_owned());
    }
    findings
}

fn repeated_technique_habits(
    metrics: &AcrHabitMetrics,
    history: &[AcrAttemptTrace],
) -> Vec<String> {
    let previous = history
        .iter()
        .rev()
        .filter(|attempt| attempt_is_reference_usable(attempt))
        .take(3)
        .map(|attempt| habit_metrics(attempt, &[]))
        .collect::<Vec<_>>();
    let repeated = |current: usize, select: fn(&AcrHabitMetrics) -> usize, threshold: usize| {
        current >= threshold && previous.iter().any(|metrics| select(metrics) >= threshold)
    };
    let mut findings = Vec::new();
    if repeated(
        metrics.brake_applications,
        |metrics| metrics.brake_applications,
        2,
    ) {
        findings.push("repeated_brake_applications".to_owned());
    }
    if repeated(
        metrics.brake_throttle_overlap_samples,
        |m| m.brake_throttle_overlap_samples,
        3,
    ) {
        findings.push("repeated_brake_throttle_overlap".to_owned());
    }
    if repeated(
        metrics.abrupt_throttle_transitions,
        |m| m.abrupt_throttle_transitions,
        3,
    ) {
        findings.push("repeated_abrupt_throttle".to_owned());
    }
    if repeated(
        metrics.abrupt_steering_transitions,
        |m| m.abrupt_steering_transitions,
        3,
    ) {
        findings.push("repeated_abrupt_steering".to_owned());
    }
    if repeated(metrics.gear_hunts, |m| m.gear_hunts, 1) {
        findings.push("repeated_gear_hunting".to_owned());
    }
    findings
}

fn assess_overrev(
    attempt: &AcrAttemptTrace,
    baseline: Option<&AcrAttemptTrace>,
) -> AcrOverrevAssessment {
    let overrev = |point: &AcrTracePoint| point.rpm as f32 > point.max_rpm as f32 * 1.02;
    let event_count = attempt
        .points
        .iter()
        .enumerate()
        .filter(|(index, point)| {
            overrev(point) && (*index == 0 || !overrev(&attempt.points[*index - 1]))
        })
        .count();
    if event_count == 0 {
        return AcrOverrevAssessment::default();
    }
    let first_index = attempt.points.iter().position(overrev).unwrap_or(0);
    let event_distance = attempt.points[first_index].distance_m;
    let start_m = (event_distance - 25.0).max(0.0);
    let exit_m = event_distance + 100.0;
    let next_m = event_distance + 250.0;
    let window = attempt
        .points
        .iter()
        .filter(|point| (start_m..=next_m).contains(&point.distance_m))
        .collect::<Vec<_>>();
    let gear_continuity = !window.windows(3).any(|triple| {
        triple[0].gear == triple[2].gear && triple[0].gear != triple[1].gear && triple[0].gear > 0
    });
    let peak_wheel_slip = window
        .iter()
        .map(|point| point.peak_wheel_slip)
        .fold(0.0_f32, f32::max);
    let Some(baseline) = baseline else {
        return AcrOverrevAssessment {
            event_count,
            classification: AcrOverrevClassification::MechanicalRiskUnverified,
            mechanical_risk: true,
            technique_gain: false,
            segment_delta_s: None,
            next_segment_delta_s: None,
            exit_speed_delta_kmh: None,
            gear_continuity,
            peak_wheel_slip,
        };
    };
    let segment_delta_s = duration_between(&attempt.points, start_m, exit_m)
        .zip(duration_between(&baseline.points, start_m, exit_m))
        .map(|(latest, previous)| latest - previous);
    let next_segment_delta_s = duration_between(&attempt.points, exit_m, next_m)
        .zip(duration_between(&baseline.points, exit_m, next_m))
        .map(|(latest, previous)| latest - previous);
    let exit_speed_delta_kmh = speed_at_distance(&attempt.points, exit_m)
        .zip(speed_at_distance(&baseline.points, exit_m))
        .map(|(latest, previous)| latest - previous);
    let technique_gain = segment_delta_s.is_some_and(|delta| delta < -0.05)
        && next_segment_delta_s.is_none_or(|delta| delta <= 0.05)
        && exit_speed_delta_kmh.is_none_or(|delta| delta >= -2.0)
        && gear_continuity
        && peak_wheel_slip <= 1.0;
    let classification = if technique_gain {
        AcrOverrevClassification::TechniqueGainWithMechanicalRisk
    } else if segment_delta_s.is_some_and(|delta| delta > 0.05)
        || next_segment_delta_s.is_some_and(|delta| delta > 0.05)
    {
        AcrOverrevClassification::MechanicalRiskWithTimeLoss
    } else {
        AcrOverrevClassification::MechanicalRiskNeutral
    };
    AcrOverrevAssessment {
        event_count,
        classification,
        mechanical_risk: true,
        technique_gain,
        segment_delta_s,
        next_segment_delta_s,
        exit_speed_delta_kmh,
        gear_continuity,
        peak_wheel_slip,
    }
}

fn assess_engine_braking(
    attempt: &AcrAttemptTrace,
    baseline: Option<&AcrAttemptTrace>,
) -> AcrEngineBrakingAssessment {
    let event_indices = attempt
        .points
        .windows(2)
        .enumerate()
        .filter_map(|(index, pair)| {
            let previous = &pair[0];
            let current = &pair[1];
            (current.throttle <= 0.12
                && current.brake <= 0.12
                && previous.gear > current.gear
                && current.gear > 0
                && current.speed_kmh + 0.5 < previous.speed_kmh)
                .then_some(index + 1)
        })
        .collect::<Vec<_>>();
    let Some(first_index) = event_indices.first().copied() else {
        return AcrEngineBrakingAssessment::default();
    };
    let event_distance = attempt.points[first_index].distance_m;
    let start_m = (event_distance - 50.0).max(0.0);
    let exit_m = event_distance + 100.0;
    let next_m = event_distance + 250.0;
    let window = attempt
        .points
        .iter()
        .filter(|point| (start_m..=next_m).contains(&point.distance_m))
        .collect::<Vec<_>>();
    let gear_continuity = !window
        .windows(3)
        .any(|triple| triple[0].gear == triple[2].gear && triple[0].gear != triple[1].gear);
    let peak_wheel_slip = window
        .iter()
        .map(|point| point.peak_wheel_slip)
        .fold(0.0_f32, f32::max);
    let Some(baseline) = baseline else {
        return AcrEngineBrakingAssessment {
            engine_braking_detected: true,
            event_count: event_indices.len(),
            classification: AcrEngineBrakingClassification::Unverified,
            entry_segment_delta_s: None,
            next_segment_delta_s: None,
            exit_speed_delta_kmh: None,
            gear_continuity,
            peak_wheel_slip,
        };
    };
    let entry_segment_delta_s = duration_between(&attempt.points, start_m, exit_m)
        .zip(duration_between(&baseline.points, start_m, exit_m))
        .map(|(latest, previous)| latest - previous);
    let next_segment_delta_s = duration_between(&attempt.points, exit_m, next_m)
        .zip(duration_between(&baseline.points, exit_m, next_m))
        .map(|(latest, previous)| latest - previous);
    let exit_speed_delta_kmh = speed_at_distance(&attempt.points, exit_m)
        .zip(speed_at_distance(&baseline.points, exit_m))
        .map(|(latest, previous)| latest - previous);
    let controlled_gain = entry_segment_delta_s.is_some_and(|delta| delta < -0.05)
        && next_segment_delta_s.is_none_or(|delta| delta <= 0.05)
        && exit_speed_delta_kmh.is_none_or(|delta| delta >= -2.0)
        && gear_continuity
        && peak_wheel_slip <= 1.0;
    let classification = if controlled_gain {
        AcrEngineBrakingClassification::ControlledGain
    } else if entry_segment_delta_s.is_some_and(|delta| delta > 0.05)
        || next_segment_delta_s.is_some_and(|delta| delta > 0.05)
    {
        AcrEngineBrakingClassification::TimeLoss
    } else {
        AcrEngineBrakingClassification::Neutral
    };
    AcrEngineBrakingAssessment {
        engine_braking_detected: true,
        event_count: event_indices.len(),
        classification,
        entry_segment_delta_s,
        next_segment_delta_s,
        exit_speed_delta_kmh,
        gear_continuity,
        peak_wheel_slip,
    }
}

enum AcrArchiveCommand {
    Line {
        stage_number: u8,
        json: String,
    },
    FinishStage {
        stage_number: u8,
        acknowledgment: mpsc::Sender<Result<(), String>>,
    },
    Shutdown {
        acknowledgment: mpsc::Sender<Result<(), String>>,
    },
}

struct AcrArchiveWriter {
    sender: SyncSender<AcrArchiveCommand>,
    worker: Option<JoinHandle<()>>,
    dropped_frames: Arc<AtomicU64>,
    raw_dir: PathBuf,
    analysis_dir: PathBuf,
    raw_retention_days: u32,
    analysis_retention_days: u32,
    session_id: String,
}

impl AcrArchiveWriter {
    fn open(
        root: &Path,
        context: &AcrSessionContext,
        raw_retention_days: u32,
        analysis_retention_days: u32,
    ) -> Result<Self, String> {
        let raw_dir = root.join("raw");
        let analysis_dir = root.join("analysis");
        create_dir_all(&raw_dir)
            .map_err(|error| format!("failed to create {}: {error}", raw_dir.display()))?;
        create_dir_all(&analysis_dir)
            .map_err(|error| format!("failed to create {}: {error}", analysis_dir.display()))?;
        let now = SystemTime::now();
        prune_owned_files(
            &raw_dir,
            RAW_ARCHIVE_PREFIX,
            &[".jsonl.zst"],
            raw_retention_days,
            now,
        )?;
        prune_owned_files(
            &analysis_dir,
            ANALYSIS_ARCHIVE_PREFIX,
            &[".json", ".md"],
            analysis_retention_days,
            now,
        )?;

        let timestamp_ms = now
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let session_id = format!("{}-{timestamp_ms}", context.slug());
        let (sender, receiver) = mpsc::sync_channel(ARCHIVE_CHANNEL_CAPACITY);
        let worker_raw_dir = raw_dir.clone();
        let worker_session_id = session_id.clone();
        let worker = thread::Builder::new()
            .name("acr-zstd-archive".to_owned())
            .spawn(move || archive_worker(receiver, worker_raw_dir, worker_session_id))
            .map_err(|error| format!("failed to start ACR archive writer: {error}"))?;
        Ok(Self {
            sender,
            worker: Some(worker),
            dropped_frames: Arc::new(AtomicU64::new(0)),
            raw_dir,
            analysis_dir,
            raw_retention_days,
            analysis_retention_days,
            session_id,
        })
    }

    fn record_frame(
        &self,
        physics: &AcrPhysicsSnapshot,
        graphics: &AcrGraphicsSnapshot,
        statics: &AcrStaticSnapshot,
        stage: StageObservation,
    ) -> Result<(), String> {
        let json = serde_json::to_string(&serde_json::json!({
            "schema_version": ARCHIVE_SCHEMA_VERSION,
            "record_type": "telemetry",
            "stage": stage.stage_number,
            "stage_state": stage.state,
            "stage_event": stage.event,
            "stage_invalid": stage.attempt_invalid,
            "elapsed_s": stage.elapsed_s,
            "official_time_ms": stage.official_time_ms,
            "max_distance_m": stage.max_distance_m,
            "physics_packet_id": physics.packet_id,
            "graphics_packet_id": graphics.packet_id,
            "track": statics.track,
            "car": statics.car_model,
            "distance_m": graphics.distance_m,
            "speed_kmh": physics.speed_kmh,
            "rpm": physics.rpm,
            "max_rpm": physics.current_max_rpm.max(statics.max_rpm),
            "gear": acr_gear(physics.raw_gear),
            "throttle": physics.throttle,
            "brake": physics.brake,
            "clutch": physics.clutch,
            "steer": physics.steer,
            "g_force": physics.g_force,
            "velocity": physics.velocity,
            "local_velocity": physics.local_velocity,
            "wheel_slip": physics.wheel_slip,
            "slip_ratio": physics.slip_ratio,
            "slip_angle": physics.slip_angle,
            "wheel_load": physics.wheel_load,
            "wheel_angular_speed": physics.wheel_angular_speed,
            "suspension_travel": physics.suspension_travel,
            "suspension_damage": physics.suspension_damage,
            "tyre_pressure": physics.tyre_pressure,
            "tyre_wear": physics.tyre_wear,
            "tyre_core_temp": physics.tyre_core_temp,
            "brake_temp": physics.brake_temp,
            "brake_pressure": physics.brake_pressure,
            "fuel_l": physics.fuel,
            "tc_active": physics.tc_in_action,
            "abs_active": physics.abs_in_action,
        }))
        .map_err(|error| format!("failed to encode ACR archive frame: {error}"))?;
        self.send_line(stage.stage_number, json, stage.event != AcrStageEvent::None)
    }

    fn record_event(
        &self,
        context: Option<&AcrSessionContext>,
        stage: StageObservation,
    ) -> Result<(), String> {
        self.record_event_as(context, stage, stage.event)
    }

    fn record_event_as(
        &self,
        context: Option<&AcrSessionContext>,
        stage: StageObservation,
        event: AcrStageEvent,
    ) -> Result<(), String> {
        let json = serde_json::to_string(&serde_json::json!({
            "schema_version": ARCHIVE_SCHEMA_VERSION,
            "record_type": "stage_event",
            "stage": stage.stage_number,
            "stage_state": stage.state,
            "stage_event": event,
            "stage_invalid": stage.attempt_invalid,
            "elapsed_s": stage.elapsed_s,
            "official_time_ms": stage.official_time_ms,
            "max_distance_m": stage.max_distance_m,
            "track": context.map(AcrSessionContext::track),
            "car": context.map(AcrSessionContext::car),
        }))
        .map_err(|error| format!("failed to encode ACR archive event: {error}"))?;
        self.send_line(stage.stage_number, json, true)
    }

    fn send_line(&self, stage_number: u8, json: String, critical: bool) -> Result<(), String> {
        let command = AcrArchiveCommand::Line { stage_number, json };
        if critical {
            return self
                .sender
                .send(command)
                .map_err(|_| "ACR archive writer stopped unexpectedly".to_owned());
        }
        match self.sender.try_send(command) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(_)) => {
                self.dropped_frames.fetch_add(1, Ordering::Relaxed);
                Ok(())
            }
            Err(TrySendError::Disconnected(_)) => {
                Err("ACR archive writer stopped unexpectedly".to_owned())
            }
        }
    }

    fn finish_stage(&self, stage_number: u8) -> Result<(), String> {
        let (sender, receiver) = mpsc::channel();
        self.sender
            .send(AcrArchiveCommand::FinishStage {
                stage_number,
                acknowledgment: sender,
            })
            .map_err(|_| "ACR archive writer stopped before stage flush".to_owned())?;
        receiver
            .recv_timeout(Duration::from_secs(5))
            .map_err(|_| "timed out flushing ACR stage archive".to_owned())??;
        // The worker acknowledgment guarantees that no raw file is open while retention runs.
        self.maintain_retention(SystemTime::now())
    }

    fn maintain_retention(&self, now: SystemTime) -> Result<(), String> {
        prune_owned_files(
            &self.raw_dir,
            RAW_ARCHIVE_PREFIX,
            &[".jsonl.zst"],
            self.raw_retention_days,
            now,
        )?;
        prune_owned_files(
            &self.analysis_dir,
            ANALYSIS_ARCHIVE_PREFIX,
            &[".json", ".md"],
            self.analysis_retention_days,
            now,
        )
    }

    fn write_report(&self, report: &AcrStageReport) -> Result<(), String> {
        let base = format!(
            "{ANALYSIS_ARCHIVE_PREFIX}{}-stage-{:03}",
            self.session_id, report.stage_number
        );
        let json_path = self.analysis_dir.join(format!("{base}.json"));
        let markdown_path = self.analysis_dir.join(format!("{base}.md"));
        let json = serde_json::to_string_pretty(report)
            .map_err(|error| format!("failed to encode ACR stage report: {error}"))?;
        fs::write(&json_path, json)
            .map_err(|error| format!("failed to write {}: {error}", json_path.display()))?;
        fs::write(&markdown_path, render_stage_report(report))
            .map_err(|error| format!("failed to write {}: {error}", markdown_path.display()))
    }

    fn dropped_frames(&self) -> u64 {
        self.dropped_frames.load(Ordering::Relaxed)
    }

    fn shutdown(mut self) -> Result<(), String> {
        self.shutdown_inner()
    }

    fn shutdown_inner(&mut self) -> Result<(), String> {
        let Some(worker) = self.worker.take() else {
            return Ok(());
        };
        let (sender, receiver) = mpsc::channel();
        let send_result = self
            .sender
            .send(AcrArchiveCommand::Shutdown {
                acknowledgment: sender,
            })
            .map_err(|_| "ACR archive writer stopped before shutdown".to_owned());
        let result = send_result.and_then(|()| {
            receiver
                .recv_timeout(Duration::from_secs(5))
                .map_err(|_| "timed out finalizing ACR archive".to_owned())?
        });
        let join_result = worker
            .join()
            .map_err(|_| "ACR archive writer panicked during shutdown".to_owned());
        result.and(join_result)
    }
}

impl Drop for AcrArchiveWriter {
    fn drop(&mut self) {
        let _ = self.shutdown_inner();
    }
}

type RawEncoder = zstd::stream::write::Encoder<'static, BufWriter<File>>;

fn archive_worker(receiver: Receiver<AcrArchiveCommand>, raw_dir: PathBuf, session_id: String) {
    let mut current_stage = None;
    let mut encoder: Option<RawEncoder> = None;
    let mut pending_error: Option<String> = None;
    let mut last_flush_at = Instant::now();
    loop {
        let command = match receiver.recv_timeout(ARCHIVE_FLUSH_INTERVAL) {
            Ok(command) => command,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if let Some(writer) = encoder.as_mut()
                    && let Err(error) = writer.flush()
                {
                    pending_error = Some(format!("failed to flush ACR zstd archive: {error}"));
                }
                last_flush_at = Instant::now();
                continue;
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                let _ = finish_raw_encoder(&mut encoder);
                break;
            }
        };
        match command {
            AcrArchiveCommand::Line { stage_number, json } => {
                if current_stage != Some(stage_number) {
                    if let Err(error) = finish_raw_encoder(&mut encoder) {
                        pending_error = Some(error);
                    }
                    match open_raw_encoder(&raw_dir, &session_id, stage_number) {
                        Ok(next) => {
                            encoder = Some(next);
                            current_stage = Some(stage_number);
                        }
                        Err(error) => pending_error = Some(error),
                    }
                }
                if let Some(writer) = encoder.as_mut()
                    && let Err(error) = writeln!(writer, "{json}")
                {
                    pending_error = Some(format!("failed to write ACR zstd archive: {error}"));
                }
            }
            AcrArchiveCommand::FinishStage {
                stage_number,
                acknowledgment,
            } => {
                let result = if current_stage == Some(stage_number) {
                    finish_raw_encoder(&mut encoder)
                } else {
                    Ok(())
                }
                .and_then(|()| pending_error.take().map_or(Ok(()), Err));
                current_stage = None;
                let _ = acknowledgment.send(result);
            }
            AcrArchiveCommand::Shutdown { acknowledgment } => {
                let result = finish_raw_encoder(&mut encoder)
                    .and_then(|()| pending_error.take().map_or(Ok(()), Err));
                let _ = acknowledgment.send(result);
                break;
            }
        }
        if last_flush_at.elapsed() >= ARCHIVE_FLUSH_INTERVAL {
            if let Some(writer) = encoder.as_mut()
                && let Err(error) = writer.flush()
            {
                pending_error = Some(format!("failed to flush ACR zstd archive: {error}"));
            }
            last_flush_at = Instant::now();
        }
    }
}

fn open_raw_encoder(
    raw_dir: &Path,
    session_id: &str,
    stage_number: u8,
) -> Result<RawEncoder, String> {
    let path = raw_dir.join(format!(
        "{RAW_ARCHIVE_PREFIX}{session_id}-stage-{stage_number:03}.jsonl.zst"
    ));
    // A terminal stage can be flushed before its later result-screen event arrives. Appending a
    // new zstd frame preserves the completed telemetry instead of truncating the stage archive.
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|error| format!("failed to open {}: {error}", path.display()))?;
    zstd::stream::write::Encoder::new(BufWriter::new(file), 3)
        .map_err(|error| format!("failed to initialize {}: {error}", path.display()))
}

fn finish_raw_encoder(encoder: &mut Option<RawEncoder>) -> Result<(), String> {
    let Some(encoder) = encoder.take() else {
        return Ok(());
    };
    let mut writer = encoder
        .finish()
        .map_err(|error| format!("failed to finish ACR zstd archive: {error}"))?;
    writer
        .flush()
        .map_err(|error| format!("failed to flush ACR zstd archive: {error}"))
}

fn prune_owned_files(
    directory: &Path,
    prefix: &str,
    suffixes: &[&str],
    retention_days: u32,
    now: SystemTime,
) -> Result<(), String> {
    let max_age = Duration::from_secs(u64::from(retention_days.max(1)) * 86_400);
    let entries = fs::read_dir(directory)
        .map_err(|error| format!("failed to scan {}: {error}", directory.display()))?;
    for entry in entries {
        let entry = entry
            .map_err(|error| format!("failed to read {} entry: {error}", directory.display()))?;
        let file_type = entry
            .file_type()
            .map_err(|error| format!("failed to inspect {}: {error}", entry.path().display()))?;
        if !file_type.is_file() {
            continue;
        }
        let file_name = entry.file_name();
        let file_name = file_name.to_string_lossy();
        if !file_name.starts_with(prefix)
            || !suffixes.iter().any(|suffix| file_name.ends_with(suffix))
        {
            continue;
        }
        let modified = entry
            .metadata()
            .and_then(|metadata| metadata.modified())
            .map_err(|error| {
                format!("failed to read age for {}: {error}", entry.path().display())
            })?;
        if now.duration_since(modified).unwrap_or_default() > max_age {
            fs::remove_file(entry.path()).map_err(|error| {
                format!(
                    "failed to remove expired {}: {error}",
                    entry.path().display()
                )
            })?;
        }
    }
    Ok(())
}

#[derive(serde::Deserialize)]
struct ArchivedAcrTrace {
    trace: AcrAttemptTrace,
}

fn load_acr_history(
    root: &Path,
    context: &AcrSessionContext,
) -> Result<Vec<AcrAttemptTrace>, String> {
    let analysis_dir = root.join("analysis");
    let entries = match fs::read_dir(&analysis_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(format!(
                "failed to scan archived ACR analysis {}: {error}",
                analysis_dir.display()
            ));
        }
    };
    let mut history = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| {
            format!(
                "failed to read archived ACR analysis entry in {}: {error}",
                analysis_dir.display()
            )
        })?;
        let file_name = entry.file_name();
        let file_name = file_name.to_string_lossy();
        if !file_name.starts_with(ANALYSIS_ARCHIVE_PREFIX) || !file_name.ends_with(".json") {
            continue;
        }
        let Ok(contents) = fs::read_to_string(entry.path()) else {
            continue;
        };
        let Ok(archived) = serde_json::from_str::<ArchivedAcrTrace>(&contents) else {
            // Reports written before persisted traces were added remain readable; they simply
            // cannot participate in telemetry comparisons after a process restart.
            continue;
        };
        if archived.trace.track != context.track() || archived.trace.car != context.car() {
            continue;
        }
        let modified = entry
            .metadata()
            .and_then(|metadata| metadata.modified())
            .unwrap_or(UNIX_EPOCH);
        history.push((modified, file_name.into_owned(), archived.trace));
    }
    history.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
    Ok(history
        .into_iter()
        .rev()
        .take(32)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .map(|(_, _, trace)| trace)
        .collect())
}

fn render_stage_report(report: &AcrStageReport) -> String {
    use std::fmt::Write as _;

    let mut output = String::new();
    let _ = writeln!(output, "# ACR Stage {}", report.stage_number);
    let _ = writeln!(output);
    let _ = writeln!(output, "- Track: {}", report.track);
    let _ = writeln!(output, "- Car: {}", report.car);
    let _ = writeln!(output, "- Outcome: {}", report.outcome.as_str());
    let _ = writeln!(output, "- Time: {:.3} s", report.elapsed_s);
    let _ = writeln!(output, "- Maximum distance: {:.1} m", report.max_distance_m);
    let _ = writeln!(
        output,
        "- Archive backpressure drops: {}",
        report.archive_backpressure_drops
    );
    let _ = writeln!(
        output,
        "- Trace quality: {:?} ({}/100)",
        report.quality.status, report.quality.score
    );
    let _ = writeln!(
        output,
        "- Quality reasons: {}",
        report
            .quality
            .reasons
            .iter()
            .map(|reason| format!("{reason:?}"))
            .collect::<Vec<_>>()
            .join(", ")
    );
    let _ = writeln!(
        output,
        "- Quality evidence: {}",
        report.quality_evidence.join(", ")
    );
    if let Some(delta) = report.target_delta_s {
        let _ = writeln!(output, "- Target delta: {delta:+.3} s");
    }
    let _ = writeln!(output);
    let _ = writeln!(output, "## Common-distance comparisons");
    for comparison in &report.comparisons {
        let _ = writeln!(
            output,
            "- Stage {} ({}) at {:.1} m: {:+.3} s (confidence {:.2})",
            comparison.baseline_stage,
            comparison.baseline_outcome.as_str(),
            comparison.common_distance_m,
            comparison.delta_s,
            comparison.confidence
        );
    }
    if report.comparisons.is_empty() {
        let _ = writeln!(output, "- No comparable prior attempt yet.");
    }
    let _ = writeln!(output);
    let _ = writeln!(output, "## Learning and habits");
    let _ = writeln!(
        output,
        "- Completed attempts: {}",
        report.learning_trend.completed_attempts
    );
    for habit in &report.habits.repeated_habits {
        let _ = writeln!(output, "- {habit}");
    }
    if report.habits.repeated_habits.is_empty() {
        let _ = writeln!(output, "- No repeated habit detected yet.");
    }
    let _ = writeln!(output);
    let _ = writeln!(output, "## Over-rev context");
    let _ = writeln!(
        output,
        "- Classification: {:?}",
        report.overrev.classification
    );
    let _ = writeln!(output, "- Events: {}", report.overrev.event_count);
    let _ = writeln!(
        output,
        "- Mechanical risk: {}; technique gain: {}",
        report.overrev.mechanical_risk, report.overrev.technique_gain
    );
    let _ = writeln!(output);
    let _ = writeln!(output, "## Engine braking context");
    let _ = writeln!(
        output,
        "- Detected: {}; classification: {:?}",
        report.engine_braking.engine_braking_detected, report.engine_braking.classification
    );
    if let Some(delta) = report.engine_braking.entry_segment_delta_s {
        let _ = writeln!(output, "- Entry segment delta: {delta:+.3} s");
    }
    if let Some(delta) = report.engine_braking.next_segment_delta_s {
        let _ = writeln!(output, "- Next segment delta: {delta:+.3} s");
    }
    output
}

#[cfg_attr(any(target_os = "macos", target_os = "windows"), allow(dead_code))]
pub fn start_acr_adapter(config: BridgeConfig) -> Result<(), String> {
    #[cfg(not(windows))]
    {
        let _ = config;
        Err(format!(
            "Assetto Corsa Rally adapter requires Windows shared memory ({ACR_PHYSICS_MAPPING_NAME}); run this on the game PC"
        ))
    }

    #[cfg(windows)]
    {
        run_acr_adapter(config, None)
    }
}

pub fn start_acr_adapter_with_hud(
    config: BridgeConfig,
    hud: Option<HudHandle>,
) -> Result<(), String> {
    #[cfg(not(windows))]
    {
        let _ = config;
        let _ = hud;
        Err(format!(
            "Assetto Corsa Rally adapter requires Windows shared memory ({ACR_PHYSICS_MAPPING_NAME}); run this on the game PC"
        ))
    }

    #[cfg(windows)]
    {
        run_acr_adapter(config, hud)
    }
}

#[cfg(windows)]
fn read_stable_mapping(
    reader: &mut Option<super::shared_memory::SharedMemoryReader>,
    name: &str,
    size: usize,
    markers: &[super::shared_memory::StabilityMarker],
) -> Result<Vec<u8>, String> {
    if reader.is_none() {
        *reader = Some(super::shared_memory::SharedMemoryReader::open(name, size)?);
    }
    match reader
        .as_ref()
        .expect("ACR shared-memory reader was initialized")
        .read_consistent(markers)
    {
        Ok(snapshot) => Ok(snapshot),
        Err(error) => {
            *reader = None;
            Err(error)
        }
    }
}

#[cfg(windows)]
fn run_acr_adapter(config: BridgeConfig, hud: Option<HudHandle>) -> Result<(), String> {
    let mut recorder_config = config.clone();
    recorder_config.input_log = None;
    let mut recorder = TelemetryRecorder::open(&recorder_config)?;
    let mut coaching_logger = config
        .input_log
        .as_deref()
        .map(AcrCoachingLogger::open)
        .transpose()?;
    let mut stage_tracker = AcrStageTracker::new(Instant::now(), config.acr_finish_distance_m);
    let mut stage_analyzer = AcrStageAnalyzer::new(config.acr_target_time_s);
    let mut frame_validator = AcrFrameValidator::default();
    let mut graphics_freshness = AcrGraphicsFreshness::default();
    let mut capture_gate = AcrCaptureGate::new(config.analysis_rate_hz);
    let mut statics = AcrStaticSnapshot::default();
    let mut context: Option<AcrSessionContext> = None;
    let archive_root = config.archive_dir.as_deref().map(PathBuf::from);
    let mut archive_writer: Option<AcrArchiveWriter> = None;
    let mut physics_reader = None;
    let mut graphics_reader = None;
    let mut static_reader = None;
    let mut frame_identifier = 0_u32;
    let mut last_physics_packet = None;
    let mut last_static_refresh = Instant::now() - STATIC_REFRESH_INTERVAL;
    let mut last_warning = Instant::now() - WARNING_INTERVAL;
    let mut last_stats = Instant::now();
    let mut samples = 0_u64;

    println!(
        "Assetto Corsa Rally adapter reading {ACR_PHYSICS_MAPPING_NAME}, {ACR_GRAPHICS_MAPPING_NAME}, and {ACR_STATIC_MAPPING_NAME}"
    );
    println!("game={} ({})", config.game.id, config.game.name);
    println!("debug={}", config.debug);
    if let Some(path) = &config.input_log {
        println!(
            "ACR legacy CSV logging enabled: {path} ({} Hz)",
            config.analysis_rate_hz
        );
    }
    if let Some(path) = &config.archive_dir {
        println!(
            "ACR zstd archive enabled: {path} ({} Hz, raw={}d, analysis={}d)",
            config.analysis_rate_hz, config.raw_retention_days, config.analysis_retention_days
        );
    }
    print_enabled_outputs(&recorder_config);
    if hud.is_some() {
        println!("HUD: native window");
    } else {
        println!("HUD: headless");
    }

    while !crate::runtime_control::shutdown_requested() {
        let now = Instant::now();
        if last_static_refresh.elapsed() >= STATIC_REFRESH_INTERVAL {
            if let Ok(snapshot) = read_stable_mapping(
                &mut static_reader,
                ACR_STATIC_MAPPING_NAME,
                ACR_STATIC_SIZE,
                &NO_STABILITY_MARKERS,
            ) && let Ok(next) = parse_acr_static(&snapshot)
            {
                if let Some(next_context) = AcrSessionContext::from_static(&next) {
                    if context.as_ref() != Some(&next_context) {
                        if let Some(writer) = archive_writer.take() {
                            writer.shutdown()?;
                        }
                        // Static track length is analysis metadata, not authoritative finish
                        // evidence. Only an explicitly configured fallback may end a stage before
                        // the game's official result arrives.
                        let finish_distance = select_acr_finish_distance(
                            config.acr_finish_distance_m,
                            next.track_length_m,
                        );
                        stage_tracker.reset_context(now, finish_distance);
                        stage_analyzer.reset_context();
                        frame_validator.reset();
                        graphics_freshness.reset();
                        capture_gate.reset();
                        last_physics_packet = None;
                        if let Some(root) = &archive_root {
                            let writer = AcrArchiveWriter::open(
                                root,
                                &next_context,
                                config.raw_retention_days,
                                config.analysis_retention_days,
                            )?;
                            let history = load_acr_history(root, &next_context)?;
                            if !history.is_empty() {
                                println!(
                                    "[acr-session] restored {} prior attempt(s) for coaching",
                                    history.len()
                                );
                            }
                            stage_analyzer.restore_history(history);
                            archive_writer = Some(writer);
                        }
                        context = Some(next_context.clone());
                        println!(
                            "[acr-session] track={} car={} length={:.1}m finish={}",
                            display_or_unknown(&next.track),
                            display_or_unknown(&next.car_model),
                            next.track_length_m,
                            finish_distance
                                .map(|distance| format!("{distance:.1}m"))
                                .unwrap_or_else(|| "official-only".to_owned())
                        );
                    }
                    statics = next;
                } else if context.is_none() {
                    // Keep incomplete startup metadata without allowing a transient blank map to
                    // reset an active attempt.
                    statics = next;
                }
            }
            last_static_refresh = now;
        }

        match read_stable_mapping(
            &mut physics_reader,
            ACR_PHYSICS_MAPPING_NAME,
            ACR_PHYSICS_SIZE,
            &PACKET_ID_MARKERS,
        ) {
            Ok(snapshot) => {
                let physics = match parse_acr_physics(&snapshot) {
                    Ok(physics) => physics,
                    Err(error) => {
                        if last_warning.elapsed() >= WARNING_INTERVAL {
                            eprintln!("[adapter-warning] {error}");
                            last_warning = now;
                        }
                        thread::sleep(POLL_INTERVAL);
                        continue;
                    }
                };
                let duplicate_physics = last_physics_packet == Some(physics.packet_id);
                last_physics_packet = Some(physics.packet_id);

                let graphics = match read_stable_mapping(
                    &mut graphics_reader,
                    ACR_GRAPHICS_MAPPING_NAME,
                    ACR_GRAPHICS_SIZE,
                    &PACKET_ID_MARKERS,
                )
                .and_then(|snapshot| parse_acr_graphics(&snapshot))
                {
                    Ok(graphics) => graphics,
                    Err(error) => {
                        if last_warning.elapsed() >= WARNING_INTERVAL {
                            eprintln!(
                                "[adapter-warning] {error}; skipping unpaired ACR physics frame"
                            );
                            last_warning = now;
                        }
                        thread::sleep(POLL_INTERVAL);
                        continue;
                    }
                };
                if !graphics_freshness.observe(physics.packet_id, &graphics, now) {
                    graphics_reader = None;
                    if last_warning.elapsed() >= WARNING_INTERVAL {
                        eprintln!(
                            "[adapter-warning] skewed ACR graphics packet {}; skipping physics packet {}",
                            graphics.packet_id, physics.packet_id
                        );
                        last_warning = now;
                    }
                    thread::sleep(POLL_INTERVAL);
                    continue;
                }

                let (stage, frame_valid) = match observe_validated_stage(
                    &mut stage_tracker,
                    &mut frame_validator,
                    &physics,
                    &graphics,
                    &statics,
                    now,
                ) {
                    Ok(stage) => stage,
                    Err(error) => {
                        if last_warning.elapsed() >= WARNING_INTERVAL {
                            eprintln!("[adapter-warning] {error}");
                            last_warning = now;
                        }
                        thread::sleep(POLL_INTERVAL);
                        continue;
                    }
                };
                if duplicate_physics
                    && stage.event == AcrStageEvent::None
                    && !stage.result_screen_entered
                {
                    thread::sleep(POLL_INTERVAL);
                    continue;
                }
                if !frame_valid && last_warning.elapsed() >= WARNING_INTERVAL {
                    eprintln!(
                        "[adapter-warning] rejected invalid ACR physics packet {} while preserving official stage completion",
                        physics.packet_id
                    );
                    last_warning = now;
                }
                if stage.event != AcrStageEvent::None {
                    println!(
                        "[acr-stage] stage={} state={} event={} distance={:.1}m",
                        stage.stage_number,
                        stage.state.as_str(),
                        stage.event.as_str(),
                        graphics.distance_m
                    );
                }

                if graphics.status != 2 || !frame_valid {
                    if stage.event.is_terminal() {
                        if let Some(archive) = &archive_writer {
                            archive.record_event(context.as_ref(), stage)?;
                        }
                        if let Some(report) = stage_analyzer.ingest(
                            &physics,
                            &graphics,
                            &statics,
                            stage,
                            false,
                            CaptureCounters {
                                rejected_frames: frame_validator.rejected_frames,
                                persistence_dropped_frames: archive_writer
                                    .as_ref()
                                    .map_or(0, AcrArchiveWriter::dropped_frames),
                                ..CaptureCounters::default()
                            },
                        ) {
                            finish_acr_stage_outputs(
                                &archive_writer,
                                coaching_logger.as_mut(),
                                &report,
                            )?;
                        }
                    }
                    if stage.result_screen_entered {
                        println!(
                            "[acr-stage] stage={} state={} event={}",
                            stage.stage_number,
                            stage.state.as_str(),
                            AcrStageEvent::ResultScreen.as_str()
                        );
                        if let Some(archive) = &archive_writer {
                            archive.record_event_as(
                                context.as_ref(),
                                stage,
                                AcrStageEvent::ResultScreen,
                            )?;
                        }
                    }
                    thread::sleep(POLL_INTERVAL);
                    continue;
                }

                frame_identifier = frame_identifier.wrapping_add(1);
                let update =
                    build_acr_update(&physics, &graphics, &statics, stage, frame_identifier);

                if let Some(hud) = &hud {
                    hud.update(&update);
                }
                recorder.ingest(&update, config.debug);

                if let Some(archive) = &archive_writer
                    && (stage.state == AcrStageState::Running || stage.event != AcrStageEvent::None)
                {
                    archive.record_frame(&physics, &graphics, &statics, stage)?;
                }
                if capture_gate.should_capture(stage.event, now) {
                    if let Some(logger) = &mut coaching_logger {
                        logger.write(&physics, &graphics, &statics, stage)?;
                    }
                    if let Some(report) = stage_analyzer.ingest(
                        &physics,
                        &graphics,
                        &statics,
                        stage,
                        true,
                        CaptureCounters {
                            rejected_frames: frame_validator.rejected_frames,
                            persistence_dropped_frames: archive_writer
                                .as_ref()
                                .map_or(0, AcrArchiveWriter::dropped_frames),
                            ..CaptureCounters::default()
                        },
                    ) {
                        finish_acr_stage_outputs(
                            &archive_writer,
                            coaching_logger.as_mut(),
                            &report,
                        )?;
                    }
                }

                samples += 1;
                if config.debug && last_stats.elapsed() >= Duration::from_secs(1) {
                    println!(
                        "[acr] samples={} packet={} stage={} distance={:.1}m speed={:.1}km/h rpm={}",
                        samples,
                        physics.packet_id,
                        stage.stage_number,
                        graphics.distance_m,
                        physics.speed_kmh,
                        physics.rpm,
                    );
                    if let Some(archive) = &archive_writer {
                        let dropped = archive.dropped_frames();
                        if dropped > 0 {
                            eprintln!("[acr] archive_backpressure_dropped={dropped}");
                        }
                    }
                    last_stats = now;
                }
            }
            Err(error) => {
                if last_warning.elapsed() >= WARNING_INTERVAL {
                    eprintln!("[adapter-warning] {error}; waiting for {ACR_PHYSICS_MAPPING_NAME}");
                    last_warning = now;
                }
            }
        }

        thread::sleep(POLL_INTERVAL);
    }
    if let Some(logger) = &mut coaching_logger {
        logger.flush()?;
    }
    if let Some(writer) = archive_writer.take() {
        writer.shutdown()?;
    }
    Ok(())
}

#[cfg(windows)]
fn finish_acr_stage_outputs(
    archive: &Option<AcrArchiveWriter>,
    coaching_logger: Option<&mut AcrCoachingLogger>,
    report: &AcrStageReport,
) -> Result<(), String> {
    if let Some(logger) = coaching_logger {
        logger.flush()?;
    }
    if let Some(archive) = archive {
        archive.finish_stage(report.stage_number)?;
        archive.write_report(report)?;
    }
    Ok(())
}

fn parse_acr_physics(snapshot: &[u8]) -> Result<AcrPhysicsSnapshot, String> {
    if snapshot.len() < ACR_PHYSICS_SIZE {
        return Err(format!(
            "ACR physics snapshot is too short: {} < {ACR_PHYSICS_SIZE}",
            snapshot.len()
        ));
    }

    Ok(AcrPhysicsSnapshot {
        packet_id: read_i32_le(snapshot, PACKET_ID_OFFSET)?,
        throttle: read_f32_le(snapshot, THROTTLE_OFFSET)?,
        brake: read_f32_le(snapshot, BRAKE_OFFSET)?,
        fuel: read_f32_le(snapshot, FUEL_OFFSET)?,
        raw_gear: read_i32_le(snapshot, GEAR_OFFSET)?,
        rpm: read_i32_le(snapshot, RPM_OFFSET)?,
        steer: read_f32_le(snapshot, STEER_OFFSET)?,
        speed_kmh: read_f32_le(snapshot, SPEED_OFFSET)?,
        velocity: read_values::<3>(snapshot, VELOCITY_OFFSET)?,
        g_force: read_values::<3>(snapshot, G_FORCE_OFFSET)?,
        wheel_slip: read_values::<4>(snapshot, WHEEL_SLIP_OFFSET)?,
        wheel_load: read_values::<4>(snapshot, WHEEL_LOAD_OFFSET)?,
        tyre_pressure: read_values::<4>(snapshot, TYRE_PRESSURE_OFFSET)?,
        wheel_angular_speed: read_values::<4>(snapshot, WHEEL_ANGULAR_SPEED_OFFSET)?,
        tyre_wear: read_values::<4>(snapshot, TYRE_WEAR_OFFSET)?,
        tyre_core_temp: read_values::<4>(snapshot, TYRE_CORE_TEMP_OFFSET)?,
        suspension_travel: read_values::<4>(snapshot, SUSPENSION_TRAVEL_OFFSET)?,
        tc: read_f32_le(snapshot, TC_OFFSET)?,
        heading: read_f32_le(snapshot, HEADING_OFFSET)?,
        pitch: read_f32_le(snapshot, PITCH_OFFSET)?,
        roll: read_f32_le(snapshot, ROLL_OFFSET)?,
        car_damage: read_values::<5>(snapshot, CAR_DAMAGE_OFFSET)?,
        pit_limiter_on: read_i32_le(snapshot, PIT_LIMITER_OFFSET)? != 0,
        abs: read_f32_le(snapshot, ABS_OFFSET)?,
        air_temp: read_f32_le(snapshot, AIR_TEMP_OFFSET)?,
        road_temp: read_f32_le(snapshot, ROAD_TEMP_OFFSET)?,
        local_angular_velocity: read_values::<3>(snapshot, LOCAL_ANGULAR_VELOCITY_OFFSET)?,
        final_ff: read_f32_le(snapshot, FINAL_FF_OFFSET)?,
        brake_temp: read_values::<4>(snapshot, BRAKE_TEMP_OFFSET)?,
        clutch: read_f32_le(snapshot, CLUTCH_OFFSET)?,
        tyre_temp_inner: read_values::<4>(snapshot, TYRE_TEMP_INNER_OFFSET)?,
        tyre_temp_middle: read_values::<4>(snapshot, TYRE_TEMP_MIDDLE_OFFSET)?,
        tyre_temp_outer: read_values::<4>(snapshot, TYRE_TEMP_OUTER_OFFSET)?,
        brake_bias: read_f32_le(snapshot, BRAKE_BIAS_OFFSET)?,
        local_velocity: read_values::<3>(snapshot, LOCAL_VELOCITY_OFFSET)?,
        current_max_rpm: read_i32_le(snapshot, CURRENT_MAX_RPM_OFFSET)?,
        slip_ratio: read_values::<4>(snapshot, SLIP_RATIO_OFFSET)?,
        slip_angle: read_values::<4>(snapshot, SLIP_ANGLE_OFFSET)?,
        tc_in_action: read_i32_le(snapshot, TC_IN_ACTION_OFFSET)? != 0,
        abs_in_action: read_i32_le(snapshot, ABS_IN_ACTION_OFFSET)? != 0,
        suspension_damage: read_values::<4>(snapshot, SUSPENSION_DAMAGE_OFFSET)?,
        water_temp: read_f32_le(snapshot, WATER_TEMP_OFFSET)?,
        brake_pressure: read_values::<4>(snapshot, BRAKE_PRESSURE_OFFSET)?,
        ignition_on: read_i32_le(snapshot, IGNITION_ON_OFFSET)? != 0,
        engine_running: read_i32_le(snapshot, ENGINE_RUNNING_OFFSET)? != 0,
    })
}

fn parse_acr_graphics(snapshot: &[u8]) -> Result<AcrGraphicsSnapshot, String> {
    if snapshot.len() < ACR_GRAPHICS_SIZE {
        return Err(format!(
            "ACR graphics snapshot is too short: {} < {ACR_GRAPHICS_SIZE}",
            snapshot.len()
        ));
    }

    Ok(AcrGraphicsSnapshot {
        packet_id: read_i32_le(snapshot, GRAPHICS_PACKET_ID_OFFSET)?,
        status: read_i32_le(snapshot, GRAPHICS_STATUS_OFFSET)?,
        session_type: read_i32_le(snapshot, GRAPHICS_SESSION_TYPE_OFFSET)?,
        completed_laps: read_i32_le(snapshot, GRAPHICS_COMPLETED_LAPS_OFFSET)?,
        position: read_i32_le(snapshot, GRAPHICS_POSITION_OFFSET)?,
        current_time_ms: read_i32_le(snapshot, GRAPHICS_CURRENT_TIME_OFFSET)?,
        last_time_ms: read_i32_le(snapshot, GRAPHICS_LAST_TIME_OFFSET)?,
        session_time_left_s: read_f32_le(snapshot, GRAPHICS_SESSION_TIME_LEFT_OFFSET)?,
        distance_m: read_f32_le(snapshot, GRAPHICS_DISTANCE_OFFSET)?,
        in_pit: read_i32_le(snapshot, GRAPHICS_IN_PIT_OFFSET)? != 0,
        sector: read_i32_le(snapshot, GRAPHICS_SECTOR_OFFSET)?,
    })
}

fn parse_acr_static(snapshot: &[u8]) -> Result<AcrStaticSnapshot, String> {
    if snapshot.len() < ACR_STATIC_SIZE {
        return Err(format!(
            "ACR static snapshot is too short: {} < {ACR_STATIC_SIZE}",
            snapshot.len()
        ));
    }

    Ok(AcrStaticSnapshot {
        car_model: read_utf16(snapshot, STATIC_CAR_MODEL_OFFSET, STATIC_UTF16_CHARS)?,
        track: read_utf16(snapshot, STATIC_TRACK_OFFSET, STATIC_UTF16_CHARS)?,
        max_rpm: read_i32_le(snapshot, STATIC_MAX_RPM_OFFSET)?,
        max_fuel: read_f32_le(snapshot, STATIC_MAX_FUEL_OFFSET)?,
        track_length_m: read_f32_le(snapshot, STATIC_TRACK_LENGTH_OFFSET)?,
    })
}

fn build_acr_update(
    physics: &AcrPhysicsSnapshot,
    graphics: &AcrGraphicsSnapshot,
    statics: &AcrStaticSnapshot,
    stage: StageObservation,
    frame_identifier: u32,
) -> TelemetryUpdate {
    let temperatures_are_kelvin = physics.air_temp > 170.0;
    let tyre_core_temp = convert_temperatures(physics.tyre_core_temp, temperatures_are_kelvin);
    let tyre_surface_temp = layered_tyre_temperatures(physics, temperatures_are_kelvin);
    let brake_temp = convert_temperatures(physics.brake_temp, temperatures_are_kelvin);
    let max_rpm = if physics.current_max_rpm > 0 {
        physics.current_max_rpm
    } else {
        statics.max_rpm
    };
    let current_time_ms = if graphics.current_time_ms > 0 {
        graphics.current_time_ms as u32
    } else {
        seconds_to_u32_ms(stage.elapsed_s)
    };
    let tyre_wear = normalized_tyre_wear(physics.tyre_wear);

    TelemetryUpdate {
        input: Some(InputSample {
            session_time: stage.elapsed_s,
            frame_identifier,
            player_car_index: 0,
            throttle: clamp_unit(physics.throttle),
            steer: finite_or_zero(physics.steer).clamp(-1.0, 1.0),
            brake: clamp_unit(physics.brake),
            clutch: percent_u8(1.0 - clamp_unit(physics.clutch)),
            speed_kmh: clamp_u16(physics.speed_kmh),
            gear: acr_gear(physics.raw_gear),
            rpm: clamp_u16(physics.rpm as f32),
            drs: false,
            rev_lights_percent: if max_rpm > 0 {
                percent_u8(physics.rpm as f32 / max_rpm as f32)
            } else {
                0
            },
            rev_lights_bit_value: 0,
            brake_temps_c: wheel_u16(brake_temp),
            tyre_surface_temps_c: wheel_u8(tyre_surface_temp),
            tyre_inner_temps_c: wheel_u8(tyre_core_temp),
            engine_temp_c: clamp_u16(temperature_c(physics.water_temp, temperatures_are_kelvin)),
            tyre_pressures_psi: wheel_f32(physics.tyre_pressure, finite_nonnegative),
        }),
        lap: Some(LapSample {
            session_time: stage.elapsed_s,
            frame_identifier,
            player_car_index: 0,
            last_lap_time_ms: stage
                .official_time_ms
                .unwrap_or_else(|| graphics.last_time_ms.max(0) as u32),
            current_lap_time_ms: current_time_ms,
            lap_distance_m: finite_nonnegative(graphics.distance_m),
            total_distance_m: finite_nonnegative(graphics.distance_m),
            car_position: graphics.position.clamp(1, u8::MAX as i32) as u8,
            current_lap_num: stage.stage_number,
            pit_status: u8::from(graphics.in_pit),
            sector: graphics.sector.clamp(0, u8::MAX as i32) as u8,
            current_lap_invalid: stage.attempt_invalid,
            driver_status: graphics.status.clamp(0, u8::MAX as i32) as u8,
            result_status: 0,
            delta_to_car_in_front_ms: None,
            delta_to_car_behind_ms: None,
            delta_to_race_leader_ms: None,
            sector1_time_ms: None,
            sector2_time_ms: None,
        }),
        session: Some(SessionSample {
            session_time: stage.elapsed_s,
            frame_identifier,
            total_laps: 1,
            track_length_m: clamp_u16(statics.track_length_m),
            session_type: graphics.session_type.clamp(0, u8::MAX as i32) as u8,
            track_id: 0,
            track_temp_c: clamp_i8(temperature_c(physics.road_temp, temperatures_are_kelvin)),
            air_temp_c: clamp_i8(temperature_c(physics.air_temp, temperatures_are_kelvin)),
            session_time_left_s: clamp_u16(graphics.session_time_left_s),
            marshal_zones: Vec::new(),
        }),
        damage: Some(DamageSample {
            session_time: stage.elapsed_s,
            frame_identifier,
            player_car_index: 0,
            tyre_wear,
            tyre_damage: wheel_u8(physics.suspension_damage.map(damage_percent)),
            tyre_blisters: zero_u8_wheels(),
            front_left_wing_damage: clamp_u8(damage_percent(physics.car_damage[0])),
            front_right_wing_damage: clamp_u8(damage_percent(physics.car_damage[0])),
            rear_wing_damage: clamp_u8(damage_percent(physics.car_damage[1])),
            gearbox_damage: 0,
            engine_damage: clamp_u8(damage_percent(physics.car_damage[4])),
        }),
        status: Some(StatusSample {
            session_time: stage.elapsed_s,
            frame_identifier,
            player_car_index: 0,
            traction_control: clamp_u8(physics.tc),
            anti_lock_brakes: clamp_u8(physics.abs),
            front_brake_bias: clamp_u8(brake_bias_percent(physics.brake_bias)),
            fuel_in_tank: finite_nonnegative(physics.fuel),
            fuel_capacity: finite_nonnegative(statics.max_fuel),
            fuel_remaining_laps: 0.0,
            max_rpm: clamp_u16(max_rpm as f32),
            idle_rpm: 0,
            max_gears: 0,
            drs_allowed: false,
            drs_activation_distance_m: 0,
            pit_limiter_active: physics.pit_limiter_on,
            actual_tyre_compound: 0,
            visual_tyre_compound: 0,
            tyres_age_laps: graphics.completed_laps.clamp(0, u8::MAX as i32) as u8,
            ers_store_energy: 0.0,
            ers_deploy_mode: 0,
            ers_deployed_this_lap: 0.0,
        }),
    }
}

#[cfg(windows)]
struct AcrCoachingLogger {
    writer: BufWriter<std::fs::File>,
    rows_since_flush: usize,
}

#[cfg(windows)]
impl AcrCoachingLogger {
    fn open(path: &str) -> Result<Self, String> {
        let path = Path::new(path);
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            create_dir_all(parent).map_err(|error| {
                format!(
                    "failed to create ACR log directory {}: {error}",
                    parent.display()
                )
            })?;
        }
        let should_write_header = metadata(path).map(|value| value.len() == 0).unwrap_or(true);
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(|error| {
                format!(
                    "failed to open ACR coaching log {}: {error}",
                    path.display()
                )
            })?;
        let mut logger = Self {
            writer: BufWriter::new(file),
            rows_since_flush: 0,
        };
        if should_write_header {
            logger
                .writer
                .write_all(ACR_COACHING_HEADER.as_bytes())
                .map_err(|error| format!("failed to write ACR coaching header: {error}"))?;
            logger
                .writer
                .flush()
                .map_err(|error| format!("failed to flush ACR coaching header: {error}"))?;
        }
        Ok(logger)
    }

    fn write(
        &mut self,
        physics: &AcrPhysicsSnapshot,
        graphics: &AcrGraphicsSnapshot,
        statics: &AcrStaticSnapshot,
        stage: StageObservation,
    ) -> Result<(), String> {
        let temperatures_are_kelvin = physics.air_temp > 170.0;
        let track_length = finite_nonnegative(statics.track_length_m);
        let distance = finite_nonnegative(graphics.distance_m);
        let progress = if track_length > 0.0 {
            (distance / track_length).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let mut fields = vec![
            format!("{:.3}", stage.elapsed_s),
            stage.stage_number.to_string(),
            stage.state.as_str().to_owned(),
            stage.event.as_str().to_owned(),
            u8::from(!stage.attempt_invalid).to_string(),
            u8::from(stage.reset).to_string(),
            stage.official_time_ms.unwrap_or_default().to_string(),
            format_f32(stage.max_distance_m),
            physics.packet_id.to_string(),
            graphics.packet_id.to_string(),
            csv_text(&statics.track),
            csv_text(&statics.car_model),
            format!("{distance:.3}"),
            format!("{progress:.6}"),
            format_f32(physics.speed_kmh),
            physics.rpm.to_string(),
            acr_gear(physics.raw_gear).to_string(),
            format_f32(clamp_unit(physics.throttle)),
            format_f32(clamp_unit(physics.brake)),
            format_f32(1.0 - clamp_unit(physics.clutch)),
            format_f32(physics.steer),
        ];
        push_values(&mut fields, physics.g_force);
        push_values(&mut fields, physics.velocity);
        push_values(&mut fields, physics.local_velocity);
        push_values(&mut fields, physics.local_angular_velocity);
        fields.extend([
            format_f32(physics.heading),
            format_f32(physics.pitch),
            format_f32(physics.roll),
        ]);
        push_values(&mut fields, physics.wheel_slip);
        push_values(&mut fields, physics.wheel_load);
        push_values(&mut fields, physics.wheel_angular_speed);
        push_values(&mut fields, physics.slip_ratio);
        push_values(&mut fields, physics.slip_angle);
        push_values(&mut fields, physics.suspension_travel);
        push_values(&mut fields, physics.suspension_damage);
        push_values(&mut fields, physics.tyre_pressure);
        push_values(&mut fields, physics.tyre_wear);
        push_values(
            &mut fields,
            convert_temperatures(physics.tyre_core_temp, temperatures_are_kelvin),
        );
        push_values(
            &mut fields,
            convert_temperatures(physics.brake_temp, temperatures_are_kelvin),
        );
        push_values(&mut fields, physics.brake_pressure);
        fields.extend([
            format_f32(physics.fuel),
            format_f32(temperature_c(physics.air_temp, temperatures_are_kelvin)),
            format_f32(temperature_c(physics.road_temp, temperatures_are_kelvin)),
            format_f32(temperature_c(physics.water_temp, temperatures_are_kelvin)),
            format_f32(brake_bias_percent(physics.brake_bias)),
            format_f32(physics.tc),
            format_f32(physics.abs),
            u8::from(physics.tc_in_action).to_string(),
            u8::from(physics.abs_in_action).to_string(),
            format_f32(physics.final_ff),
            u8::from(physics.engine_running).to_string(),
            u8::from(physics.ignition_on).to_string(),
        ]);

        writeln!(self.writer, "{}", fields.join(","))
            .map_err(|error| format!("failed to write ACR coaching row: {error}"))?;
        self.rows_since_flush += 1;
        if self.rows_since_flush >= 25 {
            self.writer
                .flush()
                .map_err(|error| format!("failed to flush ACR coaching log: {error}"))?;
            self.rows_since_flush = 0;
        }
        Ok(())
    }

    fn flush(&mut self) -> Result<(), String> {
        self.writer
            .flush()
            .map_err(|error| format!("failed to flush ACR coaching log: {error}"))?;
        self.rows_since_flush = 0;
        Ok(())
    }
}

#[cfg(windows)]
const ACR_COACHING_HEADER: &str = concat!(
    "elapsed_s,stage,stage_state,stage_event,stage_valid,stage_reset,official_time_ms,max_distance_m,",
    "physics_packet_id,graphics_packet_id,track,car,stage_distance_m,stage_progress,",
    "speed_kmh,rpm,gear,throttle,brake,clutch,steer,",
    "g_x,g_y,g_z,velocity_x,velocity_y,velocity_z,local_velocity_x,local_velocity_y,local_velocity_z,",
    "angular_velocity_x,angular_velocity_y,angular_velocity_z,heading,pitch,roll,",
    "wheel_slip_fl,wheel_slip_fr,wheel_slip_rl,wheel_slip_rr,",
    "wheel_load_fl,wheel_load_fr,wheel_load_rl,wheel_load_rr,",
    "wheel_angular_speed_fl,wheel_angular_speed_fr,wheel_angular_speed_rl,wheel_angular_speed_rr,",
    "slip_ratio_fl,slip_ratio_fr,slip_ratio_rl,slip_ratio_rr,",
    "slip_angle_fl,slip_angle_fr,slip_angle_rl,slip_angle_rr,",
    "suspension_travel_fl,suspension_travel_fr,suspension_travel_rl,suspension_travel_rr,",
    "suspension_damage_fl,suspension_damage_fr,suspension_damage_rl,suspension_damage_rr,",
    "tyre_pressure_fl,tyre_pressure_fr,tyre_pressure_rl,tyre_pressure_rr,",
    "tyre_wear_raw_fl,tyre_wear_raw_fr,tyre_wear_raw_rl,tyre_wear_raw_rr,",
    "tyre_core_temp_c_fl,tyre_core_temp_c_fr,tyre_core_temp_c_rl,tyre_core_temp_c_rr,",
    "brake_temp_c_fl,brake_temp_c_fr,brake_temp_c_rl,brake_temp_c_rr,",
    "brake_pressure_fl,brake_pressure_fr,brake_pressure_rl,brake_pressure_rr,",
    "fuel_l,air_temp_c,road_temp_c,water_temp_c,brake_bias_pct,tc_setting,abs_setting,",
    "tc_active,abs_active,final_ff,engine_running,ignition_on\n"
);

fn layered_tyre_temperatures(physics: &AcrPhysicsSnapshot, kelvin: bool) -> [f32; 4] {
    let core = convert_temperatures(physics.tyre_core_temp, kelvin);
    std::array::from_fn(|index| {
        let layers = [
            physics.tyre_temp_inner[index],
            physics.tyre_temp_middle[index],
            physics.tyre_temp_outer[index],
        ];
        let mut sum = 0.0;
        let mut count = 0;
        for value in layers {
            if value.is_finite() && value > 0.0 {
                sum += temperature_c(value, kelvin);
                count += 1;
            }
        }
        if count == 0 {
            core[index]
        } else {
            sum / count as f32
        }
    })
}

fn convert_temperatures(values: [f32; 4], kelvin: bool) -> [f32; 4] {
    values.map(|value| temperature_c(value, kelvin))
}

fn temperature_c(value: f32, kelvin: bool) -> f32 {
    if !value.is_finite() || value <= 0.0 {
        0.0
    } else if kelvin {
        (value - 273.15).max(0.0)
    } else {
        value
    }
}

fn normalized_tyre_wear(values: [f32; 4]) -> WheelValuesF32 {
    if values.iter().all(|value| value.abs() < f32::EPSILON) {
        return WheelValuesF32 {
            fl: UNKNOWN_TYRE_WEAR_PERCENT,
            fr: UNKNOWN_TYRE_WEAR_PERCENT,
            rl: UNKNOWN_TYRE_WEAR_PERCENT,
            rr: UNKNOWN_TYRE_WEAR_PERCENT,
        };
    }
    wheel_f32(values, finite_nonnegative)
}

fn wheel_f32(values: [f32; 4], convert: impl Fn(f32) -> f32) -> WheelValuesF32 {
    WheelValuesF32 {
        fl: convert(values[0]),
        fr: convert(values[1]),
        rl: convert(values[2]),
        rr: convert(values[3]),
    }
}

fn wheel_u8(values: [f32; 4]) -> WheelValuesU8 {
    WheelValuesU8 {
        fl: clamp_u8(values[0]),
        fr: clamp_u8(values[1]),
        rl: clamp_u8(values[2]),
        rr: clamp_u8(values[3]),
    }
}

fn wheel_u16(values: [f32; 4]) -> WheelValuesU16 {
    WheelValuesU16 {
        fl: clamp_u16(values[0]),
        fr: clamp_u16(values[1]),
        rl: clamp_u16(values[2]),
        rr: clamp_u16(values[3]),
    }
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

fn read_values<const N: usize>(bytes: &[u8], offset: usize) -> Result<[f32; N], String> {
    let mut values = [0.0; N];
    for (index, value) in values.iter_mut().enumerate() {
        *value = read_f32_le(bytes, offset + index * 4)?;
    }
    Ok(values)
}

fn read_utf16(bytes: &[u8], offset: usize, char_count: usize) -> Result<String, String> {
    let raw = bytes
        .get(offset..offset + char_count * 2)
        .ok_or_else(|| format!("snapshot is too short for UTF-16 string at {offset}"))?;
    let units = raw
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .take_while(|value| *value != 0)
        .collect::<Vec<_>>();
    Ok(String::from_utf16_lossy(&units).trim().to_owned())
}

fn acr_gear(raw_gear: i32) -> i8 {
    (raw_gear - 1).clamp(-1, 12) as i8
}

fn brake_bias_percent(value: f32) -> f32 {
    let value = finite_nonnegative(value);
    if value <= 1.0 { value * 100.0 } else { value }
}

fn damage_percent(value: f32) -> f32 {
    let value = finite_nonnegative(value);
    if value <= 1.0 { value * 100.0 } else { value }
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

fn clamp_i8(value: f32) -> i8 {
    finite_or_zero(value)
        .round()
        .clamp(i8::MIN as f32, i8::MAX as f32) as i8
}

fn clamp_u8(value: f32) -> u8 {
    finite_nonnegative(value).round().min(u8::MAX as f32) as u8
}

fn clamp_u16(value: f32) -> u16 {
    finite_nonnegative(value).round().min(u16::MAX as f32) as u16
}

fn percent_u8(value: f32) -> u8 {
    clamp_u8(clamp_unit(value) * 100.0)
}

fn seconds_to_u32_ms(seconds: f32) -> u32 {
    if !seconds.is_finite() || seconds <= 0.0 {
        0
    } else {
        (seconds * 1000.0).round().min(u32::MAX as f32) as u32
    }
}

fn zero_u8_wheels() -> WheelValuesU8 {
    WheelValuesU8 {
        fl: 0,
        fr: 0,
        rl: 0,
        rr: 0,
    }
}

#[cfg(windows)]
fn push_values<const N: usize>(fields: &mut Vec<String>, values: [f32; N]) {
    fields.extend(values.map(format_f32));
}

#[cfg(windows)]
fn format_f32(value: f32) -> String {
    format!("{:.6}", finite_or_zero(value))
}

#[cfg(windows)]
fn csv_text(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

#[cfg(windows)]
fn display_or_unknown(value: &str) -> &str {
    if value.is_empty() { "unknown" } else { value }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn session_context(track: &str, car: &str) -> AcrSessionContext {
        AcrSessionContext {
            identity: SessionIdentity::new("acr", track, "stage").with_vehicle(car),
        }
    }

    #[test]
    fn parses_live_acr_offsets_and_normalizes_update() {
        let mut physics_bytes = vec![0_u8; ACR_PHYSICS_SIZE];
        write_i32(&mut physics_bytes, PACKET_ID_OFFSET, 1234);
        write_f32(&mut physics_bytes, THROTTLE_OFFSET, 0.75);
        write_f32(&mut physics_bytes, BRAKE_OFFSET, 0.25);
        write_f32(&mut physics_bytes, FUEL_OFFSET, 30.0);
        write_i32(&mut physics_bytes, GEAR_OFFSET, 4);
        write_i32(&mut physics_bytes, RPM_OFFSET, 6200);
        write_f32(&mut physics_bytes, STEER_OFFSET, -0.15);
        write_f32(&mut physics_bytes, SPEED_OFFSET, 101.6);
        write_f32(&mut physics_bytes, CLUTCH_OFFSET, 1.0);
        write_f32(&mut physics_bytes, AIR_TEMP_OFFSET, 286.15);
        write_f32(&mut physics_bytes, ROAD_TEMP_OFFSET, 304.15);
        write_f32(&mut physics_bytes, WATER_TEMP_OFFSET, 351.15);
        write_f32(&mut physics_bytes, BRAKE_BIAS_OFFSET, 0.52);
        write_i32(&mut physics_bytes, CURRENT_MAX_RPM_OFFSET, 7500);
        write_wheels(&mut physics_bytes, TYRE_PRESSURE_OFFSET, [32.0; 4]);
        write_wheels(&mut physics_bytes, TYRE_CORE_TEMP_OFFSET, [363.15; 4]);
        write_wheels(&mut physics_bytes, BRAKE_TEMP_OFFSET, [323.15; 4]);

        let physics = parse_acr_physics(&physics_bytes).unwrap();
        let graphics = AcrGraphicsSnapshot {
            packet_id: 50,
            distance_m: 778.2,
            ..AcrGraphicsSnapshot::default()
        };
        let statics = AcrStaticSnapshot {
            track_length_m: 12_218.3,
            ..AcrStaticSnapshot::default()
        };
        let update = build_acr_update(
            &physics,
            &graphics,
            &statics,
            StageObservation {
                stage_number: 1,
                elapsed_s: 12.5,
                reset: false,
                state: AcrStageState::Running,
                event: AcrStageEvent::None,
                official_time_ms: None,
                max_distance_m: 778.2,
                attempt_invalid: false,
                result_screen_entered: false,
            },
            42,
        );
        let input = update.input.unwrap();
        let lap = update.lap.unwrap();
        let session = update.session.unwrap();
        let status = update.status.unwrap();

        assert_eq!(physics.packet_id, 1234);
        assert_eq!(input.gear, 3);
        assert_eq!(input.speed_kmh, 102);
        assert_eq!(input.rpm, 6200);
        assert_eq!(input.clutch, 0);
        assert_eq!(input.tyre_inner_temps_c.fl, 90);
        assert_eq!(input.brake_temps_c.fl, 50);
        assert_eq!(input.engine_temp_c, 78);
        assert!((lap.lap_distance_m - 778.2).abs() < 0.01);
        assert_eq!(lap.current_lap_time_ms, 12_500);
        assert_eq!(session.track_length_m, 12_218);
        assert_eq!(session.air_temp_c, 13);
        assert_eq!(session.track_temp_c, 31);
        assert_eq!(status.front_brake_bias, 52);
        assert_eq!(status.max_rpm, 7500);
    }

    #[test]
    fn parses_acr_graphics_and_static_context() {
        let mut graphics = vec![0_u8; ACR_GRAPHICS_SIZE];
        write_i32(&mut graphics, GRAPHICS_PACKET_ID_OFFSET, 99);
        write_f32(&mut graphics, GRAPHICS_DISTANCE_OFFSET, 3_542.5);
        let parsed_graphics = parse_acr_graphics(&graphics).unwrap();
        assert_eq!(parsed_graphics.packet_id, 99);
        assert!((parsed_graphics.distance_m - 3_542.5).abs() < 0.01);

        let mut statics = vec![0_u8; ACR_STATIC_SIZE];
        write_utf16(&mut statics, STATIC_CAR_MODEL_OFFSET, "Hyundai i20N Rally2");
        write_utf16(&mut statics, STATIC_TRACK_OFFSET, "Greece Elatia - Zeli");
        write_f32(&mut statics, STATIC_TRACK_LENGTH_OFFSET, 12_218.34);
        let parsed_statics = parse_acr_static(&statics).unwrap();
        assert_eq!(parsed_statics.car_model, "Hyundai i20N Rally2");
        assert_eq!(parsed_statics.track, "Greece Elatia - Zeli");
        assert!((parsed_statics.track_length_m - 12_218.34).abs() < 0.01);
    }

    #[derive(serde::Deserialize)]
    struct ReplayFixture {
        schema_version: u8,
        source: String,
        cases: Vec<ReplayCase>,
    }

    #[derive(serde::Deserialize)]
    struct ReplayCase {
        name: String,
        frames: Vec<ReplayFrame>,
    }

    #[derive(serde::Deserialize)]
    struct ReplayFrame {
        at_ms: u64,
        packet_id: i32,
        status: i32,
        distance_m: f32,
        speed_kmh: f32,
        engine_running: bool,
        current_time_ms: i32,
        last_time_ms: i32,
        completed_laps: i32,
        expected_stage: u8,
        expected_state: AcrStageState,
        expected_event: AcrStageEvent,
        expected_invalid: bool,
    }

    #[test]
    fn replays_anonymous_stage_lifecycle_fixture() {
        let fixture: ReplayFixture =
            serde_json::from_str(include_str!("../../tests/fixtures/acr_stage_replay.json"))
                .unwrap();
        assert_eq!(fixture.schema_version, 1);
        assert_eq!(fixture.source, "synthetic-anonymous");

        for case in fixture.cases {
            let start = Instant::now();
            let mut tracker = AcrStageTracker::new(start, None);
            for frame in case.frames {
                let graphics = AcrGraphicsSnapshot {
                    packet_id: frame.packet_id,
                    status: frame.status,
                    completed_laps: frame.completed_laps,
                    current_time_ms: frame.current_time_ms,
                    last_time_ms: frame.last_time_ms,
                    distance_m: frame.distance_m,
                    ..AcrGraphicsSnapshot::default()
                };
                let observation = tracker.observe(
                    &graphics,
                    frame.speed_kmh,
                    frame.engine_running,
                    start + Duration::from_millis(frame.at_ms),
                );
                assert_eq!(
                    observation.stage_number, frame.expected_stage,
                    "{} packet {} stage",
                    case.name, frame.packet_id
                );
                assert_eq!(
                    observation.state, frame.expected_state,
                    "{} packet {} state",
                    case.name, frame.packet_id
                );
                assert_eq!(
                    observation.event, frame.expected_event,
                    "{} packet {} event",
                    case.name, frame.packet_id
                );
                assert_eq!(
                    observation.attempt_invalid, frame.expected_invalid,
                    "{} packet {} validity",
                    case.name, frame.packet_id
                );
            }
        }
    }

    #[test]
    fn official_finish_wins_over_same_frame_distance_reset() {
        let start = Instant::now();
        let mut tracker = AcrStageTracker::new(start, None);
        tracker.observe(&stage_graphics(0.0), 0.0, true, start);
        tracker.observe(
            &stage_graphics(5.0),
            20.0,
            true,
            start + Duration::from_millis(100),
        );
        tracker.observe(
            &stage_graphics(1_200.0),
            90.0,
            true,
            start + Duration::from_secs(10),
        );
        let finished = tracker.observe(
            &AcrGraphicsSnapshot {
                status: 2,
                distance_m: 5.0,
                last_time_ms: 90_000,
                completed_laps: 1,
                ..AcrGraphicsSnapshot::default()
            },
            0.0,
            true,
            start + Duration::from_secs(90),
        );

        assert_eq!(finished.state, AcrStageState::Finished);
        assert_eq!(finished.event, AcrStageEvent::Finished);
        assert!(!finished.attempt_invalid);
        assert_eq!(finished.official_time_ms, Some(90_000));
    }

    #[test]
    fn live_status_without_a_driving_session_does_not_start_an_attempt() {
        let start = Instant::now();
        let mut tracker = AcrStageTracker::new(start, None);
        let observation = tracker.observe(
            &AcrGraphicsSnapshot {
                status: 2,
                session_type: -1,
                distance_m: 20.0,
                ..AcrGraphicsSnapshot::default()
            },
            30.0,
            true,
            start,
        );

        assert_eq!(observation.state, AcrStageState::Idle);
        assert_eq!(observation.event, AcrStageEvent::None);
    }

    #[test]
    fn manual_finish_distance_is_connected_to_tracker() {
        let start = Instant::now();
        let mut tracker = AcrStageTracker::new(start, Some(1_000.0));
        tracker.observe(&stage_graphics(0.0), 0.0, true, start);
        tracker.observe(
            &stage_graphics(5.0),
            20.0,
            true,
            start + Duration::from_millis(100),
        );
        let finished = tracker.observe(
            &AcrGraphicsSnapshot {
                current_time_ms: 59_500,
                ..stage_graphics(995.0)
            },
            80.0,
            true,
            start + Duration::from_secs(60),
        );
        assert_eq!(finished.event, AcrStageEvent::Finished);
        assert_eq!(finished.state, AcrStageState::Finished);
        assert!((finished.elapsed_s - 59.5).abs() < f32::EPSILON);
    }

    #[test]
    fn waits_for_late_official_result_without_explicit_manual_finish() {
        let start = Instant::now();
        let finish_distance = select_acr_finish_distance(None, 1_000.0);
        assert_eq!(finish_distance, None);
        let mut tracker = AcrStageTracker::new(start, finish_distance);
        tracker.observe(&stage_graphics(0.0), 0.0, true, start);
        tracker.observe(
            &stage_graphics(5.0),
            20.0,
            true,
            start + Duration::from_millis(100),
        );

        // A reported 1,000 m static track length must not implicitly turn 99.5% progress into
        // completion. With no explicit manual finish configured, the tracker stays live.
        let near_static_finish = tracker.observe(
            &AcrGraphicsSnapshot {
                current_time_ms: 59_500,
                ..stage_graphics(995.0)
            },
            80.0,
            true,
            start + Duration::from_millis(59_500),
        );
        assert_eq!(near_static_finish.state, AcrStageState::Running);
        assert_eq!(near_static_finish.event, AcrStageEvent::None);
        assert_eq!(near_static_finish.official_time_ms, None);

        let official = tracker.observe(
            &AcrGraphicsSnapshot {
                status: 1,
                distance_m: 995.0,
                last_time_ms: 60_000,
                completed_laps: 1,
                ..AcrGraphicsSnapshot::default()
            },
            0.0,
            false,
            start + Duration::from_secs(60),
        );
        assert_eq!(official.state, AcrStageState::Finished);
        assert_eq!(official.event, AcrStageEvent::Finished);
        assert_eq!(official.official_time_ms, Some(60_000));
        assert!((official.elapsed_s - 60.0).abs() < f32::EPSILON);
    }

    #[test]
    fn result_screen_is_recorded_separately_from_attempt_outcome() {
        let start = Instant::now();
        let mut tracker = AcrStageTracker::new(start, None);
        tracker.observe(&stage_graphics(0.0), 0.0, true, start);
        tracker.observe(
            &stage_graphics(5.0),
            20.0,
            true,
            start + Duration::from_millis(100),
        );
        let aborted = tracker.observe(
            &AcrGraphicsSnapshot {
                status: 3,
                distance_m: 500.0,
                ..AcrGraphicsSnapshot::default()
            },
            0.0,
            false,
            start + Duration::from_secs(20),
        );
        let same_screen = tracker.observe(
            &AcrGraphicsSnapshot {
                status: 3,
                distance_m: 500.0,
                ..AcrGraphicsSnapshot::default()
            },
            0.0,
            false,
            start + Duration::from_secs(21),
        );

        assert_eq!(aborted.state, AcrStageState::Aborted);
        assert_eq!(aborted.event, AcrStageEvent::Aborted);
        assert!(aborted.result_screen_entered);
        assert_eq!(same_screen.event, AcrStageEvent::None);
        assert!(!same_screen.result_screen_entered);

        let mut tracker = AcrStageTracker::new(start, None);
        tracker.observe(&stage_graphics(0.0), 0.0, true, start);
        tracker.observe(
            &stage_graphics(5.0),
            20.0,
            true,
            start + Duration::from_millis(100),
        );
        let finished = tracker.observe(
            &AcrGraphicsSnapshot {
                status: 2,
                distance_m: 995.0,
                last_time_ms: 58_000,
                completed_laps: 1,
                ..AcrGraphicsSnapshot::default()
            },
            0.0,
            true,
            start + Duration::from_secs(58),
        );
        let result = tracker.observe(
            &AcrGraphicsSnapshot {
                status: 3,
                distance_m: 0.0,
                last_time_ms: 58_000,
                completed_laps: 1,
                ..AcrGraphicsSnapshot::default()
            },
            0.0,
            false,
            start + Duration::from_secs(59),
        );
        assert_eq!(finished.event, AcrStageEvent::Finished);
        assert!(!finished.result_screen_entered);
        assert_eq!(result.state, AcrStageState::Finished);
        assert_eq!(result.event, AcrStageEvent::None);
        assert!(result.result_screen_entered);
    }

    #[test]
    fn validation_does_not_consume_live_terminal_and_preserves_non_live_official_finish() {
        let start = Instant::now();
        let statics = AcrStaticSnapshot {
            max_rpm: 8_000,
            ..AcrStaticSnapshot::default()
        };
        let mut tracker = AcrStageTracker::new(start, Some(1_000.0));
        let mut validator = AcrFrameValidator::default();
        let physics = valid_physics(1);

        observe_validated_stage(
            &mut tracker,
            &mut validator,
            &physics,
            &stage_graphics(0.0),
            &statics,
            start,
        )
        .unwrap();
        observe_validated_stage(
            &mut tracker,
            &mut validator,
            &physics,
            &stage_graphics(5.0),
            &statics,
            start + Duration::from_millis(100),
        )
        .unwrap();

        let mut invalid = physics.clone();
        invalid.rpm = -1;
        assert!(
            observe_validated_stage(
                &mut tracker,
                &mut validator,
                &invalid,
                &stage_graphics(995.0),
                &statics,
                start + Duration::from_secs(59),
            )
            .is_err()
        );
        assert_eq!(tracker.state, AcrStageState::Running);

        let (finished, frame_valid) = observe_validated_stage(
            &mut tracker,
            &mut validator,
            &physics,
            &stage_graphics(995.0),
            &statics,
            start + Duration::from_secs(60),
        )
        .unwrap();
        assert!(frame_valid);
        assert_eq!(finished.event, AcrStageEvent::Finished);

        let (late_official, frame_valid) = observe_validated_stage(
            &mut tracker,
            &mut validator,
            &invalid,
            &AcrGraphicsSnapshot {
                status: 2,
                distance_m: 995.0,
                last_time_ms: 60_500,
                completed_laps: 1,
                ..AcrGraphicsSnapshot::default()
            },
            &statics,
            start + Duration::from_millis(60_500),
        )
        .unwrap();
        assert!(!frame_valid);
        assert_eq!(late_official.event, AcrStageEvent::Finished);
        assert_eq!(late_official.official_time_ms, Some(60_500));

        let mut tracker = AcrStageTracker::new(start, None);
        let mut validator = AcrFrameValidator::default();
        observe_validated_stage(
            &mut tracker,
            &mut validator,
            &physics,
            &stage_graphics(0.0),
            &statics,
            start,
        )
        .unwrap();
        observe_validated_stage(
            &mut tracker,
            &mut validator,
            &physics,
            &stage_graphics(5.0),
            &statics,
            start + Duration::from_millis(100),
        )
        .unwrap();
        let (official, frame_valid) = observe_validated_stage(
            &mut tracker,
            &mut validator,
            &invalid,
            &AcrGraphicsSnapshot {
                status: 2,
                distance_m: 995.0,
                last_time_ms: 58_000,
                completed_laps: 1,
                ..AcrGraphicsSnapshot::default()
            },
            &statics,
            start + Duration::from_secs(58),
        )
        .unwrap();
        assert!(!frame_valid);
        assert_eq!(official.event, AcrStageEvent::Finished);
        assert_eq!(official.official_time_ms, Some(58_000));

        let mut tracker = AcrStageTracker::new(start, None);
        let mut validator = AcrFrameValidator::default();
        observe_validated_stage(
            &mut tracker,
            &mut validator,
            &physics,
            &stage_graphics(0.0),
            &statics,
            start,
        )
        .unwrap();
        observe_validated_stage(
            &mut tracker,
            &mut validator,
            &physics,
            &stage_graphics(5.0),
            &statics,
            start + Duration::from_millis(100),
        )
        .unwrap();
        let (official, _) = observe_validated_stage(
            &mut tracker,
            &mut validator,
            &invalid,
            &AcrGraphicsSnapshot {
                status: 1,
                distance_m: 995.0,
                last_time_ms: 59_000,
                completed_laps: 1,
                ..AcrGraphicsSnapshot::default()
            },
            &statics,
            start + Duration::from_secs(60),
        )
        .unwrap();
        assert_eq!(official.event, AcrStageEvent::Finished);
        assert_eq!(official.official_time_ms, Some(59_000));
    }

    #[test]
    fn recovery_stays_terminal_until_restart_evidence_near_start() {
        let start = Instant::now();
        let mut tracker = AcrStageTracker::new(start, None);
        tracker.observe(&stage_graphics(0.0), 0.0, true, start);
        tracker.observe(
            &stage_graphics(5.0),
            20.0,
            true,
            start + Duration::from_millis(100),
        );
        tracker.observe(
            &stage_graphics(1_200.0),
            90.0,
            true,
            start + Duration::from_secs(10),
        );
        let recovery = tracker.observe(
            &stage_graphics(600.0),
            25.0,
            true,
            start + Duration::from_secs(11),
        );
        let continuing = tracker.observe(
            &stage_graphics(650.0),
            35.0,
            true,
            start + Duration::from_secs(12),
        );
        let restart = tracker.observe(
            &stage_graphics(50.0),
            0.0,
            true,
            start + Duration::from_secs(13),
        );

        assert_eq!(recovery.event, AcrStageEvent::Recovery);
        assert_eq!(continuing.state, AcrStageState::Recovery);
        assert_eq!(continuing.event, AcrStageEvent::None);
        assert_eq!(continuing.stage_number, 1);
        assert!(continuing.attempt_invalid);
        assert_eq!(restart.event, AcrStageEvent::NextAttempt);
        assert_eq!(restart.stage_number, 2);
        assert!(!restart.attempt_invalid);
    }

    #[test]
    fn validator_rejects_negative_speed_rpm_invalid_gear_and_abrupt_g_force() {
        let now = Instant::now();
        let graphics = stage_graphics(100.0);
        let statics = AcrStaticSnapshot {
            max_rpm: 8_000,
            ..AcrStaticSnapshot::default()
        };

        let mut negative_rpm = valid_physics(1);
        negative_rpm.rpm = -1;
        assert!(
            AcrFrameValidator::default()
                .validate(&negative_rpm, &graphics, &statics, now)
                .is_err()
        );

        let mut negative_speed = valid_physics(2);
        negative_speed.speed_kmh = -1.0;
        assert!(
            AcrFrameValidator::default()
                .validate(&negative_speed, &graphics, &statics, now)
                .is_err()
        );

        let mut invalid_gear = valid_physics(3);
        invalid_gear.raw_gear = 14;
        assert!(
            AcrFrameValidator::default()
                .validate(&invalid_gear, &graphics, &statics, now)
                .is_err()
        );

        let mut excessive_rpm = valid_physics(4);
        excessive_rpm.rpm = 17_000;
        assert!(
            AcrFrameValidator::default()
                .validate(&excessive_rpm, &graphics, &statics, now)
                .is_err()
        );

        let mut validator = AcrFrameValidator::default();
        validator
            .validate(&valid_physics(5), &graphics, &statics, now)
            .unwrap();
        let mut abrupt_g = valid_physics(6);
        abrupt_g.g_force[0] = 13.0;
        let error = validator
            .validate(
                &abrupt_g,
                &graphics,
                &statics,
                now + Duration::from_millis(10),
            )
            .unwrap_err();
        assert!(error.contains("g_jump=13.0"));
    }

    #[test]
    fn capture_gate_downsamples_and_forces_stage_events() {
        let start = Instant::now();
        let mut gate = AcrCaptureGate::new(25);
        assert!(gate.should_capture(AcrStageEvent::None, start));
        assert!(!gate.should_capture(AcrStageEvent::None, start + Duration::from_millis(20)));
        assert!(gate.should_capture(AcrStageEvent::None, start + Duration::from_millis(40)));
        assert!(gate.should_capture(AcrStageEvent::Recovery, start + Duration::from_millis(45)));
    }

    #[test]
    fn rejects_skewed_live_mappings_but_allows_a_true_pause() {
        let start = Instant::now();
        let mut freshness = AcrGraphicsFreshness::default();
        let mut graphics = stage_graphics(10.0);
        graphics.packet_id = 7;
        assert!(freshness.observe(10, &graphics, start));
        assert!(freshness.observe(11, &graphics, start + Duration::from_millis(20)));
        assert!(!freshness.observe(12, &graphics, start + Duration::from_millis(100)));
        assert!(freshness.observe(12, &graphics, start + Duration::from_millis(400)));
        graphics.packet_id = 8;
        assert!(freshness.observe(13, &graphics, start + Duration::from_millis(410)));
        graphics.status = 3;
        assert!(freshness.observe(13, &graphics, start + Duration::from_secs(1)));
    }

    #[test]
    fn requires_complete_static_context_before_resetting_session() {
        assert!(
            AcrSessionContext::from_static(&AcrStaticSnapshot {
                track: "Elatia".to_owned(),
                ..AcrStaticSnapshot::default()
            })
            .is_none()
        );
        assert_eq!(
            AcrSessionContext::from_static(&AcrStaticSnapshot {
                track: "Elatia".to_owned(),
                car_model: "Rally2".to_owned(),
                ..AcrStaticSnapshot::default()
            })
            .unwrap()
            .slug(),
            "elatia-rally2"
        );
    }

    #[test]
    fn completion_compares_prior_finish_and_failure_at_common_distance() {
        let previous_finish_points = linear_attempt_points(300.0, 32.0, 161);
        let previous_failure_points = linear_attempt_points(200.0, 20.0, 101);
        let latest_points = linear_attempt_points(300.0, 28.0, 141);
        let previous_finish = attempt_trace(
            1,
            AcrAttemptOutcome::Finished,
            32.0,
            &previous_finish_points,
        );
        let previous_failure = attempt_trace(
            2,
            AcrAttemptOutcome::Recovery,
            20.0,
            &previous_failure_points,
        );
        let latest = attempt_trace(3, AcrAttemptOutcome::Finished, 28.0, &latest_points);
        let analyzer = AcrStageAnalyzer {
            target_time_s: Some(30.0),
            history: vec![previous_finish, previous_failure],
            ..AcrStageAnalyzer::default()
        };
        let report = analyzer.build_report(&latest);

        assert_eq!(report.comparisons.len(), 2);
        let failure = report
            .comparisons
            .iter()
            .find(|comparison| comparison.baseline_outcome == AcrAttemptOutcome::Recovery)
            .unwrap();
        assert!((failure.common_distance_m - 200.0).abs() < 0.01);
        assert!((failure.delta_s + 1.333_333).abs() < 0.01);
        assert_eq!(report.target_delta_s, Some(-2.0));
        assert_eq!(report.learning_trend.completed_attempts, 2);
        assert_eq!(
            report.learning_trend.delta_to_previous_completion_s,
            Some(-4.0)
        );
        assert_eq!(report.quality.status, TraceQualityStatus::Valid);
        assert!(failure.confidence < 1.0);
        assert_ne!(failure.confidence_level, AnalysisConfidenceLevel::High);
        assert!(
            failure
                .confidence_reasons
                .contains(&AnalysisLimitation::ComparisonIncludesFailedOrInvalidAttempt)
        );
    }

    #[test]
    fn excludes_partial_finished_attempts_from_coaching_references() {
        let partial = attempt_trace(
            1,
            AcrAttemptOutcome::Finished,
            30.0,
            &[(0.0, 0.0), (300.0, 30.0)],
        );
        let latest_points = linear_attempt_points(300.0, 29.0, 146);
        let latest = attempt_trace(2, AcrAttemptOutcome::Finished, 29.0, &latest_points);
        let analyzer = AcrStageAnalyzer {
            history: vec![partial],
            ..AcrStageAnalyzer::default()
        };

        let report = analyzer.build_report(&latest);

        assert!(report.comparisons.is_empty());
        assert_eq!(report.learning_trend.completed_attempts, 1);
        assert_eq!(report.learning_trend.delta_to_previous_completion_s, None);
        assert_eq!(report.quality.status, TraceQualityStatus::Valid);
    }

    #[test]
    fn reports_repeated_brake_applications() {
        let previous_points = linear_attempt_points(300.0, 30.0, 151);
        let latest_points = linear_attempt_points(300.0, 29.0, 151);
        let mut previous = attempt_trace(1, AcrAttemptOutcome::Finished, 30.0, &previous_points);
        let mut latest = attempt_trace(2, AcrAttemptOutcome::Finished, 29.0, &latest_points);
        for attempt in [&mut previous, &mut latest] {
            attempt.points[20].brake = 0.8;
            attempt.points[80].brake = 0.8;
        }
        assert!(attempt_is_reference_usable(&previous));
        assert!(attempt_is_reference_usable(&latest));
        let metrics = habit_metrics(&latest, std::slice::from_ref(&previous));
        let findings = repeated_habits(&latest, &metrics, &[previous]);

        assert_eq!(metrics.brake_applications, 2);
        assert!(findings.contains(&"repeated_brake_applications".to_owned()));
    }

    #[test]
    fn filters_partial_technique_references_without_hiding_recovery_frequency() {
        let partial_points = linear_attempt_points(300.0, 30.0, 5);
        let valid_points = linear_attempt_points(300.0, 29.0, 151);
        let mut partial_previous =
            attempt_trace(1, AcrAttemptOutcome::Finished, 30.0, &partial_points);
        let mut valid_current = attempt_trace(4, AcrAttemptOutcome::Finished, 29.0, &valid_points);
        for attempt in [&mut partial_previous, &mut valid_current] {
            attempt.points[1].brake = 0.8;
            attempt.points[3].brake = 0.8;
        }
        valid_current.points[20].brake = 0.8;
        valid_current.points[80].brake = 0.8;
        assert!(!attempt_is_reference_usable(&partial_previous));
        assert!(attempt_is_reference_usable(&valid_current));
        let recovery_one = attempt_trace(
            2,
            AcrAttemptOutcome::Recovery,
            10.0,
            &[(0.0, 0.0), (100.0, 10.0)],
        );
        let recovery_two = attempt_trace(
            3,
            AcrAttemptOutcome::Recovery,
            12.0,
            &[(0.0, 0.0), (120.0, 12.0)],
        );
        let history = vec![partial_previous, recovery_one, recovery_two];
        let metrics = habit_metrics(&valid_current, &history);
        let findings = repeated_habits(&valid_current, &metrics, &history);

        assert!(!findings.contains(&"repeated_brake_applications".to_owned()));
        assert!(findings.contains(&"repeated_recovery".to_owned()));

        let mut partial_current =
            attempt_trace(5, AcrAttemptOutcome::Finished, 30.0, &partial_points);
        partial_current.points[1].brake = 0.8;
        partial_current.points[3].brake = 0.8;
        let mut valid_previous = attempt_trace(4, AcrAttemptOutcome::Finished, 29.0, &valid_points);
        valid_previous.points[20].brake = 0.8;
        valid_previous.points[80].brake = 0.8;
        assert!(!attempt_is_reference_usable(&partial_current));
        assert!(attempt_is_reference_usable(&valid_previous));
        let history = vec![valid_previous];
        let metrics = habit_metrics(&partial_current, &history);
        let findings = repeated_habits(&partial_current, &metrics, &history);

        assert!(!findings.contains(&"repeated_brake_applications".to_owned()));
    }

    #[test]
    fn quality_score_exposes_validator_drops() {
        let dense = (0..30)
            .map(|index| (index as f32 * 5.0, index as f32 * 0.04))
            .collect::<Vec<_>>();
        let mut attempt = attempt_trace(1, AcrAttemptOutcome::Finished, 1.16, &dense);
        attempt.validator_drops = 5;
        let report = AcrStageAnalyzer::default().build_report(&attempt);

        assert!(report.quality.score < 100);
        assert!(
            report
                .quality_evidence
                .contains(&"validator_drops:5".to_owned())
        );

        let mut attempt = attempt_trace(2, AcrAttemptOutcome::Finished, 1.16, &dense);
        attempt.archive_backpressure_drops = 1;
        let report = AcrStageAnalyzer::default().build_report(&attempt);
        assert_eq!(report.quality.status, TraceQualityStatus::Partial);
        assert!(
            report
                .quality_evidence
                .contains(&"archive_backpressure_drops:1".to_owned())
        );
    }

    #[test]
    fn analyzer_reports_stage_scoped_archive_backpressure_drops() {
        let physics = valid_physics(1);
        let graphics = stage_graphics(100.0);
        let statics = AcrStaticSnapshot {
            track: "Synthetic".to_owned(),
            car_model: "Anonymous".to_owned(),
            track_length_m: 100.0,
            ..AcrStaticSnapshot::default()
        };
        let mut analyzer = AcrStageAnalyzer::default();

        assert!(
            analyzer
                .ingest(
                    &physics,
                    &graphics,
                    &statics,
                    stage_observation(2, AcrStageState::Running, AcrStageEvent::Started),
                    true,
                    CaptureCounters {
                        persistence_dropped_frames: 7,
                        ..CaptureCounters::default()
                    },
                )
                .is_none()
        );
        let report = analyzer
            .ingest(
                &physics,
                &graphics,
                &statics,
                stage_observation(2, AcrStageState::Finished, AcrStageEvent::Finished),
                true,
                CaptureCounters {
                    persistence_dropped_frames: 10,
                    ..CaptureCounters::default()
                },
            )
            .unwrap();

        assert_eq!(report.archive_backpressure_drops, 3);
        assert_eq!(report.quality.status, TraceQualityStatus::Partial);
        assert!(
            report
                .quality_evidence
                .contains(&"archive_backpressure_drops:3".to_owned())
        );
    }

    #[test]
    fn contextual_overrev_can_be_gain_and_mechanical_risk() {
        let baseline = AcrAttemptTrace {
            stage_number: 1,
            track: "Synthetic".to_owned(),
            car: "Anonymous".to_owned(),
            outcome: AcrAttemptOutcome::Finished,
            elapsed_s: 40.0,
            official_time_ms: Some(40_000),
            max_distance_m: 400.0,
            track_length_m: 400.0,
            invalid: false,
            validator_drops: 0,
            archive_backpressure_drops: 0,
            points: synthetic_overrev_points(10.0, false),
        };
        let latest = AcrAttemptTrace {
            stage_number: 2,
            track: "Synthetic".to_owned(),
            car: "Anonymous".to_owned(),
            outcome: AcrAttemptOutcome::Finished,
            elapsed_s: 36.4,
            official_time_ms: Some(36_400),
            max_distance_m: 400.0,
            track_length_m: 400.0,
            invalid: false,
            validator_drops: 0,
            archive_backpressure_drops: 0,
            points: synthetic_overrev_points(11.0, true),
        };
        let assessment = assess_overrev(&latest, Some(&baseline));

        assert_eq!(assessment.event_count, 1);
        assert!(assessment.mechanical_risk);
        assert!(assessment.technique_gain);
        assert_eq!(
            assessment.classification,
            AcrOverrevClassification::TechniqueGainWithMechanicalRisk
        );
        assert!(assessment.segment_delta_s.unwrap() < 0.0);
        assert!(assessment.next_segment_delta_s.unwrap() < 0.0);
    }

    #[test]
    fn evaluates_engine_braking_with_entry_and_next_segment_context() {
        let baseline = AcrAttemptTrace {
            stage_number: 1,
            track: "Synthetic".to_owned(),
            car: "Anonymous".to_owned(),
            outcome: AcrAttemptOutcome::Finished,
            elapsed_s: 40.0,
            official_time_ms: Some(40_000),
            max_distance_m: 400.0,
            track_length_m: 400.0,
            invalid: false,
            validator_drops: 0,
            archive_backpressure_drops: 0,
            points: synthetic_overrev_points(10.0, false),
        };
        let mut points = synthetic_overrev_points(11.0, false);
        points[0].gear = 4;
        points[1].gear = 4;
        points[1].speed_kmh = 120.0;
        points[2].gear = 3;
        points[2].speed_kmh = 100.0;
        points[2].throttle = 0.0;
        points[2].brake = 0.0;
        let latest = AcrAttemptTrace {
            stage_number: 2,
            track: "Synthetic".to_owned(),
            car: "Anonymous".to_owned(),
            outcome: AcrAttemptOutcome::Finished,
            elapsed_s: 36.4,
            official_time_ms: Some(36_400),
            max_distance_m: 400.0,
            track_length_m: 400.0,
            invalid: false,
            validator_drops: 0,
            archive_backpressure_drops: 0,
            points,
        };
        let assessment = assess_engine_braking(&latest, Some(&baseline));

        assert!(assessment.engine_braking_detected);
        assert_eq!(assessment.event_count, 1);
        assert_eq!(
            assessment.classification,
            AcrEngineBrakingClassification::ControlledGain
        );
        assert!(assessment.entry_segment_delta_s.unwrap() < 0.0);
        assert!(assessment.next_segment_delta_s.unwrap() < 0.0);
        assert!(assessment.gear_continuity);
    }

    #[test]
    fn async_archive_finishes_zstd_stage_and_writes_utf8_reports() {
        let root = unique_temp_dir("archive");
        let context = session_context("Alsace Forêt", "익명 Rally2");
        let archive = AcrArchiveWriter::open(&root, &context, 7, 90).unwrap();
        let physics = valid_physics(10);
        let graphics = stage_graphics(100.0);
        let statics = AcrStaticSnapshot {
            track: context.track().to_owned(),
            car_model: context.car().to_owned(),
            max_rpm: 8_000,
            track_length_m: 1_000.0,
            ..AcrStaticSnapshot::default()
        };
        let stage = stage_observation(1, AcrStageState::Running, AcrStageEvent::Started);
        archive
            .record_frame(&physics, &graphics, &statics, stage)
            .unwrap();
        let mut finished_stage = stage;
        finished_stage.state = AcrStageState::Finished;
        finished_stage.event = AcrStageEvent::Finished;
        archive
            .record_frame(&physics, &graphics, &statics, finished_stage)
            .unwrap();
        archive.finish_stage(1).unwrap();
        let mut completed = attempt_trace(
            1,
            AcrAttemptOutcome::Finished,
            29.0,
            &[(0.0, 0.0), (100.0, 29.0)],
        );
        completed.track = context.track().to_owned();
        completed.car = context.car().to_owned();
        let report = AcrStageAnalyzer::new(Some(30.0)).build_report(&completed);
        archive.write_report(&report).unwrap();
        let mut result_stage = finished_stage;
        result_stage.result_screen_entered = true;
        archive
            .record_event_as(Some(&context), result_stage, AcrStageEvent::ResultScreen)
            .unwrap();
        archive.shutdown().unwrap();

        let raw_files = fs::read_dir(root.join("raw"))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect::<Vec<_>>();
        assert_eq!(raw_files.len(), 1);
        let decoded = zstd::stream::decode_all(File::open(&raw_files[0]).unwrap()).unwrap();
        let decoded = String::from_utf8(decoded).unwrap();
        assert!(decoded.contains("\"record_type\":\"telemetry\""));
        assert!(decoded.contains("\"stage_event\":\"started\""));
        assert!(decoded.contains("\"stage_event\":\"finished\""));
        assert!(decoded.contains("\"stage_event\":\"result_screen\""));
        assert!(decoded.contains("Alsace Forêt"));
        assert!(decoded.contains("익명 Rally2"));

        let analysis_files = fs::read_dir(root.join("analysis"))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect::<Vec<_>>();
        assert_eq!(analysis_files.len(), 2);
        assert!(
            analysis_files
                .iter()
                .any(|path| path.extension().is_some_and(|ext| ext == "json"))
        );
        assert!(
            analysis_files
                .iter()
                .any(|path| path.extension().is_some_and(|ext| ext == "md"))
        );
        for path in &analysis_files {
            let contents = fs::read_to_string(path).unwrap();
            assert!(contents.contains("Alsace Forêt"));
            assert!(contents.contains("익명 Rally2"));
            if path
                .extension()
                .is_some_and(|extension| extension == "json")
            {
                let json: serde_json::Value = serde_json::from_str(&contents).unwrap();
                assert_eq!(json["quality"]["status"], "partial");
                assert!(json["quality"]["reasons"].is_array());
                assert!(json.get("quality_status").is_none());
            }
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn late_official_result_replaces_manual_report_without_duplicate_attempt() {
        let root = unique_temp_dir("late-official-finish");
        let context = session_context("Synthetic Stage", "Anonymous Rally");
        let archive = AcrArchiveWriter::open(&root, &context, 7, 90).unwrap();
        let physics = valid_physics(10);
        let statics = AcrStaticSnapshot {
            track: context.track().to_owned(),
            car_model: context.car().to_owned(),
            max_rpm: 8_000,
            track_length_m: 1_000.0,
            ..AcrStaticSnapshot::default()
        };
        let start = Instant::now();
        let mut tracker = AcrStageTracker::new(start, Some(1_000.0));
        let mut analyzer = AcrStageAnalyzer::default();

        tracker.observe(&stage_graphics(0.0), 0.0, true, start);
        let started = tracker.observe(
            &stage_graphics(5.0),
            20.0,
            true,
            start + Duration::from_millis(100),
        );
        archive
            .record_frame(&physics, &stage_graphics(5.0), &statics, started)
            .unwrap();
        assert!(
            analyzer
                .ingest(
                    &physics,
                    &stage_graphics(5.0),
                    &statics,
                    started,
                    true,
                    CaptureCounters::default(),
                )
                .is_none()
        );

        let manual_graphics = AcrGraphicsSnapshot {
            current_time_ms: 59_500,
            ..stage_graphics(995.0)
        };
        let manual_finish = tracker.observe(
            &manual_graphics,
            80.0,
            true,
            start + Duration::from_millis(59_500),
        );
        assert_eq!(manual_finish.event, AcrStageEvent::Finished);
        assert_eq!(manual_finish.official_time_ms, None);
        archive
            .record_frame(&physics, &manual_graphics, &statics, manual_finish)
            .unwrap();
        let provisional_report = analyzer
            .ingest(
                &physics,
                &manual_graphics,
                &statics,
                manual_finish,
                true,
                CaptureCounters::default(),
            )
            .unwrap();
        let captured_points = provisional_report.trace.points.len();
        assert_eq!(provisional_report.official_time_ms, None);
        assert_eq!(analyzer.history.len(), 1);
        archive.finish_stage(1).unwrap();
        archive.write_report(&provisional_report).unwrap();

        let official_graphics = AcrGraphicsSnapshot {
            status: 3,
            distance_m: 0.0,
            last_time_ms: 60_000,
            completed_laps: 1,
            ..AcrGraphicsSnapshot::default()
        };
        let official_finish = tracker.observe(
            &official_graphics,
            0.0,
            false,
            start + Duration::from_secs(60),
        );
        assert_eq!(official_finish.stage_number, 1);
        assert_eq!(official_finish.event, AcrStageEvent::Finished);
        assert_eq!(official_finish.official_time_ms, Some(60_000));
        assert!(official_finish.result_screen_entered);
        archive
            .record_event(Some(&context), official_finish)
            .unwrap();
        let official_report = analyzer
            .ingest(
                &physics,
                &official_graphics,
                &statics,
                official_finish,
                false,
                CaptureCounters::default(),
            )
            .unwrap();
        assert_eq!(official_report.official_time_ms, Some(60_000));
        assert_eq!(official_report.elapsed_s, 60.0);
        assert_eq!(official_report.trace.points.len(), captured_points);
        assert_eq!(analyzer.history.len(), 1);
        assert_eq!(analyzer.history[0].official_time_ms, Some(60_000));
        archive.finish_stage(1).unwrap();
        archive.write_report(&official_report).unwrap();
        archive
            .record_event_as(Some(&context), official_finish, AcrStageEvent::ResultScreen)
            .unwrap();

        let repeated_screen = tracker.observe(
            &official_graphics,
            0.0,
            false,
            start + Duration::from_secs(61),
        );
        assert_eq!(repeated_screen.event, AcrStageEvent::None);
        assert!(!repeated_screen.result_screen_entered);
        archive.shutdown().unwrap();

        let raw_path = fs::read_dir(root.join("raw"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let decoded = zstd::stream::decode_all(File::open(raw_path).unwrap()).unwrap();
        let records = String::from_utf8(decoded)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            records
                .iter()
                .filter(|record| record["stage_event"] == "finished")
                .count(),
            2
        );
        assert_eq!(
            records
                .iter()
                .filter(|record| record["stage_event"] == "result_screen")
                .count(),
            1
        );
        assert!(records.iter().all(|record| record["stage"] == 1));

        let analysis_files = fs::read_dir(root.join("analysis"))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect::<Vec<_>>();
        assert_eq!(analysis_files.len(), 2);
        let json_path = analysis_files
            .iter()
            .find(|path| {
                path.extension()
                    .is_some_and(|extension| extension == "json")
            })
            .unwrap();
        let saved: serde_json::Value =
            serde_json::from_slice(&fs::read(json_path).unwrap()).unwrap();
        assert_eq!(saved["official_time_ms"], 60_000);
        assert_eq!(saved["trace"]["official_time_ms"], 60_000);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn restores_archived_attempts_for_coaching_after_a_process_restart() {
        let root = unique_temp_dir("history-restart");
        let context = session_context("Alsace Forêt", "익명 Rally2");
        let archive = AcrArchiveWriter::open(&root, &context, 7, 90).unwrap();
        let analyzer = AcrStageAnalyzer::new(Some(30.0));
        let previous_finish_points = linear_attempt_points(100.0, 30.0, 151);
        let mut previous_finish = attempt_trace(
            1,
            AcrAttemptOutcome::Finished,
            30.0,
            &previous_finish_points,
        );
        previous_finish.track = context.track().to_owned();
        previous_finish.car = context.car().to_owned();
        let mut previous_failure = attempt_trace(
            2,
            AcrAttemptOutcome::Aborted,
            20.0,
            &[(0.0, 0.0), (80.0, 20.0)],
        );
        previous_failure.track = context.track().to_owned();
        previous_failure.car = context.car().to_owned();
        archive
            .write_report(&analyzer.build_report(&previous_finish))
            .unwrap();
        archive
            .write_report(&analyzer.build_report(&previous_failure))
            .unwrap();
        drop(archive);

        let history = load_acr_history(&root, &context).unwrap();
        assert_eq!(history.len(), 2);
        let mut restarted = AcrStageAnalyzer::new(Some(30.0));
        restarted.restore_history(history);
        let latest_points = linear_attempt_points(100.0, 29.0, 146);
        let mut latest = attempt_trace(3, AcrAttemptOutcome::Finished, 29.0, &latest_points);
        latest.track = context.track().to_owned();
        latest.car = context.car().to_owned();
        let report = restarted.build_report(&latest);

        assert_eq!(report.learning_trend.completed_attempts, 2);
        assert!(
            report
                .comparisons
                .iter()
                .any(|comparison| { comparison.baseline_outcome == AcrAttemptOutcome::Finished })
        );
        assert!(
            report
                .comparisons
                .iter()
                .any(|comparison| { comparison.baseline_outcome == AcrAttemptOutcome::Aborted })
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn async_archive_flushes_an_active_stage_periodically() {
        let root = unique_temp_dir("periodic-flush");
        let context = session_context("Synthetic", "Anonymous");
        let archive = AcrArchiveWriter::open(&root, &context, 7, 90).unwrap();
        archive
            .record_frame(
                &valid_physics(10),
                &stage_graphics(100.0),
                &AcrStaticSnapshot {
                    track: context.track().to_owned(),
                    car_model: context.car().to_owned(),
                    ..AcrStaticSnapshot::default()
                },
                stage_observation(1, AcrStageState::Running, AcrStageEvent::Started),
            )
            .unwrap();

        let deadline = Instant::now() + Duration::from_millis(500);
        let flushed = loop {
            let length = fs::read_dir(root.join("raw"))
                .ok()
                .and_then(|mut entries| entries.next())
                .and_then(Result::ok)
                .and_then(|entry| entry.metadata().ok())
                .map_or(0, |metadata| metadata.len());
            if length > 0 {
                break true;
            }
            if Instant::now() >= deadline {
                break false;
            }
            thread::sleep(Duration::from_millis(10));
        };

        assert!(flushed, "active zstd stage was not flushed before finish");
        drop(archive);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn explicit_archive_shutdown_finalizes_an_active_zstd_stage() {
        let root = unique_temp_dir("explicit-shutdown");
        let context = session_context("Synthetic", "Anonymous");
        let archive = AcrArchiveWriter::open(&root, &context, 7, 90).unwrap();
        archive
            .record_frame(
                &valid_physics(10),
                &stage_graphics(100.0),
                &AcrStaticSnapshot {
                    track: context.track().to_owned(),
                    car_model: context.car().to_owned(),
                    ..AcrStaticSnapshot::default()
                },
                stage_observation(1, AcrStageState::Running, AcrStageEvent::Started),
            )
            .unwrap();

        archive.shutdown().unwrap();

        let raw_path = fs::read_dir(root.join("raw"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let decoded = zstd::stream::decode_all(File::open(raw_path).unwrap()).unwrap();
        assert!(
            String::from_utf8(decoded)
                .unwrap()
                .contains("\"record_type\":\"telemetry\"")
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn stage_finish_applies_retention_after_closing_the_active_archive() {
        let root = unique_temp_dir("stage-finish-retention");
        let context = session_context("Synthetic", "Anonymous");
        let archive = AcrArchiveWriter::open(&root, &context, 1, 1).unwrap();
        let expired_raw = root.join("raw/acr-raw-expired.jsonl.zst");
        let expired_analysis = root.join("analysis/acr-analysis-expired.json");
        fs::write(&expired_raw, b"expired").unwrap();
        fs::write(&expired_analysis, b"expired").unwrap();
        let expired_at = SystemTime::now() - Duration::from_secs(2 * 86_400);
        for path in [&expired_raw, &expired_analysis] {
            File::options()
                .write(true)
                .open(path)
                .unwrap()
                .set_times(std::fs::FileTimes::new().set_modified(expired_at))
                .unwrap();
        }

        archive
            .record_frame(
                &valid_physics(10),
                &stage_graphics(100.0),
                &AcrStaticSnapshot {
                    track: context.track().to_owned(),
                    car_model: context.car().to_owned(),
                    ..AcrStaticSnapshot::default()
                },
                stage_observation(1, AcrStageState::Running, AcrStageEvent::Started),
            )
            .unwrap();
        archive.finish_stage(1).unwrap();

        assert!(!expired_raw.exists());
        assert!(!expired_analysis.exists());
        let current_raw = fs::read_dir(root.join("raw"))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect::<Vec<_>>();
        assert_eq!(current_raw.len(), 1);
        let decoded = zstd::stream::decode_all(File::open(&current_raw[0]).unwrap()).unwrap();
        assert!(
            String::from_utf8(decoded)
                .unwrap()
                .contains("\"record_type\":\"telemetry\"")
        );

        archive.shutdown().unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn retention_removes_only_expired_app_owned_files() {
        let root = unique_temp_dir("retention");
        let raw = root.join("raw");
        let analysis = root.join("analysis");
        create_dir_all(&raw).unwrap();
        create_dir_all(&analysis).unwrap();
        let owned_raw = raw.join("acr-raw-old.jsonl.zst");
        let unrelated_raw = raw.join("notes.jsonl.zst");
        let wrong_extension = raw.join("acr-raw-old.txt");
        let owned_analysis = analysis.join("acr-analysis-old.json");
        let unrelated_analysis = analysis.join("report.json");
        for path in [
            &owned_raw,
            &unrelated_raw,
            &wrong_extension,
            &owned_analysis,
            &unrelated_analysis,
        ] {
            fs::write(path, b"test").unwrap();
        }
        let future = SystemTime::now() + Duration::from_secs(2 * 86_400);
        prune_owned_files(&raw, RAW_ARCHIVE_PREFIX, &[".jsonl.zst"], 1, future).unwrap();
        prune_owned_files(
            &analysis,
            ANALYSIS_ARCHIVE_PREFIX,
            &[".json", ".md"],
            1,
            future,
        )
        .unwrap();

        assert!(!owned_raw.exists());
        assert!(!owned_analysis.exists());
        assert!(unrelated_raw.exists());
        assert!(wrong_extension.exists());
        assert!(unrelated_analysis.exists());
        fs::remove_dir_all(root).unwrap();
    }

    fn stage_graphics(distance_m: f32) -> AcrGraphicsSnapshot {
        AcrGraphicsSnapshot {
            distance_m,
            status: 2,
            ..AcrGraphicsSnapshot::default()
        }
    }

    fn valid_physics(packet_id: i32) -> AcrPhysicsSnapshot {
        let mut physics = parse_acr_physics(&vec![0_u8; ACR_PHYSICS_SIZE]).unwrap();
        physics.packet_id = packet_id;
        physics.throttle = 0.5;
        physics.brake = 0.0;
        physics.raw_gear = 4;
        physics.rpm = 6_000;
        physics.speed_kmh = 100.0;
        physics.current_max_rpm = 8_000;
        physics.engine_running = true;
        physics
    }

    fn stage_observation(
        stage_number: u8,
        state: AcrStageState,
        event: AcrStageEvent,
    ) -> StageObservation {
        StageObservation {
            stage_number,
            elapsed_s: 10.0,
            reset: false,
            state,
            event,
            official_time_ms: None,
            max_distance_m: 100.0,
            attempt_invalid: false,
            result_screen_entered: false,
        }
    }

    fn attempt_trace(
        stage_number: u8,
        outcome: AcrAttemptOutcome,
        elapsed_s: f32,
        points: &[(f32, f32)],
    ) -> AcrAttemptTrace {
        AcrAttemptTrace {
            stage_number,
            track: "Synthetic".to_owned(),
            car: "Anonymous".to_owned(),
            outcome,
            elapsed_s,
            official_time_ms: (outcome == AcrAttemptOutcome::Finished)
                .then_some(seconds_to_u32_ms(elapsed_s)),
            max_distance_m: points.last().map_or(0.0, |point| point.0),
            track_length_m: points.last().map_or(0.0, |point| point.0),
            invalid: outcome != AcrAttemptOutcome::Finished,
            validator_drops: 0,
            archive_backpressure_drops: 0,
            points: points
                .iter()
                .map(|(distance_m, elapsed_s)| AcrTracePoint {
                    elapsed_s: *elapsed_s,
                    distance_m: *distance_m,
                    speed_kmh: 100.0,
                    rpm: 6_000,
                    max_rpm: 8_000,
                    gear: 3,
                    throttle: 0.8,
                    brake: 0.0,
                    steer: 0.0,
                    peak_wheel_slip: 0.1,
                })
                .collect(),
        }
    }

    fn linear_attempt_points(distance_m: f32, elapsed_s: f32, count: usize) -> Vec<(f32, f32)> {
        let denominator = count.saturating_sub(1).max(1) as f32;
        (0..count)
            .map(|index| {
                let progress = index as f32 / denominator;
                (distance_m * progress, elapsed_s * progress)
            })
            .collect()
    }

    fn synthetic_overrev_points(meters_per_second: f32, overrev: bool) -> Vec<AcrTracePoint> {
        (0..=8)
            .map(|index| {
                let distance_m = index as f32 * 50.0;
                AcrTracePoint {
                    elapsed_s: distance_m / meters_per_second,
                    distance_m,
                    speed_kmh: meters_per_second * 3.6,
                    rpm: if overrev && index == 2 { 8_300 } else { 7_500 },
                    max_rpm: 8_000,
                    gear: 3,
                    throttle: 1.0,
                    brake: 0.0,
                    steer: 0.0,
                    peak_wheel_slip: 0.2,
                }
            })
            .collect()
    }

    fn unique_temp_dir(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "sim-moza-bridge-acr-{label}-{}-{nonce}",
            std::process::id()
        ));
        create_dir_all(&path).unwrap();
        path
    }

    fn write_f32(bytes: &mut [u8], offset: usize, value: f32) {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn write_i32(bytes: &mut [u8], offset: usize, value: i32) {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn write_wheels(bytes: &mut [u8], offset: usize, values: [f32; 4]) {
        for (index, value) in values.into_iter().enumerate() {
            write_f32(bytes, offset + index * 4, value);
        }
    }

    fn write_utf16(bytes: &mut [u8], offset: usize, value: &str) {
        for (index, unit) in value.encode_utf16().enumerate() {
            bytes[offset + index * 2..offset + index * 2 + 2].copy_from_slice(&unit.to_le_bytes());
        }
    }
}
