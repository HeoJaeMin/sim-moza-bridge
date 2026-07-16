use std::collections::{HashMap, VecDeque};
use std::time::Duration;

use crate::contact::ContactDetector;
use crate::lap::LapTracker;
use crate::model::{CaptureHealth, LiveSnapshot, ParsedFrame, TraceResponse};
use crate::shared_memory::{TelemetryFrame, TelemetryFrameMonitor, TelemetryFrameState};
use crate::store::{
    DashboardStore, PersistenceQueue, ResumableSession, session_signature, track_key, unix_ms,
};
use crate::telemetry_core::CaptureCounters;
use crate::telemetry_quality::TraceQuality;
use crate::track::TrackMapper;

const RECENT_CONTACT_LIMIT: usize = 50;
// Covers a two-hour race plus restart and reconnect margin.
const SESSION_RESUME_MAX_AGE_MS: u64 = 3 * 60 * 60 * 1_000;
const SESSION_TOUCH_INTERVAL_MS: u64 = 1_000;
const FRAME_RATE_WINDOW_MS: u64 = 5_000;
const PAUSE_THRESHOLD_MS: u64 = 1_000;
const SESSION_TIME_RESET_TOLERANCE_S: f64 = 0.5;
const RESUME_RUNNING_CLOCK_TOLERANCE_S: f64 = 15.0;
const RESUME_TRACE_TIME_TOLERANCE_S: f64 = 1.5;
const RESUME_TRACE_DISTANCE_TOLERANCE_M: f64 = 30.0;

pub struct EngineUpdate {
    pub live: LiveSnapshot,
    pub trace: TraceResponse,
}

pub struct DashboardEngine {
    store: DashboardStore,
    persistence: PersistenceQueue,
    source: String,
    track_mapper: TrackMapper,
    contact_detector: ContactDetector,
    lap_trackers: HashMap<i32, LapTracker>,
    telemetry_monitors: HashMap<i32, TelemetryFrameMonitor>,
    player_vehicle_id: Option<i32>,
    session_id: String,
    session_signature: String,
    session_boundary: String,
    last_session_time_s: f64,
    last_session_created_at_ms: u64,
    last_session_touch_ms: u64,
    session_resumed: bool,
    capture_counters: CaptureCounters,
    telemetry_accepted_samples: u64,
    telemetry_rejected_samples: u64,
    telemetry_duplicate_samples: u64,
    telemetry_backward_samples: u64,
    telemetry_delayed_samples: u64,
    telemetry_sudden_change_samples: u64,
    frame_times_ms: VecDeque<u64>,
    last_frame_ms: u64,
    last_player_session_time_s: Option<f64>,
    last_progress_ms: u64,
    paused: bool,
    operator_paused: bool,
    saw_empty_frame: bool,
    recent_contacts: VecDeque<crate::model::ContactEvent>,
    last_live: LiveSnapshot,
}

impl DashboardEngine {
    pub fn new(
        store: DashboardStore,
        persistence: PersistenceQueue,
        source: impl Into<String>,
    ) -> Result<Self, String> {
        let recent_contacts = store
            .recent_contacts(RECENT_CONTACT_LIMIT)?
            .into_iter()
            .collect();
        Ok(Self {
            track_mapper: TrackMapper::new(store.clone(), persistence.clone()),
            store,
            persistence,
            source: source.into(),
            contact_detector: ContactDetector::default(),
            lap_trackers: HashMap::new(),
            telemetry_monitors: HashMap::new(),
            player_vehicle_id: None,
            session_id: String::new(),
            session_signature: String::new(),
            session_boundary: String::new(),
            last_session_time_s: 0.0,
            last_session_created_at_ms: 0,
            last_session_touch_ms: 0,
            session_resumed: false,
            capture_counters: CaptureCounters::default(),
            telemetry_accepted_samples: 0,
            telemetry_rejected_samples: 0,
            telemetry_duplicate_samples: 0,
            telemetry_backward_samples: 0,
            telemetry_delayed_samples: 0,
            telemetry_sudden_change_samples: 0,
            frame_times_ms: VecDeque::new(),
            last_frame_ms: 0,
            last_player_session_time_s: None,
            last_progress_ms: 0,
            paused: false,
            operator_paused: false,
            saw_empty_frame: false,
            recent_contacts,
            last_live: LiveSnapshot::default(),
        })
    }

    pub fn process(&mut self, mut frame: ParsedFrame) -> Result<EngineUpdate, String> {
        let now = unix_ms();
        if !self.update_session(&mut frame, now)? {
            self.capture_counters.invalid_context_frames = self
                .capture_counters
                .invalid_context_frames
                .saturating_add(1);
            let mut live = self.last_live.clone();
            live.connected = true;
            live.source = self.source.clone();
            live.warning = Some("waiting for a stable session frame".to_owned());
            live.capture = self.capture_health(now, TraceQuality::default(), "degraded");
            self.last_live = live.clone();
            return Ok(EngineUpdate {
                live,
                trace: self.player_trace(),
            });
        }
        self.register_frame(&frame, now);
        let track_points = self.track_mapper.update(&frame)?;

        for contact in self.contact_detector.detect(&frame) {
            self.persistence.save_contact(contact.clone())?;
            self.recent_contacts.push_front(contact);
            self.recent_contacts.truncate(RECENT_CONTACT_LIMIT);
        }

        self.player_vehicle_id = frame.player.as_ref().map(|player| player.vehicle_id);
        let player_vehicle = self.player_vehicle_id.and_then(|player_id| {
            frame
                .vehicles
                .iter()
                .find(|vehicle| vehicle.id == player_id)
        });
        let class_positions = player_vehicle
            .map(|player| same_class_positions(&frame.vehicles, player))
            .unwrap_or_default();
        let detailed_telemetry = if frame.telemetry.is_empty() {
            frame.player.iter().cloned().collect()
        } else {
            std::mem::take(&mut frame.telemetry)
        };
        let mut player_telemetry_rejected = false;
        for telemetry in &detailed_telemetry {
            let Some(vehicle) = frame
                .vehicles
                .iter()
                .find(|vehicle| vehicle.id == telemetry.vehicle_id)
            else {
                continue;
            };
            if !class_positions.contains_key(&vehicle.id) {
                continue;
            }
            let (validation, before_stats, after_stats) = {
                let monitor = self.telemetry_monitors.entry(vehicle.id).or_default();
                let before = monitor.stats();
                let validation = monitor.observe(
                    TelemetryFrame {
                        session_time_s: Some(telemetry.session_time_s),
                        elapsed_s: Some(telemetry.lap_elapsed_s),
                        lap_number: Some(telemetry.lap_number),
                        lap_distance_m: Some(telemetry.lap_distance_m),
                        track_length_m: Some(frame.session.track_length_m),
                        speed_kmh: Some(telemetry.speed_kmh),
                        rpm: Some(telemetry.rpm),
                        gear: Some(telemetry.gear),
                        lateral_g: Some(telemetry.lateral_g),
                        longitudinal_g: Some(telemetry.longitudinal_g),
                        throttle: Some(telemetry.throttle),
                        brake: Some(telemetry.brake),
                        steer: Some(telemetry.steer),
                        clutch: Some(telemetry.clutch),
                        world_x: Some(telemetry.world.x),
                        world_z: Some(telemetry.world.z),
                    },
                    Duration::from_millis(now),
                );
                (validation, before, monitor.stats())
            };
            self.telemetry_backward_samples = self.telemetry_backward_samples.saturating_add(
                after_stats
                    .backward_frames
                    .saturating_sub(before_stats.backward_frames),
            );
            self.telemetry_delayed_samples = self.telemetry_delayed_samples.saturating_add(
                after_stats
                    .delayed_frames
                    .saturating_sub(before_stats.delayed_frames),
            );
            self.telemetry_sudden_change_samples =
                self.telemetry_sudden_change_samples.saturating_add(
                    after_stats
                        .sudden_change_frames
                        .saturating_sub(before_stats.sudden_change_frames),
                );
            match validation {
                Ok(TelemetryFrameState::Fresh | TelemetryFrameState::Reset) => {
                    self.telemetry_accepted_samples =
                        self.telemetry_accepted_samples.saturating_add(1);
                }
                Ok(TelemetryFrameState::Duplicate) => {
                    self.telemetry_duplicate_samples =
                        self.telemetry_duplicate_samples.saturating_add(1);
                    continue;
                }
                Err(_) => {
                    self.telemetry_rejected_samples =
                        self.telemetry_rejected_samples.saturating_add(1);
                    player_telemetry_rejected |= self.player_vehicle_id == Some(vehicle.id);
                    if let Some(tracker) = self.lap_trackers.get_mut(&vehicle.id) {
                        tracker.note_capture_rejection(telemetry.lap_number);
                    }
                    continue;
                }
            }
            if !self.lap_trackers.contains_key(&vehicle.id) {
                let mut tracker = LapTracker::default();
                tracker.reset(
                    &self.session_id,
                    &frame.session.track_name,
                    &frame.session.session_type,
                    frame.session.track_length_m,
                );
                if self.session_resumed
                    && let Some(partial) = self.store.load_logical_lap(
                        &self.session_id,
                        vehicle.id,
                        telemetry.lap_number,
                    )?
                {
                    tracker.restore_partial(partial);
                }
                self.lap_trackers.insert(vehicle.id, tracker);
            }
            let tracker = self
                .lap_trackers
                .get_mut(&vehicle.id)
                .expect("lap tracker was inserted");
            if let Some(lap) = tracker.ingest(
                telemetry,
                vehicle,
                class_positions
                    .get(&vehicle.id)
                    .copied()
                    .unwrap_or_default(),
            ) {
                self.persistence.save_lap(lap)?;
            }
        }
        if player_telemetry_rejected {
            frame.player = None;
        }

        let trace = self.player_trace();
        let current_lap = self
            .player_vehicle_id
            .and_then(|vehicle_id| self.lap_trackers.get(&vehicle_id))
            .and_then(LapTracker::current_info);
        let current_quality = current_lap
            .as_ref()
            .map(|lap| lap.quality.clone())
            .unwrap_or_default();
        let live = LiveSnapshot {
            connected: true,
            source: self.source.clone(),
            warning: None,
            session: Some(frame.session),
            vehicles: frame.vehicles,
            player: frame.player,
            track_points,
            recent_contacts: self.recent_contacts.iter().cloned().collect(),
            current_lap,
            capture: self.capture_health(
                now,
                current_quality,
                if self.operator_paused {
                    "paused_by_user"
                } else if self.paused {
                    "paused"
                } else {
                    "live"
                },
            ),
        };
        self.last_live = live.clone();
        Ok(EngineUpdate { live, trace })
    }

    pub fn disconnected(&mut self, warning: impl Into<String>) -> EngineUpdate {
        self.capture_counters.rejected_frames =
            self.capture_counters.rejected_frames.saturating_add(1);
        let mut live = self.last_live.clone();
        live.connected = false;
        live.source = self.source.clone();
        live.warning = Some(warning.into());
        let quality = live
            .current_lap
            .as_ref()
            .map(|lap| lap.quality.clone())
            .unwrap_or_default();
        live.capture = self.capture_health(unix_ms(), quality, "disconnected");
        EngineUpdate {
            live,
            trace: self.player_trace(),
        }
    }

    #[cfg_attr(not(windows), allow(dead_code))]
    pub fn inconsistent_snapshot(&mut self, warning: impl Into<String>) -> EngineUpdate {
        self.capture_counters.inconsistent_frames =
            self.capture_counters.inconsistent_frames.saturating_add(1);
        self.disconnected(warning)
    }

    #[cfg_attr(not(windows), allow(dead_code))]
    pub fn stalled(&mut self, warning: impl Into<String>) -> EngineUpdate {
        self.capture_counters.stalled_frames =
            self.capture_counters.stalled_frames.saturating_add(1);
        let mut live = self.last_live.clone();
        live.connected = true;
        live.source = self.source.clone();
        live.warning = Some(warning.into());
        let quality = live
            .current_lap
            .as_ref()
            .map(|lap| lap.quality.clone())
            .unwrap_or_default();
        live.capture = self.capture_health(unix_ms(), quality, "stalled");
        EngineUpdate {
            live,
            trace: self.player_trace(),
        }
    }

    pub fn prepare_shutdown(&mut self) -> Result<(), String> {
        for tracker in self.lap_trackers.values_mut() {
            if let Some(partial) = tracker.finish_partial() {
                self.persistence.save_lap(partial)?;
            }
        }
        self.track_mapper.flush()
    }

    pub fn pause_and_flush(&mut self) -> (EngineUpdate, Result<(), String>) {
        self.operator_paused = true;
        let result = (|| {
            for tracker in self.lap_trackers.values() {
                if let Some(partial) = tracker.snapshot_partial() {
                    self.persistence.save_lap(partial)?;
                }
            }
            self.track_mapper.flush()?;
            self.persistence.flush()
        })();
        (self.control_update("paused_by_user"), result)
    }

    pub fn resume(&mut self) -> EngineUpdate {
        self.operator_paused = false;
        self.last_progress_ms = unix_ms();
        self.control_update(if self.paused { "paused" } else { "live" })
    }

    fn control_update(&mut self, state: &str) -> EngineUpdate {
        let mut live = self.last_live.clone();
        let quality = live
            .current_lap
            .as_ref()
            .map(|lap| lap.quality.clone())
            .unwrap_or_default();
        live.capture = self.capture_health(unix_ms(), quality, state);
        live.capture.paused = self.operator_paused || self.paused;
        self.last_live = live.clone();
        EngineUpdate {
            live,
            trace: self.player_trace(),
        }
    }

    fn update_session(&mut self, frame: &mut ParsedFrame, now: u64) -> Result<bool, String> {
        let signature = session_signature(&frame.session);
        let boundary = session_boundary(&frame.session);
        let time_reset = frame.session.current_time_s + SESSION_TIME_RESET_TOLERANCE_S
            < self.last_session_time_s;
        let identity = frame.session.identity();
        let valid_session = identity.is_complete()
            && frame.session.track_length_m >= 100.0
            && !frame.vehicles.is_empty();
        if !valid_session {
            self.saw_empty_frame = true;
            frame.session.id.clone_from(&self.session_id);
            return Ok(false);
        }
        let reset_after_empty = self.saw_empty_frame
            && frame.session.current_time_s + SESSION_TIME_RESET_TOLERANCE_S
                < self.last_session_time_s;
        let mut new_session = (!self.session_boundary.is_empty()
            && self.session_boundary != boundary)
            || time_reset
            || reset_after_empty;
        if self.session_id.is_empty() {
            if let Some(resume) = self.store.resumable_session(
                &signature,
                frame.session.current_time_s,
                SESSION_RESUME_MAX_AGE_MS,
            )? && self.resume_evidence_matches(&resume, frame, now)?
            {
                self.session_id = resume.id;
                self.last_session_time_s = resume.last_session_time_s;
                self.session_signature.clone_from(&signature);
                self.session_boundary.clone_from(&boundary);
                self.session_resumed = true;
                self.last_session_touch_ms = now;
                frame.session.id.clone_from(&self.session_id);
                self.store
                    .save_session_with_source(&frame.session, &self.source)?;
                new_session = false;
            } else {
                new_session = true;
            }
        }

        if new_session {
            let mut saved_partial = false;
            for tracker in self.lap_trackers.values_mut() {
                if let Some(partial) = tracker.finish_partial() {
                    self.persistence.save_lap(partial)?;
                    saved_partial = true;
                }
            }
            if saved_partial {
                self.persistence.flush()?;
            }
            self.last_session_created_at_ms =
                now.max(self.last_session_created_at_ms.saturating_add(1));
            self.session_id = format!(
                "{}-{}",
                track_key(&frame.session.track_name, frame.session.track_length_m),
                self.last_session_created_at_ms
            );
            self.session_signature = signature;
            self.session_boundary = boundary;
            self.session_resumed = false;
            self.contact_detector.reset();
            self.lap_trackers.clear();
            self.telemetry_monitors.clear();
            self.player_vehicle_id = None;
            self.last_player_session_time_s = None;
            self.last_progress_ms = now;
            self.paused = false;
            frame.session.id = self.session_id.clone();
            self.store
                .save_session_with_source(&frame.session, &self.source)?;
            self.last_session_touch_ms = now;
        } else {
            frame.session.id = self.session_id.clone();
            let fingerprint_changed = self.session_signature != signature;
            self.session_signature = signature;
            self.session_boundary = boundary;
            if fingerprint_changed
                || now.saturating_sub(self.last_session_touch_ms) >= SESSION_TOUCH_INTERVAL_MS
            {
                self.store.touch_session(&frame.session)?;
                self.last_session_touch_ms = now;
            }
        }
        self.last_session_time_s = frame.session.current_time_s;
        self.saw_empty_frame = false;
        Ok(true)
    }

    fn resume_evidence_matches(
        &self,
        resume: &ResumableSession,
        frame: &ParsedFrame,
        now: u64,
    ) -> Result<bool, String> {
        let Some(partial) = self.store.latest_incomplete_player_lap(&resume.id)? else {
            return Ok(resume_without_partial_matches(
                resume,
                frame.session.current_time_s,
                now,
            ));
        };
        Ok(partial_resume_matches(&partial, resume, frame, now))
    }

    fn register_frame(&mut self, frame: &ParsedFrame, now: u64) {
        self.capture_counters.accepted_frames =
            self.capture_counters.accepted_frames.saturating_add(1);
        self.last_frame_ms = now;
        self.frame_times_ms.push_back(now);
        while self
            .frame_times_ms
            .front()
            .is_some_and(|timestamp| now.saturating_sub(*timestamp) > FRAME_RATE_WINDOW_MS)
        {
            self.frame_times_ms.pop_front();
        }
        if let Some(session_time_s) = frame.player.as_ref().map(|player| player.session_time_s) {
            if self
                .last_player_session_time_s
                .is_some_and(|previous| session_time_s <= previous + f64::EPSILON)
            {
                self.capture_counters.duplicate_frames =
                    self.capture_counters.duplicate_frames.saturating_add(1);
                self.paused = now.saturating_sub(self.last_progress_ms) >= PAUSE_THRESHOLD_MS;
            } else {
                self.last_progress_ms = now;
                self.paused = false;
            }
            self.last_player_session_time_s = Some(session_time_s);
        }
    }

    fn capture_health(
        &self,
        now: u64,
        current_quality: TraceQuality,
        state: &str,
    ) -> CaptureHealth {
        let sample_rate_hz = match (self.frame_times_ms.front(), self.frame_times_ms.back()) {
            (Some(first), Some(last)) if last > first => {
                (self.frame_times_ms.len().saturating_sub(1)) as f64 * 1_000.0
                    / last.saturating_sub(*first) as f64
            }
            _ => 0.0,
        };
        CaptureHealth {
            state: state.to_owned(),
            sample_rate_hz: (sample_rate_hz * 10.0).round() / 10.0,
            accepted_frames: self.capture_counters.accepted_frames,
            rejected_frames: self.capture_counters.rejected_frames,
            duplicate_frames: self.capture_counters.duplicate_frames,
            stalled_frames: self.capture_counters.stalled_frames,
            inconsistent_frames: self.capture_counters.inconsistent_frames,
            invalid_session_frames: self.capture_counters.invalid_context_frames,
            telemetry_accepted_samples: self.telemetry_accepted_samples,
            telemetry_rejected_samples: self.telemetry_rejected_samples,
            telemetry_duplicate_samples: self.telemetry_duplicate_samples,
            telemetry_backward_samples: self.telemetry_backward_samples,
            telemetry_delayed_samples: self.telemetry_delayed_samples,
            telemetry_sudden_change_samples: self.telemetry_sudden_change_samples,
            last_frame_age_ms: if self.last_frame_ms == 0 {
                0
            } else {
                now.saturating_sub(self.last_frame_ms)
            },
            session_resumed: self.session_resumed,
            paused: self.paused || self.operator_paused,
            operator_paused: self.operator_paused,
            current_quality,
            persistence: self.persistence.health(),
        }
    }

    fn player_trace(&self) -> TraceResponse {
        self.player_vehicle_id
            .and_then(|vehicle_id| self.lap_trackers.get(&vehicle_id))
            .map_or_else(TraceResponse::default, LapTracker::trace)
    }
}

fn session_boundary(session: &crate::model::SessionState) -> String {
    session.identity().boundary_key()
}

fn resume_without_partial_matches(
    resume: &ResumableSession,
    current_session_time_s: f64,
    now_ms: u64,
) -> bool {
    let session_advance_s = current_session_time_s - resume.last_session_time_s;
    session_advance_s.abs() <= SESSION_TIME_RESET_TOLERANCE_S
        || running_session_clock_matches(resume, current_session_time_s, now_ms)
}

fn running_session_clock_matches(
    resume: &ResumableSession,
    current_session_time_s: f64,
    now_ms: u64,
) -> bool {
    let wall_elapsed_s = now_ms.saturating_sub(resume.last_seen_ms) as f64 / 1_000.0;
    let session_advance_s = current_session_time_s - resume.last_session_time_s;
    session_advance_s.is_finite()
        && session_advance_s >= -SESSION_TIME_RESET_TOLERANCE_S
        && (session_advance_s - wall_elapsed_s).abs() <= RESUME_RUNNING_CLOCK_TOLERANCE_S
}

fn partial_resume_matches(
    partial: &crate::model::SavedLap,
    resume: &ResumableSession,
    frame: &ParsedFrame,
    now_ms: u64,
) -> bool {
    if partial.summary.completed || !partial.summary.is_player {
        return false;
    }
    let Some(player_vehicle) = frame.vehicles.iter().find(|vehicle| vehicle.is_player) else {
        return false;
    };
    let Some(player_telemetry) = frame
        .player
        .as_ref()
        .filter(|telemetry| telemetry.vehicle_id == player_vehicle.id)
        .or_else(|| {
            frame
                .telemetry
                .iter()
                .find(|telemetry| telemetry.vehicle_id == player_vehicle.id)
        })
    else {
        return false;
    };
    let Some(last) = partial.samples.last() else {
        return false;
    };
    if partial.summary.vehicle_id != player_vehicle.id
        || !partial
            .summary
            .driver_name
            .trim()
            .eq_ignore_ascii_case(player_vehicle.driver_name.trim())
        || !partial
            .summary
            .class_name
            .trim()
            .eq_ignore_ascii_case(player_vehicle.class_name.trim())
    {
        return false;
    }
    if player_telemetry.lap_number < partial.summary.lap_number {
        return false;
    }
    if player_telemetry.lap_number > partial.summary.lap_number {
        return running_session_clock_matches(resume, player_telemetry.session_time_s, now_ms);
    }
    let session_advance_s = player_telemetry.session_time_s - last.session_time_s;
    let lap_advance_s = player_telemetry.lap_elapsed_s - last.lap_elapsed_s;
    session_advance_s.is_finite()
        && lap_advance_s.is_finite()
        && player_telemetry.lap_distance_m.is_finite()
        && session_advance_s >= -SESSION_TIME_RESET_TOLERANCE_S
        && lap_advance_s >= -RESUME_TRACE_TIME_TOLERANCE_S
        && (session_advance_s - lap_advance_s).abs() <= RESUME_TRACE_TIME_TOLERANCE_S
        && player_telemetry.lap_distance_m + RESUME_TRACE_DISTANCE_TOLERANCE_M
            >= last.lap_distance_m
}

fn same_class_positions(
    vehicles: &[crate::model::VehicleState],
    player: &crate::model::VehicleState,
) -> HashMap<i32, u8> {
    let player_class = player.class_name.trim();
    let mut class_vehicles = vehicles
        .iter()
        .filter(|vehicle| {
            vehicle.id == player.id
                || (!player_class.is_empty()
                    && vehicle.class_name.trim().eq_ignore_ascii_case(player_class))
        })
        .collect::<Vec<_>>();
    class_vehicles.sort_by_key(|vehicle| {
        if vehicle.position == 0 {
            u8::MAX
        } else {
            vehicle.position
        }
    });
    class_vehicles
        .into_iter()
        .enumerate()
        .map(|(index, vehicle)| (vehicle.id, u8::try_from(index + 1).unwrap_or(u8::MAX)))
        .collect()
}

#[cfg(test)]
mod tests {
    use std::fs;

    use serde::Deserialize;

    use super::*;
    use crate::model::{
        LapSummary, Point2, SavedLap, SessionState, TelemetryPoint, VehicleState, VehicleTelemetry,
    };

    #[test]
    fn assigns_a_session_and_builds_live_state() {
        let path = std::env::temp_dir().join(format!("lmu-engine-test-{}", unix_ms()));
        let store = DashboardStore::open(&path).unwrap();
        let worker = crate::store::PersistenceWorker::start(store.clone());
        let mut engine = DashboardEngine::new(store, worker.queue(), "test").unwrap();
        let frame = ParsedFrame {
            session: SessionState {
                track_name: "Le Mans".to_owned(),
                session_type: "Race".to_owned(),
                current_time_s: 10.0,
                track_length_m: 13_626.0,
                ..SessionState::default()
            },
            vehicles: vec![VehicleState {
                id: 1,
                is_player: true,
                speed_kmh: 200.0,
                lap_distance_m: 100.0,
                world: Point2 { x: 1.0, z: 2.0 },
                ..VehicleState::default()
            }],
            player: Some(VehicleTelemetry {
                vehicle_id: 1,
                lap_number: 1,
                session_time_s: 10.0,
                ..VehicleTelemetry::default()
            }),
            ..ParsedFrame::default()
        };

        let update = engine.process(frame).unwrap();
        assert!(update.live.connected);
        assert!(!update.live.session.as_ref().unwrap().id.is_empty());
        assert_eq!(update.live.vehicles.len(), 1);
        assert_eq!(update.trace.samples.len(), 1);
        fs::remove_dir_all(path).ok();
    }

    #[test]
    fn starts_a_new_session_after_a_menu_frame() {
        let path = std::env::temp_dir().join(format!("lmu-engine-session-test-{}", unix_ms()));
        let store = DashboardStore::open(&path).unwrap();
        let worker = crate::store::PersistenceWorker::start(store.clone());
        let mut engine = DashboardEngine::new(store, worker.queue(), "test").unwrap();
        let race_frame = |time| ParsedFrame {
            session: SessionState {
                track_name: "Le Mans".to_owned(),
                session_type: "Race".to_owned(),
                current_time_s: time,
                track_length_m: 13_626.0,
                ..SessionState::default()
            },
            vehicles: vec![VehicleState {
                id: 1,
                ..VehicleState::default()
            }],
            ..ParsedFrame::default()
        };

        let first_id = engine
            .process(race_frame(4.0))
            .unwrap()
            .live
            .session
            .unwrap()
            .id;
        engine.process(ParsedFrame::default()).unwrap();
        let second_id = engine
            .process(race_frame(0.0))
            .unwrap()
            .live
            .session
            .unwrap()
            .id;

        assert_ne!(first_id, second_id);
        fs::remove_dir_all(path).ok();
    }

    #[test]
    fn starts_a_new_session_on_live_to_live_time_reset() {
        let path = std::env::temp_dir().join(format!("lmu-engine-live-reset-test-{}", unix_ms()));
        let store = DashboardStore::open(&path).unwrap();
        let worker = crate::store::PersistenceWorker::start(store.clone());
        let mut engine = DashboardEngine::new(store, worker.queue(), "test").unwrap();
        let race_frame = |time| ParsedFrame {
            session: SessionState {
                track_name: "Le Mans".to_owned(),
                session_type: "Race".to_owned(),
                current_time_s: time,
                track_length_m: 13_626.0,
                ..SessionState::default()
            },
            vehicles: vec![VehicleState {
                id: 1,
                ..VehicleState::default()
            }],
            ..ParsedFrame::default()
        };

        let first_id = engine
            .process(race_frame(4.0))
            .unwrap()
            .live
            .session
            .unwrap()
            .id;
        let reset_id = engine
            .process(race_frame(0.0))
            .unwrap()
            .live
            .session
            .unwrap()
            .id;

        assert_ne!(first_id, reset_id);
        fs::remove_dir_all(path).ok();
    }

    #[test]
    fn keeps_the_session_across_transient_empty_frames_when_time_advances() {
        let path =
            std::env::temp_dir().join(format!("lmu-engine-transient-empty-test-{}", unix_ms()));
        let store = DashboardStore::open(&path).unwrap();
        let worker = crate::store::PersistenceWorker::start(store.clone());
        let mut engine = DashboardEngine::new(store, worker.queue(), "test").unwrap();
        let race_frame = |time| ParsedFrame {
            session: SessionState {
                track_name: "Le Mans".to_owned(),
                session_type: "Race".to_owned(),
                current_time_s: time,
                track_length_m: 13_626.0,
                ..SessionState::default()
            },
            vehicles: vec![VehicleState {
                id: 1,
                ..VehicleState::default()
            }],
            ..ParsedFrame::default()
        };

        let first_id = engine
            .process(race_frame(100.0))
            .unwrap()
            .live
            .session
            .unwrap()
            .id;
        engine.process(ParsedFrame::default()).unwrap();
        engine.process(ParsedFrame::default()).unwrap();
        let resumed_id = engine
            .process(race_frame(101.0))
            .unwrap()
            .live
            .session
            .unwrap()
            .id;

        assert_eq!(first_id, resumed_id);
        fs::remove_dir_all(path).ok();
    }

    #[test]
    fn keeps_the_session_across_a_transient_track_placeholder() {
        let path = std::env::temp_dir().join(format!(
            "lmu-engine-transient-track-placeholder-test-{}",
            unix_ms()
        ));
        let store = DashboardStore::open(&path).unwrap();
        let worker = crate::store::PersistenceWorker::start(store.clone());
        let mut engine = DashboardEngine::new(store, worker.queue(), "test").unwrap();
        let frame = |track: &str, time| ParsedFrame {
            session: SessionState {
                track_name: track.to_owned(),
                session_type: "Race".to_owned(),
                current_time_s: time,
                track_length_m: 13_626.0,
                ..SessionState::default()
            },
            vehicles: vec![VehicleState {
                id: 1,
                ..VehicleState::default()
            }],
            ..ParsedFrame::default()
        };

        let first_id = engine
            .process(frame("Le Mans", 100.0))
            .unwrap()
            .live
            .session
            .unwrap()
            .id;
        let placeholder = engine.process(frame("Waiting for track", 100.5)).unwrap();
        let resumed_id = engine
            .process(frame("Le Mans", 101.0))
            .unwrap()
            .live
            .session
            .unwrap()
            .id;

        assert_eq!(placeholder.live.session.unwrap().id, first_id);
        assert_eq!(placeholder.live.capture.invalid_session_frames, 1);
        assert_eq!(resumed_id, first_id);
        fs::remove_dir_all(path).ok();
    }

    #[test]
    fn keeps_the_session_across_a_transient_unknown_session_type() {
        let path = std::env::temp_dir().join(format!(
            "lmu-engine-transient-session-type-test-{}",
            unix_ms()
        ));
        let store = DashboardStore::open(&path).unwrap();
        let worker = crate::store::PersistenceWorker::start(store.clone());
        let mut engine = DashboardEngine::new(store, worker.queue(), "test").unwrap();
        let frame = |session_type: &str, time| ParsedFrame {
            session: SessionState {
                track_name: "Le Mans".to_owned(),
                session_type: session_type.to_owned(),
                current_time_s: time,
                track_length_m: 13_626.0,
                ..SessionState::default()
            },
            vehicles: vec![VehicleState {
                id: 1,
                ..VehicleState::default()
            }],
            ..ParsedFrame::default()
        };

        let first_id = engine
            .process(frame("Race", 100.0))
            .unwrap()
            .live
            .session
            .unwrap()
            .id;
        let unknown = engine.process(frame("Unknown", 100.5)).unwrap();
        let resumed_id = engine
            .process(frame("Race", 101.0))
            .unwrap()
            .live
            .session
            .unwrap()
            .id;

        assert_eq!(unknown.live.session.unwrap().id, first_id);
        assert_eq!(unknown.live.capture.invalid_session_frames, 1);
        assert_eq!(resumed_id, first_id);
        fs::remove_dir_all(path).ok();
    }

    #[test]
    fn runtime_metadata_changes_do_not_split_the_active_session() {
        let path =
            std::env::temp_dir().join(format!("lmu-engine-runtime-metadata-test-{}", unix_ms()));
        let store = DashboardStore::open(&path).unwrap();
        let worker = crate::store::PersistenceWorker::start(store.clone());
        let mut engine = DashboardEngine::new(store.clone(), worker.queue(), "test").unwrap();
        let frame = |time, game_version, max_laps| ParsedFrame {
            session: SessionState {
                game_version,
                track_name: "Le Mans".to_owned(),
                session_type: "Race".to_owned(),
                current_time_s: time,
                max_laps,
                track_length_m: 13_626.0,
                ..SessionState::default()
            },
            vehicles: vec![VehicleState {
                id: 1,
                ..VehicleState::default()
            }],
            ..ParsedFrame::default()
        };

        let first_id = engine
            .process(frame(100.0, 13, 40))
            .unwrap()
            .live
            .session
            .unwrap()
            .id;
        let updated = frame(101.0, 14, 42);
        let updated_signature = session_signature(&updated.session);
        let updated_id = engine.process(updated).unwrap().live.session.unwrap().id;

        assert_eq!(first_id, updated_id);
        assert!(
            store
                .resumable_session(&updated_signature, 101.0, SESSION_RESUME_MAX_AGE_MS)
                .unwrap()
                .is_some()
        );
        fs::remove_dir_all(path).ok();
    }

    #[test]
    fn track_or_session_type_change_splits_the_active_session() {
        let path =
            std::env::temp_dir().join(format!("lmu-engine-runtime-boundary-test-{}", unix_ms()));
        let store = DashboardStore::open(&path).unwrap();
        let worker = crate::store::PersistenceWorker::start(store.clone());
        let mut engine = DashboardEngine::new(store, worker.queue(), "test").unwrap();
        let frame = |track: &str, session_type: &str, time| ParsedFrame {
            session: SessionState {
                track_name: track.to_owned(),
                session_type: session_type.to_owned(),
                current_time_s: time,
                track_length_m: if track == "Le Mans" {
                    13_626.0
                } else {
                    5_891.0
                },
                ..SessionState::default()
            },
            vehicles: vec![VehicleState {
                id: 1,
                ..VehicleState::default()
            }],
            ..ParsedFrame::default()
        };

        let first_id = engine
            .process(frame("Le Mans", "Race", 100.0))
            .unwrap()
            .live
            .session
            .unwrap()
            .id;
        let track_id = engine
            .process(frame("Silverstone", "Race", 101.0))
            .unwrap()
            .live
            .session
            .unwrap()
            .id;
        let type_id = engine
            .process(frame("Silverstone", "Qualifying", 102.0))
            .unwrap()
            .live
            .session
            .unwrap()
            .id;

        assert_ne!(first_id, track_id);
        assert_ne!(track_id, type_id);
        fs::remove_dir_all(path).ok();
    }

    #[test]
    fn restart_window_covers_two_hour_races_with_margin() {
        assert_eq!(SESSION_RESUME_MAX_AGE_MS, 3 * 60 * 60 * 1_000);
    }

    #[test]
    fn resume_without_partial_requires_a_paused_or_wall_clock_matched_session() {
        let now = 20_000_000;
        let resume = ResumableSession {
            id: "stored-session".to_owned(),
            last_session_time_s: 100.0,
            last_seen_ms: now - SESSION_RESUME_MAX_AGE_MS,
        };

        assert!(resume_without_partial_matches(&resume, 100.0, now));
        assert!(resume_without_partial_matches(
            &resume,
            100.0 + SESSION_RESUME_MAX_AGE_MS as f64 / 1_000.0,
            now,
        ));
        assert!(!resume_without_partial_matches(&resume, 500.0, now));
    }

    #[test]
    fn partial_resume_requires_the_same_driver_lap_and_trace_progress() {
        let now = 1_000_000;
        let resume = ResumableSession {
            id: "stored-session".to_owned(),
            last_session_time_s: 100.0,
            last_seen_ms: now - 1_000,
        };
        let partial = SavedLap {
            summary: LapSummary {
                vehicle_id: 10,
                driver_name: "Player".to_owned(),
                class_name: "Hypercar".to_owned(),
                is_player: true,
                lap_number: 3,
                completed: false,
                ..LapSummary::default()
            },
            samples: vec![TelemetryPoint {
                session_time_s: 100.0,
                lap_elapsed_s: 20.0,
                lap_distance_m: 500.0,
                ..TelemetryPoint::default()
            }],
        };
        let mut continuation = class_frame(101.0, 3);
        let player = continuation
            .telemetry
            .iter_mut()
            .find(|sample| sample.vehicle_id == 10)
            .unwrap();
        player.lap_elapsed_s = 21.0;
        player.lap_distance_m = 520.0;
        continuation.player = Some(player.clone());

        assert!(partial_resume_matches(
            &partial,
            &resume,
            &continuation,
            now,
        ));

        let mut reset = continuation;
        let player = reset
            .telemetry
            .iter_mut()
            .find(|sample| sample.vehicle_id == 10)
            .unwrap();
        player.session_time_s = 105.0;
        player.lap_elapsed_s = 5.0;
        player.lap_distance_m = 100.0;
        reset.player = Some(player.clone());
        reset.session.current_time_s = 105.0;

        assert!(!partial_resume_matches(&partial, &resume, &reset, now));

        let mut next_lap = class_frame(101.0, 4);
        let player = next_lap
            .telemetry
            .iter_mut()
            .find(|sample| sample.vehicle_id == 10)
            .unwrap();
        player.lap_elapsed_s = 0.2;
        player.lap_distance_m = 10.0;
        next_lap.player = Some(player.clone());
        assert!(partial_resume_matches(&partial, &resume, &next_lap, now));

        let previous_lap = class_frame(101.0, 2);
        assert!(!partial_resume_matches(
            &partial,
            &resume,
            &previous_lap,
            now,
        ));

        let mut late_next_lap = next_lap;
        let player = late_next_lap
            .telemetry
            .iter_mut()
            .find(|sample| sample.vehicle_id == 10)
            .unwrap();
        player.session_time_s = 130.0;
        late_next_lap.player = Some(player.clone());
        late_next_lap.session.current_time_s = 130.0;
        assert!(!partial_resume_matches(
            &partial,
            &resume,
            &late_next_lap,
            now,
        ));
    }

    #[test]
    fn same_fingerprint_late_attach_does_not_reuse_an_incomplete_old_lap() {
        let path =
            std::env::temp_dir().join(format!("lmu-engine-late-attach-partial-test-{}", unix_ms()));
        let store = DashboardStore::open(&path).unwrap();
        let session = SessionState {
            id: "stored-session".to_owned(),
            track_name: "Le Mans".to_owned(),
            session_type: "Race".to_owned(),
            current_time_s: 100.0,
            track_length_m: 13_626.0,
            ..SessionState::default()
        };
        store.save_session(&session).unwrap();
        store
            .save_lap(&SavedLap {
                summary: LapSummary {
                    id: "stored-session-player-lap-1".to_owned(),
                    session_id: session.id.clone(),
                    track_name: session.track_name.clone(),
                    session_type: session.session_type.clone(),
                    track_length_m: session.track_length_m,
                    vehicle_id: 10,
                    driver_name: "Player".to_owned(),
                    class_name: "Hypercar".to_owned(),
                    is_player: true,
                    lap_number: 1,
                    sample_count: 1,
                    created_at_unix_ms: unix_ms(),
                    completed: false,
                    ..LapSummary::default()
                },
                samples: vec![TelemetryPoint {
                    session_time_s: 100.0,
                    lap_elapsed_s: 100.0,
                    lap_distance_m: 1_000.0,
                    ..TelemetryPoint::default()
                }],
            })
            .unwrap();
        let worker = crate::store::PersistenceWorker::start(store.clone());
        let mut engine = DashboardEngine::new(store, worker.queue(), "test").unwrap();

        let update = engine.process(class_frame(105.0, 1)).unwrap();
        let live_session = update.live.session.unwrap();

        assert_ne!(live_session.id, session.id);
        assert!(!update.live.capture.session_resumed);
        fs::remove_dir_all(path).ok();
    }

    #[test]
    fn saves_only_player_class_laps_and_keeps_player_trace_live() {
        let path = std::env::temp_dir().join(format!("lmu-engine-class-test-{}", unix_ms()));
        let store = DashboardStore::open(&path).unwrap();
        let worker = crate::store::PersistenceWorker::start(store.clone());
        let mut engine = DashboardEngine::new(store.clone(), worker.queue(), "test").unwrap();

        for sample in 0..21 {
            engine.process(class_frame(sample as f64 + 1.0, 1)).unwrap();
        }
        let update = engine.process(class_frame(22.0, 2)).unwrap();

        worker.queue().flush().unwrap();
        let laps = store.list_laps().unwrap();
        assert_eq!(laps.len(), 2);
        assert!(laps.iter().any(|lap| lap.vehicle_id == 10 && lap.is_player));
        assert!(laps.iter().any(|lap| {
            lap.vehicle_id == 11
                && !lap.is_player
                && lap.class_name == "hypercar"
                && lap.class_position == 2
        }));
        assert!(!laps.iter().any(|lap| lap.vehicle_id == 12));
        assert_eq!(update.live.vehicles.len(), 3);
        assert!(update.live.vehicles.iter().any(|vehicle| vehicle.id == 12));
        assert_eq!(update.trace.summary.as_ref().unwrap().vehicle_id, 10);
        assert!(update.trace.summary.as_ref().unwrap().is_player);
        assert_eq!(update.trace.samples.len(), 1);
        assert_eq!(update.trace.samples[0].speed_kmh, 210.0);
        fs::remove_dir_all(path).ok();
    }

    #[test]
    fn saves_in_progress_laps_before_switching_sessions() {
        let path =
            std::env::temp_dir().join(format!("lmu-engine-boundary-flush-test-{}", unix_ms()));
        let store = DashboardStore::open(&path).unwrap();
        let worker = crate::store::PersistenceWorker::start(store.clone());
        let mut engine = DashboardEngine::new(store.clone(), worker.queue(), "test").unwrap();

        let mut old_session_id = String::new();
        for sample in 1..=21 {
            let update = engine.process(class_frame(sample as f64, 1)).unwrap();
            old_session_id = update.live.session.unwrap().id;
        }
        let mut next_session = class_frame(22.0, 1);
        next_session.session.session_type = "Qualifying".to_owned();
        let new_session_id = engine
            .process(next_session)
            .unwrap()
            .live
            .session
            .unwrap()
            .id;
        worker.queue().flush().unwrap();

        assert_ne!(old_session_id, new_session_id);
        let laps = store.list_laps().unwrap();
        assert_eq!(laps.len(), 2);
        assert!(laps.iter().all(|lap| {
            lap.session_id == old_session_id && !lap.completed && lap.sample_count == 21
        }));
        fs::remove_dir_all(path).ok();
    }

    #[test]
    fn rejects_non_finite_same_class_telemetry_before_it_reaches_the_trace() {
        let path =
            std::env::temp_dir().join(format!("lmu-engine-frame-quality-test-{}", unix_ms()));
        let store = DashboardStore::open(&path).unwrap();
        let worker = crate::store::PersistenceWorker::start(store.clone());
        let mut engine = DashboardEngine::new(store, worker.queue(), "test").unwrap();

        engine.process(class_frame(1.0, 1)).unwrap();
        let mut invalid = class_frame(2.0, 1);
        let player = invalid
            .telemetry
            .iter_mut()
            .find(|sample| sample.vehicle_id == 10)
            .unwrap();
        player.rpm = f64::NAN;
        invalid.player = Some(player.clone());

        let update = engine.process(invalid).unwrap();

        assert_eq!(update.live.capture.rejected_frames, 0);
        assert_eq!(update.live.capture.telemetry_rejected_samples, 1);
        assert!(update.live.player.is_none());
        assert_eq!(update.trace.samples.len(), 1);
        assert_eq!(
            update.live.current_lap.as_ref().unwrap().quality.status,
            crate::telemetry_quality::TraceQualityStatus::Partial
        );
        assert!(
            update
                .live
                .current_lap
                .as_ref()
                .unwrap()
                .quality
                .reasons
                .contains(&crate::telemetry_quality::QualityReason::SampleGap)
        );
        fs::remove_dir_all(path).ok();
    }

    #[test]
    fn operator_pause_flushes_without_losing_the_in_progress_lap() {
        let path = std::env::temp_dir().join(format!("lmu-engine-pause-test-{}", unix_ms()));
        let store = DashboardStore::open(&path).unwrap();
        let worker = crate::store::PersistenceWorker::start(store.clone());
        let mut engine = DashboardEngine::new(store.clone(), worker.queue(), "test").unwrap();
        for sample in 1..=21 {
            engine.process(class_frame(sample as f64, 1)).unwrap();
        }

        let (paused, flushed) = engine.pause_and_flush();
        flushed.unwrap();
        assert_eq!(paused.live.capture.state, "paused_by_user");
        assert!(paused.live.capture.paused);
        assert!(paused.live.capture.operator_paused);
        let partial = store
            .load_logical_lap(&paused.live.session.as_ref().unwrap().id, 10, 1)
            .unwrap()
            .unwrap();
        assert!(!partial.summary.completed);
        assert_eq!(partial.samples.len(), 21);

        let resumed = engine.resume();
        assert!(!resumed.live.capture.paused);
        assert!(!resumed.live.capture.operator_paused);
        for sample in 22..=25 {
            engine.process(class_frame(sample as f64, 1)).unwrap();
        }
        engine.process(class_frame(26.0, 2)).unwrap();
        worker.queue().flush().unwrap();
        let completed = store
            .load_logical_lap(&resumed.live.session.as_ref().unwrap().id, 10, 1)
            .unwrap()
            .unwrap();
        assert!(completed.summary.completed);
        assert_eq!(completed.samples.len(), 25);
        fs::remove_dir_all(path).ok();
    }

    #[tokio::test]
    async fn replays_the_committed_restart_and_quality_fixture() {
        let fixture: ReplayFixture =
            serde_json::from_str(include_str!("fixtures/session_replay.json")).unwrap();
        assert_eq!(fixture.schema_version, 1);
        assert_eq!(fixture.session_type, "Race");
        assert!(
            fixture
                .events
                .iter()
                .any(|event| event.label() == "menu_empty_frame")
        );
        assert!(
            fixture
                .events
                .iter()
                .any(|event| event.label() == "session_time_reset")
        );

        let path = std::env::temp_dir().join(format!("lmu-engine-replay-test-{}", unix_ms()));
        let store = DashboardStore::open(&path).unwrap();
        let mut worker = Some(crate::store::PersistenceWorker::start(store.clone()));
        let mut engine = Some(
            DashboardEngine::new(store.clone(), worker.as_ref().unwrap().queue(), "fixture")
                .unwrap(),
        );
        let mut initial_session_id = None;
        let mut resumed_session_id = None;
        let mut reset_session_id = None;

        for event in &fixture.events {
            match event {
                ReplayEvent::Samples {
                    lap_number,
                    count,
                    session_time_start_s,
                    lap_elapsed_start_s,
                    lap_distance_start_m,
                    time_step_s,
                    distance_step_m,
                    ..
                } => {
                    for index in 0..*count {
                        let update = engine
                            .as_mut()
                            .unwrap()
                            .process(
                                fixture.frame(
                                    *lap_number,
                                    *session_time_start_s + index as f64 * *time_step_s,
                                    *lap_elapsed_start_s + index as f64 * *time_step_s,
                                    (*lap_distance_start_m + index as f64 * *distance_step_m)
                                        .min(fixture.track_length_m),
                                ),
                            )
                            .unwrap();
                        let session_id = update.live.session.as_ref().unwrap().id.clone();
                        if initial_session_id.is_none() {
                            initial_session_id = Some(session_id.clone());
                        }
                        if event.label() == "resume_same_logical_lap" {
                            assert!(update.live.capture.session_resumed);
                            resumed_session_id = Some(session_id);
                        } else if event.label() == "session_time_reset" {
                            reset_session_id = Some(session_id);
                        }
                    }
                }
                ReplayEvent::Restart { .. } => {
                    engine.as_mut().unwrap().prepare_shutdown().unwrap();
                    worker.take().unwrap().shutdown().await.unwrap();
                    worker = Some(crate::store::PersistenceWorker::start(store.clone()));
                    engine = Some(
                        DashboardEngine::new(
                            store.clone(),
                            worker.as_ref().unwrap().queue(),
                            "fixture",
                        )
                        .unwrap(),
                    );
                }
                ReplayEvent::Empty { .. } => {
                    engine
                        .as_mut()
                        .unwrap()
                        .process(ParsedFrame::default())
                        .unwrap();
                }
            }
        }
        worker.as_ref().unwrap().queue().flush().unwrap();

        assert_eq!(initial_session_id, resumed_session_id);
        assert_ne!(initial_session_id, reset_session_id);
        let laps = store.list_laps().unwrap();
        assert!(laps.iter().any(|lap| {
            lap.lap_number == 3
                && lap.completed
                && lap.quality.status == crate::telemetry_quality::TraceQualityStatus::Partial
        }));
        assert!(laps.iter().any(|lap| {
            lap.lap_number == 4
                && lap.completed
                && lap.quality.status == crate::telemetry_quality::TraceQualityStatus::Valid
        }));
        assert!(laps.iter().any(|lap| lap.driver_name == "익명 플레이어"));
        assert!(laps.iter().any(|lap| lap.driver_name == "익명 기준차"));
        assert!(!laps.iter().any(|lap| lap.class_name == "LMGT3"));

        engine.as_mut().unwrap().prepare_shutdown().unwrap();
        worker.take().unwrap().shutdown().await.unwrap();
        fs::remove_dir_all(path).ok();
    }

    #[derive(Deserialize)]
    struct ReplayFixture {
        schema_version: u8,
        track_name: String,
        session_type: String,
        track_length_m: f64,
        participants: Vec<ReplayParticipant>,
        events: Vec<ReplayEvent>,
    }

    #[derive(Deserialize)]
    struct ReplayParticipant {
        id: i32,
        driver_name: String,
        class_name: String,
        position: u8,
        is_player: bool,
        last_lap_time_s: f64,
    }

    #[derive(Deserialize)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    enum ReplayEvent {
        Samples {
            label: String,
            lap_number: i32,
            count: usize,
            session_time_start_s: f64,
            lap_elapsed_start_s: f64,
            lap_distance_start_m: f64,
            time_step_s: f64,
            distance_step_m: f64,
        },
        Restart {
            label: String,
        },
        Empty {
            label: String,
        },
    }

    impl ReplayEvent {
        fn label(&self) -> &str {
            match self {
                Self::Samples { label, .. } | Self::Restart { label } | Self::Empty { label } => {
                    label
                }
            }
        }
    }

    impl ReplayFixture {
        fn frame(
            &self,
            lap_number: i32,
            session_time_s: f64,
            lap_elapsed_s: f64,
            lap_distance_m: f64,
        ) -> ParsedFrame {
            let vehicles = self
                .participants
                .iter()
                .map(|participant| VehicleState {
                    id: participant.id,
                    driver_name: participant.driver_name.clone(),
                    class_name: participant.class_name.clone(),
                    position: participant.position,
                    completed_laps: (lap_number - 1) as i16,
                    lap_distance_m,
                    last_lap_time_s: Some(participant.last_lap_time_s),
                    is_player: participant.is_player,
                    world: Point2 {
                        x: lap_distance_m,
                        z: participant.id as f64,
                    },
                    speed_kmh: 180.0,
                    ..VehicleState::default()
                })
                .collect::<Vec<_>>();
            let telemetry = self
                .participants
                .iter()
                .map(|participant| VehicleTelemetry {
                    vehicle_id: participant.id,
                    lap_number,
                    lap_distance_m,
                    lap_elapsed_s,
                    session_time_s,
                    speed_kmh: 180.0,
                    rpm: 7_000.0,
                    gear: 4,
                    throttle: 0.9,
                    world: Point2 {
                        x: lap_distance_m,
                        z: participant.id as f64,
                    },
                    ..VehicleTelemetry::default()
                })
                .collect::<Vec<_>>();
            ParsedFrame {
                session: SessionState {
                    game_version: 13,
                    track_name: self.track_name.clone(),
                    session_type: self.session_type.clone(),
                    current_time_s: session_time_s,
                    max_laps: 20,
                    track_length_m: self.track_length_m,
                    ..SessionState::default()
                },
                player: telemetry
                    .iter()
                    .find(|sample| {
                        self.participants.iter().any(|participant| {
                            participant.id == sample.vehicle_id && participant.is_player
                        })
                    })
                    .cloned(),
                vehicles,
                telemetry,
                ..ParsedFrame::default()
            }
        }
    }

    fn class_frame(session_time_s: f64, lap_number: i32) -> ParsedFrame {
        let vehicles = vec![
            VehicleState {
                id: 12,
                driver_name: "GT Driver".to_owned(),
                class_name: "LMGT3".to_owned(),
                position: 1,
                last_lap_time_s: Some(101.0),
                speed_kmh: 190.0,
                ..VehicleState::default()
            },
            VehicleState {
                id: 10,
                driver_name: "Player".to_owned(),
                class_name: " Hypercar ".to_owned(),
                position: 2,
                is_player: true,
                last_lap_time_s: Some(100.0),
                speed_kmh: 210.0,
                ..VehicleState::default()
            },
            VehicleState {
                id: 11,
                driver_name: "Class Rival".to_owned(),
                class_name: "hypercar".to_owned(),
                position: 3,
                last_lap_time_s: Some(100.5),
                speed_kmh: 205.0,
                ..VehicleState::default()
            },
        ];
        let telemetry = vehicles
            .iter()
            .map(|vehicle| VehicleTelemetry {
                vehicle_id: vehicle.id,
                lap_number,
                session_time_s,
                lap_elapsed_s: if lap_number == 1 { session_time_s } else { 0.1 },
                speed_kmh: vehicle.speed_kmh,
                ..VehicleTelemetry::default()
            })
            .collect::<Vec<_>>();
        ParsedFrame {
            session: SessionState {
                track_name: "Le Mans".to_owned(),
                session_type: "Race".to_owned(),
                current_time_s: session_time_s,
                track_length_m: 13_626.0,
                ..SessionState::default()
            },
            player: telemetry
                .iter()
                .find(|sample| sample.vehicle_id == 10)
                .cloned(),
            telemetry,
            vehicles,
            ..ParsedFrame::default()
        }
    }
}
