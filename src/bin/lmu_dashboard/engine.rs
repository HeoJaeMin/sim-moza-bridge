use std::collections::{HashMap, VecDeque};

use crate::contact::ContactDetector;
use crate::lap::LapTracker;
use crate::model::{CaptureHealth, LiveSnapshot, ParsedFrame, TraceResponse};
use crate::store::{DashboardStore, session_signature, track_key, unix_ms};
use crate::telemetry_quality::TraceQuality;
use crate::track::TrackMapper;

const RECENT_CONTACT_LIMIT: usize = 50;
const SESSION_RESUME_MAX_AGE_MS: u64 = 30 * 60 * 1_000;
const SESSION_TOUCH_INTERVAL_MS: u64 = 1_000;
const FRAME_RATE_WINDOW_MS: u64 = 5_000;

pub struct EngineUpdate {
    pub live: LiveSnapshot,
    pub trace: TraceResponse,
}

pub struct DashboardEngine {
    store: DashboardStore,
    source: String,
    track_mapper: TrackMapper,
    contact_detector: ContactDetector,
    lap_trackers: HashMap<i32, LapTracker>,
    player_vehicle_id: Option<i32>,
    session_id: String,
    session_signature: String,
    last_session_time_s: f64,
    last_session_created_at_ms: u64,
    last_session_touch_ms: u64,
    session_resumed: bool,
    accepted_frames: u64,
    rejected_frames: u64,
    duplicate_frames: u64,
    invalid_session_frames: u64,
    frame_times_ms: VecDeque<u64>,
    last_frame_ms: u64,
    last_player_session_time_s: Option<f64>,
    recent_contacts: VecDeque<crate::model::ContactEvent>,
    last_live: LiveSnapshot,
}

impl DashboardEngine {
    pub fn new(store: DashboardStore, source: impl Into<String>) -> Result<Self, String> {
        let recent_contacts = store
            .recent_contacts(RECENT_CONTACT_LIMIT)?
            .into_iter()
            .collect();
        Ok(Self {
            track_mapper: TrackMapper::new(store.clone()),
            store,
            source: source.into(),
            contact_detector: ContactDetector::default(),
            lap_trackers: HashMap::new(),
            player_vehicle_id: None,
            session_id: String::new(),
            session_signature: String::new(),
            last_session_time_s: 0.0,
            last_session_created_at_ms: 0,
            last_session_touch_ms: 0,
            session_resumed: false,
            accepted_frames: 0,
            rejected_frames: 0,
            duplicate_frames: 0,
            invalid_session_frames: 0,
            frame_times_ms: VecDeque::new(),
            last_frame_ms: 0,
            last_player_session_time_s: None,
            recent_contacts,
            last_live: LiveSnapshot::default(),
        })
    }

    pub fn process(&mut self, mut frame: ParsedFrame) -> Result<EngineUpdate, String> {
        let now = unix_ms();
        if !self.update_session(&mut frame, now)? {
            self.invalid_session_frames = self.invalid_session_frames.saturating_add(1);
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
            self.store.save_contact(&contact)?;
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
            let tracker = self.lap_trackers.entry(vehicle.id).or_insert_with(|| {
                let mut tracker = LapTracker::default();
                tracker.reset(
                    &self.session_id,
                    &frame.session.track_name,
                    &frame.session.session_type,
                    frame.session.track_length_m,
                );
                tracker
            });
            if let Some(lap) = tracker.ingest(
                telemetry,
                vehicle,
                class_positions
                    .get(&vehicle.id)
                    .copied()
                    .unwrap_or_default(),
            ) {
                self.store.save_lap(&lap)?;
            }
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
            capture: self.capture_health(now, current_quality, "live"),
        };
        self.last_live = live.clone();
        Ok(EngineUpdate { live, trace })
    }

    pub fn disconnected(&mut self, warning: impl Into<String>) -> EngineUpdate {
        self.rejected_frames = self.rejected_frames.saturating_add(1);
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

    fn update_session(&mut self, frame: &mut ParsedFrame, now: u64) -> Result<bool, String> {
        let signature = session_signature(&frame.session);
        let time_reset = frame.session.current_time_s + 5.0 < self.last_session_time_s;
        let valid_session = frame.session.track_length_m >= 100.0 && !frame.vehicles.is_empty();
        if !valid_session {
            frame.session.id.clone_from(&self.session_id);
            return Ok(false);
        }
        let mut new_session = self.session_signature != signature || time_reset;
        if self.session_id.is_empty() {
            if let Some(resume) = self.store.resumable_session(
                &signature,
                frame.session.current_time_s,
                SESSION_RESUME_MAX_AGE_MS,
            )? {
                self.session_id = resume.id;
                self.last_session_time_s = resume.last_session_time_s;
                self.session_signature.clone_from(&signature);
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
            self.last_session_created_at_ms =
                now.max(self.last_session_created_at_ms.saturating_add(1));
            self.session_id = format!(
                "{}-{}",
                track_key(&frame.session.track_name, frame.session.track_length_m),
                self.last_session_created_at_ms
            );
            self.session_signature = signature;
            self.session_resumed = false;
            self.contact_detector.reset();
            self.lap_trackers.clear();
            self.player_vehicle_id = None;
            frame.session.id = self.session_id.clone();
            self.store
                .save_session_with_source(&frame.session, &self.source)?;
            self.last_session_touch_ms = now;
        } else {
            frame.session.id = self.session_id.clone();
            if now.saturating_sub(self.last_session_touch_ms) >= SESSION_TOUCH_INTERVAL_MS {
                self.store.touch_session(&frame.session)?;
                self.last_session_touch_ms = now;
            }
        }
        self.last_session_time_s = frame.session.current_time_s;
        Ok(true)
    }

    fn register_frame(&mut self, frame: &ParsedFrame, now: u64) {
        self.accepted_frames = self.accepted_frames.saturating_add(1);
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
                self.duplicate_frames = self.duplicate_frames.saturating_add(1);
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
            accepted_frames: self.accepted_frames,
            rejected_frames: self.rejected_frames,
            duplicate_frames: self.duplicate_frames,
            invalid_session_frames: self.invalid_session_frames,
            last_frame_age_ms: if self.last_frame_ms == 0 {
                0
            } else {
                now.saturating_sub(self.last_frame_ms)
            },
            session_resumed: self.session_resumed,
            current_quality,
        }
    }

    fn player_trace(&self) -> TraceResponse {
        self.player_vehicle_id
            .and_then(|vehicle_id| self.lap_trackers.get(&vehicle_id))
            .map_or_else(TraceResponse::default, LapTracker::trace)
    }
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

    use super::*;
    use crate::model::{Point2, SessionState, VehicleState, VehicleTelemetry};

    #[test]
    fn assigns_a_session_and_builds_live_state() {
        let path = std::env::temp_dir().join(format!("lmu-engine-test-{}", unix_ms()));
        let store = DashboardStore::open(&path).unwrap();
        let mut engine = DashboardEngine::new(store, "test").unwrap();
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
        let mut engine = DashboardEngine::new(store, "test").unwrap();
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
        let second_id = engine
            .process(race_frame(1.0))
            .unwrap()
            .live
            .session
            .unwrap()
            .id;

        assert_ne!(first_id, second_id);
        fs::remove_dir_all(path).ok();
    }

    #[test]
    fn saves_only_player_class_laps_and_keeps_player_trace_live() {
        let path = std::env::temp_dir().join(format!("lmu-engine-class-test-{}", unix_ms()));
        let store = DashboardStore::open(&path).unwrap();
        let mut engine = DashboardEngine::new(store.clone(), "test").unwrap();

        for sample in 0..21 {
            engine.process(class_frame(sample as f64 + 1.0, 1)).unwrap();
        }
        let update = engine.process(class_frame(22.0, 2)).unwrap();

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
