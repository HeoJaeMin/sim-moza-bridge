use std::fmt::Write as _;

use serde::{Deserialize, Serialize};

use crate::telemetry::{
    CarSetupSample, DamageSample, InputSample, LapSample, SessionSample, StatusSample,
    TelemetryUpdate, f1_session_type_name,
};

const SEGMENT_COUNT: usize = 20;
const MIN_REPORT_SAMPLES: usize = 12;

#[derive(Clone, Debug)]
pub struct TracePoint {
    pub session_time: f32,
    pub lap_distance_m: f32,
    pub speed_kmh: u16,
    pub throttle: f32,
    pub brake: f32,
    pub steer: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CornerSummary {
    pub segment: usize,
    pub start_m: f32,
    pub end_m: f32,
    pub samples: usize,
    pub min_speed_kmh: u16,
    pub max_speed_kmh: u16,
    pub avg_speed_kmh: f32,
    pub max_brake: f32,
    pub max_throttle: f32,
    pub avg_abs_steer: f32,
    pub max_abs_steer: f32,
    pub phase: String,
}

impl CornerSummary {
    pub fn csv_header() -> &'static str {
        "lap,clean,segment,start_m,end_m,samples,min_speed_kmh,max_speed_kmh,avg_speed_kmh,max_brake,max_throttle,avg_abs_steer,max_abs_steer,phase,session_uid,session_type,session_type_name\n"
    }

    pub fn csv_row(&self, lap_num: u8, clean: bool) -> String {
        self.csv_row_with_session(lap_num, clean, None, None)
    }

    pub fn csv_row_with_session(
        &self,
        lap_num: u8,
        clean: bool,
        session_uid: Option<u64>,
        session_type: Option<u8>,
    ) -> String {
        let session_uid = session_uid
            .map(|value| value.to_string())
            .unwrap_or_default();
        let session_type_value = session_type
            .map(|value| value.to_string())
            .unwrap_or_default();
        let session_type_name = session_type.map(f1_session_type_name).unwrap_or("");
        format!(
            "{},{},{},{:.1},{:.1},{},{},{},{:.1},{:.4},{:.4},{:.4},{:.4},{},{},{},{}\n",
            lap_num,
            clean,
            self.segment,
            self.start_m,
            self.end_m,
            self.samples,
            self.min_speed_kmh,
            self.max_speed_kmh,
            self.avg_speed_kmh,
            self.max_brake,
            self.max_throttle,
            self.avg_abs_steer,
            self.max_abs_steer,
            self.phase,
            session_uid,
            session_type_value,
            session_type_name
        )
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SetupRecommendation {
    pub area: String,
    pub reason: String,
    pub action: String,
    pub confidence: String,
}

#[derive(Clone, Debug)]
pub struct CompletedLapAnalysis {
    pub session_uid: Option<u64>,
    pub session_type: Option<u8>,
    pub lap_num: u8,
    pub lap_time_ms: u32,
    pub clean: bool,
    pub invalid_reason: Option<String>,
    pub track_length_m: f32,
    pub sample_count: usize,
    pub corners: Vec<CornerSummary>,
    pub recommendations: Vec<SetupRecommendation>,
    pub latest_damage: Option<DamageSample>,
    pub latest_status: Option<StatusSample>,
    pub latest_setup: Option<CarSetupSample>,
}

impl CompletedLapAnalysis {
    pub fn to_markdown(&self) -> String {
        let mut out = String::new();
        let _ = writeln!(out, "# Sim MOZA Bridge Analysis");
        let _ = writeln!(out);
        let _ = writeln!(out, "## Latest Lap");
        let _ = writeln!(out);
        let _ = writeln!(out, "- Lap: {}", self.lap_num);
        let _ = writeln!(out, "- Time: {}", format_lap_time(self.lap_time_ms));
        let _ = writeln!(out, "- Clean: {}", if self.clean { "yes" } else { "no" });
        if let Some(reason) = &self.invalid_reason {
            let _ = writeln!(out, "- Reason: {reason}");
        }
        let _ = writeln!(out, "- Samples: {}", self.sample_count);
        let _ = writeln!(out, "- Track length: {:.0} m", self.track_length_m);

        if let Some(status) = &self.latest_status {
            let _ = writeln!(out);
            let _ = writeln!(out, "## Current Car State");
            let _ = writeln!(out);
            if let Some(fuel_delta_laps) = status.fuel_delta_laps {
                let _ = writeln!(out, "- Fuel delta: {fuel_delta_laps:+.2} laps");
            }
            let _ = writeln!(out, "- Brake bias: {}%", status.front_brake_bias);
            let _ = writeln!(out, "- ERS: {:.1}%", status.ers_percent());
            let _ = writeln!(
                out,
                "- ERS harvested this lap: {:.0} J (MGU-K {:.0} / MGU-H {:.0})",
                status.ers_harvested_this_lap(),
                status.ers_harvested_this_lap_mguk,
                status.ers_harvested_this_lap_mguh
            );
            if let Some(limit) = status.ers_harvest_limit_per_lap {
                let _ = writeln!(out, "- ERS harvest limit this lap: {limit:.0} J");
            }
            let _ = writeln!(
                out,
                "- ERS deployed this lap: {:.0} J",
                status.ers_deployed_this_lap
            );
            let _ = writeln!(out, "- Tyre age: {} laps", status.tyres_age_laps);
        }

        if let Some(damage) = &self.latest_damage {
            let _ = writeln!(out);
            let _ = writeln!(out, "## Tyre Wear");
            let _ = writeln!(out);
            let _ = writeln!(
                out,
                "- FL {:.1}% / FR {:.1}% / RL {:.1}% / RR {:.1}%",
                damage.tyre_wear.fl, damage.tyre_wear.fr, damage.tyre_wear.rl, damage.tyre_wear.rr
            );
        }

        if let Some(setup) = &self.latest_setup {
            let _ = writeln!(out);
            let _ = writeln!(out, "## Current Setup");
            let _ = writeln!(out);
            let _ = writeln!(out, "- Wings: {} / {}", setup.front_wing, setup.rear_wing);
            let _ = writeln!(
                out,
                "- Differential: {}% on / {}% off",
                setup.on_throttle_differential_percent, setup.off_throttle_differential_percent
            );
            let _ = writeln!(
                out,
                "- Suspension: {} / {}, anti-roll bars: {} / {}",
                setup.front_suspension,
                setup.rear_suspension,
                setup.front_anti_roll_bar,
                setup.rear_anti_roll_bar
            );
            let _ = writeln!(
                out,
                "- Ride height: {} / {}, brake pressure: {}%, brake bias: {}%, engine braking: {}%",
                setup.front_ride_height,
                setup.rear_ride_height,
                setup.brake_pressure_percent,
                setup.brake_bias_percent,
                setup.engine_braking_percent
            );
            let _ = writeln!(
                out,
                "- Tyre pressures FL {:.1} / FR {:.1} / RL {:.1} / RR {:.1} PSI",
                setup.tyre_pressures_psi.fl,
                setup.tyre_pressures_psi.fr,
                setup.tyre_pressures_psi.rl,
                setup.tyre_pressures_psi.rr
            );
        }

        let _ = writeln!(out);
        let _ = writeln!(out, "## Segment Trace");
        let _ = writeln!(out);
        let _ = writeln!(
            out,
            "| Segment | Range | Samples | Speed min/avg/max | Brake max | Throttle max | Steer avg/max | Phase |"
        );
        let _ = writeln!(out, "| --- | --- | ---: | --- | ---: | ---: | --- | --- |");
        for corner in &self.corners {
            let _ = writeln!(
                out,
                "| {} | {:.0}-{:.0}m | {} | {}/{:.0}/{} | {:.0}% | {:.0}% | {:.2}/{:.2} | {} |",
                corner.segment,
                corner.start_m,
                corner.end_m,
                corner.samples,
                corner.min_speed_kmh,
                corner.avg_speed_kmh,
                corner.max_speed_kmh,
                corner.max_brake * 100.0,
                corner.max_throttle * 100.0,
                corner.avg_abs_steer,
                corner.max_abs_steer,
                corner.phase
            );
        }

        let _ = writeln!(out);
        let _ = writeln!(out, "## Setup Candidates");
        let _ = writeln!(out);
        if !self.clean {
            let _ = writeln!(
                out,
                "Not evaluated because this lap was not clean. Record a clean lap before applying setup changes."
            );
        } else if self.recommendations.is_empty() {
            let _ = writeln!(
                out,
                "No strong setup candidate yet. Record more clean laps on the same fuel and tyre stint."
            );
        } else {
            for rec in &self.recommendations {
                let _ = writeln!(
                    out,
                    "- **{}** ({}) - {} Action: {}",
                    rec.area, rec.confidence, rec.reason, rec.action
                );
            }
        }

        let _ = writeln!(out);
        let _ = writeln!(
            out,
            "Note: these are telemetry heuristics, not an automatic setup solver. Confirm changes with A/B laps."
        );
        out
    }
}

#[derive(Default)]
pub struct TelemetryAnalyzer {
    session_uid: Option<u64>,
    last_session_time: Option<f32>,
    latest_input: Option<InputSample>,
    latest_lap: Option<LapSample>,
    latest_session: Option<SessionSample>,
    latest_damage: Option<DamageSample>,
    latest_status: Option<StatusSample>,
    latest_setup: Option<CarSetupSample>,
    current_points: Vec<TracePoint>,
    current_lap_invalid: bool,
    pit_seen: bool,
    last_completed_lap: Option<u8>,
}

impl TelemetryAnalyzer {
    pub fn ingest(&mut self, update: &TelemetryUpdate) -> Option<CompletedLapAnalysis> {
        if let Some(session_uid) = update.session_uid {
            if self
                .session_uid
                .is_some_and(|previous| previous != session_uid)
            {
                *self = Self::default();
            }
            self.session_uid = Some(session_uid);
        }

        let update_session_time = newest_update_session_time(update);
        let flashback = self
            .last_session_time
            .zip(update_session_time)
            .is_some_and(|(previous, current)| current + 1.0 < previous);
        if flashback && let Some(session_time) = update_session_time {
            self.rewind_timeline(update, session_time);
            self.last_session_time = Some(session_time);
        } else if let Some(session_time) = update_session_time {
            self.last_session_time = Some(
                self.last_session_time
                    .map_or(session_time, |previous| previous.max(session_time)),
            );
        }

        if let Some(session) = &update.session {
            self.latest_session = Some(session.clone());
        }
        if let Some(damage) = &update.damage {
            self.latest_damage = Some(damage.clone());
        }
        if let Some(status) = &update.status {
            self.latest_status = Some(status.clone());
        }
        if let Some(setup) = &update.setup {
            self.latest_setup = Some(setup.clone());
        }

        let mut completed = update
            .lap
            .as_ref()
            .and_then(|lap| self.ingest_lap(lap.clone()));

        if let Some(input) = &update.input {
            self.latest_input = Some(input.clone());
            self.push_trace_point(input);
        }

        if completed.is_none()
            && let Some(final_classification) = &update.final_classification
            && matches!(final_classification.result_status, 3..=7)
        {
            completed = self.complete_final_lap(final_classification.num_laps);
        }

        completed
    }

    fn ingest_lap(&mut self, lap: LapSample) -> Option<CompletedLapAnalysis> {
        if self.session_uid.is_none()
            && self
                .latest_lap
                .as_ref()
                .is_some_and(|previous| lap.session_time + 5.0 < previous.session_time)
        {
            *self = Self::default();
        }

        if let Some(previous) = &self.latest_lap
            && lap.session_time < previous.session_time
            && lap.frame_identifier < previous.frame_identifier
        {
            return None;
        }

        let completed = self
            .latest_lap
            .as_ref()
            .filter(|previous| is_new_lap(previous, &lap, self.track_length_m()))
            .and_then(|previous| {
                if self.last_completed_lap == Some(previous.current_lap_num) {
                    return None;
                }
                let lap_time_ms = if lap.last_lap_time_ms > 0 {
                    lap.last_lap_time_ms
                } else {
                    previous.current_lap_time_ms
                };
                Some(self.complete_lap(previous.current_lap_num, lap_time_ms))
            });

        if completed.is_some() {
            self.last_completed_lap = completed.as_ref().map(|lap| lap.lap_num);
            self.current_points.clear();
            self.current_lap_invalid = false;
            self.pit_seen = false;
        }

        self.current_lap_invalid |= lap.current_lap_invalid;
        self.pit_seen |= lap.pit_status != 0;
        self.latest_lap = Some(lap);
        completed
    }

    fn complete_final_lap(&mut self, final_lap_num: u8) -> Option<CompletedLapAnalysis> {
        if final_lap_num == 0
            || self.last_completed_lap == Some(final_lap_num)
            || self.current_points.len() < MIN_REPORT_SAMPLES
        {
            return None;
        }
        let latest_lap = self.latest_lap.as_ref()?;
        if !matches!(latest_lap.current_lap_num, value if value == final_lap_num || value == final_lap_num.saturating_add(1))
        {
            return None;
        }
        let lap_time_ms = if latest_lap.current_lap_num == final_lap_num {
            latest_lap.current_lap_time_ms
        } else {
            latest_lap.last_lap_time_ms
        };
        if !(10_000..600_000).contains(&lap_time_ms) {
            return None;
        }

        let completed = self.complete_lap(final_lap_num, lap_time_ms);
        self.last_completed_lap = Some(final_lap_num);
        self.current_points.clear();
        self.current_lap_invalid = false;
        self.pit_seen = false;
        Some(completed)
    }

    fn push_trace_point(&mut self, input: &InputSample) {
        let Some(lap) = &self.latest_lap else {
            return;
        };

        if lap.lap_distance_m < 0.0 || lap.current_lap_num == 0 {
            return;
        }

        self.current_points.push(TracePoint {
            session_time: input.session_time,
            lap_distance_m: lap.lap_distance_m,
            speed_kmh: input.speed_kmh,
            throttle: input.throttle,
            brake: input.brake,
            steer: input.steer,
        });
    }

    fn rewind_timeline(&mut self, update: &TelemetryUpdate, target_session_time: f32) {
        let previous_lap_num = self.latest_lap.as_ref().map(|lap| lap.current_lap_num);
        let target_lap_num = update
            .lap
            .as_ref()
            .map(|lap| lap.current_lap_num)
            .or(previous_lap_num);

        if target_lap_num == previous_lap_num {
            self.current_points
                .retain(|point| point.session_time <= target_session_time + 0.05);
        } else {
            self.current_points.clear();
            self.last_completed_lap = target_lap_num.map(|lap| lap.saturating_sub(1));
        }

        self.latest_input = None;
        self.latest_lap = update.lap.clone();
        self.latest_damage = None;
        self.latest_status = None;
        self.current_lap_invalid = update
            .lap
            .as_ref()
            .is_some_and(|lap| lap.current_lap_invalid);
        self.pit_seen = update.lap.as_ref().is_some_and(|lap| lap.pit_status != 0);
    }

    fn complete_lap(&self, lap_num: u8, lap_time_ms: u32) -> CompletedLapAnalysis {
        let track_length_m = self.track_length_m();
        let enough_samples = self.current_points.len() >= MIN_REPORT_SAMPLES;
        let full_lap_coverage = trace_covers_full_lap(&self.current_points, track_length_m);
        let clean =
            !self.current_lap_invalid && !self.pit_seen && enough_samples && full_lap_coverage;
        let invalid_reason = if clean {
            None
        } else if self.current_lap_invalid {
            Some("current lap was marked invalid by F1 25".to_owned())
        } else if self.pit_seen {
            Some("pit status was active during the lap".to_owned())
        } else if !full_lap_coverage {
            Some("trace did not cover the complete lap from start to finish".to_owned())
        } else {
            Some(format!(
                "not enough trace samples: {} < {}",
                self.current_points.len(),
                MIN_REPORT_SAMPLES
            ))
        };
        let corners = summarize_corners(&self.current_points, track_length_m);
        let recommendations = if clean {
            recommend_setup(
                &corners,
                &self.latest_damage,
                &self.latest_status,
                &self.latest_input,
                &self.latest_setup,
            )
        } else {
            Vec::new()
        };

        CompletedLapAnalysis {
            session_uid: self.session_uid,
            session_type: self
                .latest_session
                .as_ref()
                .map(|session| session.session_type),
            lap_num,
            lap_time_ms,
            clean,
            invalid_reason,
            track_length_m,
            sample_count: self.current_points.len(),
            corners,
            recommendations,
            latest_damage: self.latest_damage.clone(),
            latest_status: self.latest_status.clone(),
            latest_setup: self.latest_setup.clone(),
        }
    }

    fn track_length_m(&self) -> f32 {
        self.latest_session
            .as_ref()
            .map(|session| session.track_length_m as f32)
            .filter(|length| *length > 0.0)
            .or_else(|| {
                self.current_points
                    .iter()
                    .map(|point| point.lap_distance_m)
                    .max_by(|left, right| left.total_cmp(right))
            })
            .unwrap_or(1.0)
            .max(1.0)
    }
}

fn newest_update_session_time(update: &TelemetryUpdate) -> Option<f32> {
    [
        update.input.as_ref().map(|sample| sample.session_time),
        update.lap.as_ref().map(|sample| sample.session_time),
        update.session.as_ref().map(|sample| sample.session_time),
        update.damage.as_ref().map(|sample| sample.session_time),
        update.status.as_ref().map(|sample| sample.session_time),
        update.setup.as_ref().map(|sample| sample.session_time),
        update.tyre_sets.as_ref().map(|sample| sample.session_time),
        update
            .final_classification
            .as_ref()
            .map(|sample| sample.session_time),
    ]
    .into_iter()
    .flatten()
    .filter(|value| value.is_finite() && *value >= 0.0)
    .max_by(|left, right| left.total_cmp(right))
}

fn trace_covers_full_lap(points: &[TracePoint], track_length_m: f32) -> bool {
    if track_length_m < 1_000.0 {
        return false;
    }
    let edge_window_m = (track_length_m * 0.05).clamp(100.0, 300.0);
    let min_distance = points
        .iter()
        .map(|point| point.lap_distance_m)
        .min_by(|left, right| left.total_cmp(right));
    let max_distance = points
        .iter()
        .map(|point| point.lap_distance_m)
        .max_by(|left, right| left.total_cmp(right));
    min_distance.is_some_and(|distance| distance <= edge_window_m)
        && max_distance.is_some_and(|distance| distance >= track_length_m - edge_window_m)
}

fn is_new_lap(previous: &LapSample, current: &LapSample, track_length_m: f32) -> bool {
    if current.current_lap_num > previous.current_lap_num {
        return true;
    }

    if current.current_lap_num != previous.current_lap_num || track_length_m < 1_000.0 {
        return false;
    }

    let wrap_window_m = (track_length_m * 0.08).clamp(150.0, 500.0);
    previous.lap_distance_m > track_length_m - wrap_window_m
        && current.lap_distance_m < wrap_window_m
}

fn summarize_corners(points: &[TracePoint], track_length_m: f32) -> Vec<CornerSummary> {
    let mut bins = vec![SegmentAccumulator::default(); SEGMENT_COUNT];
    let segment_width = track_length_m / SEGMENT_COUNT as f32;

    for point in points {
        let mut segment = (point.lap_distance_m / segment_width).floor() as usize;
        if segment >= SEGMENT_COUNT {
            segment = SEGMENT_COUNT - 1;
        }
        bins[segment].push(point);
    }

    bins.into_iter()
        .enumerate()
        .filter_map(|(index, bin)| bin.into_summary(index + 1, segment_width))
        .collect()
}

fn recommend_setup(
    corners: &[CornerSummary],
    damage: &Option<DamageSample>,
    status: &Option<StatusSample>,
    input: &Option<InputSample>,
    setup: &Option<CarSetupSample>,
) -> Vec<SetupRecommendation> {
    let mut recommendations = Vec::new();
    let front_wear_delta = damage
        .as_ref()
        .map(|sample| sample.tyre_wear.front_avg() - sample.tyre_wear.rear_avg())
        .unwrap_or(0.0);
    let rear_wear_delta = -front_wear_delta;
    let front_temp_delta = input
        .as_ref()
        .map(|sample| {
            sample.tyre_surface_temps_c.front_avg() - sample.tyre_surface_temps_c.rear_avg()
        })
        .unwrap_or(0.0);
    let rear_temp_delta = -front_temp_delta;

    let mid_understeer = corners.iter().any(|corner| {
        corner.phase == "mid"
            && corner.avg_abs_steer > 0.22
            && corner.max_throttle < 0.55
            && corner.avg_speed_kmh < 210.0
    });
    if mid_understeer || front_wear_delta > 2.0 || front_temp_delta > 5.0 {
        let action = setup.as_ref().map_or_else(
            || "프런트 윙을 한 클릭 올리고, 나머지는 유지한 채 클린 랩 두 개로 비교".to_owned(),
            |setup| {
                format!(
                    "프런트 윙 {}에서 {}. 나머지는 유지하고 클린 랩 두 개로 비교",
                    setup.front_wing,
                    setup.front_wing.saturating_add(1)
                )
            },
        );
        recommendations.push(SetupRecommendation {
            area: "Mid-corner front grip".to_owned(),
            reason: format!(
                "front-limited signal: mid-corner steering demand={}, front wear delta={:.1}%, front temp delta={:.1}C.",
                yes_no(mid_understeer),
                front_wear_delta,
                front_temp_delta
            ),
            action,
            confidence: confidence(front_wear_delta.abs().max(front_temp_delta.abs()), 2.0, 5.0)
                .to_owned(),
        });
    }

    let exit_instability = corners.iter().any(|corner| {
        corner.phase == "exit"
            && corner.max_throttle > 0.75
            && corner.avg_abs_steer > 0.14
            && corner.avg_speed_kmh < 230.0
    });
    if exit_instability || rear_wear_delta > 2.0 || rear_temp_delta > 5.0 {
        let action = setup.as_ref().map_or_else(
            || {
                "온스로틀 디퍼렌셜을 5퍼센트 낮추고, 나머지는 유지한 채 클린 랩 두 개로 비교"
                    .to_owned()
            },
            |setup| {
                format!(
                    "온스로틀 디퍼렌셜 {}에서 {}. 나머지는 유지하고 클린 랩 두 개로 비교",
                    setup.on_throttle_differential_percent,
                    setup.on_throttle_differential_percent.saturating_sub(5)
                )
            },
        );
        recommendations.push(SetupRecommendation {
            area: "Corner exit traction".to_owned(),
            reason: format!(
                "rear-limited signal: exit correction={}, rear wear delta={:.1}%, rear temp delta={:.1}C.",
                yes_no(exit_instability),
                rear_wear_delta,
                rear_temp_delta
            ),
            action,
            confidence: confidence(rear_wear_delta.abs().max(rear_temp_delta.abs()), 2.0, 5.0)
                .to_owned(),
        });
    }

    let entry_load = corners.iter().any(|corner| {
        corner.phase == "entry" && corner.max_brake > 0.75 && corner.max_abs_steer > 0.20
    });
    if entry_load {
        let bias = status
            .as_ref()
            .map(|sample| format!(" current brake bias is {}%.", sample.front_brake_bias))
            .unwrap_or_default();
        recommendations.push(SetupRecommendation {
            area: "Corner entry braking".to_owned(),
            reason: format!("high brake plus steering overlap appeared in entry segments.{bias}"),
            action: setup.as_ref().map_or_else(
                || {
                    "세팅은 유지하고 브레이크를 조금 일찍 풀어 한 랩 확인한 뒤 바이어스 변경 판단"
                        .to_owned()
                },
                |setup| {
                    format!(
                        "브레이크 바이어스 {}퍼센트 유지. 브레이크를 조금 일찍 풀어 한 랩 확인",
                        setup.brake_bias_percent
                    )
                },
            ),
            confidence: "medium".to_owned(),
        });
    }

    if recommendations.is_empty()
        && corners
            .iter()
            .filter(|corner| corner.phase != "straight")
            .count()
            >= 3
    {
        recommendations.push(SetupRecommendation {
            area: "Baseline validation".to_owned(),
            reason: "trace has corner samples but no strong imbalance crossed the threshold."
                .to_owned(),
            action: "세팅을 유지하고 같은 연료, 타이어 상태, ERS 모드로 클린 랩을 추가 확보"
                .to_owned(),
            confidence: "low".to_owned(),
        });
    }

    recommendations
}

fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

fn confidence(value: f32, medium: f32, high: f32) -> &'static str {
    if value >= high {
        "high"
    } else if value >= medium {
        "medium"
    } else {
        "low"
    }
}

fn classify_phase(max_brake: f32, max_throttle: f32, avg_abs_steer: f32) -> String {
    if max_brake > 0.45 && avg_abs_steer > 0.06 {
        "entry".to_owned()
    } else if avg_abs_steer > 0.12 && max_throttle < 0.60 {
        "mid".to_owned()
    } else if max_throttle > 0.55 && avg_abs_steer > 0.05 {
        "exit".to_owned()
    } else {
        "straight".to_owned()
    }
}

fn format_lap_time(milliseconds: u32) -> String {
    let minutes = milliseconds / 60_000;
    let seconds = (milliseconds % 60_000) / 1_000;
    let millis = milliseconds % 1_000;
    format!("{minutes}:{seconds:02}.{millis:03}")
}

#[derive(Clone, Debug)]
struct SegmentAccumulator {
    samples: usize,
    min_speed_kmh: u16,
    max_speed_kmh: u16,
    speed_sum: u64,
    max_brake: f32,
    max_throttle: f32,
    abs_steer_sum: f32,
    max_abs_steer: f32,
}

impl Default for SegmentAccumulator {
    fn default() -> Self {
        Self {
            samples: 0,
            min_speed_kmh: u16::MAX,
            max_speed_kmh: 0,
            speed_sum: 0,
            max_brake: 0.0,
            max_throttle: 0.0,
            abs_steer_sum: 0.0,
            max_abs_steer: 0.0,
        }
    }
}

impl SegmentAccumulator {
    fn push(&mut self, point: &TracePoint) {
        self.samples += 1;
        self.min_speed_kmh = self.min_speed_kmh.min(point.speed_kmh);
        self.max_speed_kmh = self.max_speed_kmh.max(point.speed_kmh);
        self.speed_sum += point.speed_kmh as u64;
        self.max_brake = self.max_brake.max(point.brake);
        self.max_throttle = self.max_throttle.max(point.throttle);
        self.abs_steer_sum += point.steer.abs();
        self.max_abs_steer = self.max_abs_steer.max(point.steer.abs());
    }

    fn into_summary(self, index: usize, segment_width: f32) -> Option<CornerSummary> {
        if self.samples == 0 {
            return None;
        }

        let avg_abs_steer = self.abs_steer_sum / self.samples as f32;
        Some(CornerSummary {
            segment: index,
            start_m: (index - 1) as f32 * segment_width,
            end_m: index as f32 * segment_width,
            samples: self.samples,
            min_speed_kmh: self.min_speed_kmh,
            max_speed_kmh: self.max_speed_kmh,
            avg_speed_kmh: self.speed_sum as f32 / self.samples as f32,
            max_brake: self.max_brake,
            max_throttle: self.max_throttle,
            avg_abs_steer,
            max_abs_steer: self.max_abs_steer,
            phase: classify_phase(self.max_brake, self.max_throttle, avg_abs_steer),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::telemetry::{FinalClassificationSample, TelemetryUpdate, WheelValuesF32};

    fn lap_sample(lap_num: u8, lap_distance_m: f32, invalid: bool) -> LapSample {
        LapSample {
            session_time: 0.0,
            frame_identifier: 0,
            overall_frame_identifier: None,
            player_car_index: 0,
            last_lap_time_ms: 90_000,
            current_lap_time_ms: 10_000,
            lap_distance_m,
            total_distance_m: lap_distance_m,
            car_position: 1,
            current_lap_num: lap_num,
            pit_status: 0,
            num_pit_stops: 0,
            sector: 1,
            current_lap_invalid: invalid,
            driver_status: 4,
            result_status: 2,
            delta_to_car_in_front_ms: None,
            car_in_front_index: None,
            delta_to_car_behind_ms: None,
            car_behind_index: None,
            delta_to_race_leader_ms: None,
            safety_car_delta_s: None,
            sector1_time_ms: None,
            sector2_time_ms: None,
        }
    }

    #[test]
    fn summarizes_trace_into_segments() {
        let points = vec![
            TracePoint {
                session_time: 1.0,
                lap_distance_m: 100.0,
                speed_kmh: 90,
                throttle: 0.0,
                brake: 0.8,
                steer: 0.3,
            },
            TracePoint {
                session_time: 1.1,
                lap_distance_m: 110.0,
                speed_kmh: 100,
                throttle: 0.1,
                brake: 0.6,
                steer: 0.2,
            },
        ];

        let corners = summarize_corners(&points, 2_000.0);

        assert_eq!(corners.len(), 1);
        assert_eq!(corners[0].segment, 2);
        assert_eq!(corners[0].phase, "entry");
        assert_eq!(corners[0].min_speed_kmh, 90);
        assert_eq!(corners[0].max_speed_kmh, 100);
    }

    #[test]
    fn recommends_front_grip_from_wear_delta() {
        let damage = Some(DamageSample {
            session_time: 0.0,
            frame_identifier: 0,
            player_car_index: 0,
            tyre_wear: WheelValuesF32 {
                fl: 18.0,
                fr: 17.0,
                rl: 10.0,
                rr: 11.0,
            },
            tyre_damage: crate::telemetry::WheelValuesU8 {
                fl: 0,
                fr: 0,
                rl: 0,
                rr: 0,
            },
            tyre_blisters: crate::telemetry::WheelValuesU8 {
                fl: 0,
                fr: 0,
                rl: 0,
                rr: 0,
            },
            front_left_wing_damage: 0,
            front_right_wing_damage: 0,
            rear_wing_damage: 0,
            gearbox_damage: 0,
            engine_damage: 0,
        });

        let recommendations = recommend_setup(&[], &damage, &None, &None, &None);

        assert!(
            recommendations
                .iter()
                .any(|rec| rec.area == "Mid-corner front grip")
        );
    }

    #[test]
    fn does_not_complete_lap_on_mid_lap_distance_drop() {
        let mut analyzer = TelemetryAnalyzer::default();
        analyzer.ingest(&TelemetryUpdate {
            session: Some(SessionSample {
                session_time: 0.0,
                frame_identifier: 0,
                overall_frame_identifier: None,
                weather: 0,
                total_laps: 10,
                track_length_m: 5_000,
                session_type: 10,
                track_id: 1,
                track_temp_c: 0,
                air_temp_c: 0,
                session_time_left_s: 0,
                pit_speed_limit_kmh: 0,
                safety_car_status: 0,
                marshal_zones: Vec::new(),
                weather_forecast_samples: Vec::new(),
                pit_stop_window_ideal_lap: None,
                pit_stop_window_latest_lap: None,
                pit_stop_rejoin_position: None,
            }),
            ..TelemetryUpdate::default()
        });

        analyzer.ingest(&TelemetryUpdate {
            lap: Some(lap_sample(3, 1_200.0, false)),
            ..TelemetryUpdate::default()
        });
        let completed = analyzer.ingest(&TelemetryUpdate {
            lap: Some(lap_sample(3, 300.0, false)),
            ..TelemetryUpdate::default()
        });

        assert!(completed.is_none());
    }

    #[test]
    fn allows_same_lap_wrap_only_near_start_finish() {
        let mut analyzer = TelemetryAnalyzer::default();
        analyzer.ingest(&TelemetryUpdate {
            session: Some(SessionSample {
                session_time: 0.0,
                frame_identifier: 0,
                overall_frame_identifier: None,
                weather: 0,
                total_laps: 10,
                track_length_m: 5_000,
                session_type: 10,
                track_id: 1,
                track_temp_c: 0,
                air_temp_c: 0,
                session_time_left_s: 0,
                pit_speed_limit_kmh: 0,
                safety_car_status: 0,
                marshal_zones: Vec::new(),
                weather_forecast_samples: Vec::new(),
                pit_stop_window_ideal_lap: None,
                pit_stop_window_latest_lap: None,
                pit_stop_rejoin_position: None,
            }),
            ..TelemetryUpdate::default()
        });

        analyzer.ingest(&TelemetryUpdate {
            lap: Some(lap_sample(3, 4_900.0, false)),
            ..TelemetryUpdate::default()
        });
        let completed = analyzer.ingest(&TelemetryUpdate {
            lap: Some(lap_sample(3, 120.0, false)),
            ..TelemetryUpdate::default()
        });

        assert!(completed.is_some());
    }

    #[test]
    fn final_classification_flushes_the_last_lap_once() {
        let mut analyzer = TelemetryAnalyzer {
            latest_lap: Some(LapSample {
                current_lap_time_ms: 91_500,
                ..lap_sample(53, 4_900.0, false)
            }),
            current_points: vec![
                TracePoint {
                    session_time: 5_400.0,
                    lap_distance_m: 100.0,
                    speed_kmh: 150,
                    throttle: 0.5,
                    brake: 0.0,
                    steer: 0.1,
                };
                MIN_REPORT_SAMPLES
            ],
            ..TelemetryAnalyzer::default()
        };
        let final_classification = FinalClassificationSample {
            session_time: 5_400.0,
            frame_identifier: 90_000,
            player_car_index: 0,
            position: 1,
            num_laps: 53,
            grid_position: 4,
            points: 25,
            num_pit_stops: 1,
            result_status: 3,
            result_reason: 2,
            best_lap_time_ms: 89_000,
            total_race_time_s: 5_300.0,
            penalties_time_s: 0,
            num_penalties: 0,
            num_tyre_stints: 2,
            tyre_stints_actual: [0; 8],
            tyre_stints_visual: [0; 8],
            tyre_stints_end_laps: [0; 8],
        };

        let completed = analyzer.ingest(&TelemetryUpdate {
            final_classification: Some(final_classification.clone()),
            ..TelemetryUpdate::default()
        });
        let repeated = analyzer.ingest(&TelemetryUpdate {
            final_classification: Some(final_classification),
            ..TelemetryUpdate::default()
        });

        assert_eq!(completed.as_ref().map(|lap| lap.lap_num), Some(53));
        assert_eq!(completed.as_ref().map(|lap| lap.lap_time_ms), Some(91_500));
        assert!(repeated.is_none());
    }

    #[test]
    fn does_not_recommend_setup_for_invalid_laps() {
        let mut analyzer = TelemetryAnalyzer::default();
        analyzer.current_points = vec![
            TracePoint {
                session_time: 1.0,
                lap_distance_m: 100.0,
                speed_kmh: 100,
                throttle: 0.1,
                brake: 0.8,
                steer: 0.3,
            };
            MIN_REPORT_SAMPLES
        ];
        analyzer.current_lap_invalid = true;
        analyzer.latest_damage = Some(DamageSample {
            session_time: 0.0,
            frame_identifier: 0,
            player_car_index: 0,
            tyre_wear: WheelValuesF32 {
                fl: 20.0,
                fr: 20.0,
                rl: 10.0,
                rr: 10.0,
            },
            tyre_damage: crate::telemetry::WheelValuesU8 {
                fl: 0,
                fr: 0,
                rl: 0,
                rr: 0,
            },
            tyre_blisters: crate::telemetry::WheelValuesU8 {
                fl: 0,
                fr: 0,
                rl: 0,
                rr: 0,
            },
            front_left_wing_damage: 0,
            front_right_wing_damage: 0,
            rear_wing_damage: 0,
            gearbox_damage: 0,
            engine_damage: 0,
        });

        let analysis = analyzer.complete_lap(4, 90_000);

        assert!(!analysis.clean);
        assert!(analysis.recommendations.is_empty());
        assert!(analysis.to_markdown().contains("Not evaluated"));
    }

    #[test]
    fn completed_lap_preserves_session_context() {
        let analyzer = TelemetryAnalyzer {
            session_uid: Some(5_154_468_281_529_202_801),
            latest_session: Some(SessionSample {
                session_time: 10.0,
                frame_identifier: 100,
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
            current_points: vec![
                TracePoint {
                    session_time: 1.0,
                    lap_distance_m: 100.0,
                    speed_kmh: 150,
                    throttle: 0.5,
                    brake: 0.0,
                    steer: 0.1,
                };
                MIN_REPORT_SAMPLES
            ],
            ..TelemetryAnalyzer::default()
        };

        let completed = analyzer.complete_lap(3, 90_000);

        assert_eq!(completed.session_uid, Some(5_154_468_281_529_202_801));
        assert_eq!(completed.session_type, Some(15));
    }

    #[test]
    fn flashback_discards_superseded_corner_trace_points() {
        let mut previous_lap = lap_sample(12, 2_200.0, false);
        previous_lap.session_time = 1_074.0;
        let mut target_lap = lap_sample(12, 1_700.0, false);
        target_lap.session_time = 1_064.0;
        let mut analyzer = TelemetryAnalyzer {
            session_uid: Some(42),
            last_session_time: Some(1_074.0),
            latest_lap: Some(previous_lap),
            current_points: vec![
                TracePoint {
                    session_time: 1_060.0,
                    lap_distance_m: 1_500.0,
                    speed_kmh: 150,
                    throttle: 0.5,
                    brake: 0.0,
                    steer: 0.1,
                },
                TracePoint {
                    session_time: 1_070.0,
                    lap_distance_m: 2_000.0,
                    speed_kmh: 120,
                    throttle: 0.0,
                    brake: 0.0,
                    steer: -0.6,
                },
            ],
            ..TelemetryAnalyzer::default()
        };

        analyzer.rewind_timeline(
            &TelemetryUpdate {
                session_uid: Some(42),
                lap: Some(target_lap),
                ..TelemetryUpdate::default()
            },
            1_064.0,
        );

        assert_eq!(analyzer.current_points.len(), 1);
        assert_eq!(analyzer.current_points[0].session_time, 1_060.0);
        assert_eq!(
            analyzer.latest_lap.as_ref().map(|lap| lap.lap_distance_m),
            Some(1_700.0)
        );
    }
}
