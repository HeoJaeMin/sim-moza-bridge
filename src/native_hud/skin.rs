use eframe::egui::{
    self, Align2, Color32, FontFamily, FontId, Pos2, Rect, Stroke, StrokeKind, pos2, vec2,
};

use crate::telemetry::{DamageSample, InputSample, LapSample, StatusSample, TelemetryUpdate};

pub const APP_BG: Color32 = Color32::from_rgb(2, 3, 4);
pub const ACCENT: Color32 = Color32::from_rgb(255, 142, 14);

const SHELL: Color32 = Color32::from_rgb(3, 7, 8);
const PANEL: Color32 = Color32::from_rgba_premultiplied(5, 8, 9, 185);
const LINE_HOT: Color32 = Color32::from_rgba_premultiplied(194, 82, 0, 185);
const TEXT: Color32 = Color32::from_rgb(235, 238, 238);
const TEXT_DIM: Color32 = Color32::from_rgb(145, 151, 153);
const GREEN: Color32 = Color32::from_rgb(23, 224, 48);
const AMBER: Color32 = Color32::from_rgb(255, 147, 19);
const RED: Color32 = Color32::from_rgb(245, 28, 22);
const MAGENTA: Color32 = Color32::from_rgb(230, 22, 178);

#[derive(Default)]
pub struct HudRenderer {
    theme: HudTheme,
}

impl HudRenderer {
    pub fn paint(
        &self,
        painter: &egui::Painter,
        rect: Rect,
        state: &TelemetryUpdate,
        error: Option<&str>,
    ) {
        let frame = TelemetryFrame::from_update(state, error);
        let layout = HudLayout::new(rect);

        painter.rect_filled(rect, 0.0, APP_BG);
        DisplaySkin::new(&self.theme).paint(painter, &layout);
        DynamicDisplay::new(&self.theme).paint(painter, &layout, &frame);

        if let Some(message) = frame.error.as_deref() {
            paint_center_message(painter, layout.content, message, RED, layout.scale);
        } else if frame.waiting {
            paint_waiting_strip(painter, layout.content, layout.scale);
        }
    }
}

#[derive(Default)]
struct HudTheme;

struct HudLayout {
    shell: Rect,
    content: Rect,
    rpm: Rect,
    left_main: Rect,
    right_main: Rect,
    center_main: Rect,
    battery: Rect,
    tyres: Rect,
    ers: Rect,
    scale: f32,
}

impl HudLayout {
    fn new(rect: Rect) -> Self {
        let margin = vec2(
            (rect.width() * 0.030).clamp(16.0, 46.0),
            (rect.height() * 0.070).clamp(18.0, 42.0),
        );
        let shell = fit_aspect(rect.shrink2(margin), 2.18);
        let scale = (shell.width() / 1620.0).clamp(0.56, 1.15);
        let content = shell.shrink2(vec2(shell.width() * 0.050, shell.height() * 0.060));

        let rpm = Rect::from_min_max(
            pos2(content.left(), content.top() + content.height() * 0.02),
            pos2(content.right(), content.top() + content.height() * 0.24),
        );
        let mid = Rect::from_min_max(
            pos2(content.left(), rpm.bottom() + content.height() * 0.03),
            pos2(content.right(), content.top() + content.height() * 0.66),
        );
        let bottom = Rect::from_min_max(
            pos2(content.left(), mid.bottom() + content.height() * 0.02),
            content.right_bottom(),
        );

        let left_main = Rect::from_min_size(
            pos2(mid.left(), mid.top()),
            vec2(mid.width() * 0.315, mid.height()),
        );
        let right_main = Rect::from_min_size(
            pos2(mid.right() - mid.width() * 0.315, mid.top()),
            vec2(mid.width() * 0.315, mid.height()),
        );
        let center_main = Rect::from_min_max(
            pos2(left_main.right(), mid.top()),
            pos2(right_main.left(), mid.bottom()),
        );

        let battery = Rect::from_min_size(
            bottom.left_top(),
            vec2(bottom.width() * 0.300, bottom.height()),
        );
        let ers = Rect::from_min_size(
            pos2(bottom.right() - bottom.width() * 0.300, bottom.top()),
            vec2(bottom.width() * 0.300, bottom.height()),
        );
        let tyres = Rect::from_min_max(
            pos2(battery.right(), bottom.top()),
            pos2(ers.left(), bottom.bottom()),
        );

        Self {
            shell,
            content,
            rpm,
            left_main,
            right_main,
            center_main,
            battery,
            tyres,
            ers,
            scale,
        }
    }
}

struct TelemetryFrame {
    waiting: bool,
    error: Option<String>,
    speed: String,
    gear: String,
    rpm: String,
    rpm_ratio: f32,
    rev_ratio: f32,
    lap_current: String,
    lap_total: String,
    current_lap: String,
    best_lap: String,
    delta: String,
    battery_pct: f32,
    ers_deploy: f32,
    ers_harvest_pct: f32,
    tyres: [TyreDisplay; 4],
}

impl TelemetryFrame {
    fn from_update(state: &TelemetryUpdate, error: Option<&str>) -> Self {
        let sample = Self::sample(state.is_empty() && error.is_none(), error);
        let input = state.input.as_ref();
        let lap = state.lap.as_ref();
        let status = state.status.as_ref();
        let damage = state.damage.as_ref();
        let waiting = state.is_empty() && error.is_none();

        let max_rpm = status.map(|status| status.max_rpm.max(1)).unwrap_or(15_000);
        let rpm = input
            .map(|input| input.rpm)
            .unwrap_or((sample.rpm_ratio * max_rpm as f32) as u16)
            .min(max_rpm);
        let lap_total = state
            .session
            .as_ref()
            .map(|session| session.total_laps.max(1).to_string())
            .unwrap_or_else(|| sample.lap_total.clone());
        let battery_pct = status
            .map(StatusSample::ers_percent)
            .unwrap_or(sample.battery_pct);
        let ers_deploy = status
            .map(|status| status.ers_deployed_this_lap / 4_000_000.0)
            .unwrap_or(sample.ers_deploy)
            .clamp(0.0, 1.0);

        Self {
            waiting,
            error: error.map(ToOwned::to_owned),
            speed: input
                .map(|input| input.speed_kmh.to_string())
                .unwrap_or(sample.speed),
            gear: input
                .map(|input| gear_label(input.gear))
                .unwrap_or(sample.gear),
            rpm: rpm.to_string(),
            rpm_ratio: rpm as f32 / max_rpm as f32,
            rev_ratio: input
                .map(|input| f32::from(input.rev_lights_percent) / 100.0)
                .unwrap_or(sample.rev_ratio),
            lap_current: lap
                .map(|lap| lap.current_lap_num.to_string())
                .unwrap_or(sample.lap_current),
            lap_total,
            current_lap: lap_time(lap, |lap| lap.current_lap_time_ms, &sample.current_lap),
            best_lap: lap_time(lap, |lap| lap.last_lap_time_ms, &sample.best_lap),
            delta: lap
                .and_then(|lap| lap.delta_to_car_in_front_ms)
                .map(format_delta_ms)
                .unwrap_or(sample.delta),
            battery_pct,
            ers_deploy,
            ers_harvest_pct: status
                .map(|_| (1.0 - ers_deploy).clamp(0.0, 1.0) * 100.0)
                .unwrap_or(sample.ers_harvest_pct),
            tyres: tyre_displays(input, damage, sample.tyres),
        }
    }

    fn sample(waiting: bool, error: Option<&str>) -> Self {
        Self {
            waiting,
            error: error.map(ToOwned::to_owned),
            speed: "278".to_owned(),
            gear: "7".to_owned(),
            rpm: "11280".to_owned(),
            rpm_ratio: 0.752,
            rev_ratio: 0.88,
            lap_current: "18".to_owned(),
            lap_total: "52".to_owned(),
            current_lap: "1:24.705".to_owned(),
            best_lap: "1:23.456".to_owned(),
            delta: "-0.256".to_owned(),
            battery_pct: 56.0,
            ers_deploy: 0.60,
            ers_harvest_pct: 62.0,
            tyres: [
                TyreDisplay {
                    label: "RL",
                    temp: Some(93),
                    wear_pct: Some(88.0),
                },
                TyreDisplay {
                    label: "RR",
                    temp: Some(95),
                    wear_pct: Some(90.0),
                },
                TyreDisplay {
                    label: "FL",
                    temp: Some(98),
                    wear_pct: Some(94.0),
                },
                TyreDisplay {
                    label: "FR",
                    temp: Some(97),
                    wear_pct: Some(91.0),
                },
            ],
        }
    }
}

#[derive(Clone, Copy)]
struct TyreDisplay {
    label: &'static str,
    temp: Option<u8>,
    wear_pct: Option<f32>,
}

struct DisplaySkin<'a> {
    theme: &'a HudTheme,
}

impl<'a> DisplaySkin<'a> {
    fn new(theme: &'a HudTheme) -> Self {
        Self { theme }
    }

    fn paint(&self, painter: &egui::Painter, layout: &HudLayout) {
        let _theme = self.theme;
        paint_shell(painter, layout.shell, layout.scale);
        paint_texture(painter, layout.content, layout.scale);
        paint_section_backplates(painter, layout);
    }
}

struct DynamicDisplay<'a> {
    theme: &'a HudTheme,
}

impl<'a> DynamicDisplay<'a> {
    fn new(theme: &'a HudTheme) -> Self {
        Self { theme }
    }

    fn paint(&self, painter: &egui::Painter, layout: &HudLayout, frame: &TelemetryFrame) {
        let _theme = self.theme;
        paint_rev_lights(painter, layout.rpm, frame.rev_ratio, layout.scale);
        paint_rpm_scale(painter, layout.rpm, frame.rpm_ratio, layout.scale);
        paint_speed_panel(painter, layout.left_main, frame, layout.scale);
        paint_gear_panel(painter, layout.center_main, frame, layout.scale);
        paint_timing_panel(painter, layout.right_main, frame, layout.scale);
        paint_battery_panel(painter, layout.battery, frame, layout.scale);
        paint_tyres_panel(painter, layout.tyres, frame, layout.scale);
        paint_ers_panel(painter, layout.ers, frame, layout.scale);
    }
}

fn paint_shell(painter: &egui::Painter, rect: Rect, scale: f32) {
    let shape = shell_points(rect, scale);
    painter.add(egui::Shape::convex_polygon(
        shape.clone(),
        SHELL,
        Stroke::NONE,
    ));
    painter.add(egui::Shape::closed_line(
        shape,
        Stroke::new(1.4 * scale, Color32::from_rgb(82, 86, 86)),
    ));
    painter.add(egui::Shape::closed_line(
        shell_points(rect.shrink(8.0 * scale), scale),
        Stroke::new(0.5 * scale, Color32::from_rgb(24, 29, 30)),
    ));

    painter.line_segment(
        [
            pos2(
                rect.left() + rect.width() * 0.09,
                rect.top() + rect.height() * 0.085,
            ),
            pos2(
                rect.right() - rect.width() * 0.09,
                rect.top() + rect.height() * 0.085,
            ),
        ],
        Stroke::new(
            0.7 * scale,
            Color32::from_rgba_premultiplied(190, 100, 15, 58),
        ),
    );
}

fn paint_texture(painter: &egui::Painter, rect: Rect, scale: f32) {
    let diagonal = Color32::from_rgba_premultiplied(42, 52, 54, 18);
    for index in -5..26 {
        let x = rect.left() + index as f32 * 58.0 * scale;
        painter.line_segment(
            [
                pos2(x, rect.top() + rect.height() * 0.02),
                pos2(x + rect.height() * 0.55, rect.bottom()),
            ],
            Stroke::new(0.45 * scale, diagonal),
        );
    }
}

fn paint_section_backplates(painter: &egui::Painter, layout: &HudLayout) {
    paint_main_panel(
        painter,
        layout.left_main,
        PanelDirection::Left,
        layout.scale,
    );
    paint_main_panel(
        painter,
        layout.right_main,
        PanelDirection::Right,
        layout.scale,
    );
    paint_center_wake(painter, layout.center_main, layout.scale);
    paint_bottom_panel(painter, layout.battery, PanelDirection::Left, layout.scale);
    paint_bottom_panel(painter, layout.tyres, PanelDirection::Center, layout.scale);
    paint_bottom_panel(painter, layout.ers, PanelDirection::Right, layout.scale);
}

fn paint_rev_lights(painter: &egui::Painter, rect: Rect, rev_ratio: f32, scale: f32) {
    let count = 16;
    let radius = (9.0 * scale).clamp(5.0, 10.5);
    let gap = 38.0 * scale;
    let total = gap * (count - 1) as f32;
    let start = rect.center().x - total / 2.0;
    let active = (rev_ratio.clamp(0.0, 1.0) * count as f32).round() as usize;

    for index in 0..count {
        let color = rev_light_color(index);
        let center = pos2(
            start + gap * index as f32,
            rect.top() + rect.height() * 0.15,
        );
        painter.circle_filled(
            center,
            radius * 1.85,
            Color32::from_rgba_premultiplied(color.r(), color.g(), color.b(), 26),
        );
        painter.circle_filled(center, radius * 1.20, Color32::from_rgb(8, 10, 10));
        let lit = index < active;
        let fill = if lit {
            color
        } else {
            Color32::from_rgb(30, 36, 36)
        };
        painter.circle_filled(center, radius, fill);
        if lit {
            painter.circle_filled(
                center,
                radius * 0.42,
                Color32::from_rgba_premultiplied(255, 255, 255, 95),
            );
        }
    }
}

fn paint_rpm_scale(painter: &egui::Painter, rect: Rect, rpm_ratio: f32, scale: f32) {
    let bar = Rect::from_min_max(
        pos2(
            rect.left() + rect.width() * 0.055,
            rect.top() + rect.height() * 0.40,
        ),
        pos2(
            rect.right() - rect.width() * 0.055,
            rect.top() + rect.height() * 0.55,
        ),
    );
    let segments = 96;
    let active = (rpm_ratio.clamp(0.0, 1.0) * segments as f32).round() as usize;
    let gap = 1.0 * scale;
    let segment_w = (bar.width() - gap * (segments - 1) as f32) / segments as f32;

    for index in 0..segments {
        let t = index as f32 / (segments - 1) as f32;
        let x = bar.left() + index as f32 * (segment_w + gap);
        let top_lift = (t - 0.5).abs() * 14.0 * scale;
        let segment = Rect::from_min_size(
            pos2(x, bar.top() - top_lift),
            vec2(segment_w, bar.height() + top_lift),
        );
        let color = rpm_segment_color(t, index < active);
        let skew = 6.0 * scale;
        painter.add(egui::Shape::convex_polygon(
            vec![
                pos2(segment.left() + skew, segment.top()),
                pos2(segment.right() + skew, segment.top()),
                segment.right_bottom(),
                segment.left_bottom(),
            ],
            color,
            Stroke::NONE,
        ));
    }

    for tick in 0..=15 {
        let x = bar.left() + bar.width() * tick as f32 / 15.0;
        let number_color = if tick >= 11 { AMBER } else { TEXT };
        painter.line_segment(
            [pos2(x, bar.bottom()), pos2(x, bar.bottom() + 15.0 * scale)],
            Stroke::new(0.8 * scale, Color32::from_rgb(155, 160, 160)),
        );
        draw_number(
            painter,
            pos2(x, bar.bottom() + 30.0 * scale),
            Align2::CENTER_CENTER,
            &tick.to_string(),
            18.0 * scale,
            number_color,
        );
    }

    draw_label(
        painter,
        pos2(rect.center().x, rect.bottom() - 4.0 * scale),
        Align2::CENTER_CENTER,
        "RPM x1000",
        13.0 * scale,
        TEXT_DIM,
    );
}

fn paint_speed_panel(painter: &egui::Painter, rect: Rect, frame: &TelemetryFrame, scale: f32) {
    draw_label(
        painter,
        pos2(rect.left() + 34.0 * scale, rect.top() + 30.0 * scale),
        Align2::LEFT_CENTER,
        "LAP",
        16.0 * scale,
        TEXT,
    );
    draw_number(
        painter,
        pos2(rect.left() + 34.0 * scale, rect.top() + 70.0 * scale),
        Align2::LEFT_CENTER,
        &frame.lap_current,
        40.0 * scale,
        TEXT,
    );
    draw_number(
        painter,
        pos2(rect.left() + 92.0 * scale, rect.top() + 70.0 * scale),
        Align2::LEFT_CENTER,
        "/",
        35.0 * scale,
        ACCENT,
    );
    draw_number(
        painter,
        pos2(rect.left() + 122.0 * scale, rect.top() + 70.0 * scale),
        Align2::LEFT_CENTER,
        &frame.lap_total,
        31.0 * scale,
        TEXT,
    );

    draw_glow_number(
        painter,
        pos2(rect.left() + 32.0 * scale, rect.bottom() - 72.0 * scale),
        Align2::LEFT_CENTER,
        &frame.speed,
        92.0 * scale,
        TEXT,
    );
    draw_label(
        painter,
        pos2(rect.left() + 36.0 * scale, rect.bottom() - 24.0 * scale),
        Align2::LEFT_CENTER,
        "KPH",
        19.0 * scale,
        TEXT_DIM,
    );
}

fn paint_gear_panel(painter: &egui::Painter, rect: Rect, frame: &TelemetryFrame, scale: f32) {
    paint_center_wake(painter, rect, scale);
    draw_glow_number(
        painter,
        pos2(rect.center().x, rect.center().y - 8.0 * scale),
        Align2::CENTER_CENTER,
        &frame.gear,
        158.0 * scale,
        ACCENT,
    );
    draw_number(
        painter,
        pos2(rect.center().x, rect.bottom() - 30.0 * scale),
        Align2::CENTER_CENTER,
        &frame.rpm,
        29.0 * scale,
        TEXT,
    );
    draw_label(
        painter,
        pos2(rect.center().x, rect.bottom() - 9.0 * scale),
        Align2::CENTER_CENTER,
        "RPM",
        13.0 * scale,
        TEXT_DIM,
    );
}

fn paint_timing_panel(painter: &egui::Painter, rect: Rect, frame: &TelemetryFrame, scale: f32) {
    let x = rect.left() + rect.width() * 0.34;
    draw_label(
        painter,
        pos2(x, rect.top() + 30.0 * scale),
        Align2::LEFT_CENTER,
        "LAP TIME",
        16.0 * scale,
        TEXT,
    );
    draw_glow_number(
        painter,
        pos2(x, rect.top() + 72.0 * scale),
        Align2::LEFT_CENTER,
        &frame.current_lap,
        43.0 * scale,
        TEXT,
    );
    draw_label(
        painter,
        pos2(x, rect.center().y + 2.0 * scale),
        Align2::LEFT_CENTER,
        "BEST",
        16.0 * scale,
        TEXT,
    );
    draw_number(
        painter,
        pos2(x + 95.0 * scale, rect.center().y + 2.0 * scale),
        Align2::LEFT_CENTER,
        &frame.best_lap,
        18.0 * scale,
        TEXT_DIM,
    );
    draw_label(
        painter,
        pos2(x, rect.center().y + 58.0 * scale),
        Align2::LEFT_CENTER,
        "DELTA",
        17.0 * scale,
        TEXT,
    );
    draw_glow_number(
        painter,
        pos2(x, rect.bottom() - 31.0 * scale),
        Align2::LEFT_CENTER,
        &frame.delta,
        45.0 * scale,
        GREEN,
    );
}

fn paint_battery_panel(painter: &egui::Painter, rect: Rect, frame: &TelemetryFrame, scale: f32) {
    let left = rect.left() + 34.0 * scale;
    draw_label(
        painter,
        pos2(left, rect.top() + 34.0 * scale),
        Align2::LEFT_CENTER,
        "BATT",
        17.0 * scale,
        TEXT,
    );
    draw_number(
        painter,
        pos2(left, rect.top() + 91.0 * scale),
        Align2::LEFT_CENTER,
        &format!("{:.0}%", frame.battery_pct),
        47.0 * scale,
        TEXT,
    );
    paint_battery_icon(
        painter,
        pos2(rect.right() - 78.0 * scale, rect.top() + 37.0 * scale),
        scale,
    );
    paint_segment_bar(
        painter,
        Rect::from_min_size(
            pos2(left, rect.bottom() - 32.0 * scale),
            vec2(rect.width() - 78.0 * scale, 14.0 * scale),
        ),
        frame.battery_pct / 100.0,
        15,
        GREEN,
        scale,
    );
}

fn paint_tyres_panel(painter: &egui::Painter, rect: Rect, frame: &TelemetryFrame, scale: f32) {
    let car = Rect::from_center_size(
        pos2(rect.center().x, rect.center().y + 5.0 * scale),
        vec2(180.0 * scale, 125.0 * scale),
    );
    paint_car_plan(painter, car, scale);

    paint_tyre_metric(
        painter,
        pos2(rect.left() + 58.0 * scale, rect.top() + 44.0 * scale),
        frame.tyres[2],
        Align2::LEFT_CENTER,
        scale,
    );
    paint_tyre_metric(
        painter,
        pos2(rect.right() - 58.0 * scale, rect.top() + 44.0 * scale),
        frame.tyres[3],
        Align2::RIGHT_CENTER,
        scale,
    );
    paint_tyre_metric(
        painter,
        pos2(rect.left() + 58.0 * scale, rect.bottom() - 47.0 * scale),
        frame.tyres[0],
        Align2::LEFT_CENTER,
        scale,
    );
    paint_tyre_metric(
        painter,
        pos2(rect.right() - 58.0 * scale, rect.bottom() - 47.0 * scale),
        frame.tyres[1],
        Align2::RIGHT_CENTER,
        scale,
    );
}

fn paint_ers_panel(painter: &egui::Painter, rect: Rect, frame: &TelemetryFrame, scale: f32) {
    let left = rect.left() + 58.0 * scale;
    let right = rect.right() - 34.0 * scale;
    draw_label(
        painter,
        pos2(left, rect.top() + 34.0 * scale),
        Align2::LEFT_CENTER,
        "ERS",
        17.0 * scale,
        TEXT,
    );
    draw_label(
        painter,
        pos2(right - 86.0 * scale, rect.top() + 34.0 * scale),
        Align2::LEFT_CENTER,
        "DEPLOY",
        15.0 * scale,
        TEXT,
    );
    draw_number(
        painter,
        pos2(right, rect.top() + 34.0 * scale),
        Align2::RIGHT_CENTER,
        &format!("{:.0}", frame.ers_deploy * 4.0),
        32.0 * scale,
        ACCENT,
    );
    draw_number(
        painter,
        pos2(left, rect.top() + 92.0 * scale),
        Align2::LEFT_CENTER,
        &format!("{:.1}", frame.ers_deploy * 4.0),
        49.0 * scale,
        TEXT,
    );
    draw_label(
        painter,
        pos2(left + 104.0 * scale, rect.top() + 99.0 * scale),
        Align2::LEFT_CENTER,
        "MJ / LAP",
        15.0 * scale,
        TEXT_DIM,
    );
    draw_label(
        painter,
        pos2(right - 70.0 * scale, rect.top() + 89.0 * scale),
        Align2::LEFT_CENTER,
        "HARVEST",
        13.0 * scale,
        TEXT_DIM,
    );
    draw_number(
        painter,
        pos2(right, rect.top() + 111.0 * scale),
        Align2::RIGHT_CENTER,
        &format!("{:.0}%", frame.ers_harvest_pct),
        20.0 * scale,
        TEXT,
    );
    paint_segment_bar(
        painter,
        Rect::from_min_size(
            pos2(left, rect.bottom() - 32.0 * scale),
            vec2(rect.width() - 92.0 * scale, 14.0 * scale),
        ),
        frame.ers_deploy,
        12,
        ACCENT,
        scale,
    );
}

#[derive(Clone, Copy)]
enum PanelDirection {
    Left,
    Center,
    Right,
}

fn paint_main_panel(painter: &egui::Painter, rect: Rect, direction: PanelDirection, scale: f32) {
    let points = match direction {
        PanelDirection::Left => angled_panel_points(rect, 1.0),
        PanelDirection::Right => angled_panel_points(rect, -1.0),
        PanelDirection::Center => vec![
            rect.left_top(),
            rect.right_top(),
            rect.right_bottom(),
            rect.left_bottom(),
        ],
    };
    paint_panel_shape(painter, points, scale);
}

fn paint_bottom_panel(painter: &egui::Painter, rect: Rect, direction: PanelDirection, scale: f32) {
    let points = match direction {
        PanelDirection::Left => bottom_panel_points(rect, 1.0),
        PanelDirection::Right => bottom_panel_points(rect, -1.0),
        PanelDirection::Center => center_bottom_points(rect),
    };
    paint_panel_shape(painter, points, scale);
}

fn paint_panel_shape(painter: &egui::Painter, points: Vec<Pos2>, scale: f32) {
    let shadow = points
        .iter()
        .map(|point| *point + vec2(0.0, 4.0 * scale))
        .collect::<Vec<_>>();
    painter.add(egui::Shape::convex_polygon(
        shadow,
        Color32::from_rgba_premultiplied(0, 0, 0, 110),
        Stroke::NONE,
    ));
    painter.add(egui::Shape::convex_polygon(
        points.clone(),
        PANEL,
        Stroke::NONE,
    ));
    painter.add(egui::Shape::closed_line(
        points,
        Stroke::new(0.55 * scale, LINE_HOT),
    ));
}

fn paint_center_wake(painter: &egui::Painter, rect: Rect, scale: f32) {
    let center = rect.center();
    let color = Color32::from_rgba_premultiplied(102, 44, 0, 38);
    for index in 0..9 {
        let y_offset = (index as f32 - 4.0) * 10.0 * scale;
        painter.line_segment(
            [
                pos2(rect.left() + 24.0 * scale, center.y + y_offset),
                pos2(center.x - 84.0 * scale, center.y),
            ],
            Stroke::new(0.6 * scale, color),
        );
        painter.line_segment(
            [
                pos2(rect.right() - 24.0 * scale, center.y + y_offset),
                pos2(center.x + 84.0 * scale, center.y),
            ],
            Stroke::new(0.6 * scale, color),
        );
    }
}

fn paint_segment_bar(
    painter: &egui::Painter,
    rect: Rect,
    value: f32,
    segments: usize,
    color: Color32,
    scale: f32,
) {
    let gap = 3.0 * scale;
    let width = (rect.width() - gap * (segments.saturating_sub(1)) as f32) / segments as f32;
    let active = (value.clamp(0.0, 1.0) * segments as f32).round() as usize;

    for index in 0..segments {
        let x = rect.left() + (width + gap) * index as f32;
        let segment = Rect::from_min_size(pos2(x, rect.top()), vec2(width, rect.height()));
        let fill = if index < active {
            color
        } else {
            Color32::from_rgb(30, 35, 36)
        };
        painter.rect_filled(segment, 2.0 * scale, fill);
        painter.rect_stroke(
            segment,
            2.0 * scale,
            Stroke::new(0.4 * scale, Color32::from_rgb(60, 68, 69)),
            StrokeKind::Inside,
        );
    }
}

fn paint_tyre_metric(
    painter: &egui::Painter,
    pos: Pos2,
    tyre: TyreDisplay,
    align: Align2,
    scale: f32,
) {
    let temp = tyre
        .temp
        .map(|temp| format!("{temp}C"))
        .unwrap_or_else(|| "--C".to_owned());
    let wear = tyre
        .wear_pct
        .map(|wear| format!("{wear:.0}%"))
        .unwrap_or_else(|| "--%".to_owned());
    draw_label(
        painter,
        pos + vec2(0.0, -25.0 * scale),
        align,
        tyre.label,
        13.0 * scale,
        TEXT,
    );
    draw_number(painter, pos, align, &temp, 22.0 * scale, GREEN);
    draw_number(
        painter,
        pos + vec2(0.0, 22.0 * scale),
        align,
        &wear,
        15.0 * scale,
        TEXT_DIM,
    );
}

fn paint_car_plan(painter: &egui::Painter, rect: Rect, scale: f32) {
    let center = rect.center();
    let body = Rect::from_center_size(center, vec2(rect.width() * 0.22, rect.height() * 0.82));
    let nose = vec![
        pos2(center.x, rect.top()),
        pos2(body.left(), body.top() + rect.height() * 0.30),
        pos2(body.right(), body.top() + rect.height() * 0.30),
    ];
    painter.line_segment(
        [
            pos2(body.left(), center.y),
            pos2(rect.left(), rect.top() + rect.height() * 0.28),
        ],
        Stroke::new(4.0 * scale, Color32::from_rgb(46, 52, 53)),
    );
    painter.line_segment(
        [
            pos2(body.right(), center.y),
            pos2(rect.right(), rect.top() + rect.height() * 0.28),
        ],
        Stroke::new(4.0 * scale, Color32::from_rgb(46, 52, 53)),
    );
    painter.line_segment(
        [
            pos2(body.left(), center.y + 28.0 * scale),
            pos2(rect.left(), rect.bottom() - rect.height() * 0.28),
        ],
        Stroke::new(4.0 * scale, Color32::from_rgb(46, 52, 53)),
    );
    painter.line_segment(
        [
            pos2(body.right(), center.y + 28.0 * scale),
            pos2(rect.right(), rect.bottom() - rect.height() * 0.28),
        ],
        Stroke::new(4.0 * scale, Color32::from_rgb(46, 52, 53)),
    );
    painter.add(egui::Shape::convex_polygon(
        nose,
        Color32::from_rgb(42, 48, 49),
        Stroke::new(1.0 * scale, Color32::from_rgb(86, 93, 94)),
    ));
    painter.rect_filled(body, 8.0 * scale, Color32::from_rgb(30, 36, 37));
    painter.rect_stroke(
        body,
        8.0 * scale,
        Stroke::new(1.0 * scale, Color32::from_rgb(95, 102, 103)),
        StrokeKind::Inside,
    );
    for side in [-1.0, 1.0] {
        let x = center.x + side * rect.width() * 0.34;
        for y in [
            rect.top() + rect.height() * 0.30,
            rect.bottom() - rect.height() * 0.24,
        ] {
            let tyre = Rect::from_center_size(pos2(x, y), vec2(30.0 * scale, 47.0 * scale));
            painter.rect_filled(tyre, 5.0 * scale, Color32::from_rgb(24, 28, 29));
            painter.rect_stroke(
                tyre,
                5.0 * scale,
                Stroke::new(1.0 * scale, Color32::from_rgb(76, 82, 83)),
                StrokeKind::Inside,
            );
        }
    }
}

fn paint_battery_icon(painter: &egui::Painter, pos: Pos2, scale: f32) {
    let body = Rect::from_min_size(pos, vec2(38.0 * scale, 19.0 * scale));
    let cap = Rect::from_min_size(
        pos2(body.right(), body.top() + 5.0 * scale),
        vec2(5.0 * scale, 9.0 * scale),
    );
    painter.rect_stroke(
        body,
        3.0 * scale,
        Stroke::new(1.2 * scale, GREEN),
        StrokeKind::Inside,
    );
    painter.rect_filled(cap, 1.0 * scale, GREEN);
    draw_label(
        painter,
        body.center(),
        Align2::CENTER_CENTER,
        "E",
        16.0 * scale,
        GREEN,
    );
}

fn shell_points(rect: Rect, scale: f32) -> Vec<Pos2> {
    let cut = 32.0 * scale;
    let lower_notch = 86.0 * scale;
    vec![
        pos2(rect.left() + cut, rect.top()),
        pos2(rect.right() - cut, rect.top()),
        rect.right_top() + vec2(cut * 0.72, cut * 0.78),
        rect.right_bottom() - vec2(cut * 0.72, cut * 0.78),
        pos2(rect.center().x + lower_notch, rect.bottom()),
        pos2(
            rect.center().x + lower_notch * 0.60,
            rect.bottom() + 25.0 * scale,
        ),
        pos2(
            rect.center().x - lower_notch * 0.60,
            rect.bottom() + 25.0 * scale,
        ),
        pos2(rect.center().x - lower_notch, rect.bottom()),
        rect.left_bottom() + vec2(cut * 0.72, -cut * 0.78),
        rect.left_top() + vec2(cut * 0.72, cut * 0.78),
    ]
}

fn angled_panel_points(rect: Rect, direction: f32) -> Vec<Pos2> {
    let notch = rect.width() * 0.30;
    if direction > 0.0 {
        vec![
            rect.left_top(),
            pos2(rect.right() - notch, rect.top()),
            pos2(rect.right(), rect.center().y),
            pos2(rect.right() - notch, rect.bottom()),
            rect.left_bottom(),
        ]
    } else {
        vec![
            pos2(rect.left() + notch, rect.top()),
            rect.right_top(),
            rect.right_bottom(),
            pos2(rect.left() + notch, rect.bottom()),
            pos2(rect.left(), rect.center().y),
        ]
    }
}

fn bottom_panel_points(rect: Rect, direction: f32) -> Vec<Pos2> {
    let notch = rect.width() * 0.20;
    if direction > 0.0 {
        vec![
            rect.left_top(),
            pos2(rect.right() - notch, rect.top()),
            rect.right_bottom(),
            rect.left_bottom(),
        ]
    } else {
        vec![
            pos2(rect.left() + notch, rect.top()),
            rect.right_top(),
            rect.right_bottom(),
            rect.left_bottom(),
        ]
    }
}

fn center_bottom_points(rect: Rect) -> Vec<Pos2> {
    let notch = rect.width() * 0.08;
    vec![
        pos2(rect.left() + notch, rect.top()),
        pos2(rect.right() - notch, rect.top()),
        rect.right_bottom(),
        rect.left_bottom(),
    ]
}

fn draw_label(
    painter: &egui::Painter,
    pos: Pos2,
    align: Align2,
    value: &str,
    size: f32,
    color: Color32,
) {
    painter.text(
        pos,
        align,
        value,
        FontId::new(size, FontFamily::Proportional),
        color,
    );
}

fn draw_number(
    painter: &egui::Painter,
    pos: Pos2,
    align: Align2,
    value: &str,
    size: f32,
    color: Color32,
) {
    painter.text(
        pos,
        align,
        value,
        FontId::new(size, FontFamily::Monospace),
        color,
    );
}

fn draw_glow_number(
    painter: &egui::Painter,
    pos: Pos2,
    align: Align2,
    value: &str,
    size: f32,
    color: Color32,
) {
    let glow = Color32::from_rgba_premultiplied(color.r(), color.g(), color.b(), 34);
    for offset in [vec2(-2.0, 0.0), vec2(2.0, 0.0), vec2(0.0, 2.0)] {
        draw_number(painter, pos + offset, align, value, size + 3.0, glow);
    }
    draw_number(painter, pos, align, value, size, color);
}

fn rpm_segment_color(position: f32, active: bool) -> Color32 {
    if !active {
        return if position >= 0.88 {
            Color32::from_rgb(70, 12, 12)
        } else if position >= 0.70 {
            Color32::from_rgb(68, 34, 10)
        } else {
            Color32::from_rgb(68, 73, 73)
        };
    }

    if position >= 0.88 {
        RED
    } else if position >= 0.70 {
        ACCENT
    } else {
        Color32::from_rgb(213, 216, 216)
    }
}

fn rev_light_color(index: usize) -> Color32 {
    match index {
        0..=4 => GREEN,
        5..=10 => AMBER,
        11..=12 => RED,
        _ => MAGENTA,
    }
}

fn tyre_displays(
    input: Option<&InputSample>,
    damage: Option<&DamageSample>,
    fallback: [TyreDisplay; 4],
) -> [TyreDisplay; 4] {
    let temps = input.map(|input| input.tyre_surface_temps_c);
    let wear = damage.map(|damage| damage.tyre_wear);
    [
        TyreDisplay {
            label: "RL",
            temp: temps.map(|temps| temps.rl).or(fallback[0].temp),
            wear_pct: wear.map(|wear| wear.rl).or(fallback[0].wear_pct),
        },
        TyreDisplay {
            label: "RR",
            temp: temps.map(|temps| temps.rr).or(fallback[1].temp),
            wear_pct: wear.map(|wear| wear.rr).or(fallback[1].wear_pct),
        },
        TyreDisplay {
            label: "FL",
            temp: temps.map(|temps| temps.fl).or(fallback[2].temp),
            wear_pct: wear.map(|wear| wear.fl).or(fallback[2].wear_pct),
        },
        TyreDisplay {
            label: "FR",
            temp: temps.map(|temps| temps.fr).or(fallback[3].temp),
            wear_pct: wear.map(|wear| wear.fr).or(fallback[3].wear_pct),
        },
    ]
}

fn lap_time(
    lap: Option<&LapSample>,
    read: impl FnOnce(&LapSample) -> u32,
    fallback: &str,
) -> String {
    lap.map(read)
        .map(format_ms)
        .unwrap_or_else(|| fallback.to_owned())
}

fn fit_aspect(rect: Rect, aspect: f32) -> Rect {
    let current = rect.width() / rect.height();
    if current > aspect {
        let width = rect.height() * aspect;
        Rect::from_center_size(rect.center(), vec2(width, rect.height()))
    } else {
        let height = rect.width() / aspect;
        Rect::from_center_size(rect.center(), vec2(rect.width(), height))
    }
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
    format!("{seconds:+.3}")
}

fn paint_center_message(
    painter: &egui::Painter,
    rect: Rect,
    message: &str,
    color: Color32,
    scale: f32,
) {
    let banner = Rect::from_center_size(rect.center(), vec2(rect.width() * 0.45, 48.0 * scale));
    painter.rect_filled(
        banner,
        5.0 * scale,
        Color32::from_rgba_premultiplied(0, 0, 0, 220),
    );
    painter.rect_stroke(
        banner,
        5.0 * scale,
        Stroke::new(1.0 * scale, LINE_HOT),
        StrokeKind::Inside,
    );
    draw_label(
        painter,
        banner.center(),
        Align2::CENTER_CENTER,
        message,
        17.0 * scale,
        color,
    );
}

fn paint_waiting_strip(painter: &egui::Painter, rect: Rect, scale: f32) {
    draw_label(
        painter,
        pos2(rect.center().x, rect.bottom() - 12.0 * scale),
        Align2::CENTER_CENTER,
        "WAITING FOR TELEMETRY",
        12.0 * scale,
        TEXT_DIM,
    );
}
