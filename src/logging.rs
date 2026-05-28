use std::fs::{OpenOptions, metadata};
use std::io::{BufWriter, Write};
use std::path::Path;

use crate::analysis::{CompletedLapAnalysis, TelemetryAnalyzer};
use crate::config::BridgeConfig;
use crate::telemetry::{InputSample, TelemetryUpdate};

pub struct TelemetryRecorder {
    input_logger: Option<InputLogger>,
    corner_logger: Option<CornerLogger>,
    analyzer: Option<TelemetryAnalyzer>,
    analysis_report: Option<String>,
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
        let analyzer = if config.corner_log.is_some() || config.analysis_report.is_some() {
            Some(TelemetryAnalyzer::default())
        } else {
            None
        };

        Ok(Self {
            input_logger,
            corner_logger,
            analyzer,
            analysis_report: config.analysis_report.clone(),
        })
    }

    pub fn ingest(&mut self, update: &TelemetryUpdate, debug: bool) {
        if let Some(sample) = &update.input {
            let input_log_error = self
                .input_logger
                .as_mut()
                .and_then(|logger| logger.write(sample).err());
            if let Some(error) = input_log_error {
                eprintln!("[log-error] {error}; disabling input logging");
                self.input_logger = None;
            }
        }

        if let Some(analyzer) = &mut self.analyzer
            && !update.is_empty()
            && let Some(analysis) = analyzer.ingest(update)
        {
            let corner_log_error = self
                .corner_logger
                .as_mut()
                .and_then(|logger| logger.write(&analysis).err());
            if let Some(error) = corner_log_error {
                eprintln!("[log-error] {error}; disabling corner logging");
                self.corner_logger = None;
            }

            let report_error = self
                .analysis_report
                .as_deref()
                .and_then(|path| write_analysis_report(path, &analysis).err());
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
}

pub struct InputLogger {
    writer: BufWriter<std::fs::File>,
}

impl InputLogger {
    pub fn open(path: &str) -> Result<Self, String> {
        let should_write_header = match metadata(path) {
            Ok(meta) => meta.len() == 0,
            Err(_) => true,
        };
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(Path::new(path))
            .map_err(|error| format!("failed to open input log {path}: {error}"))?;
        let mut logger = Self {
            writer: BufWriter::new(file),
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

    pub fn write(&mut self, sample: &InputSample) -> Result<(), String> {
        self.writer
            .write_all(sample.to_csv_row().as_bytes())
            .map_err(|error| format!("failed to write input log row: {error}"))?;
        self.writer
            .flush()
            .map_err(|error| format!("failed to flush input log row: {error}"))?;
        Ok(())
    }
}

pub struct CornerLogger {
    writer: BufWriter<std::fs::File>,
}

impl CornerLogger {
    pub fn open(path: &str) -> Result<Self, String> {
        let should_write_header = match metadata(path) {
            Ok(meta) => meta.len() == 0,
            Err(_) => true,
        };
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
                .write_all(corner.csv_row(analysis.lap_num, analysis.clean).as_bytes())
                .map_err(|error| format!("failed to write corner log row: {error}"))?;
        }
        self.writer
            .flush()
            .map_err(|error| format!("failed to flush corner log: {error}"))?;
        Ok(())
    }
}

pub fn write_analysis_report(path: &str, analysis: &CompletedLapAnalysis) -> Result<(), String> {
    std::fs::write(Path::new(path), analysis.to_markdown())
        .map_err(|error| format!("failed to write analysis report {path}: {error}"))
}
