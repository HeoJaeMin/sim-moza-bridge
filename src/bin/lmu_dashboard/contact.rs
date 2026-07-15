use std::collections::{HashMap, HashSet, VecDeque};

use crate::model::{
    ContactConfidence, ContactEvent, ContactParticipant, ImpactState, ParsedFrame, Point2,
    VehicleState,
};
use crate::store::unix_ms;

const RECIPROCAL_TIME_WINDOW_S: f64 = 0.2;
const RECIPROCAL_POSITION_WINDOW_M: f64 = 6.0;
const NEARBY_VEHICLE_WINDOW_M: f64 = 7.0;
const MIN_IMPACT_MAGNITUDE: f64 = 0.1;
const MAX_DEDUPE_EVENTS: usize = 256;

#[derive(Clone)]
struct PendingImpact {
    impact: ImpactState,
    car_a: ContactParticipant,
    nearby: Option<ContactParticipant>,
}

#[derive(Default)]
pub struct ContactDetector {
    initialized: bool,
    last_impact_time: HashMap<i32, f64>,
    pending: Vec<PendingImpact>,
    dedupe_order: VecDeque<String>,
    dedupe: HashSet<String>,
}

impl ContactDetector {
    pub fn detect(&mut self, frame: &ParsedFrame) -> Vec<ContactEvent> {
        if !self.initialized {
            self.remember_impact_times(&frame.impacts);
            self.initialized = true;
            return Vec::new();
        }

        let new_impacts = frame
            .impacts
            .iter()
            .copied()
            .filter(|impact| {
                let previous = self
                    .last_impact_time
                    .get(&impact.vehicle_id)
                    .copied()
                    .unwrap_or(0.0);
                impact.event_time_s > previous + f64::EPSILON
                    && impact.magnitude >= MIN_IMPACT_MAGNITUDE
            })
            .collect::<Vec<_>>();

        self.remember_impact_times(&frame.impacts);
        for impact in new_impacts {
            let Some(car_a) = frame
                .vehicles
                .iter()
                .find(|vehicle| vehicle.id == impact.vehicle_id)
            else {
                continue;
            };
            self.pending.push(PendingImpact {
                impact,
                car_a: participant(car_a),
                nearby: nearest_vehicle(frame, impact).map(participant),
            });
        }

        let pending = std::mem::take(&mut self.pending);
        let mut consumed = vec![false; pending.len()];
        let mut events = Vec::new();
        for (index, candidate) in pending.iter().enumerate() {
            if consumed[index] {
                continue;
            }

            let reciprocal = pending
                .iter()
                .enumerate()
                .filter(|(other_index, other)| {
                    *other_index != index
                        && !consumed[*other_index]
                        && other.impact.vehicle_id != candidate.impact.vehicle_id
                        && (other.impact.event_time_s - candidate.impact.event_time_s).abs()
                            <= RECIPROCAL_TIME_WINDOW_S
                        && other.impact.position.distance_to(candidate.impact.position)
                            <= RECIPROCAL_POSITION_WINDOW_M
                })
                .min_by(|(_, left), (_, right)| {
                    left.impact
                        .position
                        .distance_to(candidate.impact.position)
                        .total_cmp(&right.impact.position.distance_to(candidate.impact.position))
                });

            let Some((other_index, other)) = reciprocal else {
                continue;
            };
            consumed[index] = true;
            consumed[other_index] = true;
            if let Some(event) = self.build_event(
                frame,
                candidate,
                Some(other.car_a.clone()),
                Some(other.impact.magnitude),
                ContactConfidence::Confirmed,
            ) {
                events.push(event);
            }
        }

        for (index, candidate) in pending.into_iter().enumerate() {
            if consumed[index] {
                continue;
            }
            if frame.session.current_time_s - candidate.impact.event_time_s
                < RECIPROCAL_TIME_WINDOW_S
            {
                self.pending.push(candidate);
                continue;
            }
            let confidence = if candidate.nearby.is_some() {
                ContactConfidence::Probable
            } else {
                ContactConfidence::Unresolved
            };
            if let Some(event) = self.build_event(
                frame,
                &candidate,
                candidate.nearby.clone(),
                None,
                confidence,
            ) {
                events.push(event);
            }
        }

        events
    }

    pub fn reset(&mut self) {
        self.initialized = false;
        self.last_impact_time.clear();
        self.pending.clear();
        self.dedupe_order.clear();
        self.dedupe.clear();
    }

    fn remember_impact_times(&mut self, impacts: &[ImpactState]) {
        for impact in impacts {
            self.last_impact_time
                .entry(impact.vehicle_id)
                .and_modify(|value| *value = value.max(impact.event_time_s))
                .or_insert(impact.event_time_s);
        }
    }

    fn build_event(
        &mut self,
        frame: &ParsedFrame,
        candidate: &PendingImpact,
        opponent: Option<ContactParticipant>,
        magnitude_b: Option<f64>,
        confidence: ContactConfidence,
    ) -> Option<ContactEvent> {
        let opponent_id = opponent.as_ref().map_or(-1, |vehicle| vehicle.vehicle_id);
        let pair_key = pair_key(
            candidate.impact.vehicle_id,
            opponent_id,
            candidate.impact.event_time_s,
            &frame.session.id,
        );
        if !self.remember(pair_key.clone()) {
            return None;
        }

        Some(ContactEvent {
            id: pair_key,
            session_id: frame.session.id.clone(),
            track_name: frame.session.track_name.clone(),
            session_time_s: candidate.impact.event_time_s,
            car_a: candidate.car_a.clone(),
            car_b: opponent,
            magnitude_a: candidate.impact.magnitude,
            magnitude_b,
            position: candidate.impact.position.xz(),
            confidence,
            created_at_unix_ms: unix_ms(),
        })
    }

    fn remember(&mut self, key: String) -> bool {
        if !self.dedupe.insert(key.clone()) {
            return false;
        }
        self.dedupe_order.push_back(key);
        while self.dedupe_order.len() > MAX_DEDUPE_EVENTS {
            if let Some(oldest) = self.dedupe_order.pop_front() {
                self.dedupe.remove(&oldest);
            }
        }
        true
    }
}

fn nearest_vehicle(frame: &ParsedFrame, impact: ImpactState) -> Option<&VehicleState> {
    frame
        .vehicles
        .iter()
        .filter(|vehicle| vehicle.id != impact.vehicle_id && !vehicle.in_pits)
        .filter_map(|vehicle| {
            let distance = distance_xz(vehicle.world, impact.position.xz());
            (distance <= NEARBY_VEHICLE_WINDOW_M).then_some((vehicle, distance))
        })
        .min_by(|(_, left), (_, right)| left.total_cmp(right))
        .map(|(vehicle, _)| vehicle)
}

fn participant(vehicle: &VehicleState) -> ContactParticipant {
    ContactParticipant {
        vehicle_id: vehicle.id,
        driver_name: vehicle.driver_name.clone(),
        class_name: vehicle.class_name.clone(),
        position: vehicle.position,
        lap_number: vehicle.completed_laps.saturating_add(1),
    }
}

fn distance_xz(left: Point2, right: Point2) -> f64 {
    let dx = left.x - right.x;
    let dz = left.z - right.z;
    dx.mul_add(dx, dz * dz).sqrt()
}

fn pair_key(car_a: i32, car_b: i32, event_time_s: f64, session_id: &str) -> String {
    let (first, second) = if car_b >= 0 && car_b < car_a {
        (car_b, car_a)
    } else {
        (car_a, car_b)
    };
    format!(
        "{}-{first}-{second}-{}",
        session_id,
        (event_time_s * 10.0).round() as i64
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Point3, SessionState};

    #[test]
    fn pairs_reciprocal_impacts_once() {
        let mut detector = ContactDetector::default();
        let mut frame = ParsedFrame {
            session: SessionState {
                id: "session-1".to_owned(),
                track_name: "Le Mans".to_owned(),
                current_time_s: 42.1,
                ..SessionState::default()
            },
            vehicles: vec![vehicle(10, "Alice", 1), vehicle(20, "Bob", 2)],
            impacts: vec![impact(10, 42.0, 3.5), impact(20, 42.05, 2.8)],
            ..ParsedFrame::default()
        };

        let impacts = std::mem::take(&mut frame.impacts);
        assert!(detector.detect(&frame).is_empty());
        frame.impacts = impacts;
        let events = detector.detect(&frame);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].car_a.driver_name, "Alice");
        assert_eq!(events[0].car_b.as_ref().unwrap().driver_name, "Bob");
        assert!(matches!(events[0].confidence, ContactConfidence::Confirmed));
        assert!(detector.detect(&frame).is_empty());
    }

    #[test]
    fn pairs_impacts_received_on_adjacent_frames() {
        let mut detector = ContactDetector::default();
        let mut frame = ParsedFrame {
            session: SessionState {
                id: "session-1".to_owned(),
                track_name: "Le Mans".to_owned(),
                current_time_s: 41.0,
                ..SessionState::default()
            },
            vehicles: vec![vehicle(10, "Alice", 1), vehicle(20, "Bob", 2)],
            ..ParsedFrame::default()
        };
        assert!(detector.detect(&frame).is_empty());

        frame.session.current_time_s = 42.0;
        frame.impacts = vec![impact(10, 42.0, 3.5)];
        assert!(detector.detect(&frame).is_empty());

        frame.session.current_time_s = 42.05;
        frame.impacts.push(impact(20, 42.05, 2.8));
        let events = detector.detect(&frame);
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0].confidence, ContactConfidence::Confirmed));
    }

    #[test]
    fn treats_the_first_impact_snapshot_as_a_baseline() {
        let mut detector = ContactDetector::default();
        let frame = ParsedFrame {
            session: SessionState {
                id: "session-1".to_owned(),
                current_time_s: 100.0,
                ..SessionState::default()
            },
            vehicles: vec![vehicle(10, "Alice", 1), vehicle(20, "Bob", 2)],
            impacts: vec![impact(10, 80.0, 3.5)],
            ..ParsedFrame::default()
        };

        assert!(detector.detect(&frame).is_empty());
        assert!(detector.detect(&frame).is_empty());
    }

    fn vehicle(id: i32, name: &str, position: u8) -> VehicleState {
        VehicleState {
            id,
            driver_name: name.to_owned(),
            position,
            world: Point2 { x: 0.0, z: 0.0 },
            ..VehicleState::default()
        }
    }

    fn impact(vehicle_id: i32, event_time_s: f64, magnitude: f64) -> ImpactState {
        ImpactState {
            vehicle_id,
            event_time_s,
            magnitude,
            position: Point3 {
                x: 1.0,
                y: 0.0,
                z: 1.0,
            },
        }
    }
}
