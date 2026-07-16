use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension, Transaction, params};

use crate::model::{
    ContactConfidence, ContactEvent, ContactParticipant, LapSummary, PersistenceHealth, SavedLap,
    SessionState, TelemetryPoint, TrackPoint,
};
use crate::telemetry_quality::{TraceQuality, TraceQualityStatus};

const SCHEMA_VERSION: u32 = 3;
const RESUME_CLOCK_TOLERANCE_S: f64 = 15.0;
const RESUME_CLOCK_BACKWARD_TOLERANCE_S: f64 = 0.5;

#[derive(Clone, Debug, PartialEq)]
pub struct ResumableSession {
    pub id: String,
    pub last_session_time_s: f64,
    pub last_seen_ms: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StoredSession {
    pub id: String,
    pub track_name: String,
    pub session_type: String,
    pub track_length_m: f64,
    pub started_at_unix_ms: u64,
}

enum PersistenceCommand {
    SaveLap(SavedLap),
    SaveContact(ContactEvent),
    SaveTrack {
        key: String,
        track_name: String,
        track_length_m: f64,
        points: Vec<TrackPoint>,
    },
    Flush(mpsc::SyncSender<Result<(), String>>),
    Shutdown(mpsc::SyncSender<Result<(), String>>),
}

#[derive(Default)]
struct PersistenceCounters {
    queued: AtomicU64,
    written: AtomicU64,
    failed: AtomicU64,
    pending: AtomicU64,
    last_error: Mutex<Option<String>>,
}

#[derive(Clone)]
pub struct PersistenceQueue {
    sender: mpsc::Sender<PersistenceCommand>,
    counters: Arc<PersistenceCounters>,
}

pub struct PersistenceWorker {
    queue: PersistenceQueue,
    thread: Option<thread::JoinHandle<()>>,
}

impl PersistenceWorker {
    pub fn start(store: DashboardStore) -> Self {
        let (sender, receiver) = mpsc::channel();
        let counters = Arc::new(PersistenceCounters::default());
        let worker_counters = counters.clone();
        let thread = thread::Builder::new()
            .name("lmu-dashboard-store".to_owned())
            .spawn(move || persistence_loop(store, receiver, &worker_counters))
            .expect("failed to start LMU dashboard persistence worker");
        Self {
            queue: PersistenceQueue { sender, counters },
            thread: Some(thread),
        }
    }

    pub fn queue(&self) -> PersistenceQueue {
        self.queue.clone()
    }

    pub async fn shutdown(mut self) -> Result<(), String> {
        tokio::task::spawn_blocking(move || self.shutdown_blocking())
            .await
            .map_err(|error| format!("persistence shutdown task failed: {error}"))?
    }

    fn shutdown_blocking(&mut self) -> Result<(), String> {
        let (acknowledge, response) = mpsc::sync_channel(1);
        self.queue
            .sender
            .send(PersistenceCommand::Shutdown(acknowledge))
            .map_err(|_| "persistence worker stopped before shutdown".to_owned())?;
        let result = response
            .recv()
            .map_err(|_| "persistence worker did not acknowledge shutdown".to_owned())?;
        if let Some(thread) = self.thread.take() {
            thread
                .join()
                .map_err(|_| "persistence worker panicked during shutdown".to_owned())?;
        }
        result
    }
}

impl Drop for PersistenceWorker {
    fn drop(&mut self) {
        if self.thread.is_some() {
            let (acknowledge, _) = mpsc::sync_channel(1);
            let _ = self
                .queue
                .sender
                .send(PersistenceCommand::Shutdown(acknowledge));
        }
    }
}

impl PersistenceQueue {
    pub fn save_lap(&self, lap: SavedLap) -> Result<(), String> {
        self.enqueue(PersistenceCommand::SaveLap(lap))
    }

    pub fn save_contact(&self, contact: ContactEvent) -> Result<(), String> {
        self.enqueue(PersistenceCommand::SaveContact(contact))
    }

    pub fn save_track(
        &self,
        key: String,
        track_name: String,
        track_length_m: f64,
        points: Vec<TrackPoint>,
    ) -> Result<(), String> {
        self.enqueue(PersistenceCommand::SaveTrack {
            key,
            track_name,
            track_length_m,
            points,
        })
    }

    pub fn flush(&self) -> Result<(), String> {
        let (acknowledge, response) = mpsc::sync_channel(1);
        self.sender
            .send(PersistenceCommand::Flush(acknowledge))
            .map_err(|_| "persistence worker stopped before flush".to_owned())?;
        response
            .recv()
            .map_err(|_| "persistence worker did not acknowledge flush".to_owned())?
    }

    pub fn health(&self) -> PersistenceHealth {
        PersistenceHealth {
            queued: self.counters.queued.load(Ordering::Relaxed),
            written: self.counters.written.load(Ordering::Relaxed),
            failed: self.counters.failed.load(Ordering::Relaxed),
            pending: self.counters.pending.load(Ordering::Relaxed),
            last_error: self
                .counters
                .last_error
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clone(),
        }
    }

    fn enqueue(&self, command: PersistenceCommand) -> Result<(), String> {
        self.counters.queued.fetch_add(1, Ordering::Relaxed);
        self.counters.pending.fetch_add(1, Ordering::Relaxed);
        if self.sender.send(command).is_err() {
            self.counters.pending.fetch_sub(1, Ordering::Relaxed);
            self.counters.failed.fetch_add(1, Ordering::Relaxed);
            let error = "persistence worker is unavailable".to_owned();
            *self
                .counters
                .last_error
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(error.clone());
            return Err(error);
        }
        Ok(())
    }
}

fn persistence_loop(
    store: DashboardStore,
    receiver: mpsc::Receiver<PersistenceCommand>,
    counters: &PersistenceCounters,
) {
    while let Ok(command) = receiver.recv() {
        match command {
            PersistenceCommand::SaveLap(lap) => {
                finish_persistence(store.save_lap(&lap), counters);
            }
            PersistenceCommand::SaveContact(contact) => {
                finish_persistence(store.save_contact(&contact), counters);
            }
            PersistenceCommand::SaveTrack {
                key,
                track_name,
                track_length_m,
                points,
            } => {
                finish_persistence(
                    store.save_track(&key, &track_name, track_length_m, &points),
                    counters,
                );
            }
            PersistenceCommand::Flush(response) => {
                let _ = response.send(persistence_result(counters));
            }
            PersistenceCommand::Shutdown(response) => {
                let result = persistence_result(counters).and_then(|()| store.optimize());
                let _ = response.send(result);
                break;
            }
        }
    }
}

fn finish_persistence(result: Result<(), String>, counters: &PersistenceCounters) {
    counters.pending.fetch_sub(1, Ordering::Relaxed);
    match result {
        Ok(()) => {
            counters.written.fetch_add(1, Ordering::Relaxed);
        }
        Err(error) => {
            counters.failed.fetch_add(1, Ordering::Relaxed);
            *counters
                .last_error
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(error);
        }
    }
}

fn persistence_result(counters: &PersistenceCounters) -> Result<(), String> {
    counters
        .last_error
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
        .map_or(Ok(()), Err)
}

#[derive(Clone, Debug)]
pub struct DashboardStore {
    database_path: PathBuf,
}

impl DashboardStore {
    pub fn open(data_dir: impl AsRef<Path>) -> Result<Self, String> {
        fs::create_dir_all(data_dir.as_ref()).map_err(|error| {
            format!(
                "failed to create dashboard data directory {}: {error}",
                data_dir.as_ref().display()
            )
        })?;
        let store = Self {
            database_path: data_dir.as_ref().join("dashboard.sqlite3"),
        };
        store.initialize()?;
        Ok(store)
    }

    #[allow(dead_code)]
    pub fn save_session(&self, session: &SessionState) -> Result<(), String> {
        self.save_session_with_source(session, "unknown")
    }

    pub fn save_session_with_source(
        &self,
        session: &SessionState,
        source: &str,
    ) -> Result<(), String> {
        let now = unix_ms() as i64;
        self.connection()?
            .execute(
                "INSERT INTO sessions
                 (id, started_at_ms, track_key, track_name, session_type, game_version,
                  signature, last_seen_ms, last_session_time_s, source, max_laps, track_length_m)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
                 ON CONFLICT(id) DO UPDATE SET
                    last_seen_ms = excluded.last_seen_ms,
                    last_session_time_s = excluded.last_session_time_s,
                    source = excluded.source,
                    game_version = excluded.game_version,
                    signature = excluded.signature,
                    max_laps = excluded.max_laps,
                    track_length_m = excluded.track_length_m",
                params![
                    session.id,
                    now,
                    track_key(&session.track_name, session.track_length_m),
                    session.track_name,
                    session.session_type,
                    session.game_version,
                    session_signature(session),
                    now,
                    session.current_time_s,
                    source,
                    session.max_laps,
                    session.track_length_m,
                ],
            )
            .map_err(|error| format!("failed to save LMU session: {error}"))?;
        Ok(())
    }

    pub fn touch_session(&self, session: &SessionState) -> Result<(), String> {
        self.connection()?
            .execute(
                "UPDATE sessions
                 SET last_seen_ms = ?2, last_session_time_s = ?3, game_version = ?4,
                     signature = ?5, max_laps = ?6, track_length_m = ?7
                 WHERE id = ?1",
                params![
                    session.id,
                    unix_ms() as i64,
                    session.current_time_s,
                    session.game_version,
                    session_signature(session),
                    session.max_laps,
                    session.track_length_m,
                ],
            )
            .map_err(|error| format!("failed to update session heartbeat: {error}"))?;
        Ok(())
    }

    pub fn resumable_session(
        &self,
        signature: &str,
        current_time_s: f64,
        max_age_ms: u64,
    ) -> Result<Option<ResumableSession>, String> {
        let now = unix_ms();
        let cutoff = now.saturating_sub(max_age_ms) as i64;
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT id, last_session_time_s, last_seen_ms FROM sessions
                 WHERE signature = ?1 AND last_seen_ms >= ?2
                 ORDER BY last_seen_ms DESC, rowid DESC LIMIT 1",
            )
            .map_err(|error| format!("failed to prepare resumable session query: {error}"))?;
        let candidates = statement
            .query_map(params![signature, cutoff], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, f64>(1)?,
                    row.get::<_, i64>(2)?.max(0) as u64,
                ))
            })
            .map_err(|error| format!("failed to find resumable session: {error}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("failed to decode resumable session: {error}"))?;
        Ok(candidates
            .into_iter()
            .find(|(_, last_session_time_s, last_seen_ms)| {
                resume_clocks_match(*last_session_time_s, current_time_s, *last_seen_ms, now)
            })
            .map(|(id, last_session_time_s, last_seen_ms)| ResumableSession {
                id,
                last_session_time_s,
                last_seen_ms,
            }))
    }

    pub fn latest_incomplete_player_lap(
        &self,
        session_id: &str,
    ) -> Result<Option<SavedLap>, String> {
        let connection = self.connection()?;
        let id = connection
            .query_row(
                "SELECT l.id FROM laps l
                 JOIN lap_vehicles v ON v.lap_id = l.id
                 WHERE l.session_id = ?1 AND l.completed = 0 AND v.is_player = 1
                 ORDER BY l.created_at_ms DESC, l.lap_number DESC LIMIT 1",
                params![session_id],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|error| format!("failed to find resumable player lap: {error}"))?;
        drop(connection);
        match id {
            Some(id) => self.load_lap(&id),
            None => Ok(None),
        }
    }

    pub fn latest_session(&self) -> Result<Option<StoredSession>, String> {
        self.connection()?
            .query_row(
                "SELECT id, track_name, session_type, track_length_m, started_at_ms
                 FROM sessions
                 ORDER BY started_at_ms DESC, rowid DESC LIMIT 1",
                [],
                |row| {
                    Ok(StoredSession {
                        id: row.get(0)?,
                        track_name: row.get(1)?,
                        session_type: row.get(2)?,
                        track_length_m: row.get(3)?,
                        started_at_unix_ms: row.get::<_, i64>(4)?.max(0) as u64,
                    })
                },
            )
            .optional()
            .map_err(|error| format!("failed to find latest session: {error}"))
    }

    #[cfg(test)]
    pub fn latest_session_of_type(
        &self,
        session_type: &str,
    ) -> Result<Option<StoredSession>, String> {
        self.connection()?
            .query_row(
                "SELECT id, track_name, session_type, track_length_m, started_at_ms
                 FROM sessions
                 WHERE lower(trim(session_type)) = lower(trim(?1))
                 ORDER BY started_at_ms DESC, rowid DESC LIMIT 1",
                params![session_type],
                |row| {
                    Ok(StoredSession {
                        id: row.get(0)?,
                        track_name: row.get(1)?,
                        session_type: row.get(2)?,
                        track_length_m: row.get(3)?,
                        started_at_unix_ms: row.get::<_, i64>(4)?.max(0) as u64,
                    })
                },
            )
            .optional()
            .map_err(|error| format!("failed to find latest {session_type} session: {error}"))
    }

    pub fn apply_retention(
        &self,
        raw_retention_days: Option<u64>,
        analysis_retention_days: Option<u64>,
    ) -> Result<(), String> {
        if raw_retention_days.is_none() && analysis_retention_days.is_none() {
            return Ok(());
        }
        let mut connection = self.connection()?;
        let transaction = connection
            .transaction()
            .map_err(|error| format!("failed to start retention transaction: {error}"))?;
        if let Some(days) = raw_retention_days {
            let cutoff = retention_cutoff(days);
            transaction
                .execute(
                    "DELETE FROM telemetry_samples
                     WHERE lap_id IN (
                        SELECT l.id FROM laps l
                        JOIN sessions s ON s.id = l.session_id
                        WHERE s.started_at_ms < ?1
                     )",
                    params![cutoff],
                )
                .map_err(|error| format!("failed to prune expired raw telemetry: {error}"))?;
        }
        if let Some(days) = analysis_retention_days {
            let cutoff = retention_cutoff(days);
            transaction
                .execute(
                    "DELETE FROM telemetry_samples
                     WHERE lap_id IN (
                        SELECT l.id FROM laps l
                        JOIN sessions s ON s.id = l.session_id
                        WHERE s.started_at_ms < ?1
                     )",
                    params![cutoff],
                )
                .map_err(|error| format!("failed to prune expired telemetry: {error}"))?;
            transaction
                .execute(
                    "DELETE FROM lap_vehicles
                     WHERE lap_id IN (
                        SELECT l.id FROM laps l
                        JOIN sessions s ON s.id = l.session_id
                        WHERE s.started_at_ms < ?1
                     )",
                    params![cutoff],
                )
                .map_err(|error| format!("failed to prune expired lap vehicles: {error}"))?;
            transaction
                .execute(
                    "DELETE FROM laps WHERE session_id IN (
                        SELECT id FROM sessions WHERE started_at_ms < ?1
                     )",
                    params![cutoff],
                )
                .map_err(|error| format!("failed to prune expired lap summaries: {error}"))?;
            transaction
                .execute(
                    "DELETE FROM contacts WHERE session_id IN (
                        SELECT id FROM sessions WHERE started_at_ms < ?1
                     )",
                    params![cutoff],
                )
                .map_err(|error| format!("failed to prune expired contacts: {error}"))?;
            transaction
                .execute(
                    "DELETE FROM sessions WHERE started_at_ms < ?1",
                    params![cutoff],
                )
                .map_err(|error| format!("failed to prune expired sessions: {error}"))?;
        }
        transaction
            .commit()
            .map_err(|error| format!("failed to commit retention policy: {error}"))?;
        Ok(())
    }

    pub fn optimize(&self) -> Result<(), String> {
        self.connection()?
            .execute_batch("PRAGMA optimize; PRAGMA wal_checkpoint(PASSIVE);")
            .map_err(|error| format!("failed to optimize dashboard database: {error}"))?;
        Ok(())
    }

    pub fn save_lap(&self, lap: &SavedLap) -> Result<(), String> {
        let mut connection = self.connection()?;
        let transaction = connection
            .transaction()
            .map_err(|error| format!("failed to start lap transaction: {error}"))?;
        let quality_json = serde_json::to_string(&lap.summary.quality)
            .map_err(|error| format!("failed to encode lap quality: {error}"))?;
        let logical_key = format!(
            "{}:{}:{}",
            lap.summary.session_id, lap.summary.vehicle_id, lap.summary.lap_number
        );
        let existing = transaction
            .query_row(
                "SELECT id, completed, quality_json, sample_count, valid, created_at_ms
                 FROM laps WHERE logical_key = ?1",
                params![logical_key],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, bool>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, i64>(3)?.max(0) as usize,
                        row.get::<_, bool>(4)?,
                        row.get::<_, i64>(5)?.max(0) as u64,
                    ))
                },
            )
            .optional()
            .map_err(|error| format!("failed to inspect existing logical lap: {error}"))?;
        if let Some((_, completed, existing_quality, sample_count, valid, created_at_ms)) =
            &existing
            && !should_replace_lap(
                *completed,
                &decode_quality(existing_quality, *valid),
                *sample_count,
                *created_at_ms,
                lap,
            )
        {
            return Ok(());
        }
        if let Some((existing_id, _, _, _, _, _)) = &existing
            && existing_id != &lap.summary.id
        {
            transaction
                .execute(
                    "DELETE FROM telemetry_samples WHERE lap_id = ?1",
                    params![existing_id],
                )
                .map_err(|error| format!("failed to remove superseded lap samples: {error}"))?;
            transaction
                .execute(
                    "DELETE FROM lap_vehicles WHERE lap_id = ?1",
                    params![existing_id],
                )
                .map_err(|error| format!("failed to remove superseded lap vehicle: {error}"))?;
            transaction
                .execute("DELETE FROM laps WHERE id = ?1", params![existing_id])
                .map_err(|error| format!("failed to remove superseded lap: {error}"))?;
        }
        transaction
            .execute(
                "INSERT OR REPLACE INTO laps
                 (id, session_id, track_name, lap_number, lap_time_ms, valid, sample_count,
                  created_at_ms, completed, quality_json, logical_key)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    lap.summary.id,
                    lap.summary.session_id,
                    lap.summary.track_name,
                    lap.summary.lap_number,
                    lap.summary.lap_time_ms,
                    lap.summary.valid,
                    lap.summary.sample_count as i64,
                    lap.summary.created_at_unix_ms as i64,
                    lap.summary.completed,
                    quality_json,
                    logical_key,
                ],
            )
            .map_err(|error| format!("failed to save lap metadata: {error}"))?;
        transaction
            .execute(
                "INSERT OR REPLACE INTO lap_vehicles
                 (lap_id, vehicle_id, driver_name, class_name, is_player, overall_position,
                  class_position)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    lap.summary.id,
                    lap.summary.vehicle_id,
                    lap.summary.driver_name,
                    lap.summary.class_name,
                    lap.summary.is_player,
                    lap.summary.overall_position,
                    lap.summary.class_position,
                ],
            )
            .map_err(|error| format!("failed to save lap vehicle metadata: {error}"))?;
        transaction
            .execute(
                "DELETE FROM telemetry_samples WHERE lap_id = ?1",
                params![lap.summary.id],
            )
            .map_err(|error| format!("failed to replace lap samples: {error}"))?;

        {
            let mut statement = transaction
                .prepare_cached(
                    "INSERT INTO telemetry_samples
                     (lap_id, seq, session_time_s, lap_elapsed_s, lap_distance_m, world_x,
                      world_z, speed_kmh, rpm, gear, throttle, brake, steer, clutch, lateral_g,
                      longitudinal_g)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14,
                             ?15, ?16)",
                )
                .map_err(|error| format!("failed to prepare lap sample insert: {error}"))?;
            for (sequence, sample) in lap.samples.iter().enumerate() {
                statement
                    .execute(params![
                        lap.summary.id,
                        sequence as i64,
                        sample.session_time_s,
                        sample.lap_elapsed_s,
                        sample.lap_distance_m,
                        sample.x,
                        sample.z,
                        sample.speed_kmh,
                        sample.rpm,
                        sample.gear,
                        sample.throttle,
                        sample.brake,
                        sample.steer,
                        sample.clutch,
                        sample.lateral_g,
                        sample.longitudinal_g,
                    ])
                    .map_err(|error| format!("failed to save lap sample: {error}"))?;
            }
        }

        transaction
            .commit()
            .map_err(|error| format!("failed to commit lap transaction: {error}"))?;
        Ok(())
    }

    pub fn load_logical_lap(
        &self,
        session_id: &str,
        vehicle_id: i32,
        lap_number: i32,
    ) -> Result<Option<SavedLap>, String> {
        let connection = self.connection()?;
        let id = connection
            .query_row(
                "SELECT id FROM laps WHERE logical_key = ?1",
                params![format!("{session_id}:{vehicle_id}:{lap_number}")],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|error| format!("failed to find logical lap: {error}"))?;
        drop(connection);
        match id {
            Some(id) => self.load_lap(&id),
            None => Ok(None),
        }
    }

    pub fn list_laps(&self) -> Result<Vec<LapSummary>, String> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT l.id, l.session_id, l.track_name, l.lap_number, l.lap_time_ms, l.valid,
                        l.sample_count, l.created_at_ms, COALESCE(v.vehicle_id, 0),
                        COALESCE(v.driver_name, ''), COALESCE(v.class_name, ''),
                        COALESCE(v.is_player, 0), COALESCE(v.overall_position, 0),
                        COALESCE(v.class_position, 0), COALESCE(s.session_type, ''),
                        l.completed, l.quality_json, COALESCE(s.track_length_m, 0)
                 FROM laps l
                 LEFT JOIN lap_vehicles v ON v.lap_id = l.id
                 LEFT JOIN sessions s ON s.id = l.session_id
                 ORDER BY l.created_at_ms DESC, l.lap_number DESC",
            )
            .map_err(|error| format!("failed to query saved laps: {error}"))?;
        let rows = statement
            .query_map([], |row| {
                let valid = row.get(5)?;
                let quality_json: String = row.get(16)?;
                Ok(LapSummary {
                    id: row.get(0)?,
                    session_id: row.get(1)?,
                    track_name: row.get(2)?,
                    session_type: row.get(14)?,
                    track_length_m: row.get(17)?,
                    vehicle_id: row.get(8)?,
                    driver_name: row.get(9)?,
                    class_name: row.get(10)?,
                    is_player: row.get(11)?,
                    overall_position: row.get(12)?,
                    class_position: row.get(13)?,
                    lap_number: row.get(3)?,
                    lap_time_ms: row.get::<_, i64>(4)?.max(0) as u32,
                    valid,
                    quality: decode_quality(&quality_json, valid),
                    sample_count: row.get::<_, i64>(6)?.max(0) as usize,
                    created_at_unix_ms: row.get::<_, i64>(7)?.max(0) as u64,
                    completed: row.get(15)?,
                })
            })
            .map_err(|error| format!("failed to read saved laps: {error}"))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("failed to decode saved laps: {error}"))
    }

    pub fn load_lap(&self, id: &str) -> Result<Option<SavedLap>, String> {
        let connection = self.connection()?;
        let summary = connection
            .query_row(
                "SELECT l.id, l.session_id, l.track_name, l.lap_number, l.lap_time_ms, l.valid,
                        l.sample_count, l.created_at_ms, COALESCE(v.vehicle_id, 0),
                        COALESCE(v.driver_name, ''), COALESCE(v.class_name, ''),
                        COALESCE(v.is_player, 0), COALESCE(v.overall_position, 0),
                        COALESCE(v.class_position, 0), COALESCE(s.session_type, ''),
                        l.completed, l.quality_json, COALESCE(s.track_length_m, 0)
                 FROM laps l
                 LEFT JOIN lap_vehicles v ON v.lap_id = l.id
                 LEFT JOIN sessions s ON s.id = l.session_id
                 WHERE l.id = ?1",
                params![id],
                |row| {
                    let valid = row.get(5)?;
                    let quality_json: String = row.get(16)?;
                    Ok(LapSummary {
                        id: row.get(0)?,
                        session_id: row.get(1)?,
                        track_name: row.get(2)?,
                        session_type: row.get(14)?,
                        track_length_m: row.get(17)?,
                        vehicle_id: row.get(8)?,
                        driver_name: row.get(9)?,
                        class_name: row.get(10)?,
                        is_player: row.get(11)?,
                        overall_position: row.get(12)?,
                        class_position: row.get(13)?,
                        lap_number: row.get(3)?,
                        lap_time_ms: row.get::<_, i64>(4)?.max(0) as u32,
                        valid,
                        quality: decode_quality(&quality_json, valid),
                        sample_count: row.get::<_, i64>(6)?.max(0) as usize,
                        created_at_unix_ms: row.get::<_, i64>(7)?.max(0) as u64,
                        completed: row.get(15)?,
                    })
                },
            )
            .optional()
            .map_err(|error| format!("failed to load lap {id}: {error}"))?;
        let Some(summary) = summary else {
            return Ok(None);
        };

        let mut statement = connection
            .prepare(
                "SELECT session_time_s, lap_elapsed_s, lap_distance_m, world_x, world_z,
                        speed_kmh, rpm, gear, throttle, brake, steer, clutch, lateral_g,
                        longitudinal_g
                 FROM telemetry_samples WHERE lap_id = ?1 ORDER BY seq",
            )
            .map_err(|error| format!("failed to query samples for lap {id}: {error}"))?;
        let rows = statement
            .query_map(params![id], |row| {
                Ok(TelemetryPoint {
                    session_time_s: row.get(0)?,
                    lap_elapsed_s: row.get(1)?,
                    lap_distance_m: row.get(2)?,
                    x: row.get(3)?,
                    z: row.get(4)?,
                    speed_kmh: row.get(5)?,
                    rpm: row.get(6)?,
                    gear: row.get(7)?,
                    throttle: row.get(8)?,
                    brake: row.get(9)?,
                    steer: row.get(10)?,
                    clutch: row.get(11)?,
                    lateral_g: row.get(12)?,
                    longitudinal_g: row.get(13)?,
                })
            })
            .map_err(|error| format!("failed to read samples for lap {id}: {error}"))?;
        let samples = rows
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("failed to decode samples for lap {id}: {error}"))?;
        Ok(Some(SavedLap { summary, samples }))
    }

    pub fn save_contact(&self, contact: &ContactEvent) -> Result<(), String> {
        self.connection()?
            .execute(
                "INSERT OR IGNORE INTO contacts
                 (id, session_id, track_name, session_time_s, car_a_id, car_a_name, car_a_class,
                  car_a_position, car_a_lap, car_b_id, car_b_name, car_b_class, car_b_position,
                  car_b_lap, magnitude_a, magnitude_b, world_x, world_z, confidence,
                  created_at_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14,
                         ?15, ?16, ?17, ?18, ?19, ?20)",
                params![
                    contact.id,
                    contact.session_id,
                    contact.track_name,
                    contact.session_time_s,
                    contact.car_a.vehicle_id,
                    contact.car_a.driver_name,
                    contact.car_a.class_name,
                    contact.car_a.position,
                    contact.car_a.lap_number,
                    contact.car_b.as_ref().map(|car| car.vehicle_id),
                    contact.car_b.as_ref().map(|car| car.driver_name.as_str()),
                    contact.car_b.as_ref().map(|car| car.class_name.as_str()),
                    contact.car_b.as_ref().map(|car| car.position),
                    contact.car_b.as_ref().map(|car| car.lap_number),
                    contact.magnitude_a,
                    contact.magnitude_b,
                    contact.position.x,
                    contact.position.z,
                    confidence_name(&contact.confidence),
                    contact.created_at_unix_ms as i64,
                ],
            )
            .map_err(|error| format!("failed to save contact event: {error}"))?;
        Ok(())
    }

    pub fn recent_contacts(&self, limit: usize) -> Result<Vec<ContactEvent>, String> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT id, session_id, track_name, session_time_s,
                        car_a_id, car_a_name, car_a_class, car_a_position, car_a_lap,
                        car_b_id, car_b_name, car_b_class, car_b_position, car_b_lap,
                        magnitude_a, magnitude_b, world_x, world_z, confidence, created_at_ms
                 FROM contacts ORDER BY created_at_ms DESC LIMIT ?1",
            )
            .map_err(|error| format!("failed to query contact events: {error}"))?;
        let rows = statement
            .query_map(params![limit as i64], |row| {
                let car_b_id: Option<i32> = row.get(9)?;
                Ok(ContactEvent {
                    id: row.get(0)?,
                    session_id: row.get(1)?,
                    track_name: row.get(2)?,
                    session_time_s: row.get(3)?,
                    car_a: ContactParticipant {
                        vehicle_id: row.get(4)?,
                        driver_name: row.get(5)?,
                        class_name: row.get(6)?,
                        position: row.get(7)?,
                        lap_number: row.get(8)?,
                    },
                    car_b: car_b_id.map(|vehicle_id| ContactParticipant {
                        vehicle_id,
                        driver_name: row
                            .get::<_, Option<String>>(10)
                            .ok()
                            .flatten()
                            .unwrap_or_default(),
                        class_name: row
                            .get::<_, Option<String>>(11)
                            .ok()
                            .flatten()
                            .unwrap_or_default(),
                        position: row
                            .get::<_, Option<u8>>(12)
                            .ok()
                            .flatten()
                            .unwrap_or_default(),
                        lap_number: row
                            .get::<_, Option<i16>>(13)
                            .ok()
                            .flatten()
                            .unwrap_or_default(),
                    }),
                    magnitude_a: row.get(14)?,
                    magnitude_b: row.get(15)?,
                    position: crate::model::Point2 {
                        x: row.get(16)?,
                        z: row.get(17)?,
                    },
                    confidence: parse_confidence(&row.get::<_, String>(18)?),
                    created_at_unix_ms: row.get::<_, i64>(19)?.max(0) as u64,
                })
            })
            .map_err(|error| format!("failed to read contact events: {error}"))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("failed to decode contact events: {error}"))
    }

    pub fn load_track(&self, key: &str) -> Result<Vec<TrackPoint>, String> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT lap_distance_m, world_x, world_z, samples
                 FROM track_points WHERE track_key = ?1 ORDER BY seq",
            )
            .map_err(|error| format!("failed to query track map: {error}"))?;
        let rows = statement
            .query_map(params![key], |row| {
                Ok(TrackPoint {
                    lap_distance_m: row.get(0)?,
                    x: row.get(1)?,
                    z: row.get(2)?,
                    samples: row.get::<_, i64>(3)?.max(0) as u32,
                })
            })
            .map_err(|error| format!("failed to read track map: {error}"))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("failed to decode track map: {error}"))
    }

    pub fn save_track(
        &self,
        key: &str,
        track_name: &str,
        track_length_m: f64,
        points: &[TrackPoint],
    ) -> Result<(), String> {
        let mut connection = self.connection()?;
        let transaction = connection
            .transaction()
            .map_err(|error| format!("failed to start track transaction: {error}"))?;
        transaction
            .execute(
                "INSERT OR REPLACE INTO tracks (track_key, name, length_m) VALUES (?1, ?2, ?3)",
                params![key, track_name, track_length_m],
            )
            .map_err(|error| format!("failed to save track metadata: {error}"))?;
        transaction
            .execute(
                "DELETE FROM track_points WHERE track_key = ?1",
                params![key],
            )
            .map_err(|error| format!("failed to replace track map: {error}"))?;
        {
            let mut statement = transaction
                .prepare_cached(
                    "INSERT INTO track_points
                     (track_key, seq, lap_distance_m, world_x, world_z, samples)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                )
                .map_err(|error| format!("failed to prepare track point insert: {error}"))?;
            for (sequence, point) in points.iter().enumerate() {
                statement
                    .execute(params![
                        key,
                        sequence as i64,
                        point.lap_distance_m,
                        point.x,
                        point.z,
                        point.samples,
                    ])
                    .map_err(|error| format!("failed to save track point: {error}"))?;
            }
        }
        transaction
            .commit()
            .map_err(|error| format!("failed to commit track map: {error}"))?;
        Ok(())
    }

    fn initialize(&self) -> Result<(), String> {
        let mut connection = self.connection()?;
        let version = schema_version(&connection)?;
        if version > SCHEMA_VERSION {
            return Err(format!(
                "dashboard database schema {version} is newer than supported {SCHEMA_VERSION}"
            ));
        }
        if version < 1 {
            migrate_v0_to_v1(&mut connection)?;
        }
        if schema_version(&connection)? < 2 {
            migrate_v1_to_v2(&mut connection)?;
        }
        if schema_version(&connection)? < 3 {
            migrate_v2_to_v3(&mut connection)?;
        }
        Ok(())
    }

    fn connection(&self) -> Result<Connection, String> {
        let connection = Connection::open(&self.database_path).map_err(|error| {
            format!(
                "failed to open dashboard database {}: {error}",
                self.database_path.display()
            )
        })?;
        connection
            .busy_timeout(Duration::from_secs(5))
            .map_err(|error| format!("failed to set SQLite busy timeout: {error}"))?;
        connection
            .pragma_update(None, "journal_mode", "WAL")
            .map_err(|error| format!("failed to enable SQLite WAL mode: {error}"))?;
        connection
            .pragma_update(None, "synchronous", "NORMAL")
            .map_err(|error| format!("failed to set SQLite synchronous mode: {error}"))?;
        connection
            .pragma_update(None, "foreign_keys", true)
            .map_err(|error| format!("failed to enable SQLite foreign keys: {error}"))?;
        Ok(connection)
    }
}

fn schema_version(connection: &Connection) -> Result<u32, String> {
    connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(|error| format!("failed to read dashboard schema version: {error}"))
}

fn migrate_v0_to_v1(connection: &mut Connection) -> Result<(), String> {
    let transaction = connection
        .transaction()
        .map_err(|error| format!("failed to start schema v1 migration: {error}"))?;
    transaction
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS sessions (
                id TEXT PRIMARY KEY,
                started_at_ms INTEGER NOT NULL,
                track_key TEXT NOT NULL,
                track_name TEXT NOT NULL,
                session_type TEXT NOT NULL,
                game_version INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS laps (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                track_name TEXT NOT NULL,
                lap_number INTEGER NOT NULL,
                lap_time_ms INTEGER NOT NULL,
                valid INTEGER NOT NULL,
                sample_count INTEGER NOT NULL,
                created_at_ms INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS telemetry_samples (
                lap_id TEXT NOT NULL,
                seq INTEGER NOT NULL,
                session_time_s REAL NOT NULL,
                lap_elapsed_s REAL NOT NULL,
                lap_distance_m REAL NOT NULL,
                world_x REAL NOT NULL,
                world_z REAL NOT NULL,
                speed_kmh REAL NOT NULL,
                rpm REAL NOT NULL,
                gear INTEGER NOT NULL,
                throttle REAL NOT NULL,
                brake REAL NOT NULL,
                steer REAL NOT NULL,
                clutch REAL NOT NULL,
                lateral_g REAL NOT NULL,
                longitudinal_g REAL NOT NULL,
                PRIMARY KEY (lap_id, seq)
             );
             CREATE TABLE IF NOT EXISTS lap_vehicles (
                lap_id TEXT PRIMARY KEY,
                vehicle_id INTEGER NOT NULL,
                driver_name TEXT NOT NULL,
                class_name TEXT NOT NULL,
                is_player INTEGER NOT NULL,
                overall_position INTEGER NOT NULL,
                class_position INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS contacts (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                track_name TEXT NOT NULL,
                session_time_s REAL NOT NULL,
                car_a_id INTEGER NOT NULL,
                car_a_name TEXT NOT NULL,
                car_a_class TEXT NOT NULL,
                car_a_position INTEGER NOT NULL,
                car_a_lap INTEGER NOT NULL,
                car_b_id INTEGER,
                car_b_name TEXT,
                car_b_class TEXT,
                car_b_position INTEGER,
                car_b_lap INTEGER,
                magnitude_a REAL NOT NULL,
                magnitude_b REAL,
                world_x REAL NOT NULL,
                world_z REAL NOT NULL,
                confidence TEXT NOT NULL,
                created_at_ms INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS tracks (
                track_key TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                length_m REAL NOT NULL
             );
             CREATE TABLE IF NOT EXISTS track_points (
                track_key TEXT NOT NULL,
                seq INTEGER NOT NULL,
                lap_distance_m REAL NOT NULL,
                world_x REAL NOT NULL,
                world_z REAL NOT NULL,
                samples INTEGER NOT NULL,
                PRIMARY KEY (track_key, seq)
             );
             CREATE INDEX IF NOT EXISTS telemetry_lap_idx
                ON telemetry_samples (lap_id, seq);
             CREATE INDEX IF NOT EXISTS lap_vehicles_class_idx
                ON lap_vehicles (class_name, vehicle_id);
             CREATE INDEX IF NOT EXISTS contacts_created_idx
                ON contacts (created_at_ms DESC);",
        )
        .map_err(|error| format!("failed to apply schema v1: {error}"))?;
    transaction
        .pragma_update(None, "user_version", 1)
        .map_err(|error| format!("failed to record schema v1: {error}"))?;
    transaction
        .commit()
        .map_err(|error| format!("failed to commit schema v1: {error}"))
}

fn migrate_v1_to_v2(connection: &mut Connection) -> Result<(), String> {
    let transaction = connection
        .transaction()
        .map_err(|error| format!("failed to start schema v2 migration: {error}"))?;
    for (table, column, definition) in [
        ("sessions", "signature", "TEXT NOT NULL DEFAULT ''"),
        ("sessions", "last_seen_ms", "INTEGER NOT NULL DEFAULT 0"),
        ("sessions", "last_session_time_s", "REAL NOT NULL DEFAULT 0"),
        ("sessions", "source", "TEXT NOT NULL DEFAULT ''"),
        ("sessions", "max_laps", "INTEGER NOT NULL DEFAULT 0"),
        ("sessions", "track_length_m", "REAL NOT NULL DEFAULT 0"),
        ("laps", "completed", "INTEGER NOT NULL DEFAULT 1"),
        ("laps", "quality_json", "TEXT NOT NULL DEFAULT '{}'"),
        ("laps", "logical_key", "TEXT"),
    ] {
        ensure_column(&transaction, table, column, definition)?;
    }
    transaction
        .execute(
            "UPDATE laps
             SET logical_key = session_id || ':' || COALESCE(
                    (SELECT vehicle_id FROM lap_vehicles WHERE lap_id = laps.id), 0
                 ) || ':' || lap_number
             WHERE logical_key IS NULL OR logical_key = ''",
            [],
        )
        .map_err(|error| format!("failed to backfill logical lap keys: {error}"))?;
    deduplicate_logical_keys(&transaction)?;
    transaction
        .execute_batch(
            "DROP INDEX IF EXISTS telemetry_lap_idx;
             CREATE INDEX IF NOT EXISTS sessions_resume_idx
                ON sessions (signature, last_seen_ms DESC);
             CREATE INDEX IF NOT EXISTS laps_session_idx
                ON laps (session_id, lap_number, created_at_ms DESC);
             CREATE UNIQUE INDEX IF NOT EXISTS laps_logical_key_idx
                ON laps (logical_key) WHERE logical_key IS NOT NULL;",
        )
        .map_err(|error| format!("failed to apply schema v2 indexes: {error}"))?;
    transaction
        .pragma_update(None, "user_version", 2)
        .map_err(|error| format!("failed to record schema v2: {error}"))?;
    transaction
        .commit()
        .map_err(|error| format!("failed to commit schema v2: {error}"))
}

fn migrate_v2_to_v3(connection: &mut Connection) -> Result<(), String> {
    let transaction = connection
        .transaction()
        .map_err(|error| format!("failed to start schema v3 migration: {error}"))?;
    deduplicate_logical_keys(&transaction)?;
    transaction
        .execute_batch(
            "CREATE UNIQUE INDEX IF NOT EXISTS laps_logical_key_idx
                ON laps (logical_key) WHERE logical_key IS NOT NULL;
             CREATE INDEX IF NOT EXISTS contacts_session_idx
                ON contacts (session_id, created_at_ms DESC);
             CREATE INDEX IF NOT EXISTS sessions_started_idx
                ON sessions (started_at_ms DESC);",
        )
        .map_err(|error| format!("failed to apply schema v3 indexes: {error}"))?;
    transaction
        .pragma_update(None, "user_version", 3)
        .map_err(|error| format!("failed to record schema v3: {error}"))?;
    transaction
        .commit()
        .map_err(|error| format!("failed to commit schema v3: {error}"))
}

fn deduplicate_logical_keys(transaction: &Transaction<'_>) -> Result<(), String> {
    let keys = {
        let mut statement = transaction
            .prepare(
                "SELECT logical_key FROM laps
                 WHERE logical_key IS NOT NULL
                 GROUP BY logical_key HAVING COUNT(*) > 1",
            )
            .map_err(|error| format!("failed to inspect duplicate logical laps: {error}"))?;
        statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|error| format!("failed to query duplicate logical laps: {error}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("failed to decode duplicate logical laps: {error}"))?
    };
    for key in keys {
        let candidates = {
            let mut statement = transaction
                .prepare(
                    "SELECT id, completed, quality_json, sample_count, valid, created_at_ms
                     FROM laps WHERE logical_key = ?1",
                )
                .map_err(|error| format!("failed to inspect logical lap {key}: {error}"))?;
            statement
                .query_map(params![key], |row| {
                    let valid = row.get::<_, bool>(4)?;
                    let quality_json = row
                        .get::<_, Option<String>>(2)?
                        .unwrap_or_else(|| "{}".to_owned());
                    let quality = decode_quality(&quality_json, valid);
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, bool>(1)?,
                        quality_rank(quality.status),
                        quality.score,
                        row.get::<_, i64>(3)?.max(0) as usize,
                        row.get::<_, i64>(5)?.max(0),
                    ))
                })
                .map_err(|error| format!("failed to query logical lap {key}: {error}"))?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| format!("failed to decode logical lap {key}: {error}"))?
        };
        let Some(keeper) = candidates.iter().max_by_key(|candidate| {
            (
                candidate.1,
                candidate.2,
                candidate.3,
                candidate.4,
                candidate.5,
            )
        }) else {
            continue;
        };
        for candidate in &candidates {
            if candidate.0 != keeper.0 {
                transaction
                    .execute(
                        "DELETE FROM telemetry_samples WHERE lap_id = ?1",
                        params![candidate.0],
                    )
                    .map_err(|error| {
                        format!(
                            "failed to remove duplicate logical lap samples {}: {error}",
                            candidate.0
                        )
                    })?;
                transaction
                    .execute(
                        "DELETE FROM lap_vehicles WHERE lap_id = ?1",
                        params![candidate.0],
                    )
                    .map_err(|error| {
                        format!(
                            "failed to remove duplicate logical lap vehicle {}: {error}",
                            candidate.0
                        )
                    })?;
                transaction
                    .execute("DELETE FROM laps WHERE id = ?1", params![candidate.0])
                    .map_err(|error| {
                        format!(
                            "failed to remove duplicate logical lap {}: {error}",
                            candidate.0
                        )
                    })?;
            }
        }
    }
    Ok(())
}

fn ensure_column(
    connection: &Connection,
    table: &str,
    column: &str,
    definition: &str,
) -> Result<(), String> {
    let mut statement = connection
        .prepare(&format!("PRAGMA table_info({table})"))
        .map_err(|error| format!("failed to inspect {table}: {error}"))?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|error| format!("failed to read {table} columns: {error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("failed to decode {table} columns: {error}"))?;
    if !columns.iter().any(|value| value == column) {
        connection
            .execute_batch(&format!(
                "ALTER TABLE {table} ADD COLUMN {column} {definition};"
            ))
            .map_err(|error| format!("failed to add {table}.{column}: {error}"))?;
    }
    Ok(())
}

fn decode_quality(json: &str, legacy_valid: bool) -> TraceQuality {
    serde_json::from_str(json).unwrap_or_else(|_| TraceQuality {
        status: if legacy_valid {
            TraceQualityStatus::Unknown
        } else {
            TraceQualityStatus::Rejected
        },
        score: if legacy_valid { 50 } else { 0 },
        ..TraceQuality::default()
    })
}

fn should_replace_lap(
    existing_completed: bool,
    existing_quality: &TraceQuality,
    existing_samples: usize,
    existing_created_at_ms: u64,
    incoming: &SavedLap,
) -> bool {
    if incoming.summary.completed != existing_completed {
        return incoming.summary.completed;
    }
    if !existing_completed {
        return incoming.samples.len() > existing_samples
            || (incoming.samples.len() == existing_samples
                && incoming.summary.created_at_unix_ms >= existing_created_at_ms);
    }
    let existing_rank = quality_rank(existing_quality.status);
    let incoming_rank = quality_rank(incoming.summary.quality.status);
    incoming_rank > existing_rank
        || (incoming_rank == existing_rank
            && (incoming.summary.quality.score > existing_quality.score
                || (incoming.summary.quality.score == existing_quality.score
                    && incoming.samples.len() > existing_samples)))
}

fn quality_rank(status: TraceQualityStatus) -> u8 {
    match status {
        TraceQualityStatus::Valid => 3,
        TraceQualityStatus::Partial => 2,
        TraceQualityStatus::Unknown => 1,
        TraceQualityStatus::Rejected => 0,
    }
}

pub fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis() as u64)
}

fn retention_cutoff(days: u64) -> i64 {
    unix_ms()
        .saturating_sub(days.saturating_mul(24 * 60 * 60 * 1_000))
        .min(i64::MAX as u64) as i64
}

fn resume_clocks_match(
    last_session_time_s: f64,
    current_session_time_s: f64,
    last_seen_ms: u64,
    now_ms: u64,
) -> bool {
    if !last_session_time_s.is_finite() || !current_session_time_s.is_finite() {
        return false;
    }
    let wall_elapsed_s = now_ms.saturating_sub(last_seen_ms) as f64 / 1_000.0;
    let session_advance_s = current_session_time_s - last_session_time_s;
    session_advance_s >= -RESUME_CLOCK_BACKWARD_TOLERANCE_S
        && session_advance_s <= wall_elapsed_s + RESUME_CLOCK_TOLERANCE_S
}

pub fn session_signature(session: &SessionState) -> String {
    session.identity().fingerprint()
}

pub fn track_key(track_name: &str, track_length_m: f64) -> String {
    crate::telemetry_core::track_key(track_name, track_length_m)
}

fn confidence_name(confidence: &ContactConfidence) -> &'static str {
    match confidence {
        ContactConfidence::Confirmed => "confirmed",
        ContactConfidence::Probable => "probable",
        ContactConfidence::Unresolved => "unresolved",
    }
}

fn parse_confidence(value: &str) -> ContactConfidence {
    match value {
        "confirmed" => ContactConfidence::Confirmed,
        "probable" => ContactConfidence::Probable,
        _ => ContactConfidence::Unresolved,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Point2, TelemetryPoint};

    #[test]
    fn creates_stable_track_keys() {
        assert_eq!(track_key("Le Mans 2024", 13_626.2), "le-mans-2024-13626");
        assert_eq!(
            track_key("Spa--Francorchamps", 7_004.8),
            "spa-francorchamps-7005"
        );
    }

    #[test]
    fn persists_laps_contacts_and_track_points() {
        let directory = std::env::temp_dir().join(format!(
            "lmu-dashboard-store-test-{}-{}",
            std::process::id(),
            unix_ms()
        ));
        let store = DashboardStore::open(&directory).unwrap();
        let session = SessionState {
            id: "session-1".to_owned(),
            track_name: "Circuit de la Sarthe".to_owned(),
            session_type: "Race".to_owned(),
            track_length_m: 13_626.0,
            ..SessionState::default()
        };
        store.save_session(&session).unwrap();

        let lap = SavedLap {
            summary: LapSummary {
                id: "session-1-lap-3".to_owned(),
                session_id: session.id.clone(),
                track_name: session.track_name.clone(),
                session_type: session.session_type.clone(),
                track_length_m: session.track_length_m,
                vehicle_id: 7,
                driver_name: "Driver A".to_owned(),
                class_name: "Hypercar".to_owned(),
                is_player: true,
                overall_position: 3,
                class_position: 2,
                lap_number: 3,
                lap_time_ms: 218_432,
                valid: true,
                quality: TraceQuality::default(),
                sample_count: 1,
                created_at_unix_ms: unix_ms(),
                completed: true,
            },
            samples: vec![TelemetryPoint {
                lap_distance_m: 1_234.0,
                speed_kmh: 287.5,
                throttle: 1.0,
                ..TelemetryPoint::default()
            }],
        };
        store.save_lap(&lap).unwrap();
        let listed_laps = store.list_laps().unwrap();
        assert_eq!(listed_laps.len(), 1);
        assert_eq!(listed_laps[0].id, lap.summary.id);
        assert_eq!(listed_laps[0].lap_time_ms, 218_432);
        assert_eq!(listed_laps[0].driver_name, "Driver A");
        assert_eq!(listed_laps[0].class_position, 2);
        assert_eq!(listed_laps[0].session_type, "Race");
        let loaded_lap = store.load_lap(&lap.summary.id).unwrap().unwrap();
        assert_eq!(loaded_lap.summary.id, lap.summary.id);
        assert_eq!(loaded_lap.summary.vehicle_id, 7);
        assert_eq!(loaded_lap.summary.session_type, "Race");
        assert_eq!(loaded_lap.samples.len(), 1);
        assert_eq!(loaded_lap.samples[0].speed_kmh, 287.5);

        let contact = ContactEvent {
            id: "contact-1".to_owned(),
            session_id: session.id.clone(),
            track_name: session.track_name.clone(),
            car_a: ContactParticipant {
                vehicle_id: 7,
                driver_name: "Driver A".to_owned(),
                ..ContactParticipant::default()
            },
            car_b: Some(ContactParticipant {
                vehicle_id: 12,
                driver_name: "Driver B".to_owned(),
                ..ContactParticipant::default()
            }),
            position: Point2 { x: 10.0, z: 20.0 },
            confidence: ContactConfidence::Confirmed,
            created_at_unix_ms: unix_ms(),
            ..ContactEvent::default()
        };
        store.save_contact(&contact).unwrap();
        let contacts = store.recent_contacts(10).unwrap();
        assert_eq!(contacts.len(), 1);
        assert_eq!(contacts[0].car_a.driver_name, "Driver A");
        assert_eq!(
            contacts[0]
                .car_b
                .as_ref()
                .map(|car| car.driver_name.as_str()),
            Some("Driver B")
        );

        let key = track_key(&session.track_name, session.track_length_m);
        let track = vec![TrackPoint {
            lap_distance_m: 100.0,
            x: 3.0,
            z: 4.0,
            samples: 5,
        }];
        store
            .save_track(&key, &session.track_name, session.track_length_m, &track)
            .unwrap();
        let loaded_track = store.load_track(&key).unwrap();
        assert_eq!(loaded_track.len(), 1);
        assert_eq!(loaded_track[0].lap_distance_m, 100.0);
        assert_eq!(loaded_track[0].samples, 5);

        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn reads_laps_created_before_vehicle_metadata_was_added() {
        let directory = std::env::temp_dir().join(format!(
            "lmu-dashboard-legacy-store-test-{}-{}",
            std::process::id(),
            unix_ms()
        ));
        fs::create_dir_all(&directory).unwrap();
        let connection = Connection::open(directory.join("dashboard.sqlite3")).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE laps (
                    id TEXT PRIMARY KEY,
                    session_id TEXT NOT NULL,
                    track_name TEXT NOT NULL,
                    lap_number INTEGER NOT NULL,
                    lap_time_ms INTEGER NOT NULL,
                    valid INTEGER NOT NULL,
                    sample_count INTEGER NOT NULL,
                    created_at_ms INTEGER NOT NULL
                 );
                 INSERT INTO laps VALUES
                    ('legacy-lap', 'legacy-session', 'Le Mans', 2, 220000, 1, 100, 1);",
            )
            .unwrap();
        drop(connection);

        let store = DashboardStore::open(&directory).unwrap();
        let laps = store.list_laps().unwrap();
        assert_eq!(laps.len(), 1);
        assert_eq!(laps[0].id, "legacy-lap");
        assert_eq!(laps[0].vehicle_id, 0);
        assert!(laps[0].driver_name.is_empty());

        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn migrates_v1_to_v3_sequentially_without_recreating_utf8_data() {
        let directory = temporary("sequential-migration");
        fs::create_dir_all(&directory).unwrap();
        let mut connection = Connection::open(directory.join("dashboard.sqlite3")).unwrap();
        migrate_v0_to_v1(&mut connection).unwrap();
        connection
            .execute(
                "INSERT INTO sessions
             (id, started_at_ms, track_key, track_name, session_type, game_version)
             VALUES ('legacy-session', 1, 'silverstone-5891', '실버스톤', 'Qualifying', 13)",
                [],
            )
            .unwrap();
        connection.execute(
            "INSERT INTO laps
             (id, session_id, track_name, lap_number, lap_time_ms, valid, sample_count, created_at_ms)
             VALUES ('legacy-lap', 'legacy-session', '실버스톤', 4, 90123, 1, 25, 2)",
            [],
        ).unwrap();
        drop(connection);

        let store = DashboardStore::open(&directory).unwrap();
        let connection = store.connection().unwrap();
        assert_eq!(schema_version(&connection).unwrap(), 3);
        drop(connection);
        let laps = store.list_laps().unwrap();
        assert_eq!(laps.len(), 1);
        assert_eq!(laps[0].id, "legacy-lap");
        assert_eq!(laps[0].track_name, "실버스톤");
        assert!(
            store
                .load_logical_lap("legacy-session", 0, 4)
                .unwrap()
                .is_some()
        );
        let session = store.latest_session_of_type("Qualifying").unwrap().unwrap();
        assert_eq!(session.track_name, "실버스톤");
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn v3_migration_removes_duplicate_rows_and_their_dependents() {
        let directory = temporary("duplicate-migration");
        fs::create_dir_all(&directory).unwrap();
        let mut connection = Connection::open(directory.join("dashboard.sqlite3")).unwrap();
        migrate_v0_to_v1(&mut connection).unwrap();
        migrate_v1_to_v2(&mut connection).unwrap();
        connection
            .execute_batch("DROP INDEX laps_logical_key_idx;")
            .unwrap();
        let rejected = serde_json::to_string(&TraceQuality {
            status: TraceQualityStatus::Rejected,
            score: 20,
            ..TraceQuality::default()
        })
        .unwrap();
        let valid = serde_json::to_string(&TraceQuality {
            status: TraceQualityStatus::Valid,
            score: 95,
            ..TraceQuality::default()
        })
        .unwrap();
        for (id, completed, quality, samples, valid_flag, created) in [
            ("weaker", false, rejected, 21_i64, false, 1_i64),
            ("keeper", true, valid, 100_i64, true, 2_i64),
        ] {
            connection
                .execute(
                    "INSERT INTO laps
                 (id, session_id, track_name, lap_number, lap_time_ms, valid, sample_count,
                  created_at_ms, completed, quality_json, logical_key)
                 VALUES (?1, 'session', '실버스톤', 2, 90000, ?2, ?3, ?4, ?5, ?6,
                         'session:7:2')",
                    params![id, valid_flag, samples, created, completed, quality],
                )
                .unwrap();
            connection
                .execute(
                    "INSERT INTO lap_vehicles
                     (lap_id, vehicle_id, driver_name, class_name, is_player, overall_position,
                      class_position)
                     VALUES (?1, 7, '드라이버', 'Hypercar', 0, 1, 1)",
                    params![id],
                )
                .unwrap();
            connection
                .execute(
                    "INSERT INTO telemetry_samples
                     (lap_id, seq, session_time_s, lap_elapsed_s, lap_distance_m, world_x,
                      world_z, speed_kmh, rpm, gear, throttle, brake, steer, clutch, lateral_g,
                      longitudinal_g)
                     VALUES (?1, 0, 1, 1, 1, 0, 0, 100, 5000, 3, 1, 0, 0, 0, 0, 0)",
                    params![id],
                )
                .unwrap();
        }
        connection.pragma_update(None, "user_version", 2).unwrap();
        drop(connection);

        let store = DashboardStore::open(&directory).unwrap();
        let connection = store.connection().unwrap();
        let total: i64 = connection
            .query_row("SELECT COUNT(*) FROM laps", [], |row| row.get(0))
            .unwrap();
        let keeper: String = connection
            .query_row(
                "SELECT id FROM laps WHERE logical_key = 'session:7:2'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let weaker_laps: i64 = connection
            .query_row("SELECT COUNT(*) FROM laps WHERE id = 'weaker'", [], |row| {
                row.get(0)
            })
            .unwrap();
        let weaker_vehicles: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM lap_vehicles WHERE lap_id = 'weaker'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let weaker_samples: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM telemetry_samples WHERE lap_id = 'weaker'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(total, 1);
        assert_eq!(keeper, "keeper");
        assert_eq!(weaker_laps, 0);
        assert_eq!(weaker_vehicles, 0);
        assert_eq!(weaker_samples, 0);
        assert_eq!(schema_version(&connection).unwrap(), 3);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn completed_logical_lap_replaces_partial_and_cannot_be_downgraded() {
        let directory = temporary("logical-replacement");
        let store = DashboardStore::open(&directory).unwrap();
        let session = test_session("session");
        store.save_session(&session).unwrap();

        let partial = test_lap(&session, "partial", false, TraceQualityStatus::Partial, 25);
        store.save_lap(&partial).unwrap();
        let complete = test_lap(&session, "complete", true, TraceQualityStatus::Valid, 100);
        store.save_lap(&complete).unwrap();
        let downgrade = test_lap(
            &session,
            "late-partial",
            false,
            TraceQualityStatus::Partial,
            120,
        );
        store.save_lap(&downgrade).unwrap();

        let stored = store.load_logical_lap("session", 7, 3).unwrap().unwrap();
        assert_eq!(stored.summary.id, "complete");
        assert!(stored.summary.completed);
        assert_eq!(stored.samples.len(), 100);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn newer_partial_replaces_an_older_partial_when_quality_deteriorates() {
        let directory = temporary("partial-quality-deterioration");
        let store = DashboardStore::open(&directory).unwrap();
        let session = test_session("session");
        store.save_session(&session).unwrap();

        let mut earlier = test_lap(&session, "earlier", false, TraceQualityStatus::Partial, 25);
        earlier.summary.created_at_unix_ms = 100;
        store.save_lap(&earlier).unwrap();
        let mut invalid = test_lap(&session, "invalid", false, TraceQualityStatus::Rejected, 25);
        invalid.summary.created_at_unix_ms = 200;
        invalid.summary.quality.score = 0;
        store.save_lap(&invalid).unwrap();
        let stored = store.load_logical_lap("session", 7, 3).unwrap().unwrap();
        assert_eq!(stored.summary.id, "invalid");
        assert_eq!(stored.summary.quality.status, TraceQualityStatus::Rejected);

        let mut newer = test_lap(&session, "newer", false, TraceQualityStatus::Rejected, 40);
        newer.summary.created_at_unix_ms = 300;
        newer.summary.quality.score = 0;
        store.save_lap(&newer).unwrap();

        let stored = store.load_logical_lap("session", 7, 3).unwrap().unwrap();
        assert_eq!(stored.summary.id, "newer");
        assert_eq!(stored.summary.quality.status, TraceQualityStatus::Rejected);
        assert_eq!(stored.samples.len(), 40);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn resume_clock_requires_session_progress_to_match_elapsed_wall_time() {
        let now = 1_000_000;
        assert!(resume_clocks_match(100.0, 130.0, now - 30_000, now));
        assert!(resume_clocks_match(100.0, 100.0, now - 5_000, now));
        assert!(resume_clocks_match(100.0, 100.0, now - 300_000, now));
        assert!(!resume_clocks_match(100.0, 500.0, now - 30_000, now));
        assert!(resume_clocks_match(100.0, 99.6, now - 30_000, now));
        assert!(!resume_clocks_match(100.0, 99.4, now - 30_000, now));
        assert!(!resume_clocks_match(100.0, 50.0, now - 30_000, now));
    }

    #[test]
    fn resumable_session_rejects_a_late_attach_to_a_new_session() {
        let directory = temporary("resume-late-attach");
        let store = DashboardStore::open(&directory).unwrap();
        let mut session = test_session("original");
        session.current_time_s = 100.0;
        store.save_session(&session).unwrap();
        let signature = session_signature(&session);

        assert!(
            store
                .resumable_session(&signature, 105.0, 60_000)
                .unwrap()
                .is_some()
        );
        assert!(
            store
                .resumable_session(&signature, 500.0, 60_000)
                .unwrap()
                .is_none()
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[tokio::test]
    async fn background_queue_flushes_utf8_laps_and_reports_counters() {
        let directory = temporary("async-queue");
        let store = DashboardStore::open(&directory).unwrap();
        let session = test_session("queue-session");
        store.save_session(&session).unwrap();
        let worker = PersistenceWorker::start(store.clone());
        let mut lap = test_lap(&session, "queue-lap", true, TraceQualityStatus::Valid, 25);
        lap.summary.driver_name = "익명 드라이버".to_owned();
        worker.queue().save_lap(lap).unwrap();
        worker.queue().flush().unwrap();
        let health = worker.queue().health();
        assert_eq!(health.queued, 1);
        assert_eq!(health.written, 1);
        assert_eq!(health.pending, 0);
        assert_eq!(store.list_laps().unwrap()[0].driver_name, "익명 드라이버");
        worker.shutdown().await.unwrap();
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn retention_removes_raw_samples_before_analysis_summaries() {
        let directory = temporary("retention");
        let store = DashboardStore::open(&directory).unwrap();
        let session = test_session("old-session");
        store.save_session(&session).unwrap();
        store
            .save_lap(&test_lap(
                &session,
                "old-lap",
                true,
                TraceQualityStatus::Valid,
                25,
            ))
            .unwrap();
        store
            .connection()
            .unwrap()
            .execute(
                "UPDATE sessions SET started_at_ms = 1 WHERE id = 'old-session'",
                [],
            )
            .unwrap();

        store.apply_retention(Some(1), None).unwrap();
        assert_eq!(store.list_laps().unwrap().len(), 1);
        assert!(
            store
                .load_lap("old-lap")
                .unwrap()
                .unwrap()
                .samples
                .is_empty()
        );
        store.apply_retention(None, Some(1)).unwrap();
        assert!(store.list_laps().unwrap().is_empty());
        assert!(store.latest_session().unwrap().is_none());
        fs::remove_dir_all(directory).unwrap();
    }

    fn temporary(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "lmu-dashboard-store-{name}-{}-{}",
            std::process::id(),
            unix_ms()
        ))
    }

    fn test_session(id: &str) -> SessionState {
        SessionState {
            id: id.to_owned(),
            track_name: "실버스톤".to_owned(),
            session_type: "Qualifying".to_owned(),
            track_length_m: 1_000.0,
            ..SessionState::default()
        }
    }

    fn test_lap(
        session: &SessionState,
        id: &str,
        completed: bool,
        status: TraceQualityStatus,
        sample_count: usize,
    ) -> SavedLap {
        let quality = TraceQuality {
            status,
            score: if status == TraceQualityStatus::Valid {
                95
            } else {
                50
            },
            ..TraceQuality::default()
        };
        SavedLap {
            summary: LapSummary {
                id: id.to_owned(),
                session_id: session.id.clone(),
                track_name: session.track_name.clone(),
                session_type: session.session_type.clone(),
                track_length_m: session.track_length_m,
                vehicle_id: 7,
                driver_name: "익명 플레이어".to_owned(),
                class_name: "Hypercar".to_owned(),
                is_player: true,
                overall_position: 2,
                class_position: 2,
                lap_number: 3,
                lap_time_ms: if completed { 90_000 } else { 0 },
                valid: status == TraceQualityStatus::Valid,
                quality,
                sample_count,
                created_at_unix_ms: unix_ms(),
                completed,
            },
            samples: (0..sample_count)
                .map(|index| TelemetryPoint {
                    session_time_s: index as f64 * 0.05,
                    lap_elapsed_s: index as f64 * 0.05,
                    lap_distance_m: index as f64 * 10.0,
                    speed_kmh: 180.0,
                    ..TelemetryPoint::default()
                })
                .collect(),
        }
    }
}
