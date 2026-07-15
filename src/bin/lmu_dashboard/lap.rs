use crate::model::{
    CurrentLapInfo, LapSummary, SavedLap, TelemetryPoint, TraceResponse, VehicleState,
    VehicleTelemetry,
};
use crate::store::unix_ms;

const MIN_SAMPLES_TO_SAVE: usize = 20;

#[derive(Default)]
pub struct LapTracker {
    session_id: String,
    track_name: String,
    current_lap_number: Option<i32>,
    current_samples: Vec<TelemetryPoint>,
    invalid: bool,
    last_session_time_s: f64,
    vehicle_id: i32,
    driver_name: String,
    class_name: String,
    is_player: bool,
    overall_position: u8,
    class_position: u8,
    last_lap_time_s: Option<f64>,
}

impl LapTracker {
    pub fn reset(&mut self, session_id: &str, track_name: &str) {
        self.session_id = session_id.to_owned();
        self.track_name = track_name.to_owned();
        self.current_lap_number = None;
        self.current_samples.clear();
        self.invalid = false;
        self.last_session_time_s = 0.0;
        self.vehicle_id = 0;
        self.driver_name.clear();
        self.class_name.clear();
        self.is_player = false;
        self.overall_position = 0;
        self.class_position = 0;
        self.last_lap_time_s = None;
    }

    pub fn ingest(
        &mut self,
        telemetry: &VehicleTelemetry,
        vehicle: &VehicleState,
        class_position: u8,
    ) -> Option<SavedLap> {
        self.update_vehicle(vehicle, class_position);
        let mut completed = None;
        match self.current_lap_number {
            None => self.start_lap(telemetry.lap_number),
            Some(current) if telemetry.lap_number > current => {
                completed = self.finish_lap();
                self.start_lap(telemetry.lap_number);
            }
            Some(current)
                if telemetry.lap_number < current
                    || telemetry.session_time_s + 1.0 < self.last_session_time_s =>
            {
                self.start_lap(telemetry.lap_number);
            }
            Some(_) => {}
        }

        if self.current_samples.is_empty()
            || telemetry.session_time_s > self.last_session_time_s + f64::EPSILON
        {
            self.current_samples.push(TelemetryPoint::from(telemetry));
            self.last_session_time_s = telemetry.session_time_s;
        }
        self.invalid |= telemetry.lap_invalidated;
        completed
    }

    pub fn current_info(&self) -> Option<CurrentLapInfo> {
        Some(CurrentLapInfo {
            lap_number: self.current_lap_number?,
            lap_elapsed_s: self
                .current_samples
                .last()
                .map_or(0.0, |sample| sample.lap_elapsed_s),
            sample_count: self.current_samples.len(),
            invalid: self.invalid,
        })
    }

    pub fn trace(&self) -> TraceResponse {
        let summary = self.current_lap_number.map(|lap_number| LapSummary {
            id: "current".to_owned(),
            session_id: self.session_id.clone(),
            track_name: self.track_name.clone(),
            vehicle_id: self.vehicle_id,
            driver_name: self.driver_name.clone(),
            class_name: self.class_name.clone(),
            is_player: self.is_player,
            overall_position: self.overall_position,
            class_position: self.class_position,
            lap_number,
            lap_time_ms: self
                .current_samples
                .last()
                .map_or(0, |sample| seconds_to_ms(sample.lap_elapsed_s)),
            valid: !self.invalid,
            sample_count: self.current_samples.len(),
            created_at_unix_ms: 0,
            completed: false,
        });
        TraceResponse {
            summary,
            samples: self.current_samples.clone(),
        }
    }

    fn start_lap(&mut self, lap_number: i32) {
        self.current_lap_number = Some(lap_number);
        self.current_samples.clear();
        self.invalid = false;
        self.last_session_time_s = 0.0;
    }

    fn update_vehicle(&mut self, vehicle: &VehicleState, class_position: u8) {
        self.vehicle_id = vehicle.id;
        self.driver_name.clone_from(&vehicle.driver_name);
        self.class_name.clone_from(&vehicle.class_name);
        self.is_player = vehicle.is_player;
        self.overall_position = vehicle.position;
        self.class_position = class_position;
        self.last_lap_time_s = vehicle.last_lap_time_s;
    }

    fn finish_lap(&mut self) -> Option<SavedLap> {
        let lap_number = self.current_lap_number?;
        if lap_number <= 0 || self.current_samples.len() < MIN_SAMPLES_TO_SAVE {
            return None;
        }
        let created_at = unix_ms();
        let measured_lap_ms = self
            .current_samples
            .last()
            .map_or(0, |sample| seconds_to_ms(sample.lap_elapsed_s));
        let official_lap_ms = self
            .last_lap_time_s
            .map(seconds_to_ms)
            .filter(|value| *value > 0);
        let summary = LapSummary {
            id: format!(
                "{}-car-{}-lap-{lap_number}-{created_at}",
                self.session_id, self.vehicle_id
            ),
            session_id: self.session_id.clone(),
            track_name: self.track_name.clone(),
            vehicle_id: self.vehicle_id,
            driver_name: self.driver_name.clone(),
            class_name: self.class_name.clone(),
            is_player: self.is_player,
            overall_position: self.overall_position,
            class_position: self.class_position,
            lap_number,
            lap_time_ms: official_lap_ms.unwrap_or(measured_lap_ms),
            valid: !self.invalid,
            sample_count: self.current_samples.len(),
            created_at_unix_ms: created_at,
            completed: true,
        };
        Some(SavedLap {
            summary,
            samples: std::mem::take(&mut self.current_samples),
        })
    }
}

fn seconds_to_ms(seconds: f64) -> u32 {
    if !seconds.is_finite() || seconds <= 0.0 {
        0
    } else {
        (seconds * 1_000.0).round().clamp(0.0, u32::MAX as f64) as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saves_one_completed_lap_and_starts_the_next() {
        let mut tracker = LapTracker::default();
        tracker.reset("session", "Le Mans");
        for index in 0..25 {
            let mut sample = player(3, index as f64);
            sample.lap_elapsed_s = index as f64 * 0.05;
            tracker.ingest(&sample, &vehicle(), 1);
        }
        let mut next = player(4, 2.0);
        next.lap_elapsed_s = 0.1;
        let completed = tracker.ingest(
            &next,
            &VehicleState {
                last_lap_time_s: Some(210.123),
                ..vehicle()
            },
            1,
        );

        let completed = completed.unwrap();
        assert_eq!(completed.summary.lap_number, 3);
        assert_eq!(completed.summary.lap_time_ms, 210_123);
        assert_eq!(completed.summary.vehicle_id, 7);
        assert_eq!(completed.summary.driver_name, "Driver");
        assert_eq!(completed.samples.len(), 25);
        assert_eq!(tracker.current_info().unwrap().lap_number, 4);
        assert_eq!(tracker.current_info().unwrap().sample_count, 1);
    }

    #[test]
    fn keeps_invalid_flag_for_the_whole_lap() {
        let mut tracker = LapTracker::default();
        tracker.reset("session", "Le Mans");
        let mut sample = player(2, 1.0);
        sample.lap_invalidated = true;
        tracker.ingest(&sample, &vehicle(), 1);
        assert!(tracker.current_info().unwrap().invalid);
    }

    fn player(lap_number: i32, session_time_s: f64) -> VehicleTelemetry {
        VehicleTelemetry {
            vehicle_id: 7,
            lap_number,
            session_time_s,
            ..VehicleTelemetry::default()
        }
    }

    fn vehicle() -> VehicleState {
        VehicleState {
            id: 7,
            driver_name: "Driver".to_owned(),
            class_name: "Hypercar".to_owned(),
            position: 2,
            is_player: true,
            ..VehicleState::default()
        }
    }
}
