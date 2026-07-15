#![allow(dead_code)]

use serde::{Deserialize, Serialize};

const MIN_SAMPLE_RATE_HZ: f64 = 5.0;
const MAX_SAMPLE_GAP_S: f64 = 1.0;
const MIN_COMPLETE_COVERAGE: f64 = 0.92;
const LAP_TIME_TOLERANCE_MS: u32 = 1_500;

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TraceQualityStatus {
    Valid,
    Partial,
    Rejected,
    #[default]
    Unknown,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QualityReason {
    NoSamples,
    GameInvalidated,
    StartsMidTrace,
    EndsBeforeFinish,
    InsufficientCoverage,
    TimingMismatch,
    SparseSamples,
    SampleGap,
    NonMonotonicDistance,
    NonMonotonicTime,
    ImplausibleTelemetry,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct TraceQuality {
    pub status: TraceQualityStatus,
    pub score: u8,
    pub reasons: Vec<QualityReason>,
    pub coverage_ratio: f64,
    pub start_progress: f64,
    pub end_progress: f64,
    pub sample_rate_hz: f64,
    pub max_gap_ms: u32,
    pub dropped_samples: u32,
    pub anomaly_count: u32,
}

impl Default for TraceQuality {
    fn default() -> Self {
        Self {
            status: TraceQualityStatus::Unknown,
            score: 0,
            reasons: Vec::new(),
            coverage_ratio: 0.0,
            start_progress: 0.0,
            end_progress: 0.0,
            sample_rate_hz: 0.0,
            max_gap_ms: 0,
            dropped_samples: 0,
            anomaly_count: 0,
        }
    }
}

impl TraceQuality {
    pub fn is_reference_usable(&self) -> bool {
        self.status == TraceQualityStatus::Valid && self.score >= 80
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct QualitySample {
    pub session_time_s: f64,
    pub elapsed_s: f64,
    pub distance_m: f64,
    pub speed_kmh: f64,
    pub rpm: f64,
    pub gear: i32,
    pub lateral_g: f64,
    pub longitudinal_g: f64,
}

pub fn assess_trace(
    samples: &[QualitySample],
    track_length_m: f64,
    official_time_ms: Option<u32>,
    game_invalidated: bool,
    completed: bool,
) -> TraceQuality {
    if samples.is_empty() {
        return TraceQuality {
            status: TraceQualityStatus::Rejected,
            reasons: vec![QualityReason::NoSamples],
            ..TraceQuality::default()
        };
    }

    let track_length_m = finite_nonnegative(track_length_m);
    let first = samples[0];
    let last = samples[samples.len() - 1];
    let mut quality = TraceQuality {
        status: if completed {
            TraceQualityStatus::Valid
        } else {
            TraceQualityStatus::Partial
        },
        score: 100,
        ..TraceQuality::default()
    };

    let mut positive_intervals = Vec::with_capacity(samples.len().saturating_sub(1));
    let mut distance_reversals = 0_u32;
    let mut time_reversals = 0_u32;
    let mut anomaly_count = 0_u32;
    let distance_backwards_tolerance = (track_length_m * 0.002).max(5.0);

    for (index, sample) in samples.iter().copied().enumerate() {
        if !sample_is_plausible(sample) {
            anomaly_count = anomaly_count.saturating_add(1);
        }
        if index == 0 {
            continue;
        }
        let previous = samples[index - 1];
        let delta_time = sample.session_time_s - previous.session_time_s;
        if delta_time > 0.0 && delta_time.is_finite() {
            positive_intervals.push(delta_time);
        } else if delta_time < -f64::EPSILON {
            time_reversals = time_reversals.saturating_add(1);
        }
        if previous.distance_m - sample.distance_m > distance_backwards_tolerance {
            distance_reversals = distance_reversals.saturating_add(1);
        }
    }

    quality.anomaly_count = anomaly_count;
    if anomaly_count > 0 {
        push_reason(&mut quality, QualityReason::ImplausibleTelemetry, 35);
    }
    if time_reversals > 0 {
        push_reason(&mut quality, QualityReason::NonMonotonicTime, 40);
    }
    let allowed_reversals = (samples.len() / 200).max(1) as u32;
    if distance_reversals > allowed_reversals {
        push_reason(&mut quality, QualityReason::NonMonotonicDistance, 35);
    }

    if !positive_intervals.is_empty() {
        positive_intervals.sort_by(f64::total_cmp);
        let median_interval = positive_intervals[positive_intervals.len() / 2];
        if median_interval > 0.0 {
            quality.sample_rate_hz = round(1.0 / median_interval, 2);
            let max_gap = positive_intervals.iter().copied().fold(0.0_f64, f64::max);
            quality.max_gap_ms = seconds_to_ms(max_gap);
            quality.dropped_samples = positive_intervals
                .iter()
                .map(|interval| ((*interval / median_interval).round() as i64 - 1).max(0) as u32)
                .sum();
            if quality.sample_rate_hz < MIN_SAMPLE_RATE_HZ {
                push_reason(&mut quality, QualityReason::SparseSamples, 25);
            }
            if max_gap > MAX_SAMPLE_GAP_S {
                push_reason(&mut quality, QualityReason::SampleGap, 20);
            }
        }
    } else if samples.len() > 1 {
        push_reason(&mut quality, QualityReason::SparseSamples, 25);
    }

    if track_length_m >= 100.0 {
        let start = finite_nonnegative(first.distance_m).min(track_length_m);
        let end = finite_nonnegative(last.distance_m).min(track_length_m);
        let min_distance = samples
            .iter()
            .map(|sample| finite_nonnegative(sample.distance_m))
            .fold(f64::INFINITY, f64::min)
            .min(track_length_m);
        let max_distance = samples
            .iter()
            .map(|sample| finite_nonnegative(sample.distance_m))
            .fold(0.0_f64, f64::max)
            .min(track_length_m);
        quality.start_progress = round(start / track_length_m, 4);
        quality.end_progress = round(end / track_length_m, 4);
        quality.coverage_ratio = round(
            ((max_distance - min_distance) / track_length_m).clamp(0.0, 1.0),
            4,
        );

        let start_window_m = (track_length_m * 0.03).clamp(100.0, 300.0);
        let finish_window_m = (track_length_m * 0.05).clamp(150.0, 500.0);
        if completed && start > start_window_m {
            push_reason(&mut quality, QualityReason::StartsMidTrace, 45);
        }
        if completed && end + finish_window_m < track_length_m {
            push_reason(&mut quality, QualityReason::EndsBeforeFinish, 45);
        }
        if completed && quality.coverage_ratio < MIN_COMPLETE_COVERAGE {
            push_reason(&mut quality, QualityReason::InsufficientCoverage, 45);
        }
    } else if completed {
        push_reason(&mut quality, QualityReason::InsufficientCoverage, 30);
    }

    if let Some(official_time_ms) = official_time_ms.filter(|time| *time > 0) {
        let observed_span_ms = seconds_to_ms(
            (last.elapsed_s - first.elapsed_s).max(last.session_time_s - first.session_time_s),
        );
        let final_elapsed_ms = seconds_to_ms(last.elapsed_s);
        let span_matches = times_consistent(observed_span_ms, official_time_ms);
        let final_matches = times_consistent(final_elapsed_ms, official_time_ms);
        let full_lap_capture = !quality.reasons.contains(&QualityReason::StartsMidTrace)
            && !quality.reasons.contains(&QualityReason::EndsBeforeFinish)
            && !quality
                .reasons
                .contains(&QualityReason::InsufficientCoverage);
        if completed && full_lap_capture && (!span_matches || !final_matches) {
            push_reason(&mut quality, QualityReason::TimingMismatch, 50);
        }
    }

    if game_invalidated {
        push_reason(&mut quality, QualityReason::GameInvalidated, 100);
    }

    let rejected = quality.reasons.iter().any(|reason| {
        matches!(
            reason,
            QualityReason::GameInvalidated
                | QualityReason::TimingMismatch
                | QualityReason::NonMonotonicTime
                | QualityReason::ImplausibleTelemetry
        )
    }) || distance_reversals > allowed_reversals.saturating_mul(3);
    let partial = quality.reasons.iter().any(|reason| {
        matches!(
            reason,
            QualityReason::StartsMidTrace
                | QualityReason::EndsBeforeFinish
                | QualityReason::InsufficientCoverage
                | QualityReason::SparseSamples
                | QualityReason::SampleGap
                | QualityReason::NonMonotonicDistance
        )
    });
    quality.status = if rejected {
        TraceQualityStatus::Rejected
    } else if !completed || partial {
        TraceQualityStatus::Partial
    } else {
        TraceQualityStatus::Valid
    };
    quality
}

fn sample_is_plausible(sample: QualitySample) -> bool {
    sample.session_time_s.is_finite()
        && sample.elapsed_s.is_finite()
        && sample.distance_m.is_finite()
        && sample.speed_kmh.is_finite()
        && sample.rpm.is_finite()
        && sample.lateral_g.is_finite()
        && sample.longitudinal_g.is_finite()
        && sample.session_time_s >= 0.0
        && sample.elapsed_s >= 0.0
        && sample.distance_m >= -50.0
        && (0.0..=650.0).contains(&sample.speed_kmh)
        && (0.0..=30_000.0).contains(&sample.rpm)
        && (-1..=12).contains(&sample.gear)
        && sample.lateral_g.abs() <= 15.0
        && sample.longitudinal_g.abs() <= 15.0
}

fn push_reason(quality: &mut TraceQuality, reason: QualityReason, penalty: u8) {
    if !quality.reasons.contains(&reason) {
        quality.reasons.push(reason);
        quality.score = quality.score.saturating_sub(penalty);
    }
}

fn times_consistent(observed_ms: u32, official_ms: u32) -> bool {
    let tolerance = (official_ms.saturating_mul(15) / 1_000).max(LAP_TIME_TOLERANCE_MS);
    observed_ms > 0 && observed_ms.abs_diff(official_ms) <= tolerance
}

fn seconds_to_ms(seconds: f64) -> u32 {
    if !seconds.is_finite() || seconds <= 0.0 {
        0
    } else {
        (seconds * 1_000.0).round().clamp(0.0, u32::MAX as f64) as u32
    }
}

fn finite_nonnegative(value: f64) -> f64 {
    if value.is_finite() && value > 0.0 {
        value
    } else {
        0.0
    }
}

fn round(value: f64, decimals: i32) -> f64 {
    let factor = 10_f64.powi(decimals);
    (value * factor).round() / factor
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_a_complete_well_sampled_trace() {
        let samples = trace(0.0, 1_000.0, 11.0, 101);
        let quality = assess_trace(&samples, 1_000.0, Some(11_000), false, true);

        assert_eq!(quality.status, TraceQualityStatus::Valid);
        assert!(quality.score >= 80);
        assert!(quality.coverage_ratio >= 0.99);
    }

    #[test]
    fn marks_a_mid_trace_capture_as_partial_even_with_an_official_time() {
        let mut samples = trace(400.0, 1_000.0, 7.0, 61);
        for sample in &mut samples {
            sample.elapsed_s += 4.0;
        }
        let quality = assess_trace(&samples, 1_000.0, Some(11_000), false, true);

        assert_ne!(quality.status, TraceQualityStatus::Valid);
        assert!(quality.reasons.contains(&QualityReason::StartsMidTrace));
        assert!(
            quality
                .reasons
                .contains(&QualityReason::InsufficientCoverage)
        );
    }

    #[test]
    fn rejects_implausible_shared_memory_values() {
        let mut samples = trace(0.0, 1_000.0, 11.0, 101);
        samples[50].rpm = 200_000.0;
        let quality = assess_trace(&samples, 1_000.0, Some(11_000), false, true);

        assert_eq!(quality.status, TraceQualityStatus::Rejected);
        assert_eq!(quality.anomaly_count, 1);
    }

    fn trace(start_m: f64, end_m: f64, duration_s: f64, count: usize) -> Vec<QualitySample> {
        (0..count)
            .map(|index| {
                let progress = index as f64 / (count - 1) as f64;
                QualitySample {
                    session_time_s: progress * duration_s,
                    elapsed_s: progress * duration_s,
                    distance_m: start_m + (end_m - start_m) * progress,
                    speed_kmh: 150.0,
                    rpm: 7_000.0,
                    gear: 4,
                    ..QualitySample::default()
                }
            })
            .collect()
    }
}
