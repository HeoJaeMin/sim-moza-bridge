mod skin;

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use eframe::egui;

use crate::config::BridgeConfig;
use crate::hud::{HudHandle, new_hud_handle};
use skin::HudRenderer;

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
            .with_inner_size([1280.0, 720.0])
            .with_min_inner_size([900.0, 520.0]),
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
                renderer: HudRenderer::default(),
            }))
        }),
    )
    .map_err(|error| format!("native HUD failed: {error}"))
}

struct NativeHudApp {
    hud: HudHandle,
    runtime_error: Arc<Mutex<Option<String>>>,
    renderer: HudRenderer,
}

impl eframe::App for NativeHudApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        ui.ctx().request_repaint_after(Duration::from_millis(40));

        let state = self.hud.snapshot();
        let error = self.runtime_error.lock().ok().and_then(|slot| slot.clone());
        let rect = ui.max_rect();
        let painter = ui.painter_at(rect);

        self.renderer
            .paint(&painter, rect, &state, error.as_deref());
    }
}

fn configure_style(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = skin::APP_BG;
    visuals.window_fill = skin::APP_BG;
    visuals.extreme_bg_color = skin::APP_BG;
    visuals.selection.bg_fill = skin::ACCENT;
    ctx.set_visuals(visuals);
}
