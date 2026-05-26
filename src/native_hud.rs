use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use eframe::egui::{
    self, Align2, Color32, FontFamily, FontId, Pos2, Rect, Stroke, StrokeKind, pos2, vec2,
};

use crate::config::BridgeConfig;
use crate::hud::{HudHandle, new_hud_handle};
use crate::telemetry::{DamageSample, InputSample, StatusSample, TelemetryUpdate};

const APP_BG: Color32 = Color32::from_rgb(4, 5, 7);
const SCREEN_BG: Color32 = Color32::from_rgb(3, 6, 8);
const LINE: Color32 = Color32::from_rgb(36, 46, 52);
const LINE_HOT: Color32 = Color32::from_rgb(190, 104, 14);
const TEXT: Color32 = Color32::from_rgb(238, 242, 244);
const MUTED: Color32 = Color32::from_rgb(142, 152, 160);
const ORANGE: Color32 = Color32::from_rgb(255, 149, 18);
const GREEN: Color32 = Color32::from_rgb(72, 241, 77);
const RED: Color32 = Color32::from_rgb(255, 55, 35);

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
            .with_inner_size([1180.0, 560.0])
            .with_min_inner_size([740.0, 360.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Sim MOZA Bridge",
        options,
        Box::new(move |cc| {
            configure_style(&cc.egui_ctx);
            Ok(Box::new(NativeHudApp { hud, runtime_error }))
        }),
    )
    .map_err(|error| format!("native HUD failed: {error}"))
}

struct NativeHudApp {
    hud: HudHandle,
    runtime_error: Arc<Mutex<Option<String>>>,
}

impl eframe::App for NativeHudApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        ui.ctx().request_repaint_after(Duration::from_millis(40));

        let state = self.hud.snapshot();
        let error = self.runtime_error.lock().ok().and_then(|slot| slot.clone());
        let rect = ui.max_rect();
        let painter = ui.painter_at(rect);

        draw_display(&painter, rect, &state, error.as_deref());
    }
}

fn configure_style(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = APP_BG;
    visuals.window_fill = APP_BG;
    visuals.extreme_bg_color = APP_BG;
    visuals.selection.bg_fill = ORANGE;
    ctx.set_visuals(visuals);
}

fn draw_display(painter: &egui::Painter, rect: Rect, state: &TelemetryUpdate, error: Option<&str>) {
    painter.rect_filled(rect, 0.0, APP_BG);

    let base = rect.width().min(rect.height());
    let margin = vec2(
        (rect.width() * 0.026).clamp(12.0, 38.0),
        (base * 0.050).clamp(14.0, 34.0),
    );
    let panel = rect.shrink2(margin);
    let scale = display_scale(panel);
    draw_panel_surface(painter, panel, scale);

    let content = panel.shrink2(vec2(panel.width() * 0.050, panel.height() * 0.055));
    draw_rev_lights(painter, panel, state.input.as_ref());
    draw_lcd_contents(painter, content, state, error);
}

fn draw_lcd_contents(
    painter: &egui::Painter,
    rect: Rect,
    state: &TelemetryUpdate,
    error: Option<&str>,
) {
    let input = state.input.as_ref();
    let status = state.status.as_ref();
    let damage = state.damage.as_ref();

    let top_h = rect.height() * 0.28;
    let mid_h = rect.height() * 0.36;

    let top = Rect::from_min_max(rect.left_top(), pos2(rect.right(), rect.top() + top_h));
    let mid = Rect::from_min_max(
        pos2(rect.left(), top.bottom()),
        pos2(rect.right(), top.bottom() + mid_h),
    );
    let bottom = Rect::from_min_max(pos2(rect.left(), mid.bottom()), rect.right_bottom());

    draw_lcd_texture(painter, rect);
    draw_top_status(painter, top, input, status);
    draw_rpm_arc(painter, mid, input, status);
    draw_speed_panel(painter, mid, state);
    draw_delta_panel(painter, mid, state);
    draw_energy_row(painter, bottom, status);
    draw_tyres_panel(painter, bottom, input, damage);

    if let Some(error) = error {
        draw_center_message(painter, rect, error, RED);
    } else if state.is_empty() {
        draw_status_strip(painter, rect, "WAITING FOR TELEMETRY", MUTED);
    }
}

fn draw_top_status(
    painter: &egui::Painter,
    rect: Rect,
    input: Option<&InputSample>,
    status: Option<&StatusSample>,
) {
    let s = display_scale(rect);
    let max_rpm = status.map(|status| status.max_rpm.max(1)).unwrap_or(15_000);
    let rpm = input.map(|input| input.rpm).unwrap_or(0).min(max_rpm);
    let rpm_ratio = rpm as f32 / max_rpm as f32;
    let band = Rect::from_min_max(
        pos2(
            rect.left() + rect.width() * 0.06,
            rect.top() + rect.height() * 0.34,
        ),
        pos2(
            rect.right() - rect.width() * 0.06,
            rect.top() + rect.height() * 0.48,
        ),
    );

    draw_rpm_bar(painter, band, rpm_ratio, s);

    for tick in 0..=15 {
        let x = band.left() + band.width() * tick as f32 / 15.0;
        let color = if tick >= 11 { ORANGE } else { TEXT };
        text(
            painter,
            pos2(x, band.bottom() + 21.0 * s),
            Align2::CENTER_CENTER,
            &tick.to_string(),
            17.0 * s,
            color,
        );
    }

    text(
        painter,
        pos2(rect.center().x, rect.bottom() - 10.0 * s),
        Align2::CENTER_CENTER,
        "RPM x1000",
        13.0 * s,
        MUTED,
    );
}

fn draw_rpm_arc(
    painter: &egui::Painter,
    rect: Rect,
    input: Option<&InputSample>,
    _status: Option<&StatusSample>,
) {
    let s = display_scale(rect);
    let center = rect.center();
    let motif_color = Color32::from_rgba_premultiplied(60, 28, 0, 95);
    draw_chevron_motif(painter, rect, motif_color, s);

    let gear = input
        .map(|input| gear_label(input.gear))
        .unwrap_or_else(|| "-".to_owned());
    glow_text(
        painter,
        pos2(center.x, center.y - 2.0 * s),
        Align2::CENTER_CENTER,
        &gear,
        132.0 * s,
        ORANGE,
        Color32::from_rgba_premultiplied(150, 70, 0, 100),
    );
    let rpm = input
        .map(|input| input.rpm.to_string())
        .unwrap_or_else(|| "----".to_owned());
    text(
        painter,
        pos2(center.x, rect.bottom() - 22.0 * s),
        Align2::CENTER_CENTER,
        &rpm,
        25.0 * s,
        TEXT,
    );
    text(
        painter,
        pos2(center.x, rect.bottom() - 2.0 * s),
        Align2::CENTER_CENTER,
        "RPM",
        13.0 * s,
        MUTED,
    );
}

fn draw_speed_panel(painter: &egui::Painter, rect: Rect, state: &TelemetryUpdate) {
    let s = display_scale(rect);
    let input = state.input.as_ref();
    let lap = state.lap.as_ref();
    let session = state.session.as_ref();
    let panel = Rect::from_min_size(
        pos2(rect.left() + 10.0 * s, rect.top() + rect.height() * 0.04),
        vec2(rect.width() * 0.32, rect.height() * 0.92),
    );
    angular_panel(painter, panel, 1.0, s);
    text(
        painter,
        pos2(panel.left() + 28.0 * s, panel.top() + 23.0 * s),
        Align2::LEFT_CENTER,
        "LAP",
        15.0 * s,
        TEXT,
    );
    let lap_value = match (lap, session) {
        (Some(lap), Some(session)) => {
            format!("{}/{}", lap.current_lap_num, session.total_laps.max(1))
        }
        (Some(lap), None) => format!("{}/--", lap.current_lap_num),
        _ => "--/--".to_owned(),
    };
    text_lap_value(
        painter,
        pos2(panel.left() + 28.0 * s, panel.top() + 62.0 * s),
        &lap_value,
        s,
    );

    let speed = input
        .map(|input| input.speed_kmh.to_string())
        .unwrap_or_else(|| "--".to_owned());
    glow_text(
        painter,
        pos2(panel.left() + 28.0 * s, panel.bottom() - 64.0 * s),
        Align2::LEFT_CENTER,
        &speed,
        74.0 * s,
        TEXT,
        Color32::from_rgba_premultiplied(60, 70, 76, 80),
    );
    text(
        painter,
        pos2(panel.left() + 31.0 * s, panel.bottom() - 19.0 * s),
        Align2::LEFT_CENTER,
        "KPH",
        17.0 * s,
        MUTED,
    );
}

fn draw_delta_panel(painter: &egui::Painter, rect: Rect, state: &TelemetryUpdate) {
    let s = display_scale(rect);
    let panel = Rect::from_min_size(
        pos2(
            rect.right() - rect.width() * 0.32 - 10.0 * s,
            rect.top() + rect.height() * 0.04,
        ),
        vec2(rect.width() * 0.32, rect.height() * 0.92),
    );
    angular_panel(painter, panel, -1.0, s);
    let lap = state.lap.as_ref();
    let current = lap
        .map(|lap| format_ms(lap.current_lap_time_ms))
        .unwrap_or_else(|| "--:--.---".to_owned());
    let best = lap
        .map(|lap| format_ms(lap.last_lap_time_ms))
        .unwrap_or_else(|| "--:--.---".to_owned());
    text(
        painter,
        pos2(panel.left() + 34.0 * s, panel.top() + 24.0 * s),
        Align2::LEFT_CENTER,
        "LAP TIME",
        15.0 * s,
        TEXT,
    );
    glow_text(
        painter,
        pos2(panel.left() + 34.0 * s, panel.top() + 68.0 * s),
        Align2::LEFT_CENTER,
        &current,
        34.0 * s,
        TEXT,
        Color32::from_rgba_premultiplied(60, 70, 76, 80),
    );
    text(
        painter,
        pos2(panel.left() + 34.0 * s, panel.center().y - 6.0 * s),
        Align2::LEFT_CENTER,
        "BEST",
        14.0 * s,
        TEXT,
    );
    text(
        painter,
        pos2(panel.left() + 112.0 * s, panel.center().y - 6.0 * s),
        Align2::LEFT_CENTER,
        &best,
        14.0 * s,
        MUTED,
    );
    text(
        painter,
        pos2(panel.left() + 34.0 * s, panel.center().y + 42.0 * s),
        Align2::LEFT_CENTER,
        "DELTA",
        15.0 * s,
        TEXT,
    );

    let delta = state
        .lap
        .as_ref()
        .and_then(|lap| lap.delta_to_car_in_front_ms)
        .map(format_delta_ms)
        .unwrap_or_else(|| "--.---".to_owned());
    glow_text(
        painter,
        pos2(panel.left() + 52.0 * s, panel.bottom() - 31.0 * s),
        Align2::LEFT_CENTER,
        &delta,
        35.0 * s,
        GREEN,
        Color32::from_rgba_premultiplied(25, 95, 30, 80),
    );
}

fn draw_energy_row(painter: &egui::Painter, rect: Rect, status: Option<&StatusSample>) {
    let s = display_scale(rect);
    let left_panel = Rect::from_min_size(
        pos2(rect.left() + 10.0 * s, rect.top() + 8.0 * s),
        vec2(rect.width() * 0.30, rect.height() - 18.0 * s),
    );
    let right_panel = Rect::from_min_size(
        pos2(
            rect.right() - rect.width() * 0.30 - 10.0 * s,
            rect.top() + 8.0 * s,
        ),
        vec2(rect.width() * 0.30, rect.height() - 18.0 * s),
    );
    angular_panel(painter, left_panel, 1.0, s);
    angular_panel(painter, right_panel, -1.0, s);

    let top = left_panel.top() + 28.0 * s;
    let left = left_panel.left() + 28.0 * s;
    let battery = status.map(|status| status.ers_percent()).unwrap_or(0.0);
    text(
        painter,
        pos2(left, top),
        Align2::LEFT_CENTER,
        "BATT",
        16.0 * s,
        TEXT,
    );
    text(
        painter,
        pos2(left, top + 48.0 * s),
        Align2::LEFT_CENTER,
        &format!("{battery:.0}%"),
        37.0 * s,
        TEXT,
    );
    draw_segment_bar(
        painter,
        Rect::from_min_size(
            pos2(left, left_panel.bottom() - 28.0 * s),
            vec2(left_panel.width() - 56.0 * s, 12.0 * s),
        ),
        battery / 100.0,
        14,
        GREEN,
    );

    let deployed = status
        .map(|status| status.ers_deployed_this_lap / 4_000_000.0)
        .unwrap_or(0.0)
        .clamp(0.0, 1.0);
    let right_left = right_panel.left() + 40.0 * s;
    let right_top = right_panel.top() + 28.0 * s;
    text(
        painter,
        pos2(right_left, right_top),
        Align2::LEFT_CENTER,
        "ERS",
        16.0 * s,
        TEXT,
    );
    text(
        painter,
        pos2(right_panel.right() - 32.0 * s, right_top),
        Align2::RIGHT_CENTER,
        "DEPLOY",
        14.0 * s,
        TEXT,
    );
    text(
        painter,
        pos2(right_panel.right() - 28.0 * s, right_top),
        Align2::LEFT_CENTER,
        &format!("{:.0}", deployed * 4.0),
        26.0 * s,
        ORANGE,
    );
    text(
        painter,
        pos2(right_left, right_top + 57.0 * s),
        Align2::LEFT_CENTER,
        &format!("{:.1}", deployed * 4.0),
        35.0 * s,
        TEXT,
    );
    text(
        painter,
        pos2(right_left + 82.0 * s, right_top + 61.0 * s),
        Align2::LEFT_CENTER,
        "MJ / LAP",
        13.0 * s,
        MUTED,
    );
    draw_segment_bar(
        painter,
        Rect::from_min_size(
            pos2(right_left, right_panel.bottom() - 28.0 * s),
            vec2(right_panel.width() - 74.0 * s, 12.0 * s),
        ),
        deployed,
        12,
        ORANGE,
    );
}

fn draw_tyres_panel(
    painter: &egui::Painter,
    rect: Rect,
    input: Option<&InputSample>,
    damage: Option<&DamageSample>,
) {
    let s = display_scale(rect);
    let panel = Rect::from_center_size(
        pos2(rect.center().x, rect.center().y + 2.0 * s),
        vec2(rect.width() * 0.38, rect.height() - 4.0 * s),
    );
    bottom_center_panel(painter, panel, s);
    let center = pos2(panel.center().x, panel.top() + panel.height() * 0.54);
    draw_mini_car(
        painter,
        Rect::from_center_size(center + vec2(0.0, 8.0 * s), vec2(74.0 * s, 92.0 * s)),
    );

    let temps = input.map(|input| input.tyre_surface_temps_c);
    let wear = damage.map(|damage| damage.tyre_wear);
    tyre_metric(
        painter,
        pos2(panel.left() + 54.0 * s, panel.top() + 42.0 * s),
        "FL",
        temps.map(|temps| temps.fl),
        wear.map(|wear| wear.fl),
        Align2::LEFT_CENTER,
        s,
    );
    tyre_metric(
        painter,
        pos2(panel.right() - 54.0 * s, panel.top() + 42.0 * s),
        "FR",
        temps.map(|temps| temps.fr),
        wear.map(|wear| wear.fr),
        Align2::RIGHT_CENTER,
        s,
    );
    tyre_metric(
        painter,
        pos2(panel.left() + 54.0 * s, panel.bottom() - 40.0 * s),
        "RL",
        temps.map(|temps| temps.rl),
        wear.map(|wear| wear.rl),
        Align2::LEFT_CENTER,
        s,
    );
    tyre_metric(
        painter,
        pos2(panel.right() - 54.0 * s, panel.bottom() - 40.0 * s),
        "RR",
        temps.map(|temps| temps.rr),
        wear.map(|wear| wear.rr),
        Align2::RIGHT_CENTER,
        s,
    );
}

fn draw_rev_lights(painter: &egui::Painter, screen: Rect, input: Option<&InputSample>) {
    let s = display_scale(screen);
    let count = 16;
    let radius = (screen.width() * 0.0085).clamp(5.5, 10.0);
    let gap = radius * 2.4;
    let total_w = gap * (count - 1) as f32;
    let start_x = screen.center().x - total_w / 2.0;
    let y = screen.top() + 33.0 * s;
    let active = input
        .map(|input| {
            ((f32::from(input.rev_lights_percent) / 100.0) * count as f32).round() as usize
        })
        .unwrap_or(0);

    for index in 0..count {
        let color = if index < 5 {
            GREEN
        } else if index < 11 {
            ORANGE
        } else if index < 13 {
            RED
        } else {
            Color32::from_rgb(232, 28, 190)
        };
        let fill = if index < active {
            color
        } else {
            Color32::from_rgb(28, 32, 34)
        };
        let center = pos2(start_x + gap * index as f32, y);
        painter.circle_filled(center, radius + 3.0 * s, Color32::from_rgb(4, 5, 6));
        painter.circle_filled(center, radius, fill);
        if index < active {
            painter.circle_stroke(center, radius + 2.5 * s, Stroke::new(1.4 * s, color));
        }
    }
}

fn draw_center_message(painter: &egui::Painter, rect: Rect, message: &str, color: Color32) {
    let s = display_scale(rect);
    let banner = Rect::from_center_size(rect.center(), vec2(rect.width() * 0.52, 46.0 * s));
    painter.rect_filled(
        banner,
        5.0 * s,
        Color32::from_rgba_premultiplied(0, 0, 0, 220),
    );
    painter.rect_stroke(
        banner,
        5.0 * s,
        Stroke::new(1.0 * s, LINE_HOT),
        StrokeKind::Inside,
    );
    text(
        painter,
        banner.center(),
        Align2::CENTER_CENTER,
        message,
        17.0 * s,
        color,
    );
}

fn draw_status_strip(painter: &egui::Painter, rect: Rect, message: &str, color: Color32) {
    let s = display_scale(rect);
    let y = rect.bottom() - 16.0 * s;
    painter.line_segment(
        [
            pos2(rect.left() + rect.width() * 0.32, y),
            pos2(rect.right() - rect.width() * 0.32, y),
        ],
        Stroke::new(1.0 * s, LINE),
    );
    text(
        painter,
        pos2(rect.center().x, y - 12.0 * s),
        Align2::CENTER_CENTER,
        message,
        13.0 * s,
        color,
    );
}

fn draw_panel_surface(painter: &egui::Painter, rect: Rect, scale: f32) {
    let cut = 30.0 * scale;
    let base_shape = vec![
        pos2(rect.left() + cut, rect.top()),
        pos2(rect.right() - cut, rect.top()),
        rect.right_top() + vec2(cut * 0.72, cut * 0.72),
        rect.right_bottom() - vec2(cut * 0.72, cut * 0.72),
        pos2(rect.right() - cut, rect.bottom()),
        pos2(rect.left() + cut, rect.bottom()),
        rect.left_bottom() + vec2(cut * 0.72, -cut * 0.72),
        rect.left_top() + vec2(cut * 0.72, cut * 0.72),
    ];
    painter.add(egui::Shape::convex_polygon(
        base_shape,
        SCREEN_BG,
        Stroke::NONE,
    ));

    for index in 0..18 {
        let offset = index as f32 * 28.0 * scale;
        let color = Color32::from_rgba_premultiplied(22, 28, 30, 28);
        painter.line_segment(
            [
                pos2(rect.left() + offset, rect.top()),
                pos2(rect.left() + offset + rect.height() * 0.55, rect.bottom()),
            ],
            Stroke::new(0.5 * scale, color),
        );
    }

    draw_outer_silhouette(painter, rect, scale);
}

fn draw_rpm_bar(painter: &egui::Painter, rect: Rect, value: f32, scale: f32) {
    let segments = 72;
    let active = (value.clamp(0.0, 1.0) * segments as f32).round() as usize;
    let gap = 1.0 * scale;
    let segment_w = (rect.width() - gap * (segments - 1) as f32) / segments as f32;

    for index in 0..segments {
        let x = rect.left() + index as f32 * (segment_w + gap);
        let t = index as f32 / segments as f32;
        let color = if index < active {
            if t > 0.88 {
                RED
            } else if t > 0.70 {
                ORANGE
            } else {
                Color32::from_rgb(205, 210, 210)
            }
        } else if t > 0.88 {
            Color32::from_rgb(65, 18, 15)
        } else if t > 0.70 {
            Color32::from_rgb(67, 37, 12)
        } else {
            Color32::from_rgb(56, 61, 62)
        };
        let skew = rect.height() * 0.35;
        let segment = vec![
            pos2(x + skew, rect.top()),
            pos2(x + segment_w + skew, rect.top()),
            pos2(x + segment_w, rect.bottom()),
            pos2(x, rect.bottom()),
        ];
        painter.add(egui::Shape::convex_polygon(segment, color, Stroke::NONE));
    }

    painter.line_segment(
        [rect.left_bottom(), rect.right_bottom()],
        Stroke::new(0.9 * scale, Color32::from_rgb(210, 216, 216)),
    );
}

fn draw_outer_silhouette(painter: &egui::Painter, rect: Rect, scale: f32) {
    let cut = 30.0 * scale;
    let bottom_tab = 72.0 * scale;
    let points = vec![
        pos2(rect.left() + cut, rect.top()),
        pos2(rect.right() - cut, rect.top()),
        rect.right_top() + vec2(cut * 0.72, cut * 0.72),
        rect.right_bottom() - vec2(cut * 0.72, cut * 0.72),
        pos2(rect.center().x + bottom_tab, rect.bottom()),
        pos2(
            rect.center().x + bottom_tab * 0.62,
            rect.bottom() + 18.0 * scale,
        ),
        pos2(
            rect.center().x - bottom_tab * 0.62,
            rect.bottom() + 18.0 * scale,
        ),
        pos2(rect.center().x - bottom_tab, rect.bottom()),
        rect.left_bottom() + vec2(cut * 0.72, -cut * 0.72),
        rect.left_top() + vec2(cut * 0.72, cut * 0.72),
        pos2(rect.left() + cut, rect.top()),
    ];
    painter.add(egui::Shape::closed_line(
        points,
        Stroke::new(1.2 * scale, Color32::from_rgb(76, 80, 79)),
    ));
}

fn draw_lcd_texture(painter: &egui::Painter, rect: Rect) {
    let s = display_scale(rect);
    let grid = Color32::from_rgba_premultiplied(20, 30, 34, 32);
    for index in 1..5 {
        let x = rect.left() + rect.width() * index as f32 / 5.0;
        painter.line_segment(
            [
                pos2(x, rect.top() + 12.0 * s),
                pos2(x, rect.bottom() - 12.0 * s),
            ],
            Stroke::new(0.45 * s, grid),
        );
    }
    for index in 1..4 {
        let y = rect.top() + rect.height() * index as f32 / 4.0;
        painter.line_segment(
            [
                pos2(rect.left() + 12.0 * s, y),
                pos2(rect.right() - 12.0 * s, y),
            ],
            Stroke::new(0.45 * s, grid),
        );
    }
    painter.rect_filled(
        Rect::from_min_size(rect.left_top(), vec2(rect.width(), rect.height() * 0.18)),
        0.0,
        Color32::from_rgba_premultiplied(24, 30, 30, 30),
    );
}

fn angular_panel(painter: &egui::Painter, rect: Rect, direction: f32, scale: f32) {
    let notch = rect.width() * 0.28;
    let mid = rect.center().y;
    let points = if direction > 0.0 {
        vec![
            rect.left_top(),
            pos2(rect.right() - notch, rect.top()),
            pos2(rect.right(), mid),
            pos2(rect.right() - notch, rect.bottom()),
            rect.left_bottom(),
        ]
    } else {
        vec![
            pos2(rect.left() + notch, rect.top()),
            rect.right_top(),
            rect.right_bottom(),
            pos2(rect.left() + notch, rect.bottom()),
            pos2(rect.left(), mid),
        ]
    };
    painter.add(egui::Shape::convex_polygon(
        points,
        Color32::from_rgba_premultiplied(6, 9, 10, 110),
        Stroke::new(
            0.8 * scale,
            Color32::from_rgba_premultiplied(170, 78, 0, 160),
        ),
    ));
}

fn bottom_center_panel(painter: &egui::Painter, rect: Rect, scale: f32) {
    let notch = rect.width() * 0.08;
    let points = vec![
        pos2(rect.left() + notch, rect.top()),
        pos2(rect.right() - notch, rect.top()),
        rect.right_bottom(),
        rect.left_bottom(),
    ];
    painter.add(egui::Shape::convex_polygon(
        points,
        Color32::from_rgba_premultiplied(8, 12, 14, 125),
        Stroke::new(
            0.8 * scale,
            Color32::from_rgba_premultiplied(170, 78, 0, 160),
        ),
    ));
}

fn draw_chevron_motif(painter: &egui::Painter, rect: Rect, color: Color32, scale: f32) {
    for index in 0..5 {
        let inset = index as f32 * 18.0 * scale;
        let left = rect.left() + rect.width() * 0.33 - inset;
        let right = rect.right() - rect.width() * 0.33 + inset;
        let top = rect.top() + 10.0 * scale + inset * 0.18;
        let bottom = rect.bottom() - 8.0 * scale - inset * 0.18;
        painter.line_segment(
            [
                pos2(left, top),
                pos2(rect.center().x - 74.0 * scale, rect.center().y),
            ],
            Stroke::new(0.7 * scale, color),
        );
        painter.line_segment(
            [
                pos2(left, bottom),
                pos2(rect.center().x - 74.0 * scale, rect.center().y),
            ],
            Stroke::new(0.7 * scale, color),
        );
        painter.line_segment(
            [
                pos2(right, top),
                pos2(rect.center().x + 74.0 * scale, rect.center().y),
            ],
            Stroke::new(0.7 * scale, color),
        );
        painter.line_segment(
            [
                pos2(right, bottom),
                pos2(rect.center().x + 74.0 * scale, rect.center().y),
            ],
            Stroke::new(0.7 * scale, color),
        );
    }
}

fn text_lap_value(painter: &egui::Painter, pos: Pos2, value: &str, scale: f32) {
    if let Some((current, total)) = value.split_once('/') {
        text(
            painter,
            pos,
            Align2::LEFT_CENTER,
            current,
            34.0 * scale,
            TEXT,
        );
        text(
            painter,
            pos + vec2(50.0 * scale, 0.0),
            Align2::LEFT_CENTER,
            "/",
            30.0 * scale,
            ORANGE,
        );
        text(
            painter,
            pos + vec2(72.0 * scale, 0.0),
            Align2::LEFT_CENTER,
            total,
            26.0 * scale,
            TEXT,
        );
    } else {
        text(painter, pos, Align2::LEFT_CENTER, value, 30.0 * scale, TEXT);
    }
}

fn glow_text(
    painter: &egui::Painter,
    pos: Pos2,
    align: Align2,
    value: &str,
    size: f32,
    color: Color32,
    glow: Color32,
) {
    for offset in [vec2(-1.0, 0.0), vec2(1.0, 0.0), vec2(0.0, 1.0)] {
        text(painter, pos + offset * 2.0, align, value, size + 2.0, glow);
    }
    text(painter, pos, align, value, size, color);
}

fn draw_segment_bar(
    painter: &egui::Painter,
    rect: Rect,
    value: f32,
    segments: usize,
    color: Color32,
) {
    let gap = (rect.height() * 0.22).clamp(2.0, 4.0);
    let segment_w = (rect.width() - gap * (segments.saturating_sub(1)) as f32) / segments as f32;
    let active = (value.clamp(0.0, 1.0) * segments as f32).round() as usize;

    for index in 0..segments {
        let x = rect.left() + (segment_w + gap) * index as f32;
        let segment = Rect::from_min_size(pos2(x, rect.top()), vec2(segment_w, rect.height()));
        let fill = if index < active {
            color
        } else {
            Color32::from_rgb(49, 55, 58)
        };
        painter.rect_filled(segment, 1.0, fill);
    }
}

fn draw_mini_car(painter: &egui::Painter, rect: Rect) {
    let center = rect.center();
    let body = Rect::from_center_size(center, vec2(rect.width() * 0.34, rect.height() * 0.74));
    let nose = [
        pos2(center.x, rect.top()),
        pos2(
            body.left() + rect.width() * 0.03,
            body.top() + rect.height() * 0.30,
        ),
        pos2(
            body.right() - rect.width() * 0.03,
            body.top() + rect.height() * 0.30,
        ),
    ];
    painter.add(egui::Shape::convex_polygon(
        nose.to_vec(),
        Color32::from_rgb(115, 124, 126),
        Stroke::new((rect.width() * 0.018).clamp(0.8, 1.4), TEXT),
    ));
    painter.rect_stroke(
        body,
        rect.width() * 0.05,
        Stroke::new((rect.width() * 0.018).clamp(0.8, 1.4), TEXT),
        StrokeKind::Inside,
    );
    painter.rect_stroke(
        Rect::from_center_size(
            pos2(center.x, rect.bottom() - rect.height() * 0.10),
            vec2(rect.width() * 0.92, rect.height() * 0.09),
        ),
        2.0,
        Stroke::new((rect.width() * 0.018).clamp(0.8, 1.4), TEXT),
        StrokeKind::Inside,
    );

    for side in [-1.0, 1.0] {
        let x = center.x + side * rect.width() * 0.34;
        painter.rect_filled(
            Rect::from_center_size(
                pos2(x, center.y - rect.height() * 0.20),
                vec2(rect.width() * 0.16, rect.height() * 0.29),
            ),
            2.0,
            Color32::from_rgb(170, 180, 184),
        );
        painter.rect_filled(
            Rect::from_center_size(
                pos2(x, center.y + rect.height() * 0.28),
                vec2(rect.width() * 0.16, rect.height() * 0.29),
            ),
            2.0,
            Color32::from_rgb(170, 180, 184),
        );
    }
}

fn tyre_metric(
    painter: &egui::Painter,
    pos: Pos2,
    label: &str,
    temp: Option<u8>,
    wear: Option<f32>,
    align: Align2,
    scale: f32,
) {
    let temp_text = temp
        .map(|temp| format!("{temp}C"))
        .unwrap_or_else(|| "--C".to_owned());
    text(
        painter,
        pos + vec2(0.0, -18.0 * scale),
        align,
        label,
        12.0 * scale,
        TEXT,
    );
    text(painter, pos, align, &temp_text, 18.0 * scale, GREEN);
    text(
        painter,
        pos + vec2(0.0, 18.0 * scale),
        align,
        &format!("{:.0}%", wear.unwrap_or(0.0).clamp(0.0, 100.0)),
        12.0 * scale,
        MUTED,
    );

    let bar_dir = if matches!(align, Align2::RIGHT_CENTER) {
        -1.0
    } else {
        1.0
    };
    let bar_origin = pos + vec2(11.0 * scale * bar_dir, -9.0 * scale);
    let bar = if bar_dir > 0.0 {
        Rect::from_min_size(bar_origin, vec2(38.0 * scale, 18.0 * scale))
    } else {
        Rect::from_min_size(
            bar_origin - vec2(38.0 * scale, 0.0),
            vec2(38.0 * scale, 18.0 * scale),
        )
    };
    painter.rect_filled(bar, 1.0, Color32::from_rgb(38, 48, 43));
    let wear_value = wear.unwrap_or(0.0).clamp(0.0, 100.0) / 100.0;
    painter.rect_filled(
        Rect::from_min_size(
            bar.left_top(),
            vec2(bar.width() * (1.0 - wear_value), bar.height()),
        ),
        1.0,
        Color32::from_rgb(140, 224, 40),
    );
}

fn display_scale(rect: Rect) -> f32 {
    (rect.width() / 1040.0).clamp(0.62, 1.28)
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
        return "--:--.---".to_owned();
    }
    let minutes = value / 60_000;
    let seconds = (value % 60_000) / 1_000;
    let millis = value % 1_000;
    format!("{minutes}:{seconds:02}.{millis:03}")
}

fn format_delta_ms(value: u32) -> String {
    let seconds = value as f32 / 1000.0;
    format!("+{seconds:.3}")
}
