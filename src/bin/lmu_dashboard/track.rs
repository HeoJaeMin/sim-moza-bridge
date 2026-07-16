use std::time::{Duration, Instant};

use crate::model::{ParsedFrame, TrackPoint};
use crate::store::{DashboardStore, PersistenceQueue, track_key};

const BUCKET_LENGTH_M: f64 = 10.0;
const SAVE_INTERVAL: Duration = Duration::from_secs(10);

#[derive(Clone, Copy, Debug, Default)]
struct TrackBin {
    x_sum: f64,
    z_sum: f64,
    samples: u32,
}

pub struct TrackMapper {
    store: DashboardStore,
    persistence: PersistenceQueue,
    key: String,
    track_name: String,
    track_length_m: f64,
    bins: Vec<TrackBin>,
    dirty: bool,
    last_save: Instant,
}

impl TrackMapper {
    pub fn new(store: DashboardStore, persistence: PersistenceQueue) -> Self {
        Self {
            store,
            persistence,
            key: String::new(),
            track_name: String::new(),
            track_length_m: 0.0,
            bins: Vec::new(),
            dirty: false,
            last_save: Instant::now(),
        }
    }

    pub fn update(&mut self, frame: &ParsedFrame) -> Result<Vec<TrackPoint>, String> {
        self.ensure_track(&frame.session.track_name, frame.session.track_length_m)?;
        if self.bins.is_empty() {
            return Ok(Vec::new());
        }

        for vehicle in &frame.vehicles {
            if vehicle.in_pits
                || vehicle.pit_state != 0
                || vehicle.speed_kmh < 15.0
                || !vehicle.lap_distance_m.is_finite()
                || !(0.0..=self.track_length_m).contains(&vehicle.lap_distance_m)
                || !vehicle.world.x.is_finite()
                || !vehicle.world.z.is_finite()
            {
                continue;
            }

            let index = ((vehicle.lap_distance_m / BUCKET_LENGTH_M).floor() as usize)
                .min(self.bins.len() - 1);
            let bin = &mut self.bins[index];
            if bin.samples < 128 {
                bin.x_sum += vehicle.world.x;
                bin.z_sum += vehicle.world.z;
                bin.samples += 1;
            } else {
                // Keep adapting slowly without letting counters grow forever.
                bin.x_sum = bin.x_sum * 0.95 + vehicle.world.x * 0.05 * 128.0;
                bin.z_sum = bin.z_sum * 0.95 + vehicle.world.z * 0.05 * 128.0;
            }
            self.dirty = true;
        }

        let points = self.points();
        if self.dirty && self.last_save.elapsed() >= SAVE_INTERVAL && points.len() >= 20 {
            self.persistence.save_track(
                self.key.clone(),
                self.track_name.clone(),
                self.track_length_m,
                points.clone(),
            )?;
            self.dirty = false;
            self.last_save = Instant::now();
        }
        Ok(points)
    }

    pub fn flush(&mut self) -> Result<(), String> {
        if !self.dirty || self.key.is_empty() {
            return Ok(());
        }
        let points = self.points();
        if !points.is_empty() {
            self.persistence.save_track(
                self.key.clone(),
                self.track_name.clone(),
                self.track_length_m,
                points,
            )?;
        }
        self.dirty = false;
        Ok(())
    }

    fn ensure_track(&mut self, name: &str, length_m: f64) -> Result<(), String> {
        if name.is_empty() || length_m < 100.0 {
            return Ok(());
        }
        let next_key = track_key(name, length_m);
        if self.key == next_key {
            return Ok(());
        }
        self.flush()?;
        self.key = next_key;
        self.track_name = name.to_owned();
        self.track_length_m = length_m;
        self.bins =
            vec![TrackBin::default(); (length_m / BUCKET_LENGTH_M).ceil().max(1.0) as usize];
        for point in self.store.load_track(&self.key)? {
            let index = ((point.lap_distance_m / BUCKET_LENGTH_M).floor() as usize)
                .min(self.bins.len() - 1);
            let samples = point.samples.max(1);
            self.bins[index] = TrackBin {
                x_sum: point.x * samples as f64,
                z_sum: point.z * samples as f64,
                samples,
            };
        }
        self.dirty = false;
        self.last_save = Instant::now();
        Ok(())
    }

    fn points(&self) -> Vec<TrackPoint> {
        self.bins
            .iter()
            .enumerate()
            .filter(|(_, bin)| bin.samples > 0)
            .map(|(index, bin)| TrackPoint {
                lap_distance_m: index as f64 * BUCKET_LENGTH_M,
                x: bin.x_sum / bin.samples as f64,
                z: bin.z_sum / bin.samples as f64,
                samples: bin.samples,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::model::{SessionState, VehicleState};
    use crate::store::unix_ms;

    #[test]
    fn learns_track_points_in_lap_distance_order() {
        let path = std::env::temp_dir().join(format!("lmu-track-test-{}", unix_ms()));
        let store = DashboardStore::open(&path).unwrap();
        let worker = crate::store::PersistenceWorker::start(store.clone());
        let mut mapper = TrackMapper::new(store, worker.queue());
        let frame = ParsedFrame {
            session: SessionState {
                track_name: "Test Circuit".to_owned(),
                track_length_m: 1_000.0,
                ..SessionState::default()
            },
            vehicles: vec![
                VehicleState {
                    lap_distance_m: 205.0,
                    world: crate::model::Point2 { x: 2.0, z: 4.0 },
                    speed_kmh: 100.0,
                    ..VehicleState::default()
                },
                VehicleState {
                    lap_distance_m: 15.0,
                    world: crate::model::Point2 { x: 1.0, z: 3.0 },
                    speed_kmh: 100.0,
                    ..VehicleState::default()
                },
            ],
            ..ParsedFrame::default()
        };

        let points = mapper.update(&frame).unwrap();
        assert_eq!(points.len(), 2);
        assert!(points[0].lap_distance_m < points[1].lap_distance_m);
        assert_eq!(points[0].x, 1.0);
        worker.queue().flush().unwrap();
        fs::remove_dir_all(path).ok();
    }
}
