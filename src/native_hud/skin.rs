use eframe::egui::{
    self, Align2, Color32, FontFamily, FontId, Rect, Stroke, StrokeKind, pos2, vec2,
};

use crate::telemetry::{LapSample, StatusSample, TelemetryUpdate};

pub const APP_BG: Color32 = Color32::BLACK;
pub const ACCENT: Color32 = Color32::from_rgb(245, 248, 248);

const SCREEN_BG: Color32 = Color32::BLACK;
const TEXT: Color32 = Color32::from_rgb(244, 247, 247);
const TEXT_DIM: Color32 = Color32::from_rgb(152, 158, 158);
const LINE: Color32 = Color32::from_rgba_premultiplied(244, 247, 247, 190);
const LINE_DIM: Color32 = Color32::from_rgba_premultiplied(244, 247, 247, 92);
const BAR_OFF: Color32 = Color32::from_rgb(20, 22, 22);

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
            paint_center_message(painter, layout.content, message, layout.scale);
        } else if frame.waiting {
            paint_waiting_strip(painter, layout.content, layout.scale);
        }
    }
}

#[derive(Default)]
struct HudTheme;

struct HudLayout {
    screen: Rect,
    content: Rect,
    top: Rect,
    middle: Rect,
    lower: Rect,
    sectors: Rect,
    scale: f32,
}

impl HudLayout {
    fn new(rect: Rect) -> Self {
        let margin = vec2(
            (rect.width() * 0.065).clamp(22.0, 78.0),
            (rect.height() * 0.160).clamp(28.0, 92.0),
        );
        let screen = fit_aspect(rect.shrink2(margin), 1.78);
        let scale = (screen.width() / 880.0).clamp(0.72, 1.65);
        let content = screen.shrink2(vec2(12.0 * scale, 10.0 * scale));

        let top = Rect::from_min_max(
            content.left_top(),
            pos2(content.right(), content.top() + content.height() * 0.255),
        );
        let middle = Rect::from_min_max(
            pos2(content.left(), top.bottom()),
            pos2(content.right(), content.top() + content.height() * 0.690),
        );
        let lower = Rect::from_min_max(
            pos2(content.left(), middle.bottom()),
            pos2(content.right(), content.top() + content.height() * 0.910),
        );
        let sectors =
            Rect::from_min_max(pos2(content.left(), lower.bottom()), content.right_bottom());

        Self {
            screen,
            content,
            top,
            middle,
            lower,
            sectors,
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
    drs: bool,
    position: String,
    position_total: String,
    lap_current: String,
    lap_total: String,
    current_lap: String,
    last_lap: String,
    delta: String,
    front_gap: String,
    behind_gap: String,
    ers_mode: String,
    ers_pct: String,
}

impl TelemetryFrame {
    fn from_update(state: &TelemetryUpdate, error: Option<&str>) -> Self {
        let sample = Self::sample(state.is_empty() && error.is_none(), error);
        let input = state.input.as_ref();
        let lap = state.lap.as_ref();
        let status = state.status.as_ref();
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
            drs: input.map(|input| input.drs).unwrap_or(sample.drs),
            position: lap
                .map(|lap| lap.car_position.max(1).to_string())
                .unwrap_or(sample.position),
            position_total: sample.position_total,
            lap_current: lap
                .map(|lap| lap.current_lap_num.to_string())
                .unwrap_or(sample.lap_current),
            lap_total,
            current_lap: lap_time(lap, |lap| lap.current_lap_time_ms, &sample.current_lap),
            last_lap: lap_time(lap, |lap| lap.last_lap_time_ms, &sample.last_lap),
            delta: lap
                .and_then(|lap| lap.delta_to_race_leader_ms)
                .map(|value| format_gap_ms(value, '-'))
                .or_else(|| {
                    lap.and_then(|lap| lap.delta_to_car_in_front_ms)
                        .map(|value| format_gap_ms(value, '-'))
                })
                .unwrap_or(sample.delta),
            front_gap: lap
                .and_then(|lap| lap.delta_to_car_in_front_ms)
                .map(|value| format_gap_ms(value, '+'))
                .unwrap_or(sample.front_gap),
            behind_gap: lap
                .and_then(|lap| lap.delta_to_car_behind_ms)
                .map(|value| format_gap_ms(value, '-'))
                .unwrap_or(sample.behind_gap),
            ers_mode: status
                .map(|status| status.ers_deploy_mode.to_string())
                .unwrap_or(sample.ers_mode),
            ers_pct: status
                .map(StatusSample::ers_percent)
                .map(|value| format!("{value:.0}%"))
                .unwrap_or(sample.ers_pct),
        }
    }

    fn sample(waiting: bool, error: Option<&str>) -> Self {
        Self {
            waiting,
            error: error.map(ToOwned::to_owned),
            speed: "264".to_owned(),
            gear: "7".to_owned(),
            rpm: "8500".to_owned(),
            rpm_ratio: 0.57,
            drs: true,
            position: "2".to_owned(),
            position_total: "20".to_owned(),
            lap_current: "2".to_owned(),
            lap_total: "30".to_owned(),
            current_lap: "1:24.50".to_owned(),
            last_lap: "1:24.50".to_owned(),
            delta: "-00.50".to_owned(),
            front_gap: "+03.04".to_owned(),
            behind_gap: "-01.04".to_owned(),
            ers_mode: "4".to_owned(),
            ers_pct: "56%".to_owned(),
        }
    }
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
        painter.rect_filled(layout.screen, 0.0, SCREEN_BG);
        stroke_rect(painter, layout.screen, 1.2 * layout.scale, LINE);
        stroke_rect(
            painter,
            layout.content,
            0.8 * layout.scale,
            Color32::from_rgba_premultiplied(244, 247, 247, 130),
        );
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
        let painter = painter.with_clip_rect(layout.content);
        paint_top_row(&painter, layout.top, frame, layout.scale);
        paint_middle_grid(&painter, layout.middle, frame, layout.scale);
        paint_lower_row(&painter, layout.lower, frame, layout.scale);
        paint_sector_bar(&painter, layout.sectors, frame.rpm_ratio, layout.scale);
    }
}

fn paint_top_row(painter: &egui::Painter, rect: Rect, frame: &TelemetryFrame, scale: f32) {
    let cells = split_by_weights(rect, &[0.18, 0.32, 0.28, 0.22]);
    paint_box(painter, cells[0], scale);
    paint_box(painter, cells[1], scale);
    paint_box(painter, cells[2], scale);
    paint_box(painter, cells[3], scale);

    paint_metric(painter, cells[0], &frame.speed, "KPH", 21.0, scale);
    paint_metric(painter, cells[1], &frame.last_lap, "Last Lap", 25.0, scale);
    paint_metric(painter, cells[2], &frame.delta, "Delta", 24.0, scale);
    paint_metric(painter, cells[3], &frame.rpm, "RPM", 21.0, scale);
}

fn paint_middle_grid(painter: &egui::Painter, rect: Rect, frame: &TelemetryFrame, scale: f32) {
    let left = Rect::from_min_max(
        rect.left_top(),
        pos2(rect.left() + rect.width() * 0.300, rect.bottom()),
    );
    let center = Rect::from_min_max(
        pos2(left.right(), rect.top()),
        pos2(rect.right() - rect.width() * 0.300, rect.bottom()),
    );
    let right = Rect::from_min_max(pos2(center.right(), rect.top()), rect.right_bottom());
    let left_rows = split_rows(left, 2);
    let right_rows = split_rows(right, 2);

    paint_box(painter, left_rows[0], scale);
    paint_box(painter, left_rows[1], scale);
    paint_box(painter, center, scale);
    paint_box(painter, right_rows[0], scale);
    paint_box(painter, right_rows[1], scale);

    draw_label(
        painter,
        left_rows[0].center(),
        Align2::CENTER_CENTER,
        if frame.drs { "DRS" } else { "---" },
        22.0 * scale,
        TEXT,
    );
    paint_fraction_metric(
        painter,
        left_rows[1],
        &frame.position,
        &frame.position_total,
        "Pos",
        scale,
    );

    draw_number(
        painter,
        center.center() + vec2(0.0, -8.0 * scale),
        Align2::CENTER_CENTER,
        &frame.gear,
        96.0 * scale,
        TEXT,
    );
    draw_label(
        painter,
        center.center() + vec2(0.0, 56.0 * scale),
        Align2::CENTER_CENTER,
        "Gear",
        12.0 * scale,
        TEXT_DIM,
    );

    draw_label(
        painter,
        right_rows[0].center() + vec2(0.0, -8.0 * scale),
        Align2::CENTER_CENTER,
        "ERS",
        21.0 * scale,
        TEXT,
    );
    draw_label(
        painter,
        right_rows[0].center() + vec2(0.0, 17.0 * scale),
        Align2::CENTER_CENTER,
        &format!("M{} {}", frame.ers_mode, frame.ers_pct),
        10.0 * scale,
        TEXT_DIM,
    );
    paint_fraction_metric(
        painter,
        right_rows[1],
        &frame.lap_current,
        &frame.lap_total,
        "Lap",
        scale,
    );
}

fn paint_lower_row(painter: &egui::Painter, rect: Rect, frame: &TelemetryFrame, scale: f32) {
    let cells = split_by_weights(rect, &[0.32, 0.36, 0.32]);
    for cell in &cells {
        paint_box(painter, *cell, scale);
    }

    paint_metric(painter, cells[0], &frame.front_gap, "D -1", 25.0, scale);
    paint_metric(painter, cells[1], &frame.current_lap, "Curr", 25.0, scale);
    paint_metric(painter, cells[2], &frame.behind_gap, "D +1", 25.0, scale);
}

fn paint_sector_bar(painter: &egui::Painter, rect: Rect, value: f32, scale: f32) {
    paint_box(painter, rect, scale);
    let inner = rect.shrink2(vec2(6.0 * scale, 5.0 * scale));
    let count = 14;
    let gap = 2.0 * scale;
    let width = (inner.width() - gap * (count - 1) as f32) / count as f32;
    let active = (value.clamp(0.0, 1.0) * count as f32).round() as usize;

    for index in 0..count {
        let segment = Rect::from_min_size(
            pos2(inner.left() + index as f32 * (width + gap), inner.top()),
            vec2(width, inner.height()),
        );
        painter.rect_filled(segment, 0.0, if index < active { TEXT } else { BAR_OFF });
    }
}

fn paint_metric(
    painter: &egui::Painter,
    rect: Rect,
    value: &str,
    label: &str,
    size: f32,
    scale: f32,
) {
    draw_number(
        painter,
        rect.center() + vec2(0.0, -6.0 * scale),
        Align2::CENTER_CENTER,
        value,
        size * scale,
        TEXT,
    );
    draw_label(
        painter,
        rect.center() + vec2(0.0, 21.0 * scale),
        Align2::CENTER_CENTER,
        label,
        10.0 * scale,
        TEXT_DIM,
    );
}

fn paint_fraction_metric(
    painter: &egui::Painter,
    rect: Rect,
    numerator: &str,
    denominator: &str,
    label: &str,
    scale: f32,
) {
    draw_number(
        painter,
        rect.center() + vec2(-8.0 * scale, -4.0 * scale),
        Align2::RIGHT_CENTER,
        numerator,
        31.0 * scale,
        TEXT,
    );
    draw_label(
        painter,
        rect.center() + vec2(-1.0 * scale, -3.0 * scale),
        Align2::CENTER_CENTER,
        "/",
        15.0 * scale,
        TEXT_DIM,
    );
    draw_number(
        painter,
        rect.center() + vec2(9.0 * scale, -4.0 * scale),
        Align2::LEFT_CENTER,
        denominator,
        18.0 * scale,
        TEXT,
    );
    draw_label(
        painter,
        rect.center() + vec2(0.0, 25.0 * scale),
        Align2::CENTER_CENTER,
        label,
        10.0 * scale,
        TEXT_DIM,
    );
}

fn paint_box(painter: &egui::Painter, rect: Rect, scale: f32) {
    stroke_rect(painter, rect, 0.7 * scale, LINE_DIM);
}

fn split_by_weights(rect: Rect, weights: &[f32]) -> Vec<Rect> {
    let total: f32 = weights.iter().sum();
    let mut left = rect.left();
    weights
        .iter()
        .map(|weight| {
            let width = rect.width() * (*weight / total);
            let cell =
                Rect::from_min_max(pos2(left, rect.top()), pos2(left + width, rect.bottom()));
            left += width;
            cell
        })
        .collect()
}

fn split_rows(rect: Rect, count: usize) -> Vec<Rect> {
    let height = rect.height() / count as f32;
    (0..count)
        .map(|index| {
            Rect::from_min_max(
                pos2(rect.left(), rect.top() + index as f32 * height),
                pos2(rect.right(), rect.top() + (index + 1) as f32 * height),
            )
        })
        .collect()
}

fn stroke_rect(painter: &egui::Painter, rect: Rect, width: f32, color: Color32) {
    painter.rect_stroke(rect, 0.0, Stroke::new(width, color), StrokeKind::Inside);
}

fn draw_label(
    painter: &egui::Painter,
    pos: egui::Pos2,
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
    pos: egui::Pos2,
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
        return "--:--.--".to_owned();
    }
    let minutes = value / 60_000;
    let seconds = (value % 60_000) / 1_000;
    let centis = (value % 1_000) / 10;
    format!("{minutes}:{seconds:02}.{centis:02}")
}

fn format_gap_ms(value: u32, sign: char) -> String {
    let minutes = value / 60_000;
    let seconds = (value % 60_000) / 1_000;
    let centis = (value % 1_000) / 10;
    format!("{sign}{minutes:02}:{seconds:02}.{centis:02}")
}

fn paint_center_message(painter: &egui::Painter, rect: Rect, message: &str, scale: f32) {
    let banner = Rect::from_center_size(rect.center(), vec2(rect.width() * 0.50, 44.0 * scale));
    painter.rect_filled(banner, 0.0, Color32::BLACK);
    stroke_rect(painter, banner, 1.0 * scale, LINE);
    draw_label(
        painter,
        banner.center(),
        Align2::CENTER_CENTER,
        message,
        16.0 * scale,
        TEXT,
    );
}

fn paint_waiting_strip(painter: &egui::Painter, rect: Rect, scale: f32) {
    draw_label(
        painter,
        pos2(rect.center().x, rect.bottom() - 10.0 * scale),
        Align2::CENTER_BOTTOM,
        "WAITING FOR TELEMETRY",
        10.0 * scale,
        TEXT_DIM,
    );
}
