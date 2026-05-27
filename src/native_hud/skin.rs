use eframe::egui::{
    self, Align2, Color32, FontFamily, FontId, Rect, Stroke, StrokeKind, pos2, vec2,
};

use crate::telemetry::{DamageSample, LapSample, SessionSample, StatusSample, TelemetryUpdate};

pub const APP_BG: Color32 = Color32::BLACK;
pub const ACCENT: Color32 = Color32::from_rgb(245, 248, 248);

const SCREEN_BG: Color32 = Color32::BLACK;
const TEXT: Color32 = Color32::from_rgb(244, 247, 247);
const TEXT_DIM: Color32 = Color32::from_rgb(152, 158, 158);
const LINE: Color32 = Color32::from_rgba_premultiplied(244, 247, 247, 190);
const LINE_DIM: Color32 = Color32::from_rgba_premultiplied(244, 247, 247, 92);
const BAR_OFF: Color32 = Color32::from_rgb(20, 22, 22);
const TYRE_WEAR_COLOR_STEPS: usize = 50;

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
    tyres: Rect,
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
            pos2(content.right(), content.top() + content.height() * 0.635),
        );
        let tyres = Rect::from_min_max(
            pos2(content.left(), middle.bottom()),
            pos2(content.right(), content.top() + content.height() * 0.760),
        );
        let lower = Rect::from_min_max(
            pos2(content.left(), tyres.bottom()),
            pos2(content.right(), content.top() + content.height() * 0.910),
        );
        let sectors =
            Rect::from_min_max(pos2(content.left(), lower.bottom()), content.right_bottom());

        Self {
            screen,
            content,
            top,
            middle,
            tyres,
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
    tyre_wear: TyreWearDisplay,
    flag: FlagLight,
    ers_mode: String,
    ers_pct: String,
}

#[derive(Clone, Debug, PartialEq)]
struct TyreWearDisplay {
    fl: TyreWearValue,
    fr: TyreWearValue,
    rl: TyreWearValue,
    rr: TyreWearValue,
}

#[derive(Clone, Debug, PartialEq)]
struct TyreWearValue {
    text: String,
    percent: Option<f32>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FlagLight {
    None,
    Green,
    Blue,
    Yellow,
    Red,
    Oil,
}

impl TelemetryFrame {
    fn from_update(state: &TelemetryUpdate, error: Option<&str>) -> Self {
        let sample = Self::sample(state.is_empty() && error.is_none(), error);
        let input = state.input.as_ref();
        let lap = state.lap.as_ref();
        let damage = state.damage.as_ref();
        let status = state.status.as_ref();
        let session = state.session.as_ref();
        let waiting = state.is_empty() && error.is_none();

        let max_rpm = status.map(|status| status.max_rpm.max(1)).unwrap_or(15_000);
        let rpm = input.map(|input| input.rpm.min(max_rpm));
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
            rpm: rpm.map(|rpm| rpm.to_string()).unwrap_or(sample.rpm),
            rpm_ratio: rpm.map(|rpm| rpm as f32 / max_rpm as f32).unwrap_or(0.0),
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
            tyre_wear: tyre_wear_display(damage),
            flag: flag_light(session, lap, sample.flag),
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
            speed: "--".to_owned(),
            gear: "--".to_owned(),
            rpm: "--".to_owned(),
            rpm_ratio: 0.0,
            drs: false,
            position: "--".to_owned(),
            position_total: "--".to_owned(),
            lap_current: "--".to_owned(),
            lap_total: "--".to_owned(),
            current_lap: "--:--.--".to_owned(),
            last_lap: "--:--.--".to_owned(),
            delta: "--.--".to_owned(),
            front_gap: "--.--".to_owned(),
            behind_gap: "--.--".to_owned(),
            tyre_wear: TyreWearDisplay::placeholder(),
            flag: FlagLight::None,
            ers_mode: "-".to_owned(),
            ers_pct: "--".to_owned(),
        }
    }
}

impl TyreWearDisplay {
    fn placeholder() -> Self {
        Self {
            fl: TyreWearValue::placeholder(),
            fr: TyreWearValue::placeholder(),
            rl: TyreWearValue::placeholder(),
            rr: TyreWearValue::placeholder(),
        }
    }
}

impl TyreWearValue {
    fn placeholder() -> Self {
        Self {
            text: "--".to_owned(),
            percent: None,
        }
    }

    fn from_percent(value: f32) -> Self {
        let percent = value.clamp(0.0, 100.0);
        Self {
            text: format!("{percent:.0}%"),
            percent: Some(percent),
        }
    }
}

impl FlagLight {
    fn is_active(self) -> bool {
        self != Self::None
    }

    fn rgb(self) -> (u8, u8, u8) {
        match self {
            Self::None => (20, 22, 22),
            Self::Green => (22, 210, 70),
            Self::Blue => (45, 128, 255),
            Self::Yellow => (245, 214, 42),
            Self::Red => (236, 38, 38),
            Self::Oil => (255, 136, 28),
        }
    }

    fn color(self) -> Color32 {
        let (red, green, blue) = self.rgb();
        Color32::from_rgb(red, green, blue)
    }

    fn tint_color(self, alpha: u8) -> Color32 {
        let (red, green, blue) = self.rgb();
        let scale = u16::from(alpha);
        Color32::from_rgba_premultiplied(
            (u16::from(red) * scale / 255) as u8,
            (u16::from(green) * scale / 255) as u8,
            (u16::from(blue) * scale / 255) as u8,
            alpha,
        )
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
        paint_flag_wash(painter, layout.screen, frame.flag, layout.scale);
        let painter = painter.with_clip_rect(layout.content);
        paint_top_row(&painter, layout.top, frame, layout.scale);
        paint_middle_grid(&painter, layout.middle, frame, layout.scale);
        paint_tyre_wear_row(&painter, layout.tyres, frame, layout.scale);
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

fn paint_tyre_wear_row(painter: &egui::Painter, rect: Rect, frame: &TelemetryFrame, scale: f32) {
    let cells = split_by_weights(rect, &[0.25, 0.25, 0.25, 0.25]);
    for cell in &cells {
        paint_box(painter, *cell, scale);
    }

    paint_tyre_wear_cell(painter, cells[0], &frame.tyre_wear.fl, "FL", scale);
    paint_tyre_wear_cell(painter, cells[1], &frame.tyre_wear.fr, "FR", scale);
    paint_tyre_wear_cell(painter, cells[2], &frame.tyre_wear.rl, "RL", scale);
    paint_tyre_wear_cell(painter, cells[3], &frame.tyre_wear.rr, "RR", scale);
}

fn paint_tyre_wear_cell(
    painter: &egui::Painter,
    rect: Rect,
    value: &TyreWearValue,
    label: &str,
    scale: f32,
) {
    draw_number(
        painter,
        rect.center() + vec2(0.0, -6.0 * scale),
        Align2::CENTER_CENTER,
        &value.text,
        17.0 * scale,
        tyre_wear_color(value.percent),
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

fn paint_lower_row(painter: &egui::Painter, rect: Rect, frame: &TelemetryFrame, scale: f32) {
    let cells = split_by_weights(rect, &[0.32, 0.36, 0.32]);
    for cell in &cells {
        paint_box(painter, *cell, scale);
    }

    paint_metric(painter, cells[0], &frame.front_gap, "Gap -1", 25.0, scale);
    paint_metric(painter, cells[1], &frame.current_lap, "Curr", 25.0, scale);
    paint_metric(painter, cells[2], &frame.behind_gap, "Gap +1", 25.0, scale);
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

fn paint_flag_wash(painter: &egui::Painter, rect: Rect, flag: FlagLight, scale: f32) {
    if !flag.is_active() {
        return;
    }

    let time = painter.ctx().input(|input| input.time);
    let blink_on = (time * 2.4) as i64 % 2 == 0;
    if !blink_on {
        return;
    }

    painter.rect_filled(rect, 0.0, flag.tint_color(34));
    painter.rect_stroke(
        rect.shrink(2.0 * scale),
        0.0,
        Stroke::new(3.0 * scale, flag.color()),
        StrokeKind::Inside,
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

fn tyre_wear_display(damage: Option<&DamageSample>) -> TyreWearDisplay {
    let Some(damage) = damage else {
        return TyreWearDisplay::placeholder();
    };
    TyreWearDisplay {
        fl: TyreWearValue::from_percent(damage.tyre_wear.fl),
        fr: TyreWearValue::from_percent(damage.tyre_wear.fr),
        rl: TyreWearValue::from_percent(damage.tyre_wear.rl),
        rr: TyreWearValue::from_percent(damage.tyre_wear.rr),
    }
}

fn tyre_wear_color(percent: Option<f32>) -> Color32 {
    let Some(percent) = percent else {
        return TEXT_DIM;
    };
    let percent = quantize_tyre_wear_percent(percent);
    if percent <= 50.0 {
        lerp_color(
            Color32::from_rgb(22, 210, 70),
            Color32::from_rgb(245, 214, 42),
            percent / 50.0,
        )
    } else {
        lerp_color(
            Color32::from_rgb(245, 214, 42),
            Color32::from_rgb(236, 38, 38),
            (percent - 50.0) / 50.0,
        )
    }
}

fn quantize_tyre_wear_percent(percent: f32) -> f32 {
    let max_step = (TYRE_WEAR_COLOR_STEPS - 1) as f32;
    let step = (percent.clamp(0.0, 100.0) / 100.0 * max_step).round();
    step / max_step * 100.0
}

fn lerp_color(from: Color32, to: Color32, amount: f32) -> Color32 {
    let amount = amount.clamp(0.0, 1.0);
    let channel = |start: u8, end: u8| -> u8 {
        (start as f32 + (end as f32 - start as f32) * amount).round() as u8
    };
    Color32::from_rgb(
        channel(from.r(), to.r()),
        channel(from.g(), to.g()),
        channel(from.b(), to.b()),
    )
}

fn flag_light(
    session: Option<&SessionSample>,
    lap: Option<&LapSample>,
    fallback: FlagLight,
) -> FlagLight {
    let Some(session) = session else {
        return fallback;
    };
    let Some(lap) = lap else {
        return fallback;
    };
    if session.marshal_zones.is_empty() || session.track_length_m == 0 {
        return fallback;
    }

    let track_length = session.track_length_m as f32;
    let progress = (lap.lap_distance_m.rem_euclid(track_length) / track_length).clamp(0.0, 1.0);
    let current_zone = session
        .marshal_zones
        .iter()
        .filter(|zone| zone.start <= progress)
        .max_by(|left, right| left.start.total_cmp(&right.start))
        .or_else(|| {
            session
                .marshal_zones
                .iter()
                .max_by(|left, right| left.start.total_cmp(&right.start))
        });

    current_zone
        .map(|zone| marshal_flag_light(zone.flag))
        .unwrap_or(fallback)
}

fn marshal_flag_light(flag: i8) -> FlagLight {
    match flag {
        0 => FlagLight::None,
        1 => FlagLight::Green,
        2 => FlagLight::Blue,
        3 => FlagLight::Yellow,
        4 => FlagLight::Red,
        5 => FlagLight::Oil,
        _ => FlagLight::None,
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
    if minutes == 0 {
        format!("{sign}{seconds:02}.{centis:02}")
    } else {
        format!("{sign}{minutes}:{seconds:02}.{centis:02}")
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::telemetry::{MarshalZoneSample, WheelValuesF32, WheelValuesU8};

    fn lap_at(distance_m: f32) -> LapSample {
        LapSample {
            session_time: 0.0,
            frame_identifier: 0,
            player_car_index: 0,
            last_lap_time_ms: 0,
            current_lap_time_ms: 0,
            lap_distance_m: distance_m,
            total_distance_m: distance_m,
            car_position: 1,
            current_lap_num: 1,
            pit_status: 0,
            sector: 0,
            current_lap_invalid: false,
            driver_status: 0,
            result_status: 0,
            delta_to_car_in_front_ms: None,
            delta_to_car_behind_ms: None,
            delta_to_race_leader_ms: None,
            sector1_time_ms: None,
            sector2_time_ms: None,
        }
    }

    fn session_with_zones() -> SessionSample {
        SessionSample {
            session_time: 0.0,
            frame_identifier: 0,
            total_laps: 30,
            track_length_m: 1000,
            session_type: 0,
            track_id: 0,
            track_temp_c: 0,
            air_temp_c: 0,
            session_time_left_s: 0,
            marshal_zones: vec![
                MarshalZoneSample {
                    start: 0.25,
                    flag: 1,
                },
                MarshalZoneSample {
                    start: 0.50,
                    flag: 3,
                },
                MarshalZoneSample {
                    start: 0.80,
                    flag: 4,
                },
            ],
        }
    }

    fn damage_with_wear(fl: f32, fr: f32, rl: f32, rr: f32) -> DamageSample {
        DamageSample {
            session_time: 0.0,
            frame_identifier: 0,
            player_car_index: 0,
            tyre_wear: WheelValuesF32 { rl, rr, fl, fr },
            tyre_damage: WheelValuesU8 {
                rl: 0,
                rr: 0,
                fl: 0,
                fr: 0,
            },
            tyre_blisters: WheelValuesU8 {
                rl: 0,
                rr: 0,
                fl: 0,
                fr: 0,
            },
            front_left_wing_damage: 0,
            front_right_wing_damage: 0,
            rear_wing_damage: 0,
            gearbox_damage: 0,
            engine_damage: 0,
        }
    }

    #[test]
    fn empty_snapshot_uses_placeholders_not_sample_numbers() {
        let frame = TelemetryFrame::from_update(&TelemetryUpdate::default(), None);

        assert!(frame.waiting);
        assert_eq!(frame.speed, "--");
        assert_eq!(frame.gear, "--");
        assert_eq!(frame.rpm, "--");
        assert_eq!(frame.rpm_ratio, 0.0);
        assert_eq!(frame.current_lap, "--:--.--");
        assert_eq!(frame.last_lap, "--:--.--");
        assert_eq!(frame.front_gap, "--.--");
        assert_eq!(frame.behind_gap, "--.--");
        assert_eq!(frame.tyre_wear.fl.text, "--");
        assert_eq!(frame.tyre_wear.fl.percent, None);
        assert_eq!(frame.flag, FlagLight::None);
    }

    #[test]
    fn displays_tyre_wear_from_damage_packet() {
        let update = TelemetryUpdate {
            damage: Some(damage_with_wear(12.0, 34.0, 56.0, 78.0)),
            ..TelemetryUpdate::default()
        };
        let frame = TelemetryFrame::from_update(&update, None);

        assert_eq!(frame.tyre_wear.fl.text, "12%");
        assert_eq!(frame.tyre_wear.fr.text, "34%");
        assert_eq!(frame.tyre_wear.rl.text, "56%");
        assert_eq!(frame.tyre_wear.rr.text, "78%");
    }

    #[test]
    fn tyre_wear_color_moves_from_green_to_red() {
        assert_eq!(tyre_wear_color(Some(0.0)), Color32::from_rgb(22, 210, 70));
        assert_ne!(tyre_wear_color(Some(50.0)), Color32::from_rgb(22, 210, 70));
        assert_ne!(tyre_wear_color(Some(50.0)), Color32::from_rgb(236, 38, 38));
        assert_eq!(tyre_wear_color(Some(100.0)), Color32::from_rgb(236, 38, 38));
        assert_eq!(tyre_wear_color(None), TEXT_DIM);
    }

    #[test]
    fn tyre_wear_color_is_split_into_50_steps() {
        let mut colors = Vec::new();
        for step in 0..TYRE_WEAR_COLOR_STEPS {
            let percent = step as f32 * 100.0 / (TYRE_WEAR_COLOR_STEPS - 1) as f32;
            let color = tyre_wear_color(Some(percent));
            colors.push((color.r(), color.g(), color.b()));
        }

        assert_eq!(colors.len(), TYRE_WEAR_COLOR_STEPS);
        for pair in colors.windows(2) {
            assert_ne!(pair[0], pair[1]);
        }
    }

    #[test]
    fn formats_gap_as_seconds_until_one_minute() {
        assert_eq!(format_gap_ms(3_040, '+'), "+03.04");
        assert_eq!(format_gap_ms(60_450, '-'), "-1:00.45");
    }

    #[test]
    fn reads_current_marshal_zone_flag_for_lap_position() {
        let session = session_with_zones();

        assert_eq!(
            flag_light(Some(&session), Some(&lap_at(600.0)), FlagLight::None),
            FlagLight::Yellow
        );
        assert_eq!(
            flag_light(Some(&session), Some(&lap_at(100.0)), FlagLight::None),
            FlagLight::Red
        );
    }

    #[test]
    fn maps_supported_flag_lights() {
        assert_eq!(marshal_flag_light(1), FlagLight::Green);
        assert_eq!(marshal_flag_light(3), FlagLight::Yellow);
        assert_eq!(marshal_flag_light(4), FlagLight::Red);
        assert_eq!(marshal_flag_light(5), FlagLight::Oil);
    }
}
