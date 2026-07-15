#![cfg_attr(not(windows), allow(dead_code))]

use std::time::Instant;

use crate::config::BridgeConfig;
use crate::hud::HudHandle;
use crate::telemetry::{
    DamageSample, InputSample, LapSample, SessionSample, StatusSample, TelemetryUpdate,
    WheelValuesF32, WheelValuesU8, WheelValuesU16,
};

#[cfg(windows)]
use std::fs::{OpenOptions, create_dir_all, metadata};
#[cfg(windows)]
use std::io::{BufWriter, Write};
#[cfg(windows)]
use std::path::Path;
#[cfg(windows)]
use std::thread;
#[cfg(windows)]
use std::time::Duration;

#[cfg(windows)]
use crate::logging::{TelemetryRecorder, print_enabled_outputs};

pub(crate) const ACR_PHYSICS_MAPPING_NAME: &str = "Local\\acpmf_physics";
pub(crate) const ACR_GRAPHICS_MAPPING_NAME: &str = "Local\\acpmf_graphics";
pub(crate) const ACR_STATIC_MAPPING_NAME: &str = "Local\\acpmf_static";
pub(crate) const ACR_PHYSICS_SIZE: usize = 800;
pub(crate) const ACR_GRAPHICS_SIZE: usize = 1_588;
pub(crate) const ACR_STATIC_SIZE: usize = 784;

const UNKNOWN_TYRE_WEAR_PERCENT: f32 = -1.0;
const STAGE_RESET_MIN_PREVIOUS_DISTANCE_M: f32 = 250.0;
const STAGE_RESET_MIN_DROP_M: f32 = 150.0;

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

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, serde::Serialize)]
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

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum AcrStageEvent {
    #[default]
    None,
    Started,
    Finished,
    Aborted,
    Recovery,
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
}

struct AcrStageTracker {
    stage_number: u8,
    started_at: Instant,
    last_distance_m: Option<f32>,
    max_distance_m: f32,
    state: AcrStageState,
    last_official_time_ms: i32,
    last_completed_laps: i32,
    manual_finish_distance_m: Option<f32>,
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
            manual_finish_distance_m: manual_finish_distance_m
                .filter(|distance| distance.is_finite() && *distance >= 250.0),
        }
    }

    fn reset_context(&mut self, now: Instant) {
        self.stage_number = 1;
        self.started_at = now;
        self.last_distance_m = None;
        self.max_distance_m = 0.0;
        self.state = AcrStageState::Idle;
        self.last_official_time_ms = 0;
        self.last_completed_laps = 0;
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
        let live = graphics.status == 2;
        let moving = speed_kmh.is_finite() && speed_kmh > 3.0;
        let progressed = self
            .last_distance_m
            .is_some_and(|previous| distance_m > previous + 0.5);
        let mut event = AcrStageEvent::None;

        if reset {
            let recovery = self.state == AcrStageState::Running
                && distance_m > 100.0
                && self.max_distance_m - distance_m > STAGE_RESET_MIN_DROP_M;
            event = if self.state == AcrStageState::Finished {
                AcrStageEvent::None
            } else if recovery {
                AcrStageEvent::Recovery
            } else if self.state == AcrStageState::Running {
                AcrStageEvent::Aborted
            } else {
                AcrStageEvent::None
            };
            self.stage_number = self.stage_number.saturating_add(1).max(1);
            self.started_at = now;
            self.max_distance_m = distance_m;
            self.state = if live {
                AcrStageState::Countdown
            } else {
                AcrStageState::Idle
            };
        }

        if !reset {
            match self.state {
                AcrStageState::Idle if live && engine_running => {
                    self.state = AcrStageState::Countdown;
                }
                AcrStageState::Countdown if live && (moving || progressed || distance_m > 2.0) => {
                    self.state = AcrStageState::Running;
                    self.started_at = now;
                    self.max_distance_m = distance_m;
                    event = AcrStageEvent::Started;
                }
                AcrStageState::Running => {
                    let manual_finish = self
                        .manual_finish_distance_m
                        .is_some_and(|finish| distance_m >= finish * 0.995);
                    if official_time_ms.is_some() || manual_finish {
                        self.state = AcrStageState::Finished;
                        event = AcrStageEvent::Finished;
                    } else if !live && graphics.status != 3 {
                        self.state = AcrStageState::Aborted;
                        event = AcrStageEvent::Aborted;
                    }
                }
                AcrStageState::Aborted | AcrStageState::Recovery => {
                    self.state = if live {
                        AcrStageState::Countdown
                    } else {
                        AcrStageState::Idle
                    };
                }
                AcrStageState::Finished => {}
                AcrStageState::Idle | AcrStageState::Countdown => {}
            }
        }

        if event == AcrStageEvent::Recovery {
            self.state = AcrStageState::Recovery;
        } else if event == AcrStageEvent::Aborted && reset {
            self.state = AcrStageState::Aborted;
        }
        self.max_distance_m = self.max_distance_m.max(distance_m);
        self.last_distance_m = Some(distance_m);
        self.last_official_time_ms = graphics.last_time_ms;
        self.last_completed_laps = graphics.completed_laps;

        let elapsed_s = if graphics.current_time_ms > 0 && self.state == AcrStageState::Running {
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
        }
    }
}

#[derive(Default)]
struct AcrFrameValidator {
    previous: Option<(AcrPhysicsSnapshot, AcrGraphicsSnapshot, Instant)>,
    rejected_frames: u64,
}

impl AcrFrameValidator {
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
            if elapsed <= 0.1
                && (speed_jump > 120.0
                    || (physics.raw_gear == previous.raw_gear && rpm_jump > 10_000))
            {
                self.rejected_frames = self.rejected_frames.saturating_add(1);
                return Err(format!(
                    "rejected discontinuous ACR frame packet={} speed_jump={speed_jump:.1} rpm_jump={rpm_jump}",
                    physics.packet_id
                ));
            }
        }

        self.previous = Some((physics.clone(), graphics.clone(), now));
        Ok(())
    }
}

#[cfg_attr(any(target_os = "macos", target_os = "windows"), allow(dead_code))]
pub fn start_acr_adapter(config: BridgeConfig) -> Result<(), String> {
    #[cfg(not(windows))]
    {
        let _ = config;
        return Err(format!(
            "Assetto Corsa Rally adapter requires Windows shared memory ({ACR_PHYSICS_MAPPING_NAME}); run this on the game PC"
        ));
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
        return Err(format!(
            "Assetto Corsa Rally adapter requires Windows shared memory ({ACR_PHYSICS_MAPPING_NAME}); run this on the game PC"
        ));
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
    let mut stage_tracker = AcrStageTracker::new(Instant::now());
    let mut graphics = AcrGraphicsSnapshot::default();
    let mut statics = AcrStaticSnapshot::default();
    let mut context_key = String::new();
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
        println!("ACR coaching telemetry logging enabled: {path}");
    }
    print_enabled_outputs(&recorder_config);
    if hud.is_some() {
        println!("HUD: native window");
    } else {
        println!("HUD: headless");
    }

    loop {
        let now = Instant::now();
        if last_static_refresh.elapsed() >= STATIC_REFRESH_INTERVAL {
            if let Ok(snapshot) =
                super::shared_memory::read_mapping(ACR_STATIC_MAPPING_NAME, ACR_STATIC_SIZE)
                && let Ok(next) = parse_acr_static(&snapshot)
            {
                let next_key = format!("{}\n{}", next.track, next.car_model);
                if next_key != context_key && (!next.track.is_empty() || !next.car_model.is_empty())
                {
                    context_key = next_key;
                    stage_tracker.reset_context(now);
                    println!(
                        "[acr-session] track={} car={} length={:.1}m",
                        display_or_unknown(&next.track),
                        display_or_unknown(&next.car_model),
                        next.track_length_m
                    );
                }
                statics = next;
            }
            last_static_refresh = now;
        }

        match super::shared_memory::read_mapping(ACR_PHYSICS_MAPPING_NAME, ACR_PHYSICS_SIZE) {
            Ok(snapshot) => {
                let physics = parse_acr_physics(&snapshot)?;
                if last_physics_packet == Some(physics.packet_id) {
                    thread::sleep(POLL_INTERVAL);
                    continue;
                }
                last_physics_packet = Some(physics.packet_id);

                if let Ok(snapshot) =
                    super::shared_memory::read_mapping(ACR_GRAPHICS_MAPPING_NAME, ACR_GRAPHICS_SIZE)
                    && let Ok(next) = parse_acr_graphics(&snapshot)
                {
                    graphics = next;
                }

                let stage = stage_tracker.observe(graphics.distance_m, now);
                if stage.reset {
                    println!(
                        "[acr-stage] detected distance discontinuity; segment={}",
                        stage.stage_number
                    );
                }
                frame_identifier = frame_identifier.wrapping_add(1);
                let update =
                    build_acr_update(&physics, &graphics, &statics, stage, frame_identifier);

                if let Some(hud) = &hud {
                    hud.update(&update);
                }
                recorder.ingest(&update, config.debug);
                if let Some(logger) = &mut coaching_logger {
                    logger.write(&physics, &graphics, &statics, stage)?;
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
                        physics.rpm
                    );
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
            last_lap_time_ms: graphics.last_time_ms.max(0) as u32,
            current_lap_time_ms: current_time_ms,
            lap_distance_m: finite_nonnegative(graphics.distance_m),
            total_distance_m: finite_nonnegative(graphics.distance_m),
            car_position: graphics.position.clamp(1, u8::MAX as i32) as u8,
            current_lap_num: stage.stage_number,
            pit_status: u8::from(graphics.in_pit),
            sector: graphics.sector.clamp(0, u8::MAX as i32) as u8,
            current_lap_invalid: false,
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
}

#[cfg(windows)]
const ACR_COACHING_HEADER: &str = concat!(
    "elapsed_s,stage,physics_packet_id,graphics_packet_id,track,car,stage_distance_m,stage_progress,",
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

    #[test]
    fn detects_stage_distance_reset() {
        let start = Instant::now();
        let mut tracker = AcrStageTracker::new(start);
        let first = tracker.observe(1_200.0, start + Duration::from_secs(5));
        let reset = tracker.observe(25.0, start + Duration::from_secs(10));

        assert_eq!(first.stage_number, 1);
        assert!(!first.reset);
        assert_eq!(reset.stage_number, 2);
        assert!(reset.reset);
        assert!(reset.elapsed_s.abs() < f32::EPSILON);
    }

    #[test]
    fn splits_recovery_that_jumps_back_mid_stage() {
        let start = Instant::now();
        let mut tracker = AcrStageTracker::new(start);
        tracker.observe(6_029.7, start + Duration::from_secs(5));
        let recovery = tracker.observe(4_605.5, start + Duration::from_secs(6));

        assert_eq!(recovery.stage_number, 2);
        assert!(recovery.reset);
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
