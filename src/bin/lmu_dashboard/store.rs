use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension, params};

use crate::model::{
    ContactConfidence, ContactEvent, ContactParticipant, LapSummary, SavedLap, SessionState,
    TelemetryPoint, TrackPoint,
};
use crate::telemetry_quality::{TraceQuality, TraceQualityStatus};

#[derive(Clone, Debug, PartialEq)]
pub struct ResumableSession {
    pub id: String,
    pub last_session_time_s: f64,
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
                    source = excluded.source",
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
                "UPDATE sessions SET last_seen_ms = ?2, last_session_time_s = ?3 WHERE id = ?1",
                params![session.id, unix_ms() as i64, session.current_time_s],
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
        let cutoff = unix_ms().saturating_sub(max_age_ms) as i64;
        self.connection()?
            .query_row(
                "SELECT id, last_session_time_s FROM sessions
                 WHERE signature = ?1 AND last_seen_ms >= ?2
                   AND last_session_time_s <= ?3 + 5.0
                 ORDER BY last_seen_ms DESC LIMIT 1",
                params![signature, cutoff, current_time_s],
                |row| {
                    Ok(ResumableSession {
                        id: row.get(0)?,
                        last_session_time_s: row.get(1)?,
                    })
                },
            )
            .optional()
            .map_err(|error| format!("failed to find resumable session: {error}"))
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

    pub fn list_laps(&self) -> Result<Vec<LapSummary>, String> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT l.id, l.session_id, l.track_name, l.lap_number, l.lap_time_ms, l.valid,
                        l.sample_count, l.created_at_ms, COALESCE(v.vehicle_id, 0),
                        COALESCE(v.driver_name, ''), COALESCE(v.class_name, ''),
                        COALESCE(v.is_player, 0), COALESCE(v.overall_position, 0),
                        COALESCE(v.class_position, 0), COALESCE(s.session_type, ''),
                        l.completed, l.quality_json
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
                        l.completed, l.quality_json
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
        let connection = self.connection()?;
        connection
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS sessions (
                    id TEXT PRIMARY KEY,
                    started_at_ms INTEGER NOT NULL,
                    track_key TEXT NOT NULL,
                    track_name TEXT NOT NULL,
                    session_type TEXT NOT NULL,
                    game_version INTEGER NOT NULL,
                    signature TEXT NOT NULL DEFAULT '',
                    last_seen_ms INTEGER NOT NULL DEFAULT 0,
                    last_session_time_s REAL NOT NULL DEFAULT 0,
                    source TEXT NOT NULL DEFAULT '',
                    max_laps INTEGER NOT NULL DEFAULT 0,
                    track_length_m REAL NOT NULL DEFAULT 0
                 );
                 CREATE TABLE IF NOT EXISTS laps (
                    id TEXT PRIMARY KEY,
                    session_id TEXT NOT NULL,
                    track_name TEXT NOT NULL,
                    lap_number INTEGER NOT NULL,
                    lap_time_ms INTEGER NOT NULL,
                    valid INTEGER NOT NULL,
                    sample_count INTEGER NOT NULL,
                    created_at_ms INTEGER NOT NULL,
                    completed INTEGER NOT NULL DEFAULT 1,
                    quality_json TEXT NOT NULL DEFAULT '{}',
                    logical_key TEXT
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
                 CREATE INDEX IF NOT EXISTS lap_vehicles_class_idx
                    ON lap_vehicles (class_name, vehicle_id);
                 CREATE INDEX IF NOT EXISTS contacts_created_idx
                    ON contacts (created_at_ms DESC);",
            )
            .map_err(|error| format!("failed to initialize dashboard database: {error}"))?;

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
            ensure_column(&connection, table, column, definition)?;
        }
        connection
            .execute_batch(
                "DROP INDEX IF EXISTS telemetry_lap_idx;
                 CREATE INDEX IF NOT EXISTS sessions_resume_idx
                    ON sessions (signature, last_seen_ms DESC);
                 CREATE INDEX IF NOT EXISTS laps_session_idx
                    ON laps (session_id, lap_number, created_at_ms DESC);
                 CREATE UNIQUE INDEX IF NOT EXISTS laps_logical_key_idx
                    ON laps (logical_key) WHERE logical_key IS NOT NULL;
                 PRAGMA user_version = 2;",
            )
            .map_err(|error| format!("failed to finalize dashboard database migration: {error}"))?;
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

pub fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis() as u64)
}

pub fn session_signature(session: &SessionState) -> String {
    format!(
        "{}:{}:{}:{}",
        track_key(&session.track_name, session.track_length_m),
        session.session_type.trim().to_ascii_lowercase(),
        session.game_version,
        session.max_laps
    )
}

pub fn track_key(track_name: &str, track_length_m: f64) -> String {
    let mut normalized = String::with_capacity(track_name.len());
    for character in track_name.chars() {
        if character.is_ascii_alphanumeric() {
            normalized.push(character.to_ascii_lowercase());
        } else if !normalized.ends_with('-') {
            normalized.push('-');
        }
    }
    format!(
        "{}-{}",
        normalized.trim_matches('-'),
        track_length_m.round() as i64
    )
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
}
