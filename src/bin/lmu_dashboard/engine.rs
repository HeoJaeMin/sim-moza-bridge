use std::collections::{HashMap, VecDeque};

use crate::contact::ContactDetector;
use crate::lap::LapTracker;
use crate::model::{LiveSnapshot, ParsedFrame, TraceResponse};
use crate::store::{DashboardStore, track_key, unix_ms};
use crate::track::TrackMapper;

const RECENT_CONTACT_LIMIT: usize = 50;

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
    previous_frame_had_valid_session: bool,
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
            previous_frame_had_valid_session: false,
            recent_contacts,
            last_live: LiveSnapshot::default(),
        })
    }

    pub fn process(&mut self, mut frame: ParsedFrame) -> Result<EngineUpdate, String> {
        self.update_session(&mut frame)?;
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
                tracker.reset(&self.session_id, &frame.session.track_name);
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
        };
        self.last_live = live.clone();
        Ok(EngineUpdate { live, trace })
    }

    pub fn disconnected(&mut self, warning: impl Into<String>) -> EngineUpdate {
        let mut live = self.last_live.clone();
        live.connected = false;
        live.source = self.source.clone();
        live.warning = Some(warning.into());
        EngineUpdate {
            live,
            trace: self.player_trace(),
        }
    }

    fn update_session(&mut self, frame: &mut ParsedFrame) -> Result<(), String> {
        let signature = format!(
            "{}:{}",
            track_key(&frame.session.track_name, frame.session.track_length_m),
            frame.session.session_type
        );
        let time_reset = frame.session.current_time_s + 5.0 < self.last_session_time_s;
        let valid_session = frame.session.track_length_m >= 100.0 && !frame.vehicles.is_empty();
        if !valid_session {
            frame.session.id.clear();
            self.previous_frame_had_valid_session = false;
            return Ok(());
        }
        let new_session = valid_session
            && (self.session_id.is_empty()
                || !self.previous_frame_had_valid_session
                || self.session_signature != signature
                || time_reset);

        if new_session {
            self.last_session_created_at_ms =
                unix_ms().max(self.last_session_created_at_ms.saturating_add(1));
            self.session_id = format!(
                "{}-{}",
                track_key(&frame.session.track_name, frame.session.track_length_m),
                self.last_session_created_at_ms
            );
            self.session_signature = signature;
            self.contact_detector.reset();
            self.lap_trackers.clear();
            self.player_vehicle_id = None;
            frame.session.id = self.session_id.clone();
            self.store.save_session(&frame.session)?;
        } else {
            frame.session.id = self.session_id.clone();
        }
        self.last_session_time_s = frame.session.current_time_s;
        self.previous_frame_had_valid_session = true;
        Ok(())
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
