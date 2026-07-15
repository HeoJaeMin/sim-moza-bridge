use std::cmp::Ordering;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Serialize;

use crate::model::{LapSummary, SavedLap, TelemetryPoint};
use crate::store::{DashboardStore, unix_ms};

const SEGMENT_COUNT: usize = 20;
const MIN_REPORT_DISTANCE_M: f64 = 500.0;

#[derive(Clone, Debug, Serialize)]
pub struct CoachingReport {
    schema_version: u8,
    status: &'static str,
    generated_at_unix_ms: u64,
    session_id: String,
    session_type: String,
    track_name: String,
    class_name: String,
    player: ReportLap,
    p1: ReportLap,
    total_delta_ms: i64,
    segments: Vec<SegmentComparison>,
    focus_zones: Vec<FocusZone>,
}

#[derive(Clone, Debug, Serialize)]
struct ReportLap {
    lap_id: String,
    driver_name: String,
    vehicle_id: i32,
    lap_number: i32,
    lap_time_ms: u32,
}

impl From<&LapSummary> for ReportLap {
    fn from(summary: &LapSummary) -> Self {
        Self {
            lap_id: summary.id.clone(),
            driver_name: summary.driver_name.clone(),
            vehicle_id: summary.vehicle_id,
            lap_number: summary.lap_number,
            lap_time_ms: summary.lap_time_ms,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
struct SegmentComparison {
    number: usize,
    start_m: f64,
    end_m: f64,
    loss_ms: f64,
    cumulative_delta_ms: f64,
    player_min_speed_kmh: f64,
    p1_min_speed_kmh: f64,
    player_average_throttle: f64,
    p1_average_throttle: f64,
    player_max_brake: f64,
    p1_max_brake: f64,
    player_brake_onset_m: Option<f64>,
    p1_brake_onset_m: Option<f64>,
    player_throttle_commit_m: Option<f64>,
    p1_throttle_commit_m: Option<f64>,
}

#[derive(Clone, Debug, Serialize)]
struct FocusZone {
    rank: usize,
    segment_number: usize,
    start_m: f64,
    end_m: f64,
    loss_ms: f64,
    coaching_cues: Vec<String>,
}

#[derive(Clone, Copy, Debug, Default)]
struct SegmentStats {
    min_speed_kmh: f64,
    average_throttle: f64,
    max_brake: f64,
    brake_onset_m: Option<f64>,
    throttle_commit_m: Option<f64>,
}

enum CoachingBuild {
    Ready(Box<CoachingReport>),
    Waiting {
        status: &'static str,
        message: String,
    },
}

pub async fn run(store: DashboardStore, report_path: PathBuf) {
    let mut last_state = None;
    loop {
        let store = store.clone();
        let result = tokio::task::spawn_blocking(move || build_report(&store)).await;
        match result {
            Ok(Ok(CoachingBuild::Ready(report))) => {
                let state = format!("ready:{}:{}", report.player.lap_id, report.p1.lap_id);
                if last_state.as_ref() != Some(&state) {
                    match write_report(&report_path, &report) {
                        Ok(()) => {
                            println!(
                                "qualifying coaching report updated: {}",
                                report_path.display()
                            );
                            last_state = Some(state);
                        }
                        Err(error) => eprintln!("failed to write coaching report: {error}"),
                    }
                }
            }
            Ok(Ok(CoachingBuild::Waiting { status, message })) => {
                let state = format!("{status}:{message}");
                if last_state.as_ref() != Some(&state) {
                    match write_status_report(&report_path, status, &message) {
                        Ok(()) => last_state = Some(state),
                        Err(error) => eprintln!("failed to write coaching status: {error}"),
                    }
                }
            }
            Ok(Err(error)) => eprintln!("failed to build qualifying coaching report: {error}"),
            Err(error) => eprintln!("qualifying coaching worker stopped unexpectedly: {error}"),
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

fn build_report(store: &DashboardStore) -> Result<CoachingBuild, String> {
    let laps = store.list_laps()?;
    let Some(latest_player_lap) = laps.iter().find(|lap| {
        lap.is_player
            && lap.valid
            && lap.completed
            && lap.lap_time_ms > 0
            && lap.session_type.eq_ignore_ascii_case("Qualifying")
    }) else {
        return Ok(CoachingBuild::Waiting {
            status: "waiting_for_qualifying_laps",
            message: "본인과 동일 클래스 P1의 유효 퀄리파잉 완주 랩을 기다리고 있습니다."
                .to_owned(),
        });
    };

    let session_id = &latest_player_lap.session_id;
    let class_name = &latest_player_lap.class_name;
    let eligible = |lap: &&LapSummary| {
        lap.session_id == *session_id
            && lap.valid
            && lap.completed
            && lap.lap_time_ms > 0
            && lap.class_name.eq_ignore_ascii_case(class_name)
    };
    let player_summary = laps
        .iter()
        .filter(eligible)
        .filter(|lap| lap.is_player)
        .min_by_key(|lap| lap.lap_time_ms)
        .ok_or_else(|| "qualifying player best lap disappeared from the store".to_owned())?;
    let p1_summary = laps
        .iter()
        .filter(eligible)
        .min_by_key(|lap| lap.lap_time_ms)
        .ok_or_else(|| "same-class qualifying reference lap is unavailable".to_owned())?;

    let player_lap = store
        .load_lap(&player_summary.id)?
        .ok_or_else(|| format!("player lap {} is unavailable", player_summary.id))?;
    let p1_lap = store
        .load_lap(&p1_summary.id)?
        .ok_or_else(|| format!("P1 lap {} is unavailable", p1_summary.id))?;
    let segments = match compare_laps(&player_lap, &p1_lap) {
        Ok(segments) => segments,
        Err(message) => {
            return Ok(CoachingBuild::Waiting {
                status: "reference_trace_unusable",
                message,
            });
        }
    };
    let focus_zones = select_focus_zones(&segments);

    Ok(CoachingBuild::Ready(Box::new(CoachingReport {
        schema_version: 1,
        status: "ready",
        generated_at_unix_ms: unix_ms(),
        session_id: session_id.clone(),
        session_type: latest_player_lap.session_type.clone(),
        track_name: latest_player_lap.track_name.clone(),
        class_name: class_name.clone(),
        player: ReportLap::from(player_summary),
        p1: ReportLap::from(p1_summary),
        total_delta_ms: i64::from(player_summary.lap_time_ms) - i64::from(p1_summary.lap_time_ms),
        segments,
        focus_zones,
    })))
}

fn compare_laps(player: &SavedLap, p1: &SavedLap) -> Result<Vec<SegmentComparison>, String> {
    let player_samples = validated_samples(player, "본인 베스트")?;
    let p1_samples = validated_samples(p1, "클래스 P1 베스트")?;

    let start_m = player_samples[0]
        .lap_distance_m
        .max(p1_samples[0].lap_distance_m);
    let end_m = player_samples[player_samples.len() - 1]
        .lap_distance_m
        .min(p1_samples[p1_samples.len() - 1].lap_distance_m);
    if end_m - start_m < MIN_REPORT_DISTANCE_M {
        return Err(format!(
            "qualifying laps overlap for only {:.0} m; at least {MIN_REPORT_DISTANCE_M:.0} m is required",
            end_m - start_m
        ));
    }

    let player_start_time = interpolate(&player_samples, start_m, |sample| sample.lap_elapsed_s);
    let p1_start_time = interpolate(&p1_samples, start_m, |sample| sample.lap_elapsed_s);
    let segment_length = (end_m - start_m) / SEGMENT_COUNT as f64;
    let mut segments = Vec::with_capacity(SEGMENT_COUNT);

    for index in 0..SEGMENT_COUNT {
        let segment_start = start_m + segment_length * index as f64;
        let segment_end = if index + 1 == SEGMENT_COUNT {
            end_m
        } else {
            start_m + segment_length * (index + 1) as f64
        };
        let player_time_start = interpolate(&player_samples, segment_start, |sample| {
            sample.lap_elapsed_s
        });
        let player_time_end =
            interpolate(&player_samples, segment_end, |sample| sample.lap_elapsed_s);
        let p1_time_start = interpolate(&p1_samples, segment_start, |sample| sample.lap_elapsed_s);
        let p1_time_end = interpolate(&p1_samples, segment_end, |sample| sample.lap_elapsed_s);
        let player_stats = segment_stats(&player_samples, segment_start, segment_end);
        let p1_stats = segment_stats(&p1_samples, segment_start, segment_end);

        segments.push(SegmentComparison {
            number: index + 1,
            start_m: round_to(segment_start, 1),
            end_m: round_to(segment_end, 1),
            loss_ms: round_to(
                ((player_time_end - player_time_start) - (p1_time_end - p1_time_start)) * 1_000.0,
                1,
            ),
            cumulative_delta_ms: round_to(
                ((player_time_end - player_start_time) - (p1_time_end - p1_start_time)) * 1_000.0,
                1,
            ),
            player_min_speed_kmh: round_to(player_stats.min_speed_kmh, 1),
            p1_min_speed_kmh: round_to(p1_stats.min_speed_kmh, 1),
            player_average_throttle: round_to(player_stats.average_throttle, 3),
            p1_average_throttle: round_to(p1_stats.average_throttle, 3),
            player_max_brake: round_to(player_stats.max_brake, 3),
            p1_max_brake: round_to(p1_stats.max_brake, 3),
            player_brake_onset_m: player_stats.brake_onset_m.map(|value| round_to(value, 1)),
            p1_brake_onset_m: p1_stats.brake_onset_m.map(|value| round_to(value, 1)),
            player_throttle_commit_m: player_stats
                .throttle_commit_m
                .map(|value| round_to(value, 1)),
            p1_throttle_commit_m: p1_stats.throttle_commit_m.map(|value| round_to(value, 1)),
        });
    }
    Ok(segments)
}

fn normalize_samples(samples: &[TelemetryPoint]) -> Vec<TelemetryPoint> {
    let usable: Vec<_> = samples
        .iter()
        .filter(|sample| {
            sample.lap_distance_m.is_finite()
                && sample.lap_elapsed_s.is_finite()
                && sample.speed_kmh.is_finite()
                && sample.lap_distance_m >= 0.0
                && sample.lap_elapsed_s >= 0.0
        })
        .collect();
    let track_length_m = usable
        .iter()
        .map(|sample| sample.lap_distance_m)
        .fold(0.0, f64::max);
    if track_length_m <= 0.0 {
        return Vec::new();
    }

    let mut normalized = Vec::with_capacity(usable.len());
    let mut wrap_offset = 0.0;
    let mut previous_raw = None;
    let mut origin = None;
    for sample in usable {
        if previous_raw
            .is_some_and(|previous| previous - sample.lap_distance_m > track_length_m * 0.5)
        {
            wrap_offset += track_length_m;
        }
        previous_raw = Some(sample.lap_distance_m);
        let unwrapped = sample.lap_distance_m + wrap_offset;
        let origin = *origin.get_or_insert(unwrapped);
        let progress = unwrapped - origin;
        if progress < 0.0
            || normalized
                .last()
                .is_some_and(|previous: &TelemetryPoint| progress <= previous.lap_distance_m + 0.01)
        {
            continue;
        }
        let mut normalized_sample = sample.clone();
        normalized_sample.lap_distance_m = progress;
        normalized.push(normalized_sample);
    }
    normalized
}

fn validated_samples(lap: &SavedLap, label: &str) -> Result<Vec<TelemetryPoint>, String> {
    let samples = normalize_samples(&lap.samples);
    if samples.len() < 2 {
        return Err(format!(
            "{label}의 원시 텔레메트리 샘플이 부족해 비교할 수 없습니다."
        ));
    }
    let covered_distance = samples[samples.len() - 1].lap_distance_m;
    if covered_distance < MIN_REPORT_DISTANCE_M {
        return Err(format!(
            "{label}의 유효 주행 거리가 {covered_distance:.0}m뿐이라 비교할 수 없습니다."
        ));
    }
    let observed_time_s = samples[samples.len() - 1].lap_elapsed_s - samples[0].lap_elapsed_s;
    let official_time_s = f64::from(lap.summary.lap_time_ms) / 1_000.0;
    let tolerance_s = (official_time_s * 0.015).max(1.5);
    if (observed_time_s - official_time_s).abs() > tolerance_s {
        return Err(format!(
            "{label}의 공식 랩타임은 {}이지만 원시 트레이스는 {}입니다. 랩 경계가 섞인 기록이라 코칭에서 제외하고 다음 정상 퀄리파잉 랩을 기다립니다.",
            format_lap_time(lap.summary.lap_time_ms),
            format_seconds(observed_time_s)
        ));
    }
    Ok(samples)
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

fn segment_stats(samples: &[TelemetryPoint], start_m: f64, end_m: f64) -> SegmentStats {
    let points: Vec<_> = samples
        .iter()
        .filter(|sample| sample.lap_distance_m >= start_m && sample.lap_distance_m <= end_m)
        .collect();
    if points.is_empty() {
        let midpoint = (start_m + end_m) / 2.0;
        return SegmentStats {
            min_speed_kmh: interpolate(samples, midpoint, |sample| sample.speed_kmh),
            average_throttle: interpolate(samples, midpoint, |sample| sample.throttle),
            max_brake: interpolate(samples, midpoint, |sample| sample.brake),
            ..SegmentStats::default()
        };
    }

    let min_speed_kmh = points
        .iter()
        .map(|sample| sample.speed_kmh)
        .fold(f64::INFINITY, f64::min);
    let average_throttle = points
        .iter()
        .map(|sample| sample.throttle.clamp(0.0, 1.0))
        .sum::<f64>()
        / points.len() as f64;
    let max_brake = points
        .iter()
        .map(|sample| sample.brake.clamp(0.0, 1.0))
        .fold(0.0, f64::max);
    let brake_onset_m = points
        .iter()
        .find(|sample| sample.brake >= 0.15)
        .map(|sample| sample.lap_distance_m);
    let peak_brake_index = points
        .iter()
        .enumerate()
        .max_by(|(_, left), (_, right)| {
            left.brake
                .partial_cmp(&right.brake)
                .unwrap_or(Ordering::Equal)
        })
        .map(|(index, _)| index);
    let throttle_commit_m = peak_brake_index.and_then(|index| {
        points[index..]
            .iter()
            .find(|sample| sample.throttle >= 0.75 && sample.brake <= 0.10)
            .map(|sample| sample.lap_distance_m)
    });

    SegmentStats {
        min_speed_kmh,
        average_throttle,
        max_brake,
        brake_onset_m,
        throttle_commit_m,
    }
}

fn select_focus_zones(segments: &[SegmentComparison]) -> Vec<FocusZone> {
    let mut candidates: Vec<_> = segments
        .iter()
        .filter(|segment| segment.loss_ms > 5.0)
        .collect();
    candidates.sort_by(|left, right| {
        right
            .loss_ms
            .partial_cmp(&left.loss_ms)
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
            loss_ms: segment.loss_ms,
            coaching_cues: coaching_cues(segment),
        })
        .collect()
}

fn coaching_cues(segment: &SegmentComparison) -> Vec<String> {
    let mut cues = Vec::new();
    if let (Some(player), Some(p1)) = (segment.player_brake_onset_m, segment.p1_brake_onset_m) {
        if p1 - player > 12.0 {
            cues.push(format!(
                "P1보다 약 {:.0} m 일찍 제동합니다. 기준점을 뒤로 옮기되 제동 해제는 부드럽게 이어가세요.",
                p1 - player
            ));
        } else if player - p1 > 12.0
            && segment.player_min_speed_kmh + 2.0 < segment.p1_min_speed_kmh
        {
            cues.push(format!(
                "P1보다 약 {:.0} m 늦게 제동하면서 최저속도가 더 낮습니다. 진입 과속을 줄여 회전 속도를 살리세요.",
                player - p1
            ));
        }
    }
    let minimum_speed_gap = segment.p1_min_speed_kmh - segment.player_min_speed_kmh;
    if minimum_speed_gap > 4.0 {
        cues.push(format!(
            "코너 최저속도가 P1보다 {:.1} km/h 낮습니다. 진입보다 미드코너 속도 유지에 우선순위를 두세요.",
            minimum_speed_gap
        ));
    }
    if let (Some(player), Some(p1)) = (
        segment.player_throttle_commit_m,
        segment.p1_throttle_commit_m,
    ) && player - p1 > 12.0
    {
        cues.push(format!(
            "강한 스로틀 재개가 P1보다 약 {:.0} m 늦습니다. 조향을 풀며 더 이르게 가속을 연결하세요.",
            player - p1
        ));
    }
    if segment.p1_average_throttle - segment.player_average_throttle > 0.12 {
        cues.push(format!(
            "구간 평균 스로틀이 P1보다 {:.0}%p 낮습니다. 탈출 라인과 가속 재개 시점을 함께 확인하세요.",
            (segment.p1_average_throttle - segment.player_average_throttle) * 100.0
        ));
    }
    if cues.is_empty() {
        cues.push(
            "속도 차이보다 라인과 짧은 리프트 구간에서 손실이 발생했을 가능성이 큽니다. 원시 트레이스를 겹쳐 확인하세요."
                .to_owned(),
        );
    }
    cues
}

fn write_status_report(path: &Path, status: &str, message: &str) -> Result<(), String> {
    ensure_parent(path)?;
    let label = match status {
        "reference_trace_unusable" => "기준 랩 데이터 불완전",
        _ => "퀄리파잉 유효 베스트랩 수집 대기 중",
    };
    fs::write(
        path,
        format!("# LMU 퀄리파잉 AI 코칭\n\n상태: {label}\n\n{message}\n"),
    )
    .map_err(|error| format!("failed to write {}: {error}", path.display()))?;
    let json_path = json_path(path);
    let waiting = serde_json::json!({
        "schema_version": 1,
        "status": status,
        "message": message,
        "generated_at_unix_ms": unix_ms(),
    });
    let body = serde_json::to_string_pretty(&waiting)
        .map_err(|error| format!("failed to encode waiting report: {error}"))?;
    fs::write(&json_path, body)
        .map_err(|error| format!("failed to write {}: {error}", json_path.display()))?;
    Ok(())
}

fn write_report(path: &Path, report: &CoachingReport) -> Result<(), String> {
    ensure_parent(path)?;
    let json_path = json_path(path);
    let json = serde_json::to_string_pretty(report)
        .map_err(|error| format!("failed to encode coaching report: {error}"))?;
    fs::write(&json_path, json)
        .map_err(|error| format!("failed to write {}: {error}", json_path.display()))?;
    fs::write(path, render_markdown(report))
        .map_err(|error| format!("failed to write {}: {error}", path.display()))?;
    Ok(())
}

fn render_markdown(report: &CoachingReport) -> String {
    let mut body = format!(
        "# LMU 퀄리파잉 AI 코칭\n\n- 상태: 비교 준비 완료\n- 트랙: {}\n- 세션: {}\n- 클래스: {}\n- 본인 베스트: {} / {}\n- 클래스 P1 베스트: {} / {}\n- 총 차이: {:+.3}초\n\n## 핵심 손실 구간\n",
        report.track_name,
        report.session_type,
        report.class_name,
        report.player.driver_name,
        format_lap_time(report.player.lap_time_ms),
        report.p1.driver_name,
        format_lap_time(report.p1.lap_time_ms),
        report.total_delta_ms as f64 / 1_000.0,
    );
    if report.focus_zones.is_empty() {
        body.push_str("\n현재 본인 베스트가 클래스 P1 기준과 같거나, 5ms를 넘는 뚜렷한 손실 구간이 없습니다.\n");
    } else {
        for zone in &report.focus_zones {
            body.push_str(&format!(
                "\n### {}. {:.0}-{:.0}m ({:+.3}초)\n",
                zone.rank,
                zone.start_m,
                zone.end_m,
                zone.loss_ms / 1_000.0
            ));
            for cue in &zone.coaching_cues {
                body.push_str(&format!("- {cue}\n"));
            }
        }
    }
    body.push_str(
        "\n## 데이터\n\n전체 20개 거리 구간의 시간, 속도, 제동, 스로틀 비교값은 같은 이름의 JSON 파일에 저장됩니다.\n",
    );
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

fn format_seconds(seconds: f64) -> String {
    format_lap_time((seconds.max(0.0) * 1_000.0).round() as u32)
}

fn round_to(value: f64, decimal_places: i32) -> f64 {
    let factor = 10_f64.powi(decimal_places);
    (value * factor).round() / factor
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_laps_by_distance_and_finds_loss() {
        let player = synthetic_lap("player", 11.0, 95.0, 0.35, true);
        let p1 = synthetic_lap("p1", 10.0, 101.0, 0.55, false);
        let segments = compare_laps(&player, &p1).unwrap();

        assert_eq!(segments.len(), SEGMENT_COUNT);
        assert!(segments.iter().map(|segment| segment.loss_ms).sum::<f64>() > 900.0);
        assert!(segments[8].p1_min_speed_kmh > segments[8].player_min_speed_kmh);
    }

    #[test]
    fn ranks_only_positive_focus_zones() {
        let segments = vec![
            segment(1, -15.0),
            segment(2, 120.0),
            segment(3, 80.0),
            segment(4, 210.0),
            segment(5, 4.0),
        ];
        let focus = select_focus_zones(&segments);

        assert_eq!(focus.len(), 3);
        assert_eq!(focus[0].segment_number, 4);
        assert_eq!(focus[1].segment_number, 2);
        assert_eq!(focus[2].segment_number, 3);
    }

    #[test]
    fn formats_lap_time_for_coaching_header() {
        assert_eq!(format_lap_time(218_432), "3:38.432");
    }

    #[test]
    fn builds_report_from_qualifying_player_and_class_p1() {
        let directory = std::env::temp_dir().join(format!(
            "lmu-dashboard-coach-test-{}-{}",
            std::process::id(),
            unix_ms()
        ));
        let store = DashboardStore::open(&directory).unwrap();
        let session = crate::model::SessionState {
            id: "qualifying-session".to_owned(),
            track_name: "Le Mans".to_owned(),
            session_type: "Qualifying".to_owned(),
            track_length_m: 1_000.0,
            ..crate::model::SessionState::default()
        };
        store.save_session(&session).unwrap();
        let mut player = synthetic_lap("player", 11.0, 95.0, 0.35, true);
        configure_lap(&mut player, &session, 7, "Player", 2);
        let mut p1 = synthetic_lap("p1", 10.0, 101.0, 0.55, false);
        configure_lap(&mut p1, &session, 12, "Reference", 1);
        store.save_lap(&player).unwrap();
        store.save_lap(&p1).unwrap();

        let CoachingBuild::Ready(report) = build_report(&store).unwrap() else {
            panic!("expected a ready qualifying coaching report");
        };

        assert_eq!(report.session_type, "Qualifying");
        assert_eq!(report.player.driver_name, "Player");
        assert_eq!(report.p1.driver_name, "Reference");
        assert_eq!(report.total_delta_ms, 1_000);
        assert_eq!(report.segments.len(), SEGMENT_COUNT);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn unwraps_start_line_without_reordering_samples() {
        let samples = vec![
            sample_at(990.0, 0.0),
            sample_at(5.0, 1.0),
            sample_at(20.0, 2.0),
        ];

        let normalized = normalize_samples(&samples);

        assert_eq!(normalized.len(), 3);
        assert_eq!(normalized[0].lap_distance_m, 0.0);
        assert_eq!(normalized[1].lap_distance_m, 5.0);
        assert_eq!(normalized[2].lap_distance_m, 20.0);
        assert_eq!(normalized[2].lap_elapsed_s, 2.0);
    }

    #[test]
    fn rejects_trace_time_that_does_not_match_official_lap() {
        let player = synthetic_lap("player", 11.0, 95.0, 0.35, true);
        let mut p1 = synthetic_lap("p1", 10.0, 101.0, 0.55, false);
        p1.samples.last_mut().unwrap().lap_elapsed_s = 25.0;

        let error = compare_laps(&player, &p1).unwrap_err();

        assert!(error.contains("랩 경계가 섞인 기록"));
    }

    fn synthetic_lap(
        id: &str,
        lap_time_s: f64,
        corner_speed: f64,
        throttle: f64,
        is_player: bool,
    ) -> SavedLap {
        let samples = (0..=100)
            .map(|index| {
                let progress = index as f64 / 100.0;
                TelemetryPoint {
                    lap_elapsed_s: progress * lap_time_s,
                    lap_distance_m: progress * 1_000.0,
                    speed_kmh: if (0.4..=0.55).contains(&progress) {
                        corner_speed
                    } else {
                        220.0
                    },
                    throttle: if (0.4..=0.55).contains(&progress) {
                        throttle
                    } else {
                        1.0
                    },
                    brake: if (0.36..=0.43).contains(&progress) {
                        0.8
                    } else {
                        0.0
                    },
                    ..TelemetryPoint::default()
                }
            })
            .collect();
        SavedLap {
            summary: LapSummary {
                id: id.to_owned(),
                is_player,
                lap_time_ms: (lap_time_s * 1_000.0) as u32,
                valid: true,
                completed: true,
                ..LapSummary::default()
            },
            samples,
        }
    }

    fn segment(number: usize, loss_ms: f64) -> SegmentComparison {
        SegmentComparison {
            number,
            start_m: number as f64 * 100.0,
            end_m: (number + 1) as f64 * 100.0,
            loss_ms,
            cumulative_delta_ms: loss_ms,
            player_min_speed_kmh: 90.0,
            p1_min_speed_kmh: 100.0,
            player_average_throttle: 0.4,
            p1_average_throttle: 0.6,
            player_max_brake: 0.9,
            p1_max_brake: 0.8,
            player_brake_onset_m: None,
            p1_brake_onset_m: None,
            player_throttle_commit_m: None,
            p1_throttle_commit_m: None,
        }
    }

    fn configure_lap(
        lap: &mut SavedLap,
        session: &crate::model::SessionState,
        vehicle_id: i32,
        driver_name: &str,
        class_position: u8,
    ) {
        lap.summary.session_id.clone_from(&session.id);
        lap.summary.track_name.clone_from(&session.track_name);
        lap.summary.vehicle_id = vehicle_id;
        lap.summary.driver_name = driver_name.to_owned();
        lap.summary.class_name = "Hypercar".to_owned();
        lap.summary.class_position = class_position;
        lap.summary.sample_count = lap.samples.len();
        lap.summary.created_at_unix_ms = unix_ms();
    }

    fn sample_at(distance_m: f64, elapsed_s: f64) -> TelemetryPoint {
        TelemetryPoint {
            lap_distance_m: distance_m,
            lap_elapsed_s: elapsed_s,
            speed_kmh: 100.0,
            ..TelemetryPoint::default()
        }
    }
}
