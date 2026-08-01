use std::fs::{File, OpenOptions, metadata};
use std::io::{BufRead, BufReader, BufWriter, ErrorKind, Write};
use std::path::Path;
use std::time::{Duration, Instant};

use crate::analysis::{CompletedLapAnalysis, TelemetryAnalyzer};
use crate::config::BridgeConfig;
use crate::race_engineer::RaceEngineer;
use crate::telemetry::{InputSample, TelemetryUpdate};

pub struct TelemetryRecorder {
    input_logger: Option<InputLogger>,
    corner_logger: Option<CornerLogger>,
    analyzer: Option<TelemetryAnalyzer>,
    analysis_report: Option<String>,
    race_engineer: Option<RaceEngineer>,
    session_context: SessionCsvContext,
}

#[derive(Default)]
struct SessionCsvContext {
    uid: Option<u64>,
    session_type: Option<u8>,
}

impl SessionCsvContext {
    fn update(&mut self, update: &TelemetryUpdate) {
        if let Some(uid) = update.session_uid
            && self.uid != Some(uid)
        {
            self.uid = Some(uid);
            self.session_type = None;
        }
        if let Some(session) = &update.session {
            self.session_type = Some(session.session_type);
        }
    }
}

impl TelemetryRecorder {
    pub fn open(config: &BridgeConfig) -> Result<Self, String> {
        let input_logger = config
            .input_log
            .as_deref()
            .map(InputLogger::open)
            .transpose()?;
        let corner_logger = config
            .corner_log
            .as_deref()
            .map(CornerLogger::open)
            .transpose()?;
        let analyzer = if config.corner_log.is_some()
            || config.analysis_report.is_some()
            || config.race_engineer
        {
            Some(TelemetryAnalyzer::default())
        } else {
            None
        };
        let race_engineer = RaceEngineer::open(config)?;

        Ok(Self {
            input_logger,
            corner_logger,
            analyzer,
            analysis_report: config.analysis_report.clone(),
            race_engineer,
            session_context: SessionCsvContext::default(),
        })
    }

    pub fn ingest(&mut self, source: &str, update: &TelemetryUpdate, debug: bool) {
        self.session_context.update(update);
        let completed_analysis = self
            .analyzer
            .as_mut()
            .filter(|_| !update.is_empty())
            .and_then(|analyzer| analyzer.ingest(update));

        if let Some(sample) = &update.input {
            let session_uid = self.session_context.uid;
            let session_type = self.session_context.session_type;
            let input_log_error = self
                .input_logger
                .as_mut()
                .and_then(|logger| logger.write(sample, session_uid, session_type).err());
            if let Some(error) = input_log_error {
                eprintln!("[log-error] {error}; disabling input logging");
                self.input_logger = None;
            }
        }
        if update.final_classification.is_some()
            && let Some(logger) = &mut self.input_logger
            && let Err(error) = logger.flush()
        {
            eprintln!("[log-error] {error}; disabling input logging");
            self.input_logger = None;
        }

        if let Some(analysis) = completed_analysis.as_ref() {
            let corner_log_error = self
                .corner_logger
                .as_mut()
                .and_then(|logger| logger.write(analysis).err());
            if let Some(error) = corner_log_error {
                eprintln!("[log-error] {error}; disabling corner logging");
                self.corner_logger = None;
            }

            let report_error = self
                .analysis_report
                .as_deref()
                .and_then(|path| write_analysis_report(path, analysis).err());
            if let Some(error) = report_error {
                eprintln!("[log-error] {error}; disabling analysis report writes");
                self.analysis_report = None;
            }

            if debug {
                println!(
                    "[analysis] lap={} clean={} samples={} recommendations={}",
                    analysis.lap_num,
                    analysis.clean,
                    analysis.sample_count,
                    analysis.recommendations.len()
                );
            }
        }

        if let Some(engineer) = &mut self.race_engineer {
            engineer.ingest(source, update, completed_analysis.as_ref());
        }
    }
}

impl Drop for TelemetryRecorder {
    fn drop(&mut self) {
        if let Some(logger) = &mut self.input_logger
            && let Err(error) = logger.flush()
        {
            eprintln!("[log-error] {error} while finalizing input logging");
        }
        if let Some(engineer) = &mut self.race_engineer {
            engineer.finish_session("bridge_shutdown");
        }
    }
}

pub fn print_enabled_outputs(config: &BridgeConfig) {
    if let Some(path) = &config.input_log {
        println!("input logging enabled: {path}");
    }
    if let Some(path) = &config.corner_log {
        println!("corner trace logging enabled: {path}");
    }
    if let Some(path) = &config.analysis_report {
        println!("analysis report enabled: {path}");
    }
    if config.race_engineer {
        println!(
            "race engineer enabled: {}{}",
            if config.engineer_ai_hook.is_some() {
                "AI decisions"
            } else {
                "console"
            },
            if config.engineer_voice && config.engineer_ai_hook.is_none() {
                " + Windows TTS"
            } else {
                ""
            }
        );
    }
    if let Some(path) = &config.engineer_state {
        println!("race engineer live state: {path}");
    }
    if let Some(path) = &config.engineer_history {
        println!("race engineer state history: {path}");
    }
    if let Some(path) = &config.engineer_trigger {
        println!("race engineer event trigger: {path}");
    }
    if let Some(path) = &config.engineer_hook {
        println!("race engineer event hook: {path}");
    }
    if let Some(path) = &config.engineer_ai_hook {
        println!("race engineer AI decision hook: {path}");
    }
    if let Some(task_id) = &config.engineer_ai_task_id {
        println!("race engineer AI task: {task_id}");
    }
}

pub struct InputLogger {
    writer: BufWriter<std::fs::File>,
    last_flush: Instant,
}

impl InputLogger {
    pub fn open(path: &str) -> Result<Self, String> {
        let should_write_header =
            should_write_csv_header(path, InputSample::csv_header(), "input log")?;
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(Path::new(path))
            .map_err(|error| format!("failed to open input log {path}: {error}"))?;
        let mut logger = Self {
            writer: BufWriter::new(file),
            last_flush: Instant::now(),
        };

        if should_write_header {
            logger
                .writer
                .write_all(InputSample::csv_header().as_bytes())
                .map_err(|error| format!("failed to write CSV header: {error}"))?;
            logger
                .writer
                .flush()
                .map_err(|error| format!("failed to flush CSV header: {error}"))?;
        }

        Ok(logger)
    }

    pub fn write(
        &mut self,
        sample: &InputSample,
        session_uid: Option<u64>,
        session_type: Option<u8>,
    ) -> Result<(), String> {
        self.writer
            .write_all(
                sample
                    .to_csv_row_with_session(session_uid, session_type)
                    .as_bytes(),
            )
            .map_err(|error| format!("failed to write input log row: {error}"))?;
        if self.last_flush.elapsed() >= Duration::from_secs(1) {
            self.writer
                .flush()
                .map_err(|error| format!("failed to flush input log row: {error}"))?;
            self.last_flush = Instant::now();
        }
        Ok(())
    }

    pub fn flush(&mut self) -> Result<(), String> {
        self.writer
            .flush()
            .map_err(|error| format!("failed to flush input log: {error}"))?;
        self.last_flush = Instant::now();
        Ok(())
    }
}

pub struct CornerLogger {
    writer: BufWriter<std::fs::File>,
}

impl CornerLogger {
    pub fn open(path: &str) -> Result<Self, String> {
        let should_write_header = should_write_csv_header(
            path,
            crate::analysis::CornerSummary::csv_header(),
            "corner log",
        )?;
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(Path::new(path))
            .map_err(|error| format!("failed to open corner log {path}: {error}"))?;
        let mut logger = Self {
            writer: BufWriter::new(file),
        };

        if should_write_header {
            logger
                .writer
                .write_all(crate::analysis::CornerSummary::csv_header().as_bytes())
                .map_err(|error| format!("failed to write corner CSV header: {error}"))?;
            logger
                .writer
                .flush()
                .map_err(|error| format!("failed to flush corner CSV header: {error}"))?;
        }

        Ok(logger)
    }

    pub fn write(&mut self, analysis: &CompletedLapAnalysis) -> Result<(), String> {
        for corner in &analysis.corners {
            self.writer
                .write_all(
                    corner
                        .csv_row_with_session(
                            analysis.lap_num,
                            analysis.clean,
                            analysis.session_uid,
                            analysis.session_type,
                        )
                        .as_bytes(),
                )
                .map_err(|error| format!("failed to write corner log row: {error}"))?;
        }
        self.writer
            .flush()
            .map_err(|error| format!("failed to flush corner log: {error}"))?;
        Ok(())
    }
}

fn should_write_csv_header(
    path: &str,
    expected_header: &str,
    description: &str,
) -> Result<bool, String> {
    match metadata(path) {
        Ok(meta) if meta.len() == 0 => Ok(true),
        Ok(_) => {
            let file = File::open(Path::new(path))
                .map_err(|error| format!("failed to inspect {description} {path}: {error}"))?;
            let mut reader = BufReader::new(file);
            let mut actual_header = String::new();
            reader
                .read_line(&mut actual_header)
                .map_err(|error| format!("failed to inspect {description} {path}: {error}"))?;
            let actual_header = actual_header.trim_end_matches(['\r', '\n']);
            let expected_header = expected_header.trim_end_matches(['\r', '\n']);
            if actual_header != expected_header {
                return Err(format!(
                    "{description} {path} has an incompatible CSV header; refusing to append rows with the current schema"
                ));
            }
            Ok(false)
        }
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(true),
        Err(error) => Err(format!("failed to inspect {description} {path}: {error}")),
    }
}

pub fn write_analysis_report(path: &str, analysis: &CompletedLapAnalysis) -> Result<(), String> {
    std::fs::write(Path::new(path), analysis.to_markdown())
        .map_err(|error| format!("failed to write analysis report {path}: {error}"))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;
    use crate::analysis::CornerSummary;
    use crate::telemetry::{
        InputSample, SessionSample, TelemetryUpdate, WheelValuesF32, WheelValuesU8, WheelValuesU16,
    };

    fn temporary_path(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "sim-moza-bridge-{label}-{}-{unique}.csv",
            std::process::id()
        ))
    }

    fn input_sample() -> InputSample {
        InputSample {
            session_time: 12.5,
            frame_identifier: 77,
            player_car_index: 3,
            throttle: 0.75,
            steer: -0.1,
            brake: 0.25,
            clutch: 0,
            speed_kmh: 245,
            gear: 7,
            rpm: 11_500,
            drs: true,
            rev_lights_percent: 80,
            rev_lights_bit_value: 0,
            brake_temps_c: WheelValuesU16 {
                rl: 600,
                rr: 610,
                fl: 620,
                fr: 630,
            },
            tyre_surface_temps_c: WheelValuesU8 {
                rl: 90,
                rr: 91,
                fl: 92,
                fr: 93,
            },
            tyre_inner_temps_c: WheelValuesU8 {
                rl: 94,
                rr: 95,
                fl: 96,
                fr: 97,
            },
            engine_temp_c: 105,
            tyre_pressures_psi: WheelValuesF32 {
                rl: 22.1,
                rr: 22.2,
                fl: 24.1,
                fr: 24.2,
            },
        }
    }

    #[test]
    fn session_context_clears_type_when_uid_changes() {
        let mut context = SessionCsvContext::default();
        context.update(&TelemetryUpdate {
            session_uid: Some(7),
            session: Some(SessionSample {
                session_time: 1.0,
                frame_identifier: 1,
                overall_frame_identifier: None,
                weather: 0,
                total_laps: 57,
                track_length_m: 5_000,
                session_type: 15,
                track_id: 1,
                track_temp_c: 30,
                air_temp_c: 20,
                session_time_left_s: 3_600,
                pit_speed_limit_kmh: 80,
                safety_car_status: 0,
                marshal_zones: Vec::new(),
                weather_forecast_samples: Vec::new(),
                pit_stop_window_ideal_lap: None,
                pit_stop_window_latest_lap: None,
                pit_stop_rejoin_position: None,
            }),
            ..TelemetryUpdate::default()
        });
        assert_eq!(context.uid, Some(7));
        assert_eq!(context.session_type, Some(15));

        context.update(&TelemetryUpdate {
            session_uid: Some(8),
            input: Some(input_sample()),
            ..TelemetryUpdate::default()
        });
        assert_eq!(context.uid, Some(8));
        assert_eq!(context.session_type, None);
    }

    #[test]
    fn input_logger_appends_session_columns_without_reordering_existing_columns() {
        let path = temporary_path("input-context");
        let path_text = path.to_string_lossy();
        let mut logger = InputLogger::open(&path_text).unwrap();
        logger
            .write(&input_sample(), Some(5_154_468_281_529_202_801), Some(15))
            .unwrap();
        logger.flush().unwrap();
        drop(logger);

        let contents = std::fs::read_to_string(&path).unwrap();
        let mut lines = contents.lines();
        let header = lines.next().unwrap().split(',').collect::<Vec<_>>();
        let row = lines.next().unwrap().split(',').collect::<Vec<_>>();
        assert_eq!(
            &header[..3],
            &["session_time", "frame_identifier", "player_car_index"]
        );
        assert_eq!(
            &header[header.len() - 3..],
            &["session_uid", "session_type", "session_type_name"]
        );
        assert_eq!(header.len(), row.len());
        assert_eq!(
            &row[row.len() - 3..],
            &["5154468281529202801", "15", "race"]
        );

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn corner_logger_uses_completed_lap_session_context() {
        let path = temporary_path("corner-context");
        let path_text = path.to_string_lossy();
        let mut logger = CornerLogger::open(&path_text).unwrap();
        logger
            .write(&CompletedLapAnalysis {
                session_uid: Some(42),
                session_type: Some(2),
                lap_num: 4,
                lap_time_ms: 90_000,
                clean: true,
                invalid_reason: None,
                track_length_m: 5_000.0,
                sample_count: 100,
                corners: vec![CornerSummary {
                    segment: 1,
                    start_m: 0.0,
                    end_m: 250.0,
                    samples: 20,
                    min_speed_kmh: 100,
                    max_speed_kmh: 250,
                    avg_speed_kmh: 175.0,
                    max_brake: 0.8,
                    max_throttle: 1.0,
                    avg_abs_steer: 0.2,
                    max_abs_steer: 0.5,
                    phase: "entry".to_owned(),
                }],
                recommendations: Vec::new(),
                latest_damage: None,
                latest_status: None,
                latest_setup: None,
            })
            .unwrap();
        drop(logger);

        let contents = std::fs::read_to_string(&path).unwrap();
        let mut lines = contents.lines();
        let header = lines.next().unwrap().split(',').collect::<Vec<_>>();
        let row = lines.next().unwrap().split(',').collect::<Vec<_>>();
        assert_eq!(&header[..2], &["lap", "clean"]);
        assert_eq!(
            &header[header.len() - 3..],
            &["session_uid", "session_type", "session_type_name"]
        );
        assert_eq!(header.len(), row.len());
        assert_eq!(&row[row.len() - 3..], &["42", "2", "practice"]);

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn refuses_to_append_to_legacy_csv_headers() {
        let input_path = temporary_path("legacy-input");
        std::fs::write(
            &input_path,
            concat!(
                "session_time,frame_identifier,player_car_index,throttle,brake,steer,clutch,",
                "speed_kmh,gear,rpm,drs,rev_lights_percent,engine_temp_c,",
                "brake_temp_rl,brake_temp_rr,brake_temp_fl,brake_temp_fr,",
                "tyre_surface_rl,tyre_surface_rr,tyre_surface_fl,tyre_surface_fr,",
                "tyre_pressure_rl,tyre_pressure_rr,tyre_pressure_fl,tyre_pressure_fr\n"
            ),
        )
        .unwrap();
        let input_error = InputLogger::open(&input_path.to_string_lossy())
            .err()
            .unwrap();
        assert!(input_error.contains("incompatible CSV header"));

        let corner_path = temporary_path("legacy-corner");
        std::fs::write(
            &corner_path,
            "lap,clean,segment,start_m,end_m,samples,min_speed_kmh,max_speed_kmh,avg_speed_kmh,max_brake,max_throttle,avg_abs_steer,max_abs_steer,phase\n",
        )
        .unwrap();
        let corner_error = CornerLogger::open(&corner_path.to_string_lossy())
            .err()
            .unwrap();
        assert!(corner_error.contains("incompatible CSV header"));

        let _ = std::fs::remove_file(input_path);
        let _ = std::fs::remove_file(corner_path);
    }
}
