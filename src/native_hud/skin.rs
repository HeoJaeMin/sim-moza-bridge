use eframe::egui::{
    self, Align2, Color32, FontFamily, FontId, Pos2, Rect, Stroke, StrokeKind, pos2, vec2,
};

use crate::telemetry::{DamageSample, InputSample, LapSample, StatusSample, TelemetryUpdate};

pub const APP_BG: Color32 = Color32::BLACK;
pub const ACCENT: Color32 = Color32::from_rgb(244, 247, 247);

const TEXT: Color32 = Color32::from_rgb(244, 247, 247);
const TEXT_DIM: Color32 = Color32::from_rgb(145, 152, 152);
const LINE: Color32 = Color32::from_rgba_premultiplied(244, 247, 247, 170);
const LINE_DIM: Color32 = Color32::from_rgba_premultiplied(244, 247, 247, 86);
const WARNING: Color32 = Color32::from_rgb(238, 238, 238);
const BAR_OFF: Color32 = Color32::from_rgb(24, 27, 27);

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
    display: Rect,
    content: Rect,
    status: Rect,
    car: Rect,
    footer: Rect,
    scale: f32,
}

impl HudLayout {
    fn new(rect: Rect) -> Self {
        let margin = vec2(
            (rect.width() * 0.060).clamp(24.0, 72.0),
            (rect.height() * 0.115).clamp(24.0, 68.0),
        );
        let display = fit_aspect(rect.shrink2(margin), 1.78);
        let scale = (display.width() / 1620.0).clamp(0.56, 1.10);
        let content = display.shrink2(vec2(display.width() * 0.045, display.height() * 0.070));

        let status = Rect::from_min_max(
            content.left_top(),
            pos2(content.right(), content.top() + content.height() * 0.140),
        );
        let footer = Rect::from_min_max(
            pos2(content.left(), content.bottom() - content.height() * 0.185),
            content.right_bottom(),
        );
        let car = Rect::from_min_max(
            pos2(content.left(), status.bottom()),
            pos2(content.right(), footer.top()),
        );

        Self {
            display,
            content,
            status,
            car,
            footer,
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
    drs: bool,
    lap_current: String,
    lap_total: String,
    current_lap: String,
    best_lap: String,
    delta: String,
    battery_pct: f32,
    ers_deploy: f32,
    ers_mode: String,
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
            drs: input.map(|input| input.drs).unwrap_or(sample.drs),
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
            ers_mode: status
                .map(|status| status.ers_deploy_mode.to_string())
                .unwrap_or(sample.ers_mode),
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
            rev_ratio: 1.0,
            drs: true,
            lap_current: "18".to_owned(),
            lap_total: "52".to_owned(),
            current_lap: "1:24.705".to_owned(),
            best_lap: "1:23.456".to_owned(),
            delta: "-0.256".to_owned(),
            battery_pct: 56.0,
            ers_deploy: 0.60,
            ers_mode: "4".to_owned(),
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
        painter.rect_filled(layout.display, 0.0, APP_BG);
        stroke_rect(painter, layout.display, 1.4 * layout.scale, LINE);
        stroke_rect(
            painter,
            layout.display.shrink(7.0 * layout.scale),
            0.6 * layout.scale,
            LINE_DIM,
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
        paint_status_strip(&painter, layout.status, frame, layout.scale);
        paint_car_damage_panel(&painter, layout.car, frame, layout.scale);
        paint_footer_strip(&painter, layout.footer, frame, layout.scale);
    }
}

fn paint_status_strip(painter: &egui::Painter, rect: Rect, frame: &TelemetryFrame, scale: f32) {
    draw_label(
        painter,
        rect.left_center() + vec2(0.0, -5.0 * scale),
        Align2::LEFT_CENTER,
        if frame.drs { "DRS" } else { "DRS -" },
        17.0 * scale,
        TEXT,
    );
    draw_label(
        painter,
        rect.left_center() + vec2(46.0 * scale, 12.0 * scale),
        Align2::LEFT_CENTER,
        "K2",
        11.0 * scale,
        TEXT_DIM,
    );

    let led_rect = Rect::from_center_size(
        rect.center() + vec2(0.0, -5.0 * scale),
        vec2(rect.width() * 0.48, 12.0 * scale),
    );
    paint_led_strip(painter, led_rect, frame.rev_ratio, scale);

    draw_label(
        painter,
        rect.right_center() + vec2(0.0, -5.0 * scale),
        Align2::RIGHT_CENTER,
        &format!("ERS {}", frame.ers_mode),
        15.0 * scale,
        TEXT,
    );
}

fn paint_car_damage_panel(painter: &egui::Painter, rect: Rect, frame: &TelemetryFrame, scale: f32) {
    let center = rect.center();
    let car = Rect::from_center_size(center, vec2(130.0 * scale, 190.0 * scale));

    paint_percent_circle(
        painter,
        pos2(
            rect.left() + rect.width() * 0.34,
            rect.top() + rect.height() * 0.22,
        ),
        frame.tyres[2].label,
        frame.tyres[2].wear_pct.unwrap_or(0.0),
        scale,
    );
    paint_percent_circle(
        painter,
        pos2(
            rect.right() - rect.width() * 0.34,
            rect.top() + rect.height() * 0.22,
        ),
        frame.tyres[3].label,
        frame.tyres[3].wear_pct.unwrap_or(0.0),
        scale,
    );
    paint_percent_circle(
        painter,
        pos2(
            rect.left() + rect.width() * 0.34,
            rect.bottom() - rect.height() * 0.25,
        ),
        frame.tyres[0].label,
        frame.tyres[0].wear_pct.unwrap_or(0.0),
        scale,
    );
    paint_percent_circle(
        painter,
        pos2(
            rect.right() - rect.width() * 0.34,
            rect.bottom() - rect.height() * 0.25,
        ),
        frame.tyres[1].label,
        frame.tyres[1].wear_pct.unwrap_or(0.0),
        scale,
    );

    paint_warning_triangle(
        painter,
        pos2(rect.left() + rect.width() * 0.245, rect.center().y),
        scale,
    );
    paint_warning_triangle(
        painter,
        pos2(rect.right() - rect.width() * 0.245, rect.center().y),
        scale,
    );

    draw_label(
        painter,
        pos2(center.x, car.top() - 12.0 * scale),
        Align2::CENTER_BOTTOM,
        &format!("{:.0}%", frame.battery_pct),
        14.0 * scale,
        TEXT,
    );
    paint_car_outline(painter, car, scale);
    draw_label(
        painter,
        pos2(center.x, rect.bottom() - 7.0 * scale),
        Align2::CENTER_BOTTOM,
        "THRUSTMASTER",
        13.0 * scale,
        TEXT,
    );
}

fn paint_footer_strip(painter: &egui::Painter, rect: Rect, frame: &TelemetryFrame, scale: f32) {
    let cells = split_columns(rect, 5, 5.0 * scale);
    paint_footer_cell(painter, cells[0], "SPD", &frame.speed, scale);
    paint_footer_cell(painter, cells[1], "GEAR", &frame.gear, scale);
    paint_footer_cell(painter, cells[2], "RPM", &frame.rpm, scale);
    paint_footer_cell(
        painter,
        cells[3],
        "LAP",
        &format!("{}/{}", frame.lap_current, frame.lap_total),
        scale,
    );
    paint_footer_cell(painter, cells[4], "DIF", &frame.delta, scale);
}

fn paint_footer_cell(painter: &egui::Painter, rect: Rect, label: &str, value: &str, scale: f32) {
    stroke_rect(painter, rect, 0.6 * scale, LINE_DIM);
    draw_label(
        painter,
        rect.left_top() + vec2(7.0 * scale, 5.0 * scale),
        Align2::LEFT_TOP,
        label,
        9.0 * scale,
        TEXT_DIM,
    );
    draw_number(
        painter,
        pos2(rect.center().x, rect.bottom() - 8.0 * scale),
        Align2::CENTER_BOTTOM,
        value,
        18.0 * scale,
        TEXT,
    );
}

fn paint_percent_circle(
    painter: &egui::Painter,
    center: Pos2,
    label: &str,
    value: f32,
    scale: f32,
) {
    let radius = 39.0 * scale;
    painter.circle_stroke(center, radius, Stroke::new(1.5 * scale, LINE));
    painter.circle_stroke(center, radius * 0.78, Stroke::new(0.5 * scale, LINE_DIM));
    draw_label(
        painter,
        center + vec2(0.0, -14.0 * scale),
        Align2::CENTER_CENTER,
        label,
        10.0 * scale,
        TEXT_DIM,
    );
    draw_number(
        painter,
        center + vec2(0.0, 5.0 * scale),
        Align2::CENTER_CENTER,
        &format!("{value:.0}%"),
        18.0 * scale,
        TEXT,
    );
}

fn paint_warning_triangle(painter: &egui::Painter, center: Pos2, scale: f32) {
    let half = 22.0 * scale;
    let top = pos2(center.x, center.y - half);
    let left = pos2(center.x - half * 0.90, center.y + half * 0.75);
    let right = pos2(center.x + half * 0.90, center.y + half * 0.75);
    painter.add(egui::Shape::closed_line(
        vec![top, right, left],
        Stroke::new(1.3 * scale, WARNING),
    ));
    draw_label(
        painter,
        center + vec2(0.0, 3.0 * scale),
        Align2::CENTER_CENTER,
        "!",
        18.0 * scale,
        WARNING,
    );
}

fn paint_car_outline(painter: &egui::Painter, rect: Rect, scale: f32) {
    let center = rect.center();
    let body = Rect::from_center_size(center, vec2(rect.width() * 0.34, rect.height() * 0.68));
    let nose = vec![
        pos2(center.x, rect.top()),
        pos2(body.left(), body.top() + rect.height() * 0.26),
        pos2(body.right(), body.top() + rect.height() * 0.26),
    ];

    painter.add(egui::Shape::closed_line(
        nose,
        Stroke::new(1.4 * scale, LINE),
    ));
    stroke_rect(painter, body, 1.4 * scale, LINE);
    painter.line_segment(
        [pos2(center.x, body.top()), pos2(center.x, body.bottom())],
        Stroke::new(0.7 * scale, LINE_DIM),
    );

    let axle_y = [
        rect.top() + rect.height() * 0.30,
        rect.bottom() - rect.height() * 0.25,
    ];
    for y in axle_y {
        painter.line_segment(
            [
                pos2(body.left(), y),
                pos2(rect.left() + rect.width() * 0.18, y),
            ],
            Stroke::new(3.0 * scale, LINE_DIM),
        );
        painter.line_segment(
            [
                pos2(body.right(), y),
                pos2(rect.right() - rect.width() * 0.18, y),
            ],
            Stroke::new(3.0 * scale, LINE_DIM),
        );
    }

    for side in [-1.0, 1.0] {
        let x = center.x + side * rect.width() * 0.38;
        for y in axle_y {
            let tyre = Rect::from_center_size(pos2(x, y), vec2(24.0 * scale, 47.0 * scale));
            stroke_rect(painter, tyre, 1.2 * scale, LINE);
        }
    }
}

fn paint_led_strip(painter: &egui::Painter, rect: Rect, value: f32, scale: f32) {
    let count = 15;
    let gap = 3.0 * scale;
    let width = (rect.width() - gap * (count - 1) as f32) / count as f32;
    let active = (value.clamp(0.0, 1.0) * count as f32).round() as usize;
    for index in 0..count {
        let led = Rect::from_min_size(
            pos2(rect.left() + index as f32 * (width + gap), rect.top()),
            vec2(width, rect.height()),
        );
        painter.rect_filled(led, 0.0, if index < active { TEXT } else { BAR_OFF });
    }
}

fn split_columns(rect: Rect, count: usize, gap: f32) -> Vec<Rect> {
    let width = (rect.width() - gap * (count.saturating_sub(1)) as f32) / count as f32;
    (0..count)
        .map(|index| {
            let left = rect.left() + index as f32 * (width + gap);
            Rect::from_min_size(pos2(left, rect.top()), vec2(width, rect.height()))
        })
        .collect()
}

fn stroke_rect(painter: &egui::Painter, rect: Rect, width: f32, color: Color32) {
    painter.rect_stroke(rect, 0.0, Stroke::new(width, color), StrokeKind::Inside);
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

fn paint_center_message(painter: &egui::Painter, rect: Rect, message: &str, scale: f32) {
    let banner = Rect::from_center_size(rect.center(), vec2(rect.width() * 0.45, 48.0 * scale));
    painter.rect_filled(banner, 0.0, Color32::BLACK);
    stroke_rect(painter, banner, 1.0 * scale, LINE);
    draw_label(
        painter,
        banner.center(),
        Align2::CENTER_CENTER,
        message,
        17.0 * scale,
        TEXT,
    );
}

fn paint_waiting_strip(painter: &egui::Painter, rect: Rect, scale: f32) {
    draw_label(
        painter,
        pos2(rect.center().x, rect.bottom() - 12.0 * scale),
        Align2::CENTER_BOTTOM,
        "WAITING FOR TELEMETRY",
        11.0 * scale,
        TEXT_DIM,
    );
}
