pub mod ace;
pub mod lmu;

#[cfg(windows)]
mod shared_memory;

#[cfg(windows)]
use std::thread;
#[cfg(windows)]
use std::time::{Duration, Instant};

#[cfg(windows)]
use crate::config::BridgeConfig;
#[cfg(windows)]
use crate::hud::HudHandle;
#[cfg(windows)]
use crate::logging::{TelemetryRecorder, print_enabled_outputs};
#[cfg(windows)]
use crate::telemetry::TelemetryUpdate;

#[cfg(windows)]
const POLL_INTERVAL: Duration = Duration::from_millis(50);
#[cfg(windows)]
const WARNING_INTERVAL: Duration = Duration::from_secs(2);

#[cfg(windows)]
#[cfg_attr(windows, allow(dead_code))]
fn run_shared_memory_adapter<F>(
    config: BridgeConfig,
    adapter_name: &str,
    mapping_name: &str,
    snapshot_size: usize,
    parse_update: F,
) -> Result<(), String>
where
    F: FnMut(&[u8], u32) -> Result<Option<TelemetryUpdate>, String>,
{
    run_shared_memory_adapter_loop(
        config,
        adapter_name,
        mapping_name,
        snapshot_size,
        parse_update,
        None,
    )
}

#[cfg(windows)]
fn run_shared_memory_adapter_with_hud<F>(
    config: BridgeConfig,
    adapter_name: &str,
    mapping_name: &str,
    snapshot_size: usize,
    parse_update: F,
    hud: Option<HudHandle>,
) -> Result<(), String>
where
    F: FnMut(&[u8], u32) -> Result<Option<TelemetryUpdate>, String>,
{
    run_shared_memory_adapter_loop(
        config,
        adapter_name,
        mapping_name,
        snapshot_size,
        parse_update,
        hud,
    )
}

#[cfg(windows)]
fn run_shared_memory_adapter_loop<F>(
    config: BridgeConfig,
    adapter_name: &str,
    mapping_name: &str,
    snapshot_size: usize,
    mut parse_update: F,
    hud: Option<HudHandle>,
) -> Result<(), String>
where
    F: FnMut(&[u8], u32) -> Result<Option<TelemetryUpdate>, String>,
{
    let mut recorder = TelemetryRecorder::open(&config)?;
    let mut frame_identifier = 0_u32;
    let mut warned = false;
    let mut last_warning = Instant::now();

    println!("{adapter_name} adapter reading {mapping_name}");
    println!("game={} ({})", config.game.id, config.game.name);
    println!("debug={}", config.debug);
    print_enabled_outputs(&config);
    print_optional_hud(hud.is_some());

    loop {
        match shared_memory::read_mapping(mapping_name, snapshot_size) {
            Ok(snapshot) => {
                warned = false;
                frame_identifier = frame_identifier.wrapping_add(1);
                if let Some(update) = parse_update(&snapshot, frame_identifier)? {
                    if let Some(hud) = &hud {
                        hud.update(&update);
                    }
                    recorder.ingest(&update, config.debug);
                }
            }
            Err(error) => {
                if !warned || last_warning.elapsed() >= WARNING_INTERVAL {
                    eprintln!("[adapter-warning] {error}; waiting for {mapping_name}");
                    warned = true;
                    last_warning = Instant::now();
                }
            }
        }

        thread::sleep(POLL_INTERVAL);
    }
}

#[cfg(windows)]
pub fn read_lmu_update(frame_identifier: u32) -> Result<Option<TelemetryUpdate>, String> {
    let snapshot = shared_memory::read_mapping(lmu::LMU_MAPPING_NAME, lmu::LMU_VIEW_SIZE)?;
    lmu::parse_lmu_update(&snapshot, frame_identifier)
}

#[cfg(windows)]
pub fn read_ace_update(frame_identifier: u32) -> Result<Option<TelemetryUpdate>, String> {
    let snapshot = shared_memory::read_mapping(ace::ACE_MAPPING_NAME, ace::ACE_PHYSICS_MIN_SIZE)?;
    ace::parse_ace_update(&snapshot, frame_identifier)
}

#[cfg(windows)]
pub fn lmu_mapping_exists() -> bool {
    shared_memory::mapping_exists(lmu::LMU_MAPPING_NAME)
}

#[cfg(windows)]
pub fn ace_mapping_exists() -> bool {
    shared_memory::mapping_exists(ace::ACE_MAPPING_NAME)
}

#[cfg(windows)]
fn print_optional_hud(enabled: bool) {
    if enabled {
        println!("HUD: native window");
    }
}
