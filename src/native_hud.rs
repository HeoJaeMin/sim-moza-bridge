use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use eframe::egui;

use crate::config::BridgeConfig;
use crate::hud::{HudHandle, new_hud_handle};
use crate::telemetry::{
    DamageSample, InputSample, LapSample, SessionSample, StatusSample, WheelValuesF32,
    WheelValuesU8, WheelValuesU16,
};

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
            .with_inner_size([430.0, 760.0])
            .with_min_inner_size([360.0, 560.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Sim MOZA Bridge",
        options,
        Box::new(move |_cc| Ok(Box::new(NativeHudApp { hud, runtime_error }))),
    )
    .map_err(|error| format!("native HUD failed: {error}"))
}

struct NativeHudApp {
    hud: HudHandle,
    runtime_error: Arc<Mutex<Option<String>>>,
}

impl eframe::App for NativeHudApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        ui.ctx().request_repaint_after(Duration::from_millis(50));

        let state = self.hud.snapshot();
        ui.add_space(8.0);
        ui.heading("Sim MOZA Bridge");

        if let Some(error) = self.runtime_error.lock().ok().and_then(|slot| slot.clone()) {
            ui.colored_label(egui::Color32::RED, error);
        } else if state.is_empty() {
            ui.label("Waiting for telemetry...");
        }

        ui.separator();
        draw_primary(ui, state.input.as_ref(), state.status.as_ref());
        ui.separator();
        draw_inputs(ui, state.input.as_ref());
        ui.separator();
        draw_lap(ui, state.lap.as_ref(), state.session.as_ref());
        ui.separator();
        draw_tyres(ui, state.input.as_ref(), state.damage.as_ref());
        ui.separator();
        draw_status(ui, state.status.as_ref());
        ui.separator();
        draw_damage(ui, state.damage.as_ref());
    }
}

fn draw_primary(ui: &mut egui::Ui, input: Option<&InputSample>, status: Option<&StatusSample>) {
    ui.horizontal(|ui| {
        metric(
            ui,
            "Speed",
            input.map(|value| format!("{} km/h", value.speed_kmh)),
        );
        metric(ui, "Gear", input.map(|value| gear_label(value.gear)));
        metric(ui, "RPM", input.map(|value| value.rpm.to_string()));
    });

    ui.horizontal(|ui| {
        metric(
            ui,
            "ERS",
            status.map(|value| format!("{:.0}%", value.ers_percent())),
        );
        metric(
            ui,
            "Fuel",
            status.map(|value| format!("{:.1} / {:.1}", value.fuel_in_tank, value.fuel_capacity)),
        );
        metric(
            ui,
            "DRS",
            input.map(|value| {
                if value.drs {
                    "Open".to_owned()
                } else {
                    "Closed".to_owned()
                }
            }),
        );
    });
}

fn draw_inputs(ui: &mut egui::Ui, input: Option<&InputSample>) {
    ui.label("Inputs");
    if let Some(input) = input {
        progress(ui, "Throttle", input.throttle);
        progress(ui, "Brake", input.brake);
        progress(ui, "Clutch", f32::from(input.clutch) / 100.0);
        signed_progress(ui, "Steer", input.steer);
        progress(
            ui,
            "Rev lights",
            f32::from(input.rev_lights_percent) / 100.0,
        );
    } else {
        ui.label("No input packet yet");
    }
}

fn draw_lap(ui: &mut egui::Ui, lap: Option<&LapSample>, session: Option<&SessionSample>) {
    ui.label("Lap");
    ui.horizontal(|ui| {
        metric(
            ui,
            "Lap",
            lap.map(|value| value.current_lap_num.to_string()),
        );
        metric(
            ui,
            "Position",
            lap.map(|value| value.car_position.to_string()),
        );
        metric(
            ui,
            "Current",
            lap.map(|value| format_ms(value.current_lap_time_ms)),
        );
    });
    ui.horizontal(|ui| {
        metric(
            ui,
            "Front",
            lap.and_then(|value| value.delta_to_car_in_front_ms)
                .map(format_ms),
        );
        metric(
            ui,
            "Behind",
            lap.and_then(|value| value.delta_to_car_behind_ms)
                .map(format_ms),
        );
        metric(
            ui,
            "Leader",
            lap.and_then(|value| value.delta_to_race_leader_ms)
                .map(format_ms),
        );
    });
    ui.horizontal(|ui| {
        metric(
            ui,
            "Track",
            session.map(|value| format!("{} m", value.track_length_m)),
        );
        metric(
            ui,
            "Air",
            session.map(|value| format!("{} C", value.air_temp_c)),
        );
        metric(
            ui,
            "Track temp",
            session.map(|value| format!("{} C", value.track_temp_c)),
        );
    });
}

fn draw_tyres(ui: &mut egui::Ui, input: Option<&InputSample>, damage: Option<&DamageSample>) {
    ui.label("Tyres");
    egui::Grid::new("tyre-grid")
        .num_columns(5)
        .spacing([14.0, 6.0])
        .show(ui, |ui| {
            ui.label("");
            ui.label("FL");
            ui.label("FR");
            ui.label("RL");
            ui.label("RR");
            ui.end_row();

            wheel_f32_row(ui, "Wear", damage.map(|value| &value.tyre_wear), "%");
            wheel_u8_row(
                ui,
                "Surface",
                input.map(|value| &value.tyre_surface_temps_c),
                " C",
            );
            wheel_u8_row(
                ui,
                "Inner",
                input.map(|value| &value.tyre_inner_temps_c),
                " C",
            );
            wheel_f32_row(
                ui,
                "Pressure",
                input.map(|value| &value.tyre_pressures_psi),
                " psi",
            );
            wheel_u16_row(ui, "Brake", input.map(|value| &value.brake_temps_c), " C");
        });
}

fn draw_status(ui: &mut egui::Ui, status: Option<&StatusSample>) {
    ui.label("Status");
    ui.horizontal(|ui| {
        metric(
            ui,
            "TC",
            status.map(|value| value.traction_control.to_string()),
        );
        metric(
            ui,
            "ABS",
            status.map(|value| value.anti_lock_brakes.to_string()),
        );
        metric(
            ui,
            "Brake bias",
            status.map(|value| format!("{}%", value.front_brake_bias)),
        );
    });
    ui.horizontal(|ui| {
        metric(
            ui,
            "Tyre age",
            status.map(|value| format!("{} laps", value.tyres_age_laps)),
        );
        metric(
            ui,
            "Pit limiter",
            status.map(|value| {
                if value.pit_limiter_active {
                    "On".to_owned()
                } else {
                    "Off".to_owned()
                }
            }),
        );
        metric(
            ui,
            "ERS mode",
            status.map(|value| value.ers_deploy_mode.to_string()),
        );
    });
}

fn draw_damage(ui: &mut egui::Ui, damage: Option<&DamageSample>) {
    ui.label("Damage");
    ui.horizontal(|ui| {
        metric(
            ui,
            "Wing FL",
            damage.map(|value| format!("{}%", value.front_left_wing_damage)),
        );
        metric(
            ui,
            "Wing FR",
            damage.map(|value| format!("{}%", value.front_right_wing_damage)),
        );
        metric(
            ui,
            "Rear wing",
            damage.map(|value| format!("{}%", value.rear_wing_damage)),
        );
    });
    ui.horizontal(|ui| {
        metric(
            ui,
            "Gearbox",
            damage.map(|value| format!("{}%", value.gearbox_damage)),
        );
        metric(
            ui,
            "Engine",
            damage.map(|value| format!("{}%", value.engine_damage)),
        );
    });
}

fn metric(ui: &mut egui::Ui, label: &str, value: Option<String>) {
    ui.vertical(|ui| {
        ui.small(label);
        ui.strong(value.unwrap_or_else(|| "--".to_owned()));
    });
}

fn progress(ui: &mut egui::Ui, label: &str, value: f32) {
    let clamped = value.clamp(0.0, 1.0);
    ui.add(
        egui::ProgressBar::new(clamped)
            .desired_width(ui.available_width())
            .text(format!("{label}: {:.0}%", clamped * 100.0)),
    );
}

fn signed_progress(ui: &mut egui::Ui, label: &str, value: f32) {
    let normalized = ((value.clamp(-1.0, 1.0) + 1.0) / 2.0).clamp(0.0, 1.0);
    ui.add(
        egui::ProgressBar::new(normalized)
            .desired_width(ui.available_width())
            .text(format!("{label}: {:.0}%", value * 100.0)),
    );
}

fn wheel_f32_row(ui: &mut egui::Ui, label: &str, values: Option<&WheelValuesF32>, suffix: &str) {
    ui.label(label);
    if let Some(values) = values {
        ui.label(format!("{:.1}{suffix}", values.fl));
        ui.label(format!("{:.1}{suffix}", values.fr));
        ui.label(format!("{:.1}{suffix}", values.rl));
        ui.label(format!("{:.1}{suffix}", values.rr));
    } else {
        empty_wheel_row(ui);
    }
    ui.end_row();
}

fn wheel_u8_row(ui: &mut egui::Ui, label: &str, values: Option<&WheelValuesU8>, suffix: &str) {
    ui.label(label);
    if let Some(values) = values {
        ui.label(format!("{}{suffix}", values.fl));
        ui.label(format!("{}{suffix}", values.fr));
        ui.label(format!("{}{suffix}", values.rl));
        ui.label(format!("{}{suffix}", values.rr));
    } else {
        empty_wheel_row(ui);
    }
    ui.end_row();
}

fn wheel_u16_row(ui: &mut egui::Ui, label: &str, values: Option<&WheelValuesU16>, suffix: &str) {
    ui.label(label);
    if let Some(values) = values {
        ui.label(format!("{}{suffix}", values.fl));
        ui.label(format!("{}{suffix}", values.fr));
        ui.label(format!("{}{suffix}", values.rl));
        ui.label(format!("{}{suffix}", values.rr));
    } else {
        empty_wheel_row(ui);
    }
    ui.end_row();
}

fn empty_wheel_row(ui: &mut egui::Ui) {
    ui.label("--");
    ui.label("--");
    ui.label("--");
    ui.label("--");
}

fn gear_label(gear: i8) -> String {
    match gear {
        -1 => "R".to_owned(),
        0 => "N".to_owned(),
        value => value.to_string(),
    }
}

fn format_ms(value: u32) -> String {
    let minutes = value / 60_000;
    let seconds = (value % 60_000) / 1_000;
    let millis = value % 1_000;
    format!("{minutes}:{seconds:02}.{millis:03}")
}
