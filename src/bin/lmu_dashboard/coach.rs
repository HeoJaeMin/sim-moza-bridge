use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Serialize;
use tokio::sync::watch;

use crate::model::{ClassLeaderIdentity, LapSummary, SavedLap, TelemetryPoint};
use crate::server::DashboardState;
use crate::store::{DashboardStore, StoredSession, unix_ms};
use crate::telemetry_core::{AnalysisConfidence, AnalysisConfidenceLevel, AnalysisLimitation};
use crate::telemetry_quality::{QualityReason, TraceQualityStatus};

const SEGMENT_COUNT: usize = 20;
const MIN_REPORT_DISTANCE_M: f64 = 500.0;
const MIN_MEANINGFUL_LOSS_MS: f64 = 5.0;

#[derive(Clone, Debug, Serialize)]
pub struct CoachingReport {
    pub schema_version: u8,
    pub status: String,
    pub message: String,
    pub generated_at_unix_ms: u64,
    pub session_id: String,
    pub session_type: String,
    pub track_name: String,
    pub class_name: String,
    pub cohort: CohortSummary,
    pub player: Option<DriverBenchmark>,
    pub actual_p1: Option<DriverBenchmark>,
    pub fastest_captured: Option<DriverBenchmark>,
    pub segments: Vec<SegmentComparison>,
    pub focus_zones: Vec<FocusZone>,
    pub exclusions: Vec<ExclusionSummary>,
    pub limitations: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct CohortSummary {
    pub participant_count: usize,
    pub valid_lap_count: usize,
    pub top_quartile_count: usize,
    pub top_quartile_median_ms: Option<u32>,
}

#[derive(Clone, Debug, Serialize)]
pub struct DriverBenchmark {
    pub driver_name: String,
    pub vehicle_id: i32,
    pub rank: usize,
    pub percentile: f64,
    pub valid_lap_count: usize,
    pub selected_lap_count: usize,
    pub best_lap_ms: u32,
    pub best_two_median_ms: u32,
    pub selected_lap_ids: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct SegmentComparison {
    pub number: usize,
    pub start_m: f64,
    pub end_m: f64,
    pub player_time_ms: f64,
    pub fastest_time_ms: f64,
    pub top_quartile_time_ms: f64,
    pub actual_p1_time_ms: Option<f64>,
    pub loss_to_fastest_ms: f64,
    pub loss_to_top_quartile_ms: f64,
    pub delta_to_actual_p1_ms: Option<f64>,
    pub cumulative_loss_ms: f64,
    pub cumulative_delta_to_actual_p1_ms: Option<f64>,
    pub rank: usize,
    pub percentile: f64,
    pub participant_count: usize,
    pub lap_sample_count: usize,
    pub confidence: AnalysisConfidenceLevel,
    pub pattern: &'static str,
    pub loss_origin: &'static str,
    pub player_min_speed_kmh: f64,
    pub top_quartile_min_speed_kmh: f64,
    pub actual_p1_min_speed_kmh: Option<f64>,
    pub player_brake_onset_m: Option<f64>,
    pub top_quartile_brake_onset_m: Option<f64>,
    pub actual_p1_brake_onset_m: Option<f64>,
    pub player_throttle_commit_m: Option<f64>,
    pub top_quartile_throttle_commit_m: Option<f64>,
    pub actual_p1_throttle_commit_m: Option<f64>,
}

#[derive(Clone, Debug, Serialize)]
pub struct FocusZone {
    pub rank: usize,
    pub segment_number: usize,
    pub start_m: f64,
    pub end_m: f64,
    pub loss_ms: f64,
    pub confidence: AnalysisConfidenceLevel,
    pub pattern: &'static str,
    pub loss_origin: &'static str,
    pub coaching_cues: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ExclusionSummary {
    pub code: String,
    pub count: usize,
    pub description: String,
}

#[derive(Clone, Copy, Debug, Default)]
struct SegmentStats {
    time_ms: f64,
    entry_speed_kmh: f64,
    min_speed_kmh: f64,
    average_throttle: f64,
    max_brake: f64,
    brake_onset_m: Option<f64>,
    throttle_commit_m: Option<f64>,
}

#[derive(Clone)]
struct TraceLap {
    summary: LapSummary,
    segments: Vec<Option<SegmentStats>>,
}

struct DriverAggregate {
    driver_name: String,
    vehicle_id: i32,
    all_lap_count: usize,
    selected: Vec<TraceLap>,
    best_lap_ms: u32,
    median_lap_ms: u32,
    segments: Vec<Option<SegmentStats>>,
}

pub async fn run(
    store: DashboardStore,
    report_path: Option<PathBuf>,
    state: DashboardState,
    mut shutdown: watch::Receiver<bool>,
) {
    let mut last_state = String::new();
    loop {
        if *shutdown.borrow() {
            break;
        }
        let build_store = store.clone();
        let class_leader = state.class_leader().await;
        let result =
            tokio::task::spawn_blocking(move || build_report(&build_store, class_leader.as_ref()))
                .await;
        match result {
            Ok(Ok(report)) => {
                let next_state = report_state(&report);
                if next_state != last_state {
                    match serde_json::to_value(&report) {
                        Ok(value) => state.publish_analysis(value).await,
                        Err(error) => eprintln!("failed to encode coaching API report: {error}"),
                    }
                    if let Some(path) = report_path.as_deref()
                        && let Err(error) = write_report(path, &report)
                    {
                        eprintln!("failed to write coaching report: {error}");
                    }
                    last_state = next_state;
                }
            }
            Ok(Err(error)) => eprintln!("failed to build LMU coaching report: {error}"),
            Err(error) => eprintln!("LMU coaching worker stopped unexpectedly: {error}"),
        }
        tokio::select! {
            () = tokio::time::sleep(Duration::from_secs(2)) => {}
            result = shutdown.changed() => {
                if result.is_err() || *shutdown.borrow() {
                    break;
                }
            }
        }
    }
}

fn build_report(
    store: &DashboardStore,
    live_class_leader: Option<&ClassLeaderIdentity>,
) -> Result<CoachingReport, String> {
    let Some(session) = store.latest_session()? else {
        return Ok(waiting_report(
            None,
            "waiting_for_session",
            "LMU 세션을 기다리고 있습니다.",
        ));
    };
    let session_laps = store
        .list_laps()?
        .into_iter()
        .filter(|lap| lap.session_id == session.id)
        .collect::<Vec<_>>();
    let player_metadata = match live_class_leader {
        Some(identity) => session_laps.iter().find(|lap| {
            lap.is_player
                && lap.vehicle_id == identity.player_vehicle_id
                && lap.driver_name == identity.player_driver_name
        }),
        None => session_laps
            .iter()
            .filter(|lap| lap.is_player)
            .max_by_key(|lap| lap.created_at_unix_ms),
    };
    let Some(player_metadata) = player_metadata else {
        return Ok(waiting_report(
            Some(&session),
            "waiting_for_player_lap",
            &format!(
                "최신 {} 세션의 플레이어 랩을 기다리고 있습니다.",
                session.session_type
            ),
        ));
    };
    let class_name = player_metadata.class_name.trim().to_owned();
    let player_vehicle_id = player_metadata.vehicle_id;
    let player_driver_name = player_metadata.driver_name.clone();
    let actual_p1_identity = live_class_leader.filter(|leader| {
        leader.session_id == session.id && same_class(&leader.class_name, &class_name)
    });
    let mut exclusions = BTreeMap::<String, usize>::new();
    let mut traces = Vec::new();

    for summary in session_laps {
        if !same_class(&summary.class_name, &class_name) {
            count_exclusion(&mut exclusions, "other_class");
            continue;
        }
        if exclude_for_quality(&summary, &mut exclusions) {
            continue;
        }
        let Some(lap) = store.load_lap(&summary.id)? else {
            count_exclusion(&mut exclusions, "missing_trace");
            continue;
        };
        match prepare_trace(lap, session.track_length_m) {
            Ok(trace) => traces.push(trace),
            Err(code) => count_exclusion(&mut exclusions, code),
        }
    }

    let mut drivers = aggregate_drivers(traces, session.track_length_m);
    drivers.sort_by_key(|driver| driver.median_lap_ms);
    if drivers.is_empty() {
        let mut report = waiting_report(
            Some(&session),
            "waiting_for_valid_laps",
            &format!(
                "최신 {} 세션에 비교 가능한 동일 클래스 완주 랩이 없습니다.",
                session.session_type
            ),
        );
        report.class_name = class_name;
        report.exclusions = exclusion_summaries(&exclusions);
        return Ok(report);
    }

    let participant_count = drivers.len();
    let top_quartile_count = participant_count.div_ceil(4).max(1);
    let top_quartile_median_ms = median_u32(
        &drivers[..top_quartile_count]
            .iter()
            .map(|driver| driver.median_lap_ms)
            .collect::<Vec<_>>(),
    );
    let benchmarks = drivers
        .iter()
        .enumerate()
        .map(|(index, driver)| driver_benchmark(driver, index + 1, participant_count))
        .collect::<Vec<_>>();
    let player_index = drivers.iter().position(|driver| {
        driver.vehicle_id == player_vehicle_id && driver.driver_name == player_driver_name
    });
    let actual_p1_index = actual_p1_identity.and_then(|leader| {
        drivers.iter().position(|driver| {
            driver.vehicle_id == leader.vehicle_id && driver.driver_name == leader.driver_name
        })
    });
    let mut limitations = fixed_limitations();
    if participant_count < 4 {
        limitations.push(
            "동일 클래스 유효 참가자가 4명 미만이라 백분위와 상위 25% 신뢰도가 낮습니다."
                .to_owned(),
        );
    }
    if drivers.iter().any(|driver| driver.selected.len() < 2) {
        limitations.push(
            "일부 드라이버는 유효 랩이 1개뿐이라 베스트 2랩 반복성을 확인할 수 없습니다."
                .to_owned(),
        );
    }
    match (actual_p1_identity, actual_p1_index) {
        (None, _) => limitations.push(
            "최신 live standings에서 현재 클래스 P1을 확인할 수 없어 P1 비교를 대기합니다."
                .to_owned(),
        ),
        (Some(leader), None) => limitations.push(format!(
            "현재 클래스 P1 {}의 유효 원시 트레이스가 없습니다. 새 P1 또는 드라이버 교대 뒤에는 해당 드라이버의 유효 랩을 기다립니다.",
            leader.driver_name
        )),
        (Some(_), Some(_)) => {}
    }
    let Some(player_index) = player_index else {
        let mut report = waiting_report(
            Some(&session),
            "waiting_for_valid_player_trace",
            "플레이어의 유효 원시 트레이스를 기다리고 있습니다.",
        );
        report.class_name = class_name;
        report.cohort = CohortSummary {
            participant_count,
            valid_lap_count: drivers.iter().map(|driver| driver.all_lap_count).sum(),
            top_quartile_count,
            top_quartile_median_ms,
        };
        report.fastest_captured = benchmarks.first().cloned();
        report.actual_p1 = actual_p1_index.map(|index| benchmarks[index].clone());
        report.exclusions = exclusion_summaries(&exclusions);
        report.limitations = limitations;
        return Ok(report);
    };

    let segments = compare_cohort(&drivers, player_index, actual_p1_index, top_quartile_count);
    let focus_zones = select_focus_zones(&segments);
    let status = if actual_p1_index.is_some() {
        "ready"
    } else {
        "waiting_for_actual_p1_trace"
    };
    let message = if actual_p1_index.is_some() {
        "최신 LMU 세션의 동일 클래스 코호트 및 실제 P1 비교가 준비되었습니다."
    } else {
        "코호트 분석은 가능하지만 실제 클래스 P1 트레이스가 없어 P1 비교는 대기 중입니다."
    };
    Ok(CoachingReport {
        schema_version: 3,
        status: status.to_owned(),
        message: message.to_owned(),
        generated_at_unix_ms: unix_ms(),
        session_id: session.id,
        session_type: session.session_type,
        track_name: session.track_name,
        class_name,
        cohort: CohortSummary {
            participant_count,
            valid_lap_count: drivers.iter().map(|driver| driver.all_lap_count).sum(),
            top_quartile_count,
            top_quartile_median_ms,
        },
        player: Some(benchmarks[player_index].clone()),
        actual_p1: actual_p1_index.map(|index| benchmarks[index].clone()),
        fastest_captured: benchmarks.first().cloned(),
        segments,
        focus_zones,
        exclusions: exclusion_summaries(&exclusions),
        limitations,
    })
}

fn waiting_report(session: Option<&StoredSession>, status: &str, message: &str) -> CoachingReport {
    CoachingReport {
        schema_version: 3,
        status: status.to_owned(),
        message: message.to_owned(),
        generated_at_unix_ms: unix_ms(),
        session_id: session.map_or_else(String::new, |value| value.id.clone()),
        session_type: session.map_or_else(String::new, |value| value.session_type.clone()),
        track_name: session.map_or_else(String::new, |value| value.track_name.clone()),
        class_name: String::new(),
        cohort: CohortSummary::default(),
        player: None,
        actual_p1: None,
        fastest_captured: None,
        segments: Vec::new(),
        focus_zones: Vec::new(),
        exclusions: Vec::new(),
        limitations: fixed_limitations(),
    }
}

fn report_state(report: &CoachingReport) -> String {
    let player = report.player.as_ref().map_or("", |value| {
        value.selected_lap_ids.first().map_or("", String::as_str)
    });
    let fastest = report.fastest_captured.as_ref().map_or("", |value| {
        value.selected_lap_ids.first().map_or("", String::as_str)
    });
    let actual_p1 = report.actual_p1.as_ref().map_or("", |value| {
        value.selected_lap_ids.first().map_or("", String::as_str)
    });
    format!(
        "{}:{}:{}:{}:{}:{}:{}:{}",
        report.status,
        report.session_id,
        player,
        actual_p1,
        fastest,
        report.cohort.valid_lap_count,
        report
            .exclusions
            .iter()
            .map(|value| value.count)
            .sum::<usize>(),
        report.limitations.join("|")
    )
}

fn prepare_trace(lap: SavedLap, track_length_m: f64) -> Result<TraceLap, &'static str> {
    let samples = normalize_samples(&lap.samples, track_length_m);
    if samples.len() < 2 {
        return Err("missing_trace");
    }
    let start_m = samples[0].lap_distance_m;
    let end_m = samples[samples.len() - 1].lap_distance_m;
    if end_m - start_m < MIN_REPORT_DISTANCE_M
        || (track_length_m >= MIN_REPORT_DISTANCE_M && end_m - start_m < track_length_m * 0.90)
    {
        return Err("insufficient_coverage");
    }
    let observed_time_s = samples[samples.len() - 1].lap_elapsed_s - samples[0].lap_elapsed_s;
    let official_time_s = f64::from(lap.summary.lap_time_ms) / 1_000.0;
    let tolerance_s = (official_time_s * 0.015).max(1.5);
    if (observed_time_s - official_time_s).abs() > tolerance_s {
        return Err("timing_mismatch");
    }
    let segments = (0..SEGMENT_COUNT)
        .map(|index| {
            let start = track_length_m * index as f64 / SEGMENT_COUNT as f64;
            let end = track_length_m * (index + 1) as f64 / SEGMENT_COUNT as f64;
            segment_stats(&samples, start, end)
        })
        .collect();
    Ok(TraceLap {
        summary: lap.summary,
        segments,
    })
}

fn normalize_samples(samples: &[TelemetryPoint], track_length_m: f64) -> Vec<TelemetryPoint> {
    if !track_length_m.is_finite() || track_length_m < MIN_REPORT_DISTANCE_M {
        return Vec::new();
    }
    let usable = samples
        .iter()
        .filter(|sample| {
            sample.lap_distance_m.is_finite()
                && sample.lap_elapsed_s.is_finite()
                && sample.speed_kmh.is_finite()
                && sample.lap_distance_m >= -50.0
                && sample.lap_distance_m <= track_length_m + 50.0
                && sample.lap_elapsed_s >= 0.0
        })
        .cloned()
        .collect::<Vec<_>>();
    let mut normalized = Vec::with_capacity(usable.len());
    let mut previous_distance = None;
    for (index, mut sample) in usable.into_iter().enumerate() {
        sample.lap_distance_m = sample.lap_distance_m.clamp(0.0, track_length_m);
        if previous_distance
            .is_some_and(|previous| previous - sample.lap_distance_m > track_length_m * 0.5)
        {
            if index <= samples.len() / 10 + 1 {
                normalized.clear();
            } else {
                break;
            }
        }
        previous_distance = Some(sample.lap_distance_m);
        if normalized.last().is_some_and(|previous: &TelemetryPoint| {
            sample.lap_distance_m <= previous.lap_distance_m + 0.01
        }) {
            continue;
        }
        normalized.push(sample);
    }
    normalized
}

fn aggregate_drivers(traces: Vec<TraceLap>, _track_length_m: f64) -> Vec<DriverAggregate> {
    let mut grouped = HashMap::<(i32, String), Vec<TraceLap>>::new();
    for trace in traces {
        grouped
            .entry((trace.summary.vehicle_id, trace.summary.driver_name.clone()))
            .or_default()
            .push(trace);
    }
    grouped
        .into_iter()
        .filter_map(|((vehicle_id, driver_name), mut laps)| {
            laps.sort_by_key(|lap| lap.summary.lap_time_ms);
            let all_lap_count = laps.len();
            let selected = laps.into_iter().take(2).collect::<Vec<_>>();
            let best_lap_ms = selected.first()?.summary.lap_time_ms;
            let median_lap_ms = median_u32(
                &selected
                    .iter()
                    .map(|lap| lap.summary.lap_time_ms)
                    .collect::<Vec<_>>(),
            )?;
            let segments = (0..SEGMENT_COUNT)
                .map(|index| median_segment(&selected, index))
                .collect();
            Some(DriverAggregate {
                driver_name,
                vehicle_id,
                all_lap_count,
                selected,
                best_lap_ms,
                median_lap_ms,
                segments,
            })
        })
        .collect()
}

fn median_segment(laps: &[TraceLap], segment_index: usize) -> Option<SegmentStats> {
    let values = laps
        .iter()
        .filter_map(|lap| lap.segments.get(segment_index).copied().flatten())
        .collect::<Vec<_>>();
    if values.is_empty() {
        return None;
    }
    Some(SegmentStats {
        time_ms: median_f64(&values.iter().map(|value| value.time_ms).collect::<Vec<_>>()),
        entry_speed_kmh: median_f64(
            &values
                .iter()
                .map(|value| value.entry_speed_kmh)
                .collect::<Vec<_>>(),
        ),
        min_speed_kmh: median_f64(
            &values
                .iter()
                .map(|value| value.min_speed_kmh)
                .collect::<Vec<_>>(),
        ),
        average_throttle: median_f64(
            &values
                .iter()
                .map(|value| value.average_throttle)
                .collect::<Vec<_>>(),
        ),
        max_brake: median_f64(
            &values
                .iter()
                .map(|value| value.max_brake)
                .collect::<Vec<_>>(),
        ),
        brake_onset_m: median_option(
            &values
                .iter()
                .filter_map(|value| value.brake_onset_m)
                .collect::<Vec<_>>(),
        ),
        throttle_commit_m: median_option(
            &values
                .iter()
                .filter_map(|value| value.throttle_commit_m)
                .collect::<Vec<_>>(),
        ),
    })
}

fn compare_cohort(
    drivers: &[DriverAggregate],
    player_index: usize,
    actual_p1_index: Option<usize>,
    top_quartile_count: usize,
) -> Vec<SegmentComparison> {
    let player = &drivers[player_index];
    let mut cumulative_loss_ms = 0.0;
    let mut cumulative_actual_p1_ms = 0.0;
    (0..SEGMENT_COUNT)
        .filter_map(|index| {
            let player_stats = player.segments[index]?;
            let mut cohort = drivers
                .iter()
                .filter_map(|driver| driver.segments[index].map(|stats| (driver, stats)))
                .collect::<Vec<_>>();
            cohort.sort_by(|left, right| {
                left.1
                    .time_ms
                    .partial_cmp(&right.1.time_ms)
                    .unwrap_or(Ordering::Equal)
            });
            let fastest = cohort.first()?.1;
            let top = cohort
                .iter()
                .filter(|(driver, _)| {
                    drivers
                        .iter()
                        .position(|candidate| same_driver(candidate, driver))
                        .is_some_and(|rank| rank < top_quartile_count)
                })
                .map(|(_, stats)| *stats)
                .collect::<Vec<_>>();
            let top_stats = aggregate_segment_values(&top)?;
            let actual_p1_stats = actual_p1_index
                .and_then(|driver_index| drivers.get(driver_index))
                .and_then(|driver| driver.segments[index]);
            let rank = cohort
                .iter()
                .position(|(driver, _)| same_driver(driver, player))?
                + 1;
            let participant_count = cohort.len();
            let percentile = percentile(rank, participant_count);
            let lap_sample_count = cohort.iter().map(|(driver, _)| driver.selected.len()).sum();
            let loss_to_fastest_ms = player_stats.time_ms - fastest.time_ms;
            let loss_to_top_quartile_ms = player_stats.time_ms - top_stats.time_ms;
            let delta_to_actual_p1_ms =
                actual_p1_stats.map(|stats| player_stats.time_ms - stats.time_ms);
            cumulative_loss_ms += loss_to_fastest_ms;
            if let Some(delta) = delta_to_actual_p1_ms {
                cumulative_actual_p1_ms += delta;
            }
            let pattern = segment_pattern(player, index, top_stats.time_ms);
            let loss_origin = loss_origin(player_stats, top_stats, loss_to_top_quartile_ms);
            Some(SegmentComparison {
                number: index + 1,
                start_m: round_to(
                    player.selected[0].summary.track_length_m * index as f64 / SEGMENT_COUNT as f64,
                    1,
                ),
                end_m: round_to(
                    player.selected[0].summary.track_length_m * (index + 1) as f64
                        / SEGMENT_COUNT as f64,
                    1,
                ),
                player_time_ms: round_to(player_stats.time_ms, 1),
                fastest_time_ms: round_to(fastest.time_ms, 1),
                top_quartile_time_ms: round_to(top_stats.time_ms, 1),
                actual_p1_time_ms: actual_p1_stats.map(|stats| round_to(stats.time_ms, 1)),
                loss_to_fastest_ms: round_to(loss_to_fastest_ms, 1),
                loss_to_top_quartile_ms: round_to(loss_to_top_quartile_ms, 1),
                delta_to_actual_p1_ms: delta_to_actual_p1_ms.map(|value| round_to(value, 1)),
                cumulative_loss_ms: round_to(cumulative_loss_ms, 1),
                cumulative_delta_to_actual_p1_ms: delta_to_actual_p1_ms
                    .map(|_| round_to(cumulative_actual_p1_ms, 1)),
                rank,
                percentile: round_to(percentile, 1),
                participant_count,
                lap_sample_count,
                confidence: confidence(participant_count, player.selected.len()),
                pattern,
                loss_origin,
                player_min_speed_kmh: round_to(player_stats.min_speed_kmh, 1),
                top_quartile_min_speed_kmh: round_to(top_stats.min_speed_kmh, 1),
                actual_p1_min_speed_kmh: actual_p1_stats
                    .map(|stats| round_to(stats.min_speed_kmh, 1)),
                player_brake_onset_m: player_stats.brake_onset_m.map(|value| round_to(value, 1)),
                top_quartile_brake_onset_m: top_stats.brake_onset_m.map(|value| round_to(value, 1)),
                actual_p1_brake_onset_m: actual_p1_stats
                    .and_then(|stats| stats.brake_onset_m)
                    .map(|value| round_to(value, 1)),
                player_throttle_commit_m: player_stats
                    .throttle_commit_m
                    .map(|value| round_to(value, 1)),
                top_quartile_throttle_commit_m: top_stats
                    .throttle_commit_m
                    .map(|value| round_to(value, 1)),
                actual_p1_throttle_commit_m: actual_p1_stats
                    .and_then(|stats| stats.throttle_commit_m)
                    .map(|value| round_to(value, 1)),
            })
        })
        .collect()
}

fn same_driver(left: &DriverAggregate, right: &DriverAggregate) -> bool {
    left.vehicle_id == right.vehicle_id && left.driver_name == right.driver_name
}

fn aggregate_segment_values(values: &[SegmentStats]) -> Option<SegmentStats> {
    if values.is_empty() {
        return None;
    }
    median_segment(
        &values
            .iter()
            .enumerate()
            .map(|(index, value)| TraceLap {
                summary: LapSummary {
                    id: format!("aggregate-{index}"),
                    ..LapSummary::default()
                },
                segments: vec![Some(*value)],
            })
            .collect::<Vec<_>>(),
        0,
    )
}

fn segment_pattern(
    player: &DriverAggregate,
    segment_index: usize,
    top_quartile_time_ms: f64,
) -> &'static str {
    let losses = player
        .selected
        .iter()
        .filter_map(|lap| lap.segments[segment_index])
        .map(|stats| stats.time_ms - top_quartile_time_ms)
        .collect::<Vec<_>>();
    match losses.as_slice() {
        [first, second, ..]
            if *first > MIN_MEANINGFUL_LOSS_MS && *second > MIN_MEANINGFUL_LOSS_MS =>
        {
            "recurring"
        }
        [first, second, ..]
            if *first > MIN_MEANINGFUL_LOSS_MS || *second > MIN_MEANINGFUL_LOSS_MS =>
        {
            "one_off"
        }
        [_] => "single_lap",
        _ => "no_consistent_loss",
    }
}

fn loss_origin(player: SegmentStats, reference: SegmentStats, loss_ms: f64) -> &'static str {
    if loss_ms <= MIN_MEANINGFUL_LOSS_MS {
        return "none";
    }
    let entry_gap = reference.entry_speed_kmh - player.entry_speed_kmh;
    let minimum_gap = reference.min_speed_kmh - player.min_speed_kmh;
    if entry_gap > 4.0 && minimum_gap <= 4.0 {
        "carry"
    } else if entry_gap > 4.0 && minimum_gap > 4.0 {
        "mixed"
    } else {
        "intrinsic"
    }
}

fn select_focus_zones(segments: &[SegmentComparison]) -> Vec<FocusZone> {
    let mut candidates = segments
        .iter()
        .filter(|segment| segment.loss_to_top_quartile_ms > MIN_MEANINGFUL_LOSS_MS)
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| {
        right
            .loss_to_top_quartile_ms
            .partial_cmp(&left.loss_to_top_quartile_ms)
            .unwrap_or(Ordering::Equal)
    });
    candidates
        .into_iter()
        .take(3)
        .enumerate()
        .map(|(index, segment)| FocusZone {
            rank: index + 1,
            segment_number: segment.number,
            start_m: segment.start_m,
            end_m: segment.end_m,
            loss_ms: segment.loss_to_top_quartile_ms,
            confidence: segment.confidence,
            pattern: segment.pattern,
            loss_origin: segment.loss_origin,
            coaching_cues: coaching_cues(segment),
        })
        .collect()
}

fn coaching_cues(segment: &SegmentComparison) -> Vec<String> {
    let mut cues = Vec::new();
    if segment.pattern == "one_off" {
        cues.push("한 랩에서만 나타난 손실입니다. 반복 습관으로 단정하지 말고 다음 유효 랩에서 재확인하세요.".to_owned());
    } else if segment.pattern == "recurring" {
        cues.push("플레이어 베스트 2랩 모두에서 반복된 손실입니다.".to_owned());
    }
    if segment.loss_origin == "carry" {
        cues.push(
            "구간 진입 속도 차이가 커 이전 구간 탈출에서 이어진 손실일 가능성이 높습니다."
                .to_owned(),
        );
    } else if segment.loss_origin == "intrinsic" {
        cues.push(
            "진입 속도보다 이 구간 자체의 제동·회전·가속 과정에서 생긴 손실로 보입니다.".to_owned(),
        );
    }
    if let (Some(player), Some(reference)) = (
        segment.player_brake_onset_m,
        segment.top_quartile_brake_onset_m,
    ) {
        if reference - player > 12.0 {
            cues.push(format!(
                "상위 25%보다 약 {:.0}m 일찍 제동합니다. 기준점을 조금씩 뒤로 옮겨 확인하세요.",
                reference - player
            ));
        } else if player - reference > 12.0
            && segment.player_min_speed_kmh + 2.0 < segment.top_quartile_min_speed_kmh
        {
            cues.push(format!(
                "상위 25%보다 약 {:.0}m 늦게 제동하면서 최저속도가 더 낮습니다. 진입 과속을 줄이세요.",
                player - reference
            ));
        }
    }
    let minimum_speed_gap = segment.top_quartile_min_speed_kmh - segment.player_min_speed_kmh;
    if minimum_speed_gap > 4.0 {
        cues.push(format!(
            "최저속도가 상위 25%보다 {:.1}km/h 낮습니다. 미드코너 속도 유지와 라인을 확인하세요.",
            minimum_speed_gap
        ));
    }
    if let (Some(player), Some(reference)) = (
        segment.player_throttle_commit_m,
        segment.top_quartile_throttle_commit_m,
    ) && player - reference > 12.0
    {
        cues.push(format!(
            "강한 스로틀 재개가 상위 25%보다 약 {:.0}m 늦습니다. 조향을 풀며 가속을 연결하세요.",
            player - reference
        ));
    }
    if cues.is_empty() {
        cues.push(
            "명확한 입력 차이가 없어 라인과 짧은 리프트를 원시 트레이스에서 확인해야 합니다."
                .to_owned(),
        );
    }
    cues
}

fn segment_stats(samples: &[TelemetryPoint], start_m: f64, end_m: f64) -> Option<SegmentStats> {
    if samples.first()?.lap_distance_m > start_m || samples.last()?.lap_distance_m < end_m {
        return None;
    }
    let time_start = interpolate(samples, start_m, |sample| sample.lap_elapsed_s);
    let time_end = interpolate(samples, end_m, |sample| sample.lap_elapsed_s);
    if time_end <= time_start {
        return None;
    }
    let points = samples
        .iter()
        .filter(|sample| sample.lap_distance_m >= start_m && sample.lap_distance_m <= end_m)
        .collect::<Vec<_>>();
    if points.is_empty() {
        return None;
    }
    let peak_brake_index = points
        .iter()
        .enumerate()
        .max_by(|(_, left), (_, right)| {
            left.brake
                .partial_cmp(&right.brake)
                .unwrap_or(Ordering::Equal)
        })
        .map(|(index, _)| index);
    Some(SegmentStats {
        time_ms: (time_end - time_start) * 1_000.0,
        entry_speed_kmh: interpolate(samples, start_m, |sample| sample.speed_kmh),
        min_speed_kmh: points
            .iter()
            .map(|sample| sample.speed_kmh)
            .fold(f64::INFINITY, f64::min),
        average_throttle: points
            .iter()
            .map(|sample| sample.throttle.clamp(0.0, 1.0))
            .sum::<f64>()
            / points.len() as f64,
        max_brake: points
            .iter()
            .map(|sample| sample.brake.clamp(0.0, 1.0))
            .fold(0.0, f64::max),
        brake_onset_m: points
            .iter()
            .find(|sample| sample.brake >= 0.15)
            .map(|sample| sample.lap_distance_m),
        throttle_commit_m: peak_brake_index.and_then(|index| {
            points[index..]
                .iter()
                .find(|sample| sample.throttle >= 0.75 && sample.brake <= 0.10)
                .map(|sample| sample.lap_distance_m)
        }),
    })
}

fn interpolate(
    samples: &[TelemetryPoint],
    distance_m: f64,
    value: impl Fn(&TelemetryPoint) -> f64,
) -> f64 {
    let upper = samples.partition_point(|sample| sample.lap_distance_m < distance_m);
    if upper == 0 {
        return value(&samples[0]);
    }
    if upper >= samples.len() {
        return value(&samples[samples.len() - 1]);
    }
    let before = &samples[upper - 1];
    let after = &samples[upper];
    let span = after.lap_distance_m - before.lap_distance_m;
    if span.abs() < f64::EPSILON {
        return value(after);
    }
    let ratio = ((distance_m - before.lap_distance_m) / span).clamp(0.0, 1.0);
    value(before) + (value(after) - value(before)) * ratio
}

fn driver_benchmark(
    driver: &DriverAggregate,
    rank: usize,
    participant_count: usize,
) -> DriverBenchmark {
    DriverBenchmark {
        driver_name: driver.driver_name.clone(),
        vehicle_id: driver.vehicle_id,
        rank,
        percentile: round_to(percentile(rank, participant_count), 1),
        valid_lap_count: driver.all_lap_count,
        selected_lap_count: driver.selected.len(),
        best_lap_ms: driver.best_lap_ms,
        best_two_median_ms: driver.median_lap_ms,
        selected_lap_ids: driver
            .selected
            .iter()
            .map(|lap| lap.summary.id.clone())
            .collect(),
    }
}

fn percentile(rank: usize, count: usize) -> f64 {
    if count <= 1 {
        100.0
    } else {
        (count.saturating_sub(rank)) as f64 / (count - 1) as f64 * 100.0
    }
}

fn confidence(participants: usize, player_laps: usize) -> AnalysisConfidenceLevel {
    AnalysisConfidence::for_cohort(participants, player_laps).level
}

fn same_class(left: &str, right: &str) -> bool {
    !right.is_empty() && left.trim().eq_ignore_ascii_case(right.trim())
}

fn count_exclusion(exclusions: &mut BTreeMap<String, usize>, code: &str) {
    *exclusions.entry(code.to_owned()).or_default() += 1;
}

fn exclude_for_quality(summary: &LapSummary, exclusions: &mut BTreeMap<String, usize>) -> bool {
    if summary.quality.status == TraceQualityStatus::Valid
        && summary.valid
        && summary.completed
        && summary.lap_time_ms > 0
    {
        return false;
    }
    if summary.quality.status == TraceQualityStatus::Unknown {
        count_exclusion(exclusions, "quality:unknown");
        return true;
    }
    if summary.quality.reasons.is_empty() {
        let code = match summary.quality.status {
            TraceQualityStatus::Partial => "quality:partial",
            TraceQualityStatus::Rejected => "quality:rejected",
            TraceQualityStatus::Valid => "quality:invalid_metadata",
            TraceQualityStatus::Unknown => unreachable!(),
        };
        count_exclusion(exclusions, code);
        return true;
    }
    for reason in &summary.quality.reasons {
        count_exclusion(exclusions, quality_reason_code(*reason));
    }
    true
}

fn quality_reason_code(reason: QualityReason) -> &'static str {
    match reason {
        QualityReason::NoSamples => "quality:no_samples",
        QualityReason::GameInvalidated => "quality:game_invalidated",
        QualityReason::StartsMidTrace => "quality:starts_mid_trace",
        QualityReason::EndsBeforeFinish => "quality:ends_before_finish",
        QualityReason::InsufficientCoverage => "quality:insufficient_coverage",
        QualityReason::TimingMismatch => "quality:timing_mismatch",
        QualityReason::SparseSamples => "quality:sparse_samples",
        QualityReason::SampleGap => "quality:sample_gap",
        QualityReason::DuplicateSamples => "quality:duplicate_samples",
        QualityReason::NonMonotonicDistance => "quality:non_monotonic_distance",
        QualityReason::NonMonotonicTime => "quality:non_monotonic_time",
        QualityReason::DistanceOutOfRange => "quality:distance_out_of_range",
        QualityReason::ForwardDistanceSpike => "quality:forward_distance_spike",
        QualityReason::ImplausibleTelemetry => "quality:implausible_telemetry",
    }
}

fn fixed_limitations() -> Vec<String> {
    AnalysisLimitation::ENVIRONMENTAL_DEFAULTS
        .iter()
        .map(|limitation| limitation.description_ko().to_owned())
        .collect()
}

fn exclusion_summaries(exclusions: &BTreeMap<String, usize>) -> Vec<ExclusionSummary> {
    exclusions
        .iter()
        .map(|(code, count)| ExclusionSummary {
            code: code.clone(),
            count: *count,
            description: match code.as_str() {
                "other_class" => "플레이어와 다른 클래스",
                "missing_trace" => "원시 텔레메트리가 없거나 보존 기간이 지난 랩",
                "insufficient_coverage" => "트랙 커버리지가 부족한 랩",
                "timing_mismatch" => "공식 랩타임과 원시 트레이스 시간이 불일치한 랩",
                code if code.starts_with("quality:") => "구조화된 품질 판정으로 제외된 랩",
                _ => "분석 기준을 충족하지 않은 랩",
            }
            .to_owned(),
        })
        .collect()
}

fn median_u32(values: &[u32]) -> Option<u32> {
    if values.is_empty() {
        return None;
    }
    let mut values = values.to_vec();
    values.sort_unstable();
    let middle = values.len() / 2;
    Some(if values.len().is_multiple_of(2) {
        ((u64::from(values[middle - 1]) + u64::from(values[middle])) / 2) as u32
    } else {
        values[middle]
    })
}

fn median_f64(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mut values = values.to_vec();
    values.sort_by(f64::total_cmp);
    let middle = values.len() / 2;
    if values.len().is_multiple_of(2) {
        (values[middle - 1] + values[middle]) / 2.0
    } else {
        values[middle]
    }
}

fn median_option(values: &[f64]) -> Option<f64> {
    (!values.is_empty()).then(|| median_f64(values))
}

fn write_report(path: &Path, report: &CoachingReport) -> Result<(), String> {
    ensure_parent(path)?;
    let json_path = json_path(path);
    if json_path == path {
        return Err(
            "--coach-report must not use a .json extension; choose a Markdown path".to_owned(),
        );
    }
    let json = serde_json::to_string_pretty(report)
        .map_err(|error| format!("failed to encode coaching report: {error}"))?;
    write_atomic(&json_path, json.as_bytes())?;
    write_atomic(path, render_markdown(report).as_bytes())
}

fn write_atomic(path: &Path, body: &[u8]) -> Result<(), String> {
    let temporary = path.with_extension(format!(
        "{}.tmp",
        path.extension()
            .and_then(|value| value.to_str())
            .unwrap_or("report")
    ));
    fs::write(&temporary, body)
        .map_err(|error| format!("failed to write {}: {error}", temporary.display()))?;
    fs::rename(&temporary, path)
        .map_err(|error| format!("failed to replace {}: {error}", path.display()))
}

fn render_markdown(report: &CoachingReport) -> String {
    let mut body = format!(
        "# LMU {} 코칭 분석\n\n- 상태: {}\n- 트랙: {}\n- 클래스: {}\n- 유효 참가자/랩: {}명 / {}랩\n- 메시지: {}\n",
        report.session_type,
        report.status,
        report.track_name,
        report.class_name,
        report.cohort.participant_count,
        report.cohort.valid_lap_count,
        report.message,
    );
    if let Some(player) = &report.player {
        body.push_str(&format!(
            "- 플레이어 베스트 2 중앙값: {} (P{}, 백분위 {:.1})\n",
            format_lap_time(player.best_two_median_ms),
            player.rank,
            player.percentile
        ));
    }
    if let Some(p1) = &report.actual_p1 {
        body.push_str(&format!(
            "- 실제 클래스 P1: {} / {}\n",
            p1.driver_name,
            format_lap_time(p1.best_two_median_ms)
        ));
    }
    body.push_str("\n## 우선 확인 구간\n");
    for zone in &report.focus_zones {
        body.push_str(&format!(
            "\n### {}. {:.0}-{:.0}m ({:+.3}초, {}, {}, {})\n",
            zone.rank,
            zone.start_m,
            zone.end_m,
            zone.loss_ms / 1_000.0,
            zone.confidence,
            zone.pattern,
            zone.loss_origin
        ));
        for cue in &zone.coaching_cues {
            body.push_str(&format!("- {cue}\n"));
        }
    }
    if !report.limitations.is_empty() {
        body.push_str("\n## 데이터 한계\n");
        for limitation in &report.limitations {
            body.push_str(&format!("- {limitation}\n"));
        }
    }
    if !report.exclusions.is_empty() {
        body.push_str("\n## 제외된 데이터\n");
        for exclusion in &report.exclusions {
            body.push_str(&format!(
                "- {}: {}개 ({})\n",
                exclusion.code, exclusion.count, exclusion.description
            ));
        }
    }
    body
}

fn ensure_parent(path: &Path) -> Result<(), String> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)
            .map_err(|error| format!("failed to create {}: {error}", parent.display()))?;
    }
    Ok(())
}

fn json_path(path: &Path) -> PathBuf {
    path.with_extension("json")
}

fn format_lap_time(milliseconds: u32) -> String {
    let minutes = milliseconds / 60_000;
    let seconds = (milliseconds % 60_000) / 1_000;
    let remainder = milliseconds % 1_000;
    format!("{minutes}:{seconds:02}.{remainder:03}")
}

fn round_to(value: f64, decimal_places: i32) -> f64 {
    let factor = 10_f64.powi(decimal_places);
    (value * factor).round() / factor
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{LapSummary, SessionState};
    use crate::telemetry_quality::{TraceQuality, TraceQualityStatus};

    #[test]
    fn preserves_physical_distance_when_laps_start_at_different_points() {
        let samples = vec![
            sample_at(100.0, 1.0),
            sample_at(500.0, 5.0),
            sample_at(990.0, 10.0),
        ];
        let normalized = normalize_samples(&samples, 1_000.0);
        assert_eq!(normalized[0].lap_distance_m, 100.0);
        assert_eq!(normalized[2].lap_distance_m, 990.0);
    }

    #[test]
    fn drops_only_the_stale_prefix_when_scoring_wraps_after_the_line() {
        let samples = vec![
            sample_at(990.0, 0.0),
            sample_at(5.0, 0.1),
            sample_at(500.0, 5.0),
            sample_at(990.0, 10.0),
        ];
        let normalized = normalize_samples(&samples, 1_000.0);
        assert_eq!(normalized[0].lap_distance_m, 5.0);
        assert_eq!(normalized.last().unwrap().lap_distance_m, 990.0);
    }

    #[test]
    fn latest_session_without_valid_laps_does_not_reuse_an_older_report() {
        let directory = temporary("latest-session");
        let store = DashboardStore::open(&directory).unwrap();
        let old = qualifying("old", 1);
        store.save_session(&old).unwrap();
        store
            .save_lap(&synthetic_lap(&old, 7, "플레이어", true, 1, 10_000))
            .unwrap();
        let mut current = qualifying("current", 2);
        current.current_time_s = 1.0;
        store.save_session(&current).unwrap();

        let report = build_report(&store, None).unwrap();
        assert_eq!(report.session_id, "current");
        assert_eq!(report.status, "waiting_for_player_lap");
        fs::remove_dir_all(directory).ok();
    }

    #[test]
    fn does_not_call_the_player_p1_when_actual_p1_trace_is_missing() {
        let directory = temporary("missing-p1");
        let store = DashboardStore::open(&directory).unwrap();
        let session = qualifying("session", 1);
        store.save_session(&session).unwrap();
        store
            .save_lap(&synthetic_lap(&session, 7, "플레이어", true, 2, 10_000))
            .unwrap();
        store
            .save_lap(&synthetic_lap(&session, 8, "이전 P1", false, 1, 10_500))
            .unwrap();

        let current_leader = leader(&session, 9, "새 P1");
        let report = build_report(&store, Some(&current_leader)).unwrap();
        assert_eq!(report.status, "waiting_for_actual_p1_trace");
        assert!(report.actual_p1.is_none());
        assert_eq!(report.fastest_captured.unwrap().driver_name, "플레이어");
        assert!(report.limitations.iter().any(|item| item.contains("새 P1")));
        fs::remove_dir_all(directory).ok();
    }

    #[test]
    fn uses_best_two_median_and_reports_cohort_statistics() {
        let directory = temporary("cohort");
        let store = DashboardStore::open(&directory).unwrap();
        let session = qualifying("session", 1);
        store.save_session(&session).unwrap();
        for (vehicle, driver, player, class_position, times) in [
            (7, "플레이어", true, 3, vec![10_500, 10_700, 11_500]),
            (8, "기준", false, 1, vec![10_000, 10_200]),
            (9, "동료", false, 2, vec![10_300, 10_400]),
            (10, "동료 B", false, 4, vec![10_800, 10_900]),
        ] {
            for (index, time) in times.into_iter().enumerate() {
                let mut lap =
                    synthetic_lap(&session, vehicle, driver, player, class_position, time);
                lap.summary.id = format!("session-{vehicle}-{}", index + 1);
                lap.summary.lap_number = index as i32 + 1;
                store.save_lap(&lap).unwrap();
            }
        }

        let current_leader = leader(&session, 8, "기준");
        let report = build_report(&store, Some(&current_leader)).unwrap();
        assert_eq!(report.status, "ready");
        assert_eq!(report.cohort.participant_count, 4);
        assert_eq!(report.cohort.top_quartile_count, 1);
        assert_eq!(report.cohort.top_quartile_median_ms, Some(10_100));
        assert_eq!(report.player.unwrap().best_two_median_ms, 10_600);
        assert!(!report.segments.is_empty());
        assert!(report.segments[0].participant_count >= 4);
        assert!(report.segments[0].actual_p1_time_ms.is_some());
        assert!(report.segments[0].delta_to_actual_p1_ms.unwrap() > 0.0);
        assert!(
            report
                .limitations
                .iter()
                .any(|item| item.contains("교통량"))
        );
        assert!(
            report
                .limitations
                .iter()
                .any(|item| item.contains("슬립스트림"))
        );
        assert!(
            report
                .limitations
                .iter()
                .any(|item| item.contains("연료량"))
        );
        assert!(
            report
                .limitations
                .iter()
                .any(|item| item.contains("타이어"))
        );
        fs::remove_dir_all(directory).ok();
    }

    #[test]
    fn uses_the_live_leader_instead_of_the_latest_saved_p1_evidence() {
        let directory = temporary("latest-p1-evidence");
        let store = DashboardStore::open(&directory).unwrap();
        let session = qualifying("session", 1);
        store.save_session(&session).unwrap();
        let mut player = synthetic_lap(&session, 7, "플레이어", true, 3, 10_500);
        player.summary.created_at_unix_ms = 100;
        store.save_lap(&player).unwrap();
        let mut early_p1 = synthetic_lap(&session, 8, "초기 P1", false, 1, 9_900);
        early_p1.summary.created_at_unix_ms = 200;
        store.save_lap(&early_p1).unwrap();
        let mut latest_p1 = synthetic_lap(&session, 9, "현재 P1", false, 1, 10_100);
        latest_p1.summary.created_at_unix_ms = 300;
        store.save_lap(&latest_p1).unwrap();

        let current_leader = leader(&session, 8, "초기 P1");
        let report = build_report(&store, Some(&current_leader)).unwrap();
        assert_eq!(report.status, "ready");
        assert_eq!(report.actual_p1.unwrap().driver_name, "초기 P1");
        assert!(
            report
                .segments
                .iter()
                .all(|segment| segment.actual_p1_time_ms.is_some())
        );
        assert_eq!(
            report.segments[0].actual_p1_time_ms,
            Some(report.segments[0].fastest_time_ms)
        );
        let expected_delta =
            report.segments[0].player_time_ms - report.segments[0].actual_p1_time_ms.unwrap();
        assert!((report.segments[0].delta_to_actual_p1_ms.unwrap() - expected_delta).abs() < 0.2);
        fs::remove_dir_all(directory).ok();
    }

    #[test]
    fn waits_for_a_valid_lap_after_the_live_p1_changes_driver() {
        let directory = temporary("p1-driver-swap");
        let store = DashboardStore::open(&directory).unwrap();
        let session = qualifying("session", 1);
        store.save_session(&session).unwrap();
        store
            .save_lap(&synthetic_lap(&session, 7, "플레이어", true, 2, 10_500))
            .unwrap();
        store
            .save_lap(&synthetic_lap(&session, 8, "드라이버 A", false, 1, 10_000))
            .unwrap();

        let current_leader = leader(&session, 8, "드라이버 B");
        let report = build_report(&store, Some(&current_leader)).unwrap();

        assert_eq!(report.status, "waiting_for_actual_p1_trace");
        assert!(report.actual_p1.is_none());
        assert!(
            report
                .limitations
                .iter()
                .any(|item| item.contains("드라이버 교대"))
        );
        fs::remove_dir_all(directory).ok();
    }

    #[test]
    fn keeps_player_rank_and_top_quartile_bound_to_the_current_driver_after_a_swap() {
        let directory = temporary("player-driver-swap");
        let store = DashboardStore::open(&directory).unwrap();
        let session = qualifying("session", 1);
        store.save_session(&session).unwrap();
        for (index, (vehicle, driver, player, time)) in [
            (7, "이전 플레이어", true, 9_000),
            (8, "P1", false, 9_500),
            (9, "동료 A", false, 10_000),
            (10, "동료 B", false, 11_000),
            (7, "현재 플레이어", true, 12_000),
        ]
        .into_iter()
        .enumerate()
        {
            let mut lap = synthetic_lap(&session, vehicle, driver, player, index as u8 + 1, time);
            lap.summary.id = format!("driver-swap-{index}");
            lap.summary.lap_number = index as i32 + 1;
            lap.summary.created_at_unix_ms = index as u64 + 1;
            store.save_lap(&lap).unwrap();
        }

        let mut current_leader = leader(&session, 8, "P1");
        current_leader.player_driver_name = "현재 플레이어".to_owned();
        let report = build_report(&store, Some(&current_leader)).unwrap();

        assert_eq!(report.player.as_ref().unwrap().driver_name, "현재 플레이어");
        assert_eq!(report.player.as_ref().unwrap().rank, 5);
        assert!(report.segments.iter().all(|segment| segment.rank == 5));
        fs::remove_dir_all(directory).ok();
    }

    #[test]
    fn waits_for_the_current_player_driver_instead_of_reusing_the_previous_driver() {
        let directory = temporary("player-driver-swap-wait");
        let store = DashboardStore::open(&directory).unwrap();
        let session = qualifying("session", 1);
        store.save_session(&session).unwrap();
        store
            .save_lap(&synthetic_lap(
                &session,
                7,
                "이전 플레이어",
                true,
                2,
                10_000,
            ))
            .unwrap();
        store
            .save_lap(&synthetic_lap(&session, 8, "P1", false, 1, 9_500))
            .unwrap();

        let mut current_leader = leader(&session, 8, "P1");
        current_leader.player_driver_name = "현재 플레이어".to_owned();
        let report = build_report(&store, Some(&current_leader)).unwrap();

        assert_eq!(report.status, "waiting_for_player_lap");
        assert!(report.player.is_none());
        fs::remove_dir_all(directory).ok();
    }

    #[test]
    fn builds_the_same_coaching_analysis_for_a_race_session() {
        let directory = temporary("race-session");
        let store = DashboardStore::open(&directory).unwrap();
        let mut session = qualifying("race", 1);
        session.session_type = "Race".to_owned();
        store.save_session(&session).unwrap();
        store
            .save_lap(&synthetic_lap(&session, 7, "플레이어", true, 2, 10_500))
            .unwrap();
        store
            .save_lap(&synthetic_lap(&session, 8, "레이스 P1", false, 1, 10_000))
            .unwrap();

        let current_leader = leader(&session, 8, "레이스 P1");
        let report = build_report(&store, Some(&current_leader)).unwrap();
        assert_eq!(report.status, "ready");
        assert_eq!(report.session_type, "Race");
        assert_eq!(report.actual_p1.unwrap().driver_name, "레이스 P1");
        assert!(!report.segments.is_empty());
        fs::remove_dir_all(directory).ok();
    }

    #[test]
    fn exposes_structured_quality_exclusions_and_keeps_unknown_separate() {
        let directory = temporary("quality-exclusions");
        let store = DashboardStore::open(&directory).unwrap();
        let session = qualifying("session", 1);
        store.save_session(&session).unwrap();

        let mut valid = synthetic_lap(&session, 7, "플레이어", true, 2, 10_000);
        valid.summary.lap_number = 1;
        store.save_lap(&valid).unwrap();
        let mut rejected = synthetic_lap(&session, 8, "익명 기준", false, 1, 10_100);
        rejected.summary.id.push_str("-rejected");
        rejected.summary.lap_number = 2;
        rejected.summary.valid = false;
        rejected.summary.quality.status = TraceQualityStatus::Rejected;
        rejected.summary.quality.reasons = vec![QualityReason::SampleGap];
        store.save_lap(&rejected).unwrap();
        let mut unknown = synthetic_lap(&session, 9, "레거시 익명", false, 3, 10_200);
        unknown.summary.id.push_str("-unknown");
        unknown.summary.lap_number = 3;
        unknown.summary.quality = TraceQuality::default();
        store.save_lap(&unknown).unwrap();

        let current_leader = leader(&session, 8, "익명 기준");
        let report = build_report(&store, Some(&current_leader)).unwrap();
        assert!(
            report
                .exclusions
                .iter()
                .any(|item| item.code == "quality:sample_gap")
        );
        assert!(
            report
                .exclusions
                .iter()
                .any(|item| item.code == "quality:unknown")
        );
        fs::remove_dir_all(directory).ok();
    }

    fn synthetic_lap(
        session: &SessionState,
        vehicle_id: i32,
        driver: &str,
        is_player: bool,
        class_position: u8,
        lap_time_ms: u32,
    ) -> SavedLap {
        let samples = (0..=100)
            .map(|index| {
                let progress = index as f64 / 100.0;
                TelemetryPoint {
                    session_time_s: progress * f64::from(lap_time_ms) / 1_000.0,
                    lap_elapsed_s: progress * f64::from(lap_time_ms) / 1_000.0,
                    lap_distance_m: progress * session.track_length_m,
                    speed_kmh: 180.0 - (progress * 8.0).sin().abs() * 80.0,
                    rpm: 7_000.0,
                    gear: 4,
                    throttle: if index % 20 < 4 { 0.2 } else { 1.0 },
                    brake: if index % 20 < 3 { 0.8 } else { 0.0 },
                    ..TelemetryPoint::default()
                }
            })
            .collect::<Vec<_>>();
        SavedLap {
            summary: LapSummary {
                id: format!("{}-car-{vehicle_id}-lap-1", session.id),
                session_id: session.id.clone(),
                track_name: session.track_name.clone(),
                session_type: session.session_type.clone(),
                track_length_m: session.track_length_m,
                vehicle_id,
                driver_name: driver.to_owned(),
                class_name: "Hypercar".to_owned(),
                is_player,
                class_position,
                lap_number: 1,
                lap_time_ms,
                valid: true,
                quality: TraceQuality {
                    status: TraceQualityStatus::Valid,
                    score: 100,
                    ..TraceQuality::default()
                },
                sample_count: samples.len(),
                created_at_unix_ms: unix_ms(),
                completed: true,
                ..LapSummary::default()
            },
            samples,
        }
    }

    fn qualifying(id: &str, order: u64) -> SessionState {
        SessionState {
            id: id.to_owned(),
            game_version: 13,
            track_name: "테스트 서킷".to_owned(),
            session_type: "Qualifying".to_owned(),
            current_time_s: order as f64,
            track_length_m: 1_000.0,
            ..SessionState::default()
        }
    }

    fn leader(session: &SessionState, vehicle_id: i32, driver: &str) -> ClassLeaderIdentity {
        ClassLeaderIdentity {
            session_id: session.id.clone(),
            vehicle_id,
            driver_name: driver.to_owned(),
            class_name: "Hypercar".to_owned(),
            player_vehicle_id: 7,
            player_driver_name: "플레이어".to_owned(),
        }
    }

    fn sample_at(distance_m: f64, elapsed_s: f64) -> TelemetryPoint {
        TelemetryPoint {
            lap_distance_m: distance_m,
            lap_elapsed_s: elapsed_s,
            speed_kmh: 100.0,
            ..TelemetryPoint::default()
        }
    }

    fn temporary(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "lmu-dashboard-coach-{name}-{}-{}",
            std::process::id(),
            unix_ms()
        ))
    }
}
