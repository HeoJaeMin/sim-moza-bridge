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
const SCREEN_BG: Color32 = Color32::from_rgb(7, 10, 13);
const LINE: Color32 = Color32::from_rgb(42, 54, 64);
const LINE_HOT: Color32 = Color32::from_rgb(180, 98, 12);
const TEXT: Color32 = Color32::from_rgb(238, 242, 244);
const MUTED: Color32 = Color32::from_rgb(142, 152, 160);
const ORANGE: Color32 = Color32::from_rgb(255, 149, 18);
const GREEN: Color32 = Color32::from_rgb(72, 241, 77);
const BLUE: Color32 = Color32::from_rgb(28, 164, 255);
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
            .with_inner_size([1080.0, 640.0])
            .with_min_inner_size([860.0, 500.0]),
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

    let screen = fit_aspect(rect.shrink2(vec2(34.0, 30.0)), 16.0 / 9.0);
    let glow = screen.expand(10.0);
    painter.rect_stroke(
        glow,
        18.0,
        Stroke::new(4.0, Color32::from_rgba_premultiplied(255, 124, 0, 70)),
        StrokeKind::Inside,
    );
    painter.rect_filled(screen, 18.0, Color32::from_rgb(18, 22, 23));
    painter.rect_stroke(
        screen,
        18.0,
        Stroke::new(3.0, Color32::from_rgb(60, 67, 68)),
        StrokeKind::Inside,
    );

    let bezel = screen.shrink2(vec2(28.0, 24.0));
    painter.rect_filled(bezel, 10.0, Color32::from_rgb(1, 2, 4));
    painter.rect_stroke(
        bezel,
        10.0,
        Stroke::new(2.0, Color32::from_rgb(33, 38, 42)),
        StrokeKind::Inside,
    );

    let lcd = bezel.shrink2(vec2(24.0, 22.0));
    painter.rect_filled(lcd, 4.0, SCREEN_BG);
    painter.rect_stroke(
        lcd,
        4.0,
        Stroke::new(1.0, Color32::from_rgb(35, 44, 50)),
        StrokeKind::Inside,
    );

    draw_rev_lights(painter, screen, state.input.as_ref());
    draw_lcd_contents(painter, lcd, state, error);
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

    let top_h = rect.height() * 0.20;
    let mid_h = rect.height() * 0.38;

    let top = Rect::from_min_max(rect.left_top(), pos2(rect.right(), rect.top() + top_h));
    let mid = Rect::from_min_max(
        pos2(rect.left(), top.bottom()),
        pos2(rect.right(), top.bottom() + mid_h),
    );
    let bottom = Rect::from_min_max(pos2(rect.left(), mid.bottom()), rect.right_bottom());

    draw_top_status(painter, top, state);
    draw_rpm_arc(painter, mid, input, status);
    draw_speed_panel(painter, mid, input);
    draw_gear_panel(painter, mid, input);
    draw_delta_panel(painter, mid, state);
    draw_energy_row(painter, bottom, status);
    draw_tyres_panel(painter, bottom, input, damage);
    draw_input_row(painter, bottom, input);

    if let Some(error) = error {
        draw_center_message(painter, rect, error, RED);
    } else if state.is_empty() {
        draw_center_message(painter, rect, "WAITING FOR TELEMETRY", MUTED);
    }
}

fn draw_top_status(painter: &egui::Painter, rect: Rect, state: &TelemetryUpdate) {
    let lap = state.lap.as_ref();
    let session = state.session.as_ref();

    text(
        painter,
        pos2(rect.left() + 16.0, rect.top() + 18.0),
        Align2::LEFT_CENTER,
        "LAP",
        26.0,
        TEXT,
    );
    let lap_value = match (lap, session) {
        (Some(lap), Some(session)) => {
            format!("{}/{}", lap.current_lap_num, session.total_laps.max(1))
        }
        (Some(lap), None) => format!("{}/--", lap.current_lap_num),
        _ => "--/--".to_owned(),
    };
    text(
        painter,
        pos2(rect.left() + 16.0, rect.top() + 53.0),
        Align2::LEFT_CENTER,
        &lap_value,
        32.0,
        TEXT,
    );
    painter.line_segment(
        [
            pos2(rect.left() + 14.0, rect.bottom() - 6.0),
            pos2(rect.left() + 140.0, rect.bottom() - 6.0),
        ],
        Stroke::new(2.0, LINE),
    );

    let current = lap
        .map(|lap| format_ms(lap.current_lap_time_ms))
        .unwrap_or_else(|| "--:--.---".to_owned());
    let best = lap
        .map(|lap| format_ms(lap.last_lap_time_ms))
        .unwrap_or_else(|| "--:--.---".to_owned());
    text(
        painter,
        pos2(rect.right() - 16.0, rect.top() + 24.0),
        Align2::RIGHT_CENTER,
        &current,
        32.0,
        TEXT,
    );
    text(
        painter,
        pos2(rect.right() - 16.0, rect.top() + 58.0),
        Align2::RIGHT_CENTER,
        &format!("BEST {best}"),
        16.0,
        TEXT,
    );
}

fn draw_rpm_arc(
    painter: &egui::Painter,
    rect: Rect,
    input: Option<&InputSample>,
    status: Option<&StatusSample>,
) {
    let center = pos2(rect.center().x, rect.bottom() + rect.height() * 0.56);
    let radius = rect.width() * 0.40;
    let start = std::f32::consts::PI * 1.18;
    let end = std::f32::consts::PI * 1.82;
    let max_rpm = status.map(|status| status.max_rpm.max(1)).unwrap_or(15_000);
    let rpm = input.map(|input| input.rpm).unwrap_or(0).min(max_rpm);
    let rpm_ratio = rpm as f32 / max_rpm as f32;

    draw_arc(
        painter,
        center,
        radius,
        start,
        end,
        Stroke::new(5.0, Color32::from_rgb(150, 160, 164)),
    );
    draw_arc(
        painter,
        center,
        radius,
        start + (end - start) * 0.70,
        end,
        Stroke::new(5.0, RED),
    );
    draw_arc(
        painter,
        center,
        radius,
        start,
        start + (end - start) * rpm_ratio,
        Stroke::new(6.0, ORANGE),
    );

    for tick in 4..=15 {
        let t = (tick - 4) as f32 / 11.0;
        let angle = start + (end - start) * t;
        let outer = polar(center, radius + 8.0, angle);
        let inner = polar(
            center,
            radius - if tick % 2 == 0 { 17.0 } else { 11.0 },
            angle,
        );
        painter.line_segment([inner, outer], Stroke::new(1.5, TEXT));
        if tick == 4
            || tick == 6
            || tick == 8
            || tick == 10
            || tick == 12
            || tick == 14
            || tick == 15
        {
            let label = polar(center, radius - 42.0, angle);
            text(
                painter,
                label,
                Align2::CENTER_CENTER,
                &tick.to_string(),
                18.0,
                TEXT,
            );
        }
    }

    let needle = polar(center, radius - 4.0, start + (end - start) * rpm_ratio);
    painter.line_segment([center, needle], Stroke::new(3.0, ORANGE));
    painter.circle_filled(needle, 8.0, ORANGE);
    text(
        painter,
        pos2(rect.center().x, rect.top() + 64.0),
        Align2::CENTER_CENTER,
        "RPM x1000",
        15.0,
        TEXT,
    );
}

fn draw_speed_panel(painter: &egui::Painter, rect: Rect, input: Option<&InputSample>) {
    let panel = Rect::from_min_size(
        pos2(rect.left() + 12.0, rect.top() + rect.height() * 0.46),
        vec2(rect.width() * 0.26, rect.height() * 0.42),
    );
    angled_panel(painter, panel, LINE_HOT);

    let speed = input
        .map(|input| input.speed_kmh.to_string())
        .unwrap_or_else(|| "--".to_owned());
    text(
        painter,
        pos2(panel.left() + 30.0, panel.center().y - 4.0),
        Align2::LEFT_CENTER,
        &speed,
        60.0,
        TEXT,
    );
    text(
        painter,
        pos2(panel.left() + 34.0, panel.center().y + 42.0),
        Align2::LEFT_CENTER,
        "KPH",
        22.0,
        TEXT,
    );
}

fn draw_gear_panel(painter: &egui::Painter, rect: Rect, input: Option<&InputSample>) {
    let gear = input
        .map(|input| gear_label(input.gear))
        .unwrap_or_else(|| "-".to_owned());
    text(
        painter,
        pos2(rect.center().x, rect.top() + rect.height() * 0.60),
        Align2::CENTER_CENTER,
        &gear,
        148.0,
        ORANGE,
    );
    let rpm = input
        .map(|input| input.rpm.to_string())
        .unwrap_or_else(|| "----".to_owned());
    text(
        painter,
        pos2(rect.center().x, rect.bottom() - 14.0),
        Align2::CENTER_CENTER,
        &format!("RPM {rpm}"),
        28.0,
        TEXT,
    );
}

fn draw_delta_panel(painter: &egui::Painter, rect: Rect, state: &TelemetryUpdate) {
    let panel = Rect::from_min_size(
        pos2(
            rect.right() - rect.width() * 0.27 - 12.0,
            rect.top() + rect.height() * 0.46,
        ),
        vec2(rect.width() * 0.26, rect.height() * 0.42),
    );
    angled_panel(painter, panel, LINE_HOT);

    let delta = state
        .lap
        .as_ref()
        .and_then(|lap| lap.delta_to_car_in_front_ms)
        .map(format_delta_ms)
        .unwrap_or_else(|| "--.---".to_owned());
    text(
        painter,
        pos2(panel.center().x, panel.center().y - 8.0),
        Align2::CENTER_CENTER,
        &delta,
        42.0,
        GREEN,
    );
    text(
        painter,
        pos2(panel.center().x, panel.center().y + 35.0),
        Align2::CENTER_CENTER,
        "FRONT",
        21.0,
        GREEN,
    );
}

fn draw_energy_row(painter: &egui::Painter, rect: Rect, status: Option<&StatusSample>) {
    let top = rect.top() + 6.0;
    let battery = status.map(|status| status.ers_percent()).unwrap_or(0.0);
    text(
        painter,
        pos2(rect.left() + 14.0, top + 12.0),
        Align2::LEFT_CENTER,
        "BATTERY",
        20.0,
        ORANGE,
    );
    text(
        painter,
        pos2(rect.left() + 14.0, top + 48.0),
        Align2::LEFT_CENTER,
        &format!("{battery:.0}%"),
        31.0,
        TEXT,
    );
    draw_segment_bar(
        painter,
        Rect::from_min_size(pos2(rect.left() + 104.0, top + 32.0), vec2(178.0, 24.0)),
        battery / 100.0,
        12,
        ORANGE,
    );

    let deployed = status
        .map(|status| status.ers_deployed_this_lap / 4_000_000.0)
        .unwrap_or(0.0)
        .clamp(0.0, 1.0);
    text(
        painter,
        pos2(rect.right() - 240.0, top + 18.0),
        Align2::LEFT_CENTER,
        "ERS",
        20.0,
        ORANGE,
    );
    text(
        painter,
        pos2(rect.right() - 240.0, top + 52.0),
        Align2::LEFT_CENTER,
        &format!("{:+.1}", deployed * 4.0),
        31.0,
        BLUE,
    );
    draw_segment_bar(
        painter,
        Rect::from_min_size(pos2(rect.right() - 124.0, top + 35.0), vec2(104.0, 17.0)),
        deployed,
        8,
        BLUE,
    );
}

fn draw_tyres_panel(
    painter: &egui::Painter,
    rect: Rect,
    input: Option<&InputSample>,
    damage: Option<&DamageSample>,
) {
    let center = pos2(rect.center().x, rect.top() + rect.height() * 0.50);
    text(
        painter,
        pos2(center.x, rect.top() + 26.0),
        Align2::CENTER_CENTER,
        "TYRES",
        20.0,
        TEXT,
    );
    draw_mini_car(
        painter,
        Rect::from_center_size(center + vec2(0.0, 26.0), vec2(58.0, 82.0)),
    );

    let temps = input.map(|input| input.tyre_surface_temps_c);
    let wear = damage.map(|damage| damage.tyre_wear);
    tyre_metric(
        painter,
        pos2(center.x - 120.0, center.y - 4.0),
        temps.map(|temps| temps.fl),
        wear.map(|wear| wear.fl),
        Align2::RIGHT_CENTER,
    );
    tyre_metric(
        painter,
        pos2(center.x + 120.0, center.y - 4.0),
        temps.map(|temps| temps.fr),
        wear.map(|wear| wear.fr),
        Align2::LEFT_CENTER,
    );
    tyre_metric(
        painter,
        pos2(center.x - 120.0, center.y + 58.0),
        temps.map(|temps| temps.rl),
        wear.map(|wear| wear.rl),
        Align2::RIGHT_CENTER,
    );
    tyre_metric(
        painter,
        pos2(center.x + 120.0, center.y + 58.0),
        temps.map(|temps| temps.rr),
        wear.map(|wear| wear.rr),
        Align2::LEFT_CENTER,
    );
}

fn draw_input_row(painter: &egui::Painter, rect: Rect, input: Option<&InputSample>) {
    let bottom = rect.bottom() - 24.0;
    let left = rect.left() + 14.0;
    let right = rect.right() - 14.0;
    input_bar(
        painter,
        Rect::from_min_size(pos2(left, bottom - 28.0), vec2(185.0, 12.0)),
        "THR",
        input.map(|input| input.throttle).unwrap_or(0.0),
        GREEN,
    );
    input_bar(
        painter,
        Rect::from_min_size(pos2(left + 230.0, bottom - 28.0), vec2(185.0, 12.0)),
        "BRK",
        input.map(|input| input.brake).unwrap_or(0.0),
        RED,
    );
    input_bar(
        painter,
        Rect::from_min_size(pos2(right - 185.0, bottom - 28.0), vec2(185.0, 12.0)),
        "REV",
        input
            .map(|input| f32::from(input.rev_lights_percent) / 100.0)
            .unwrap_or(0.0),
        ORANGE,
    );
}

fn draw_rev_lights(painter: &egui::Painter, screen: Rect, input: Option<&InputSample>) {
    let count = 18;
    let radius = (screen.width() * 0.010).clamp(7.0, 11.0);
    let gap = radius * 2.4;
    let total_w = gap * (count - 1) as f32;
    let start_x = screen.center().x - total_w / 2.0;
    let y = screen.top() + 48.0;
    let active = input
        .map(|input| {
            ((f32::from(input.rev_lights_percent) / 100.0) * count as f32).round() as usize
        })
        .unwrap_or(0);

    for index in 0..count {
        let color = if index < 6 {
            GREEN
        } else if index < 13 {
            RED
        } else {
            BLUE
        };
        let fill = if index < active {
            color
        } else {
            Color32::from_rgb(28, 32, 34)
        };
        let center = pos2(start_x + gap * index as f32, y);
        painter.circle_filled(center, radius + 4.0, Color32::from_rgb(4, 5, 6));
        painter.circle_filled(center, radius, fill);
        if index < active {
            painter.circle_stroke(center, radius + 3.0, Stroke::new(2.0, color));
        }
    }
}

fn draw_center_message(painter: &egui::Painter, rect: Rect, message: &str, color: Color32) {
    let banner = Rect::from_center_size(rect.center(), vec2(rect.width() * 0.62, 54.0));
    painter.rect_filled(banner, 5.0, Color32::from_rgba_premultiplied(0, 0, 0, 210));
    painter.rect_stroke(banner, 5.0, Stroke::new(1.0, LINE), StrokeKind::Inside);
    text(
        painter,
        banner.center(),
        Align2::CENTER_CENTER,
        message,
        20.0,
        color,
    );
}

fn angled_panel(painter: &egui::Painter, rect: Rect, color: Color32) {
    let notch = rect.width() * 0.12;
    let points = vec![
        rect.left_top(),
        pos2(rect.right(), rect.top()),
        rect.right_bottom(),
        pos2(rect.left() + notch, rect.bottom()),
        rect.left_top(),
    ];
    painter.add(egui::Shape::closed_line(points, Stroke::new(2.0, color)));
    painter.rect_filled(
        rect.shrink(2.0),
        2.0,
        Color32::from_rgba_premultiplied(8, 12, 15, 150),
    );
}

fn draw_segment_bar(
    painter: &egui::Painter,
    rect: Rect,
    value: f32,
    segments: usize,
    color: Color32,
) {
    let gap = 4.0;
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
        pos2(body.left() + 2.0, body.top() + 24.0),
        pos2(body.right() - 2.0, body.top() + 24.0),
    ];
    painter.add(egui::Shape::convex_polygon(
        nose.to_vec(),
        Color32::from_rgb(115, 124, 126),
        Stroke::new(1.0, TEXT),
    ));
    painter.rect_stroke(body, 3.0, Stroke::new(1.0, TEXT), StrokeKind::Inside);
    painter.rect_stroke(
        Rect::from_center_size(
            pos2(center.x, rect.bottom() - 8.0),
            vec2(rect.width() * 0.92, 8.0),
        ),
        1.0,
        Stroke::new(1.0, TEXT),
        StrokeKind::Inside,
    );

    for side in [-1.0, 1.0] {
        let x = center.x + side * rect.width() * 0.34;
        painter.rect_filled(
            Rect::from_center_size(pos2(x, center.y - 17.0), vec2(9.0, 24.0)),
            2.0,
            Color32::from_rgb(170, 180, 184),
        );
        painter.rect_filled(
            Rect::from_center_size(pos2(x, center.y + 23.0), vec2(9.0, 24.0)),
            2.0,
            Color32::from_rgb(170, 180, 184),
        );
    }
}

fn tyre_metric(
    painter: &egui::Painter,
    pos: Pos2,
    temp: Option<u8>,
    wear: Option<f32>,
    align: Align2,
) {
    let temp_text = temp
        .map(|temp| format!("{temp}C"))
        .unwrap_or_else(|| "--C".to_owned());
    text(painter, pos, align, &temp_text, 24.0, TEXT);

    let bar_dir = if matches!(align, Align2::RIGHT_CENTER) {
        -1.0
    } else {
        1.0
    };
    let bar_origin = pos + vec2(12.0 * bar_dir, -11.0);
    let bar = if bar_dir > 0.0 {
        Rect::from_min_size(bar_origin, vec2(42.0, 22.0))
    } else {
        Rect::from_min_size(bar_origin - vec2(42.0, 0.0), vec2(42.0, 22.0))
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

fn input_bar(painter: &egui::Painter, rect: Rect, label: &str, value: f32, color: Color32) {
    text(
        painter,
        pos2(rect.left(), rect.top() - 12.0),
        Align2::LEFT_CENTER,
        label,
        12.0,
        MUTED,
    );
    painter.rect_filled(rect, 1.0, Color32::from_rgb(40, 47, 52));
    painter.rect_filled(
        Rect::from_min_size(
            rect.left_top(),
            vec2(rect.width() * value.clamp(0.0, 1.0), rect.height()),
        ),
        1.0,
        color,
    );
}

fn draw_arc(
    painter: &egui::Painter,
    center: Pos2,
    radius: f32,
    start: f32,
    end: f32,
    stroke: Stroke,
) {
    let steps = 48;
    let mut previous = polar(center, radius, start);
    for index in 1..=steps {
        let t = index as f32 / steps as f32;
        let next = polar(center, radius, start + (end - start) * t);
        painter.line_segment([previous, next], stroke);
        previous = next;
    }
}

fn polar(center: Pos2, radius: f32, angle: f32) -> Pos2 {
    pos2(
        center.x + angle.cos() * radius,
        center.y + angle.sin() * radius,
    )
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
