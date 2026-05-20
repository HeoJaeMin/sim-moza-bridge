use std::fmt::Write as _;

use crate::telemetry::{
    DamageSample, InputSample, LapSample, SessionSample, StatusSample, TelemetryUpdate,
};

const SEGMENT_COUNT: usize = 20;
const MIN_REPORT_SAMPLES: usize = 12;

#[derive(Clone, Debug)]
pub struct TracePoint {
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
        "lap,clean,segment,start_m,end_m,samples,min_speed_kmh,max_speed_kmh,avg_speed_kmh,max_brake,max_throttle,avg_abs_steer,max_abs_steer,phase\n"
    }

    pub fn csv_row(&self, lap_num: u8, clean: bool) -> String {
        format!(
            "{},{},{},{:.1},{:.1},{},{},{},{:.1},{:.4},{:.4},{:.4},{:.4},{}\n",
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
            self.phase
        )
    }
}

#[derive(Clone, Debug)]
pub struct SetupRecommendation {
    pub area: String,
    pub reason: String,
    pub action: String,
    pub confidence: &'static str,
}

#[derive(Clone, Debug)]
pub struct CompletedLapAnalysis {
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
            let _ = writeln!(
                out,
                "- Fuel remaining: {:.2} laps",
                status.fuel_remaining_laps
            );
            let _ = writeln!(out, "- Brake bias: {}%", status.front_brake_bias);
            let _ = writeln!(out, "- ERS: {:.1}%", status.ers_percent());
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
    latest_input: Option<InputSample>,
    latest_lap: Option<LapSample>,
    latest_session: Option<SessionSample>,
    latest_damage: Option<DamageSample>,
    latest_status: Option<StatusSample>,
    current_points: Vec<TracePoint>,
    current_lap_invalid: bool,
    pit_seen: bool,
}

impl TelemetryAnalyzer {
    pub fn ingest(&mut self, update: &TelemetryUpdate) -> Option<CompletedLapAnalysis> {
        if let Some(session) = &update.session {
            self.latest_session = Some(session.clone());
        }
        if let Some(damage) = &update.damage {
            self.latest_damage = Some(damage.clone());
        }
        if let Some(status) = &update.status {
            self.latest_status = Some(status.clone());
        }

        let completed = update
            .lap
            .as_ref()
            .and_then(|lap| self.ingest_lap(lap.clone()));

        if let Some(input) = &update.input {
            self.latest_input = Some(input.clone());
            self.push_trace_point(input);
        }

        completed
    }

    fn ingest_lap(&mut self, lap: LapSample) -> Option<CompletedLapAnalysis> {
        let completed = self
            .latest_lap
            .as_ref()
            .filter(|previous| is_new_lap(previous, &lap, self.track_length_m()))
            .map(|previous| {
                let lap_time_ms = if lap.last_lap_time_ms > 0 {
                    lap.last_lap_time_ms
                } else {
                    previous.current_lap_time_ms
                };
                self.complete_lap(previous.current_lap_num, lap_time_ms)
            });

        if completed.is_some() {
            self.current_points.clear();
            self.current_lap_invalid = false;
            self.pit_seen = false;
        }

        self.current_lap_invalid |= lap.current_lap_invalid;
        self.pit_seen |= lap.pit_status != 0;
        self.latest_lap = Some(lap);
        completed
    }

    fn push_trace_point(&mut self, input: &InputSample) {
        let Some(lap) = &self.latest_lap else {
            return;
        };

        if lap.lap_distance_m < 0.0 || lap.current_lap_num == 0 {
            return;
        }

        self.current_points.push(TracePoint {
            lap_distance_m: lap.lap_distance_m,
            speed_kmh: input.speed_kmh,
            throttle: input.throttle,
            brake: input.brake,
            steer: input.steer,
        });
    }

    fn complete_lap(&self, lap_num: u8, lap_time_ms: u32) -> CompletedLapAnalysis {
        let track_length_m = self.track_length_m();
        let enough_samples = self.current_points.len() >= MIN_REPORT_SAMPLES;
        let clean = !self.current_lap_invalid && !self.pit_seen && enough_samples;
        let invalid_reason = if clean {
            None
        } else if self.current_lap_invalid {
            Some("current lap was marked invalid by F1 25".to_owned())
        } else if self.pit_seen {
            Some("pit status was active during the lap".to_owned())
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
            )
        } else {
            Vec::new()
        };

        CompletedLapAnalysis {
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
        recommendations.push(SetupRecommendation {
            area: "Mid-corner front grip".to_owned(),
            reason: format!(
                "front-limited signal: mid-corner steering demand={}, front wear delta={:.1}%, front temp delta={:.1}C.",
                yes_no(mid_understeer),
                front_wear_delta,
                front_temp_delta
            ),
            action: "Try front wing +1, soften front anti-roll bar one click, or reduce front tyre pressure by 0.1 PSI.".to_owned(),
            confidence: confidence(front_wear_delta.abs().max(front_temp_delta.abs()), 2.0, 5.0),
        });
    }

    let exit_instability = corners.iter().any(|corner| {
        corner.phase == "exit"
            && corner.max_throttle > 0.75
            && corner.avg_abs_steer > 0.14
            && corner.avg_speed_kmh < 230.0
    });
    if exit_instability || rear_wear_delta > 2.0 || rear_temp_delta > 5.0 {
        recommendations.push(SetupRecommendation {
            area: "Corner exit traction".to_owned(),
            reason: format!(
                "rear-limited signal: exit correction={}, rear wear delta={:.1}%, rear temp delta={:.1}C.",
                yes_no(exit_instability),
                rear_wear_delta,
                rear_temp_delta
            ),
            action: "Try on-throttle differential -3 to -5, rear wing +1, or rear tyre pressure -0.1 PSI.".to_owned(),
            confidence: confidence(rear_wear_delta.abs().max(rear_temp_delta.abs()), 2.0, 5.0),
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
            reason: format!(
                "high brake plus steering overlap appeared in entry segments.{bias}"
            ),
            action: "If the rear rotates, move brake bias +1 forward or increase off-throttle diff slightly; if it pushes wide, move bias -1 and trail off brake earlier.".to_owned(),
            confidence: "medium",
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
            reason: "trace has corner samples but no strong imbalance crossed the threshold.".to_owned(),
            action: "Keep setup unchanged and record another clean lap with the same fuel, tyre age, and ERS mode.".to_owned(),
            confidence: "low",
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
    use crate::telemetry::{TelemetryUpdate, WheelValuesF32};

    fn lap_sample(lap_num: u8, lap_distance_m: f32, invalid: bool) -> LapSample {
        LapSample {
            session_time: 0.0,
            frame_identifier: 0,
            player_car_index: 0,
            last_lap_time_ms: 90_000,
            current_lap_time_ms: 10_000,
            lap_distance_m,
            total_distance_m: lap_distance_m,
            car_position: 1,
            current_lap_num: lap_num,
            pit_status: 0,
            sector: 1,
            current_lap_invalid: invalid,
            driver_status: 4,
            result_status: 2,
            delta_to_car_in_front_ms: None,
            delta_to_car_behind_ms: None,
            delta_to_race_leader_ms: None,
            sector1_time_ms: None,
            sector2_time_ms: None,
        }
    }

    #[test]
    fn summarizes_trace_into_segments() {
        let points = vec![
            TracePoint {
                lap_distance_m: 100.0,
                speed_kmh: 90,
                throttle: 0.0,
                brake: 0.8,
                steer: 0.3,
            },
            TracePoint {
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

        let recommendations = recommend_setup(&[], &damage, &None, &None);

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
                total_laps: 10,
                track_length_m: 5_000,
                session_type: 10,
                track_id: 1,
                track_temp_c: 0,
                air_temp_c: 0,
                session_time_left_s: 0,
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
                total_laps: 10,
                track_length_m: 5_000,
                session_type: 10,
                track_id: 1,
                track_temp_c: 0,
                air_temp_c: 0,
                session_time_left_s: 0,
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
    fn does_not_recommend_setup_for_invalid_laps() {
        let mut analyzer = TelemetryAnalyzer::default();
        analyzer.current_points = vec![
            TracePoint {
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
}
