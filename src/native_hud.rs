use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use eframe::egui::{
    self, Align2, Color32, FontFamily, FontId, Pos2, Rect, Stroke, StrokeKind, pos2, vec2,
};

use crate::config::BridgeConfig;
use crate::hud::{HudHandle, new_hud_handle};
use crate::telemetry::{DamageSample, InputSample, TelemetryUpdate};

const BG: Color32 = Color32::from_rgb(18, 23, 31);
const PANEL: Color32 = Color32::from_rgb(35, 43, 55);
const PANEL_SOFT: Color32 = Color32::from_rgb(45, 56, 72);
const LINE: Color32 = Color32::from_rgba_premultiplied(190, 214, 238, 80);
const TEXT: Color32 = Color32::from_rgb(240, 244, 248);
const TITLE: Color32 = Color32::from_rgb(190, 202, 214);
const MUTED: Color32 = Color32::from_rgb(144, 156, 170);
const BLUE: Color32 = Color32::from_rgb(42, 160, 226);
const YELLOW: Color32 = Color32::from_rgb(238, 196, 19);
const GREEN: Color32 = Color32::from_rgb(76, 205, 129);
const RED: Color32 = Color32::from_rgb(231, 82, 75);

pub fn run(config: BridgeConfig) -> Result<(), String> {
    let hud = new_hud_handle();
    let worker_hud = hud.clone();
    let runtime_error = Arc::new(Mutex::new(None));
    let worker_error = Arc::clone(&runtime_error);

    thread::spawn(move || {
        if let Err(error) = crate::start_runtime_with_hud(config, Some(worker_hud)) {
            eprintln!("[startup-error] {error}");
            if let Ok(mut slot) = worker_error.lock() {
                *slot = Some(error);
            }
        }
    });

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Sim MOZA Bridge")
            .with_inner_size([1440.0, 860.0])
            .with_min_inner_size([1180.0, 820.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Sim MOZA Bridge",
        options,
        Box::new(move |cc| {
            configure_style(&cc.egui_ctx);
            Ok(Box::new(NativeHudApp {
                hud,
                runtime_error,
                trace: Vec::new(),
                last_trace_frame: None,
            }))
        }),
    )
    .map_err(|error| format!("native HUD failed: {error}"))
}

struct NativeHudApp {
    hud: HudHandle,
    runtime_error: Arc<Mutex<Option<String>>>,
    trace: Vec<InputTracePoint>,
    last_trace_frame: Option<u32>,
}

#[derive(Clone, Copy)]
struct InputTracePoint {
    throttle: f32,
    brake: f32,
    rev: f32,
    steer: f32,
}

impl eframe::App for NativeHudApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        ui.ctx().request_repaint_after(Duration::from_millis(50));

        let state = self.hud.snapshot();
        let error = self.runtime_error.lock().ok().and_then(|slot| slot.clone());
        self.record_trace(&state);
        let rect = ui.max_rect();
        let painter = ui.painter_at(rect);

        draw_background(&painter, rect);
        draw_header(&painter, rect, &state, error.as_deref());
        draw_dashboard(
            &painter,
            rect.shrink2(vec2(24.0, 22.0)),
            &state,
            &self.trace,
            error.as_deref(),
        );
    }
}

impl NativeHudApp {
    fn record_trace(&mut self, state: &TelemetryUpdate) {
        let Some(input) = state.input.as_ref() else {
            return;
        };
        if self.last_trace_frame == Some(input.frame_identifier) {
            return;
        }

        self.last_trace_frame = Some(input.frame_identifier);
        self.trace.push(InputTracePoint {
            throttle: input.throttle.clamp(0.0, 1.0),
            brake: input.brake.clamp(0.0, 1.0),
            rev: (f32::from(input.rev_lights_percent) / 100.0).clamp(0.0, 1.0),
            steer: ((input.steer.clamp(-1.0, 1.0) + 1.0) / 2.0).clamp(0.0, 1.0),
        });

        const TRACE_LIMIT: usize = 180;
        if self.trace.len() > TRACE_LIMIT {
            let overflow = self.trace.len() - TRACE_LIMIT;
            self.trace.drain(0..overflow);
        }
    }
}

fn configure_style(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = BG;
    visuals.window_fill = BG;
    visuals.extreme_bg_color = Color32::from_rgb(20, 24, 30);
    visuals.selection.bg_fill = BLUE;
    ctx.set_visuals(visuals);
}

fn draw_dashboard(
    painter: &egui::Painter,
    rect: Rect,
    state: &TelemetryUpdate,
    trace: &[InputTracePoint],
    error: Option<&str>,
) {
    let body = Rect::from_min_max(pos2(rect.left(), rect.top() + 62.0), rect.right_bottom());
    let timeline_h = 118.0;
    let content = Rect::from_min_max(
        body.left_top(),
        pos2(body.right(), body.bottom() - timeline_h),
    );
    let timeline = Rect::from_min_max(
        pos2(body.left(), content.bottom() + 18.0),
        body.right_bottom(),
    );

    let gap = 18.0;
    let side_w = (content.width() * 0.265).clamp(260.0, 390.0);
    let left = Rect::from_min_size(content.left_top(), vec2(side_w, content.height()));
    let right = Rect::from_min_size(
        pos2(content.right() - side_w, content.top()),
        vec2(side_w, content.height()),
    );
    let center = Rect::from_min_max(
        pos2(left.right() + gap, content.top()),
        pos2(right.left() - gap, content.bottom()),
    );

    draw_driver_panel(painter, left, state, BLUE, "PLAYER CAR", "BRIDGE INPUT");
    draw_center_panel(painter, center, state, trace, error);
    draw_driver_panel(painter, right, state, YELLOW, "MOZA OUTPUT", "FORWARDING");
    draw_lap_progress_panel(painter, timeline, state);
}

fn draw_background(painter: &egui::Painter, rect: Rect) {
    painter.rect_filled(rect, 0.0, BG);
    painter.rect_filled(
        Rect::from_min_max(rect.left_top(), pos2(rect.right(), rect.top() + 96.0)),
        0.0,
        Color32::from_rgb(21, 27, 37),
    );
    painter.line_segment(
        [
            pos2(rect.left(), rect.top() + 96.0),
            pos2(rect.right(), rect.top() + 96.0),
        ],
        Stroke::new(1.0, Color32::from_rgba_premultiplied(210, 225, 240, 28)),
    );
}

fn draw_header(painter: &egui::Painter, rect: Rect, state: &TelemetryUpdate, error: Option<&str>) {
    let header = Rect::from_min_max(rect.left_top(), pos2(rect.right(), rect.top() + 72.0));
    painter.rect_filled(
        header,
        0.0,
        Color32::from_rgba_premultiplied(18, 23, 31, 248),
    );
    painter.line_segment(
        [
            pos2(header.left() + 24.0, header.bottom() - 1.0),
            pos2(header.right() - 24.0, header.bottom() - 1.0),
        ],
        Stroke::new(1.0, LINE),
    );

    text(
        painter,
        pos2(header.left() + 26.0, header.top() + 25.0),
        Align2::LEFT_CENTER,
        "SIM MOZA BRIDGE",
        22.0,
        TEXT,
    );
    text(
        painter,
        pos2(header.right() - 26.0, header.top() + 25.0),
        Align2::RIGHT_CENTER,
        "F1 25 TELEMETRY / MOZA PIT HOUSE",
        13.0,
        MUTED,
    );

    let status_text = if let Some(error) = error {
        error
    } else if state.is_empty() {
        "WAITING FOR TELEMETRY"
    } else {
        "LIVE TELEMETRY"
    };
    let status_color = if error.is_some() {
        RED
    } else if state.is_empty() {
        MUTED
    } else {
        GREEN
    };
    painter.circle_filled(
        pos2(header.left() + 28.0, header.bottom() - 18.0),
        4.0,
        status_color,
    );
    text(
        painter,
        pos2(header.left() + 40.0, header.bottom() - 18.0),
        Align2::LEFT_CENTER,
        status_text,
        11.0,
        status_color,
    );
}

fn draw_driver_panel(
    painter: &egui::Painter,
    rect: Rect,
    state: &TelemetryUpdate,
    accent: Color32,
    title: &str,
    subtitle: &str,
) {
    panel(painter, rect, title, accent);
    text(
        painter,
        pos2(rect.left() + 20.0, rect.top() + 42.0),
        Align2::LEFT_CENTER,
        subtitle,
        12.0,
        MUTED,
    );

    let input = state.input.as_ref();
    let lap = state.lap.as_ref();
    let status = state.status.as_ref();
    let damage = state.damage.as_ref();

    draw_big_metric(
        painter,
        Rect::from_min_size(
            pos2(rect.left() + 20.0, rect.top() + 82.0),
            vec2(130.0, 130.0),
        ),
        "POSITION",
        lap.map(|value| value.car_position.to_string()),
        "",
        accent,
    );
    draw_big_metric(
        painter,
        Rect::from_min_size(
            pos2(rect.left() + 20.0, rect.top() + 232.0),
            vec2(154.0, 120.0),
        ),
        "TOP LAP SPEED",
        input.map(|value| value.speed_kmh.to_string()),
        "km/h",
        TEXT,
    );

    let bar_left = rect.left() + 20.0;
    let bar_w = rect.width() - 40.0;
    let tyre_rect = Rect::from_min_size(pos2(bar_left, rect.bottom() - 112.0), vec2(bar_w, 92.0));
    let lower_top = tyre_rect.top() - 108.0;
    let throttle_top = lower_top - 82.0;
    let brake_top = throttle_top + 34.0;
    let car_bottom = (throttle_top - 22.0).max(rect.top() + 260.0);
    let car_rect = Rect::from_min_max(
        pos2(rect.left() + rect.width() * 0.38, rect.top() + 108.0),
        pos2(rect.right() - 20.0, car_bottom),
    );
    draw_car_silhouette(painter, car_rect, accent);

    draw_bar(
        painter,
        Rect::from_min_size(pos2(bar_left, throttle_top), vec2(bar_w, 22.0)),
        "THROTTLE",
        input.map(|value| value.throttle).unwrap_or(0.0),
        BLUE,
    );
    draw_bar(
        painter,
        Rect::from_min_size(pos2(bar_left, brake_top), vec2(bar_w, 22.0)),
        "BRAKE",
        input.map(|value| value.brake).unwrap_or(0.0),
        YELLOW,
    );

    draw_small_pair(
        painter,
        pos2(bar_left, lower_top),
        "CURRENT LAP",
        lap.map(|value| format_ms(value.current_lap_time_ms)),
        "BEST LAP",
        lap.map(|value| format_ms(value.last_lap_time_ms)),
        accent,
    );
    draw_small_pair(
        painter,
        pos2(bar_left, lower_top + 56.0),
        "FUEL",
        status.map(|value| format!("{:.1} kg", value.fuel_in_tank)),
        "ERS",
        status.map(|value| format!("{:.0}%", value.ers_percent())),
        accent,
    );

    draw_tyre_life(painter, tyre_rect, input, damage, accent);
}

fn draw_center_panel(
    painter: &egui::Painter,
    rect: Rect,
    state: &TelemetryUpdate,
    trace: &[InputTracePoint],
    error: Option<&str>,
) {
    panel(
        painter,
        rect,
        "RACE CONTROL",
        Color32::from_rgb(150, 170, 185),
    );

    let map_rect = Rect::from_min_max(
        pos2(rect.left() + 26.0, rect.top() + 104.0),
        pos2(rect.right() - 26.0, rect.top() + 332.0),
    );
    draw_session_strip(painter, rect, state, error);
    draw_track_map(painter, map_rect, state);

    let chart_rect = Rect::from_min_max(
        pos2(rect.left() + 26.0, map_rect.bottom() + 24.0),
        pos2(rect.right() - 26.0, map_rect.bottom() + 174.0),
    );
    draw_trace_chart(painter, chart_rect, trace);

    let raw_rect = Rect::from_min_max(
        pos2(rect.left() + 26.0, chart_rect.bottom() + 24.0),
        pos2(rect.right() - 26.0, rect.bottom() - 22.0),
    );
    draw_raw_panel(painter, raw_rect, state);
}

fn draw_session_strip(
    painter: &egui::Painter,
    rect: Rect,
    state: &TelemetryUpdate,
    error: Option<&str>,
) {
    let lap = state.lap.as_ref();
    let session = state.session.as_ref();
    let status = state.status.as_ref();

    let y = rect.top() + 58.0;
    metric_inline(
        painter,
        pos2(rect.left() + 28.0, y),
        "TRACK",
        session
            .map(|value| format!("ID {} / {} m", value.track_id, value.track_length_m))
            .unwrap_or_else(|| "WAITING".to_owned()),
        BLUE,
    );
    metric_inline(
        painter,
        pos2(rect.left() + rect.width() * 0.36, y),
        "LAP",
        lap.map(|value| format!("{}", value.current_lap_num))
            .unwrap_or_else(|| "--".to_owned()),
        TEXT,
    );
    metric_inline(
        painter,
        pos2(rect.left() + rect.width() * 0.54, y),
        "AIR / TRACK",
        session
            .map(|value| format!("{}C / {}C", value.air_temp_c, value.track_temp_c))
            .unwrap_or_else(|| "-- / --".to_owned()),
        TEXT,
    );
    metric_inline(
        painter,
        pos2(rect.left() + rect.width() * 0.76, y),
        "FUEL LAPS",
        status
            .map(|value| format!("{:.1}", value.fuel_remaining_laps))
            .unwrap_or_else(|| "--".to_owned()),
        YELLOW,
    );

    if let Some(error) = error {
        text(
            painter,
            pos2(rect.center().x, rect.top() + 92.0),
            Align2::CENTER_CENTER,
            error,
            13.0,
            RED,
        );
    }
}

fn draw_track_map(painter: &egui::Painter, rect: Rect, state: &TelemetryUpdate) {
    painter.rect_filled(rect, 6.0, Color32::from_rgb(29, 36, 47));
    painter.rect_stroke(rect, 6.0, Stroke::new(1.0, LINE), StrokeKind::Inside);

    let points = [
        (0.18, 0.72),
        (0.32, 0.72),
        (0.38, 0.62),
        (0.30, 0.46),
        (0.26, 0.22),
        (0.42, 0.14),
        (0.50, 0.34),
        (0.62, 0.56),
        (0.82, 0.58),
        (0.90, 0.68),
        (0.78, 0.75),
        (0.56, 0.75),
        (0.38, 0.75),
        (0.18, 0.72),
    ];
    let mapped = points
        .iter()
        .map(|(x, y)| {
            pos2(
                rect.left() + rect.width() * x,
                rect.top() + rect.height() * y,
            )
        })
        .collect::<Vec<_>>();

    for pair in mapped.windows(2) {
        painter.line_segment(
            [pair[0], pair[1]],
            Stroke::new(15.0, Color32::from_rgb(67, 82, 98)),
        );
    }
    for pair in mapped.windows(2).take(4) {
        painter.line_segment(
            [pair[0], pair[1]],
            Stroke::new(8.0, Color32::from_rgb(154, 177, 196)),
        );
    }

    let progress = telemetry_track_progress(state);
    draw_polyline_progress(painter, &mapped, progress, Stroke::new(8.0, BLUE));

    for (index, point) in mapped.iter().enumerate().take(mapped.len() - 1) {
        painter.circle_filled(*point, 3.5, Color32::from_rgb(101, 116, 130));
        text(
            painter,
            *point + vec2(8.0, -9.0),
            Align2::LEFT_CENTER,
            &format!("{:02}", index + 1),
            10.0,
            MUTED,
        );
    }

    if state.lap.is_some() {
        triangle(painter, point_at_progress(&mapped, progress), 12.0, BLUE);
    }

    if let (Some(lap), Some(session)) = (state.lap.as_ref(), state.session.as_ref()) {
        text(
            painter,
            pos2(rect.right() - 24.0, rect.top() + 24.0),
            Align2::RIGHT_CENTER,
            &format!(
                "{:.0} / {} m",
                lap.lap_distance_m.max(0.0),
                session.track_length_m
            ),
            11.0,
            MUTED,
        );
    }

    let start_label = pos2(rect.left() + rect.width() * 0.18, rect.bottom() - 24.0);
    text(
        painter,
        start_label,
        Align2::CENTER_CENTER,
        "START",
        10.0,
        MUTED,
    );
}

fn draw_trace_chart(painter: &egui::Painter, rect: Rect, trace: &[InputTracePoint]) {
    panel_box(painter, rect, "INPUT HISTORY", BLUE);

    let summary_top = rect.top() + 40.0;
    let latest = trace.last().copied();
    draw_input_stat(
        painter,
        pos2(rect.left() + 22.0, summary_top),
        "THROTTLE",
        latest.map(|point| point.throttle),
        BLUE,
    );
    draw_input_stat(
        painter,
        pos2(rect.left() + 150.0, summary_top),
        "BRAKE",
        latest.map(|point| point.brake),
        YELLOW,
    );
    draw_input_stat(
        painter,
        pos2(rect.left() + 278.0, summary_top),
        "REV",
        latest.map(|point| point.rev),
        GREEN,
    );
    draw_input_stat(
        painter,
        pos2(rect.left() + 406.0, summary_top),
        "STEER",
        latest.map(|point| point.steer * 2.0 - 1.0),
        Color32::from_rgb(190, 202, 214),
    );

    let plot = Rect::from_min_max(
        pos2(rect.left() + 22.0, rect.top() + 76.0),
        pos2(rect.right() - 22.0, rect.bottom() - 18.0),
    );
    painter.rect_filled(plot, 2.0, Color32::from_rgb(37, 46, 59));
    painter.rect_stroke(plot, 2.0, Stroke::new(1.0, LINE), StrokeKind::Inside);
    for index in 1..4 {
        let y = plot.top() + plot.height() / 4.0 * index as f32;
        painter.line_segment(
            [pos2(plot.left(), y), pos2(plot.right(), y)],
            Stroke::new(1.0, Color32::from_rgba_premultiplied(210, 225, 240, 20)),
        );
    }
    for index in 1..8 {
        let x = plot.left() + plot.width() / 8.0 * index as f32;
        painter.line_segment(
            [pos2(x, plot.top()), pos2(x, plot.bottom())],
            Stroke::new(1.0, Color32::from_rgba_premultiplied(210, 225, 240, 16)),
        );
    }

    if trace.is_empty() {
        text(
            painter,
            plot.center(),
            Align2::CENTER_CENTER,
            "NO INPUT PACKETS YET",
            12.0,
            MUTED,
        );
        return;
    }

    draw_trace_series(painter, plot, trace, |point| point.throttle, BLUE);
    draw_trace_series(painter, plot, trace, |point| point.brake, YELLOW);
    draw_trace_series(painter, plot, trace, |point| point.rev, GREEN);
    draw_trace_series(
        painter,
        plot,
        trace,
        |point| point.steer,
        Color32::from_rgb(190, 202, 214),
    );
}

fn draw_raw_panel(painter: &egui::Painter, rect: Rect, state: &TelemetryUpdate) {
    panel_box(
        painter,
        rect,
        "RAW TELEMETRY DATA",
        Color32::from_rgb(150, 170, 185),
    );

    let input = state.input.as_ref();
    let status = state.status.as_ref();
    let lines = [
        (
            "vCar",
            input.map(|value| format!("{} kph", value.speed_kmh)),
        ),
        ("nGear", input.map(|value| gear_label(value.gear))),
        (
            "rThrottlePedal",
            input.map(|value| format!("{:.1}%", value.throttle * 100.0)),
        ),
        (
            "pBrakeR",
            input.map(|value| format!("{:.1}%", value.brake * 100.0)),
        ),
        (
            "ERS",
            status.map(|value| format!("{:.0}%", value.ers_percent())),
        ),
    ];

    for (index, (label, value)) in lines.into_iter().enumerate() {
        let row = if index < 3 { 0 } else { 1 };
        let col = if index < 3 { index } else { index - 3 };
        let label_x = rect.left() + 22.0 + col as f32 * rect.width() * 0.32;
        let value_x = label_x + 112.0;
        let y = rect.top() + 44.0 + row as f32 * 28.0;
        text(
            painter,
            pos2(label_x, y),
            Align2::LEFT_CENTER,
            label,
            12.0,
            MUTED,
        );
        text(
            painter,
            pos2(value_x, y),
            Align2::LEFT_CENTER,
            &value.unwrap_or_else(|| "--".to_owned()),
            13.0,
            BLUE,
        );
    }
}

fn draw_lap_progress_panel(painter: &egui::Painter, rect: Rect, state: &TelemetryUpdate) {
    panel(
        painter,
        rect,
        "LAP PROGRESS",
        Color32::from_rgb(150, 170, 185),
    );

    let progress = telemetry_track_progress(state);
    let current_lap = state
        .lap
        .as_ref()
        .map(|lap| lap.current_lap_num)
        .unwrap_or(0);
    let total_laps = state
        .session
        .as_ref()
        .map(|session| session.total_laps)
        .unwrap_or(53)
        .max(1);
    let lap_distance = state
        .lap
        .as_ref()
        .map(|lap| lap.lap_distance_m.max(0.0))
        .unwrap_or(0.0);
    let track_length = state
        .session
        .as_ref()
        .map(|session| session.track_length_m)
        .unwrap_or(0);

    let bar = Rect::from_min_max(
        pos2(rect.left() + 30.0, rect.top() + 62.0),
        pos2(rect.right() - 30.0, rect.top() + 84.0),
    );
    painter.rect_filled(bar, 2.0, Color32::from_rgb(49, 60, 75));
    painter.rect_filled(
        Rect::from_min_max(
            bar.left_top(),
            pos2(bar.left() + bar.width() * progress, bar.bottom()),
        ),
        2.0,
        BLUE,
    );
    painter.rect_stroke(bar, 2.0, Stroke::new(1.0, LINE), StrokeKind::Inside);

    for tick in 0..=10 {
        let x = bar.left() + bar.width() * tick as f32 / 10.0;
        painter.line_segment(
            [pos2(x, bar.bottom() + 8.0), pos2(x, bar.bottom() + 20.0)],
            Stroke::new(1.0, Color32::from_rgba_premultiplied(200, 215, 230, 80)),
        );
    }

    triangle(
        painter,
        pos2(bar.left() + bar.width() * progress, bar.top() - 10.0),
        10.0,
        BLUE,
    );

    let metrics_y = rect.bottom() - 28.0;
    metric_line(
        painter,
        pos2(rect.left() + 32.0, metrics_y),
        "LAP",
        if current_lap == 0 {
            "-- / --".to_owned()
        } else {
            format!("{current_lap} / {total_laps}")
        },
        BLUE,
    );
    metric_line(
        painter,
        pos2(rect.left() + 210.0, metrics_y),
        "DISTANCE",
        if track_length == 0 {
            "--".to_owned()
        } else {
            format!("{lap_distance:.0} / {track_length} m")
        },
        TEXT,
    );
    metric_line(
        painter,
        pos2(rect.left() + 470.0, metrics_y),
        "CURRENT",
        state
            .lap
            .as_ref()
            .map(|lap| format_ms(lap.current_lap_time_ms))
            .unwrap_or_else(|| "--".to_owned()),
        TEXT,
    );
    metric_line(
        painter,
        pos2(rect.left() + 690.0, metrics_y),
        "BEST",
        state
            .lap
            .as_ref()
            .map(|lap| format_ms(lap.last_lap_time_ms))
            .unwrap_or_else(|| "--".to_owned()),
        YELLOW,
    );
}

fn panel(painter: &egui::Painter, rect: Rect, title: &str, accent: Color32) {
    painter.rect_filled(rect, 6.0, PANEL);
    painter.rect_stroke(rect, 6.0, Stroke::new(1.0, LINE), StrokeKind::Inside);
    painter.line_segment(
        [
            pos2(rect.left() + 18.0, rect.top() + 34.0),
            pos2(rect.right() - 18.0, rect.top() + 34.0),
        ],
        Stroke::new(1.0, Color32::from_rgba_premultiplied(214, 228, 242, 86)),
    );
    painter.rect_filled(
        Rect::from_min_size(pos2(rect.left() + 18.0, rect.top() + 33.0), vec2(72.0, 2.0)),
        1.0,
        accent,
    );
    text(
        painter,
        pos2(rect.left() + 18.0, rect.top() + 18.0),
        Align2::LEFT_CENTER,
        title,
        13.0,
        TITLE,
    );
}

fn panel_box(painter: &egui::Painter, rect: Rect, title: &str, accent: Color32) {
    painter.rect_filled(rect, 4.0, PANEL_SOFT);
    painter.rect_stroke(rect, 4.0, Stroke::new(1.0, LINE), StrokeKind::Inside);
    text(
        painter,
        pos2(rect.left() + 16.0, rect.top() + 17.0),
        Align2::LEFT_CENTER,
        title,
        11.0,
        TITLE,
    );
    painter.rect_filled(
        Rect::from_min_size(pos2(rect.left() + 16.0, rect.top() + 30.0), vec2(56.0, 2.0)),
        1.0,
        accent,
    );
}

fn draw_big_metric(
    painter: &egui::Painter,
    rect: Rect,
    label: &str,
    value: Option<String>,
    unit: &str,
    color: Color32,
) {
    text(
        painter,
        rect.left_top(),
        Align2::LEFT_TOP,
        label,
        11.0,
        MUTED,
    );
    text(
        painter,
        pos2(rect.left(), rect.top() + 58.0),
        Align2::LEFT_CENTER,
        &value.unwrap_or_else(|| "--".to_owned()),
        68.0,
        color,
    );
    if !unit.is_empty() {
        text(
            painter,
            pos2(rect.left() + 94.0, rect.top() + 74.0),
            Align2::LEFT_CENTER,
            unit,
            14.0,
            TEXT,
        );
    }
}

fn draw_small_pair(
    painter: &egui::Painter,
    origin: Pos2,
    left_label: &str,
    left_value: Option<String>,
    right_label: &str,
    right_value: Option<String>,
    accent: Color32,
) {
    let right_x = origin.x + 138.0;
    text(painter, origin, Align2::LEFT_TOP, left_label, 10.0, MUTED);
    text(
        painter,
        pos2(origin.x, origin.y + 25.0),
        Align2::LEFT_CENTER,
        &left_value.unwrap_or_else(|| "--".to_owned()),
        19.0,
        TEXT,
    );
    text(
        painter,
        pos2(right_x, origin.y),
        Align2::LEFT_TOP,
        right_label,
        10.0,
        MUTED,
    );
    text(
        painter,
        pos2(right_x, origin.y + 25.0),
        Align2::LEFT_CENTER,
        &right_value.unwrap_or_else(|| "--".to_owned()),
        19.0,
        accent,
    );
}

fn draw_bar(painter: &egui::Painter, rect: Rect, label: &str, value: f32, accent: Color32) {
    let clamped = value.clamp(0.0, 1.0);
    painter.rect_filled(rect, 2.0, Color32::from_rgb(58, 67, 80));
    painter.rect_filled(
        Rect::from_min_size(rect.left_top(), vec2(rect.width() * clamped, rect.height())),
        2.0,
        accent,
    );
    text(
        painter,
        rect.left_center() + vec2(8.0, 0.0),
        Align2::LEFT_CENTER,
        label,
        11.0,
        TEXT,
    );
    text(
        painter,
        rect.right_center() + vec2(-8.0, 0.0),
        Align2::RIGHT_CENTER,
        &format!("{:.0}%", clamped * 100.0),
        11.0,
        TEXT,
    );
}

fn draw_car_silhouette(painter: &egui::Painter, rect: Rect, accent: Color32) {
    let center = rect.center();
    let body_w = rect.width() * 0.28;
    let body = Rect::from_center_size(center, vec2(body_w, rect.height() * 0.72));
    let cockpit = Rect::from_center_size(
        center + vec2(0.0, -rect.height() * 0.06),
        vec2(body_w * 0.66, body.height() * 0.24),
    );
    let nose = [
        pos2(center.x, rect.top() + 14.0),
        pos2(body.left() + 8.0, body.top() + 74.0),
        pos2(body.right() - 8.0, body.top() + 74.0),
    ];

    painter.rect_filled(body, 10.0, Color32::from_rgb(184, 192, 199));
    painter.add(egui::Shape::convex_polygon(
        nose.to_vec(),
        Color32::from_rgb(218, 224, 229),
        Stroke::new(1.0, Color32::from_rgb(238, 242, 246)),
    ));
    painter.rect_filled(cockpit, 8.0, Color32::from_rgb(28, 33, 42));
    painter.rect_filled(
        Rect::from_center_size(
            pos2(center.x, rect.bottom() - 18.0),
            vec2(body_w * 1.5, 20.0),
        ),
        2.0,
        accent,
    );
    painter.rect_filled(
        Rect::from_center_size(pos2(center.x, rect.top() + 58.0), vec2(body_w * 2.2, 16.0)),
        2.0,
        accent,
    );

    for side in [-1.0, 1.0] {
        let x = center.x + side * body_w * 0.98;
        painter.rect_filled(
            Rect::from_center_size(
                pos2(x, center.y - body.height() * 0.20),
                vec2(body_w * 0.42, body.height() * 0.22),
            ),
            4.0,
            Color32::from_rgb(12, 14, 18),
        );
        painter.rect_filled(
            Rect::from_center_size(
                pos2(x, center.y + body.height() * 0.28),
                vec2(body_w * 0.48, body.height() * 0.28),
            ),
            4.0,
            Color32::from_rgb(12, 14, 18),
        );
    }
}

fn draw_tyre_life(
    painter: &egui::Painter,
    rect: Rect,
    input: Option<&InputSample>,
    damage: Option<&DamageSample>,
    accent: Color32,
) {
    panel_box(painter, rect, "TYRE PRESSURE AND LIFE", accent);
    let wear = damage
        .map(|value| value.tyre_wear.front_avg())
        .unwrap_or(0.0)
        .clamp(0.0, 100.0);
    let pressure = input
        .map(|value| value.tyre_pressures_psi.front_avg())
        .unwrap_or(0.0);
    draw_ring(
        painter,
        pos2(rect.left() + 58.0, rect.top() + 54.0),
        24.0,
        wear / 100.0,
        accent,
        &format!("{wear:.0}%"),
    );
    draw_ring(
        painter,
        pos2(rect.left() + 146.0, rect.top() + 54.0),
        24.0,
        (pressure / 30.0).clamp(0.0, 1.0),
        BLUE,
        &format!("{pressure:.1}"),
    );
    text(
        painter,
        pos2(rect.right() - 12.0, rect.top() + 52.0),
        Align2::RIGHT_CENTER,
        "front avg",
        12.0,
        MUTED,
    );
}

fn draw_ring(
    painter: &egui::Painter,
    center: Pos2,
    radius: f32,
    value: f32,
    color: Color32,
    label: &str,
) {
    painter.circle_stroke(
        center,
        radius,
        Stroke::new(4.0, Color32::from_rgb(74, 84, 96)),
    );
    let segments = 40;
    let active = (segments as f32 * value.clamp(0.0, 1.0)).round() as usize;
    for index in 0..active {
        let a0 =
            -std::f32::consts::FRAC_PI_2 + index as f32 / segments as f32 * std::f32::consts::TAU;
        let a1 = -std::f32::consts::FRAC_PI_2
            + (index + 1) as f32 / segments as f32 * std::f32::consts::TAU;
        painter.line_segment(
            [
                center + vec2(a0.cos() * radius, a0.sin() * radius),
                center + vec2(a1.cos() * radius, a1.sin() * radius),
            ],
            Stroke::new(4.0, color),
        );
    }
    text(painter, center, Align2::CENTER_CENTER, label, 16.0, TEXT);
}

fn metric_inline(painter: &egui::Painter, pos: Pos2, label: &str, value: String, color: Color32) {
    text(painter, pos, Align2::LEFT_CENTER, label, 10.0, MUTED);
    text(
        painter,
        pos + vec2(0.0, 22.0),
        Align2::LEFT_CENTER,
        &value,
        18.0,
        color,
    );
}

fn metric_line(painter: &egui::Painter, pos: Pos2, label: &str, value: String, color: Color32) {
    text(painter, pos, Align2::LEFT_CENTER, label, 10.0, MUTED);
    text(
        painter,
        pos + vec2(72.0, 0.0),
        Align2::LEFT_CENTER,
        &value,
        16.0,
        color,
    );
}

fn draw_input_stat(
    painter: &egui::Painter,
    pos: Pos2,
    label: &str,
    value: Option<f32>,
    color: Color32,
) {
    let text_value = match value {
        Some(value) if label == "STEER" => format!("{:+.0}%", value * 100.0),
        Some(value) => format!("{:.0}%", value * 100.0),
        None => "--".to_owned(),
    };
    text(painter, pos, Align2::LEFT_CENTER, label, 10.0, MUTED);
    painter.rect_filled(
        Rect::from_min_size(pos + vec2(0.0, 16.0), vec2(54.0, 3.0)),
        1.0,
        color,
    );
    text(
        painter,
        pos + vec2(0.0, 34.0),
        Align2::LEFT_CENTER,
        &text_value,
        18.0,
        TEXT,
    );
}

fn draw_trace_series<F>(
    painter: &egui::Painter,
    rect: Rect,
    trace: &[InputTracePoint],
    value: F,
    color: Color32,
) where
    F: Fn(&InputTracePoint) -> f32,
{
    let mut points = Vec::with_capacity(trace.len());
    let denominator = trace.len().saturating_sub(1).max(1) as f32;
    for (index, point) in trace.iter().enumerate() {
        let t = index as f32 / denominator;
        let y = rect.bottom() - rect.height() * value(point).clamp(0.0, 1.0);
        points.push(pos2(rect.left() + rect.width() * t, y));
    }
    for pair in points.windows(2) {
        painter.line_segment([pair[0], pair[1]], Stroke::new(2.0, color));
    }
}

fn telemetry_track_progress(state: &TelemetryUpdate) -> f32 {
    let Some(lap) = state.lap.as_ref() else {
        return 0.0;
    };
    let track_length = state
        .session
        .as_ref()
        .map(|session| f32::from(session.track_length_m))
        .filter(|length| *length > 0.0)
        .unwrap_or(5200.0);

    (lap.lap_distance_m.max(0.0) / track_length).rem_euclid(1.0)
}

fn draw_polyline_progress(painter: &egui::Painter, points: &[Pos2], progress: f32, stroke: Stroke) {
    let total = polyline_length(points);
    if total <= 0.0 {
        return;
    }

    let target = total * progress.clamp(0.0, 1.0);
    let mut walked = 0.0;
    for pair in points.windows(2) {
        let segment = pair[0].distance(pair[1]);
        if walked + segment <= target {
            painter.line_segment([pair[0], pair[1]], stroke);
        } else if target > walked && segment > 0.0 {
            let end = lerp_pos(pair[0], pair[1], (target - walked) / segment);
            painter.line_segment([pair[0], end], stroke);
            break;
        } else {
            break;
        }
        walked += segment;
    }
}

fn point_at_progress(points: &[Pos2], progress: f32) -> Pos2 {
    let total = polyline_length(points);
    if total <= 0.0 {
        return points.first().copied().unwrap_or(pos2(0.0, 0.0));
    }

    let target = total * progress.clamp(0.0, 1.0);
    let mut walked = 0.0;
    for pair in points.windows(2) {
        let segment = pair[0].distance(pair[1]);
        if walked + segment >= target && segment > 0.0 {
            return lerp_pos(pair[0], pair[1], (target - walked) / segment);
        }
        walked += segment;
    }

    points.last().copied().unwrap_or(pos2(0.0, 0.0))
}

fn polyline_length(points: &[Pos2]) -> f32 {
    points
        .windows(2)
        .map(|pair| pair[0].distance(pair[1]))
        .sum()
}

fn lerp_pos(start: Pos2, end: Pos2, t: f32) -> Pos2 {
    pos2(
        start.x + (end.x - start.x) * t,
        start.y + (end.y - start.y) * t,
    )
}

fn triangle(painter: &egui::Painter, center: Pos2, size: f32, color: Color32) {
    let points = vec![
        pos2(center.x, center.y - size),
        pos2(center.x - size * 0.8, center.y + size * 0.65),
        pos2(center.x + size * 0.8, center.y + size * 0.65),
    ];
    painter.add(egui::Shape::convex_polygon(points, color, Stroke::NONE));
}

fn text(painter: &egui::Painter, pos: Pos2, align: Align2, value: &str, size: f32, color: Color32) {
    painter.text(
        pos,
        align,
        value,
        FontId::new(size, FontFamily::Proportional),
        color,
    );
}

fn gear_label(gear: i8) -> String {
    match gear {
        -1 => "R".to_owned(),
        0 => "N".to_owned(),
        value => value.to_string(),
    }
}

fn format_ms(value: u32) -> String {
    if value == 0 {
        return "--".to_owned();
    }
    let minutes = value / 60_000;
    let seconds = (value % 60_000) / 1_000;
    let millis = value % 1_000;
    format!("{minutes}:{seconds:02}.{millis:03}")
}
