use std::fs::{OpenOptions, metadata};
use std::io::{BufWriter, Write};
use std::path::Path;

use crate::analysis::CompletedLapAnalysis;
use crate::telemetry::InputSample;

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
