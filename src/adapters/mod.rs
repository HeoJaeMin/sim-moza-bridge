pub mod ace;
pub mod lmu;

#[cfg(windows)]
mod shared_memory;

#[cfg(windows)]
use std::process::Command;
#[cfg(windows)]
use std::thread;
#[cfg(windows)]
use std::time::{Duration, Instant};

#[cfg(windows)]
use crate::config::BridgeConfig;
#[cfg(windows)]
use crate::hud::{HudHandle, start_hud_server};
#[cfg(windows)]
use crate::telemetry::TelemetryUpdate;

#[cfg(windows)]
const POLL_INTERVAL: Duration = Duration::from_millis(50);
#[cfg(windows)]
const WARNING_INTERVAL: Duration = Duration::from_secs(2);

#[cfg(windows)]
#[cfg_attr(windows, allow(dead_code))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum HudLaunch {
    Web,
    Native,
}

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
    let hud = start_optional_hud(&config)?;
    run_shared_memory_adapter_loop(
        config,
        adapter_name,
        mapping_name,
        snapshot_size,
        parse_update,
        hud,
        HudLaunch::Web,
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
        HudLaunch::Native,
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
    hud_launch: HudLaunch,
) -> Result<(), String>
where
    F: FnMut(&[u8], u32) -> Result<Option<TelemetryUpdate>, String>,
{
    let mut frame_identifier = 0_u32;
    let mut warned = false;
    let mut last_warning = Instant::now();

    println!("{adapter_name} adapter reading {mapping_name}");
    println!("game={} ({})", config.game.id, config.game.name);
    println!("debug={}", config.debug);
    print_optional_hud(&config, hud_launch, hud.is_some());

    loop {
        match shared_memory::read_mapping(mapping_name, snapshot_size) {
            Ok(snapshot) => {
                warned = false;
                frame_identifier = frame_identifier.wrapping_add(1);
                if let Some(update) = parse_update(&snapshot, frame_identifier)?
                    && let Some(hud) = &hud
                {
                    hud.update(&update);
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
#[cfg_attr(windows, allow(dead_code))]
fn start_optional_hud(config: &BridgeConfig) -> Result<Option<HudHandle>, String> {
    config
        .hud_http_port
        .map(|port| start_hud_server(&config.hud_host, port))
        .transpose()
}

#[cfg(windows)]
fn print_optional_hud(config: &BridgeConfig, launch: HudLaunch, enabled: bool) {
    if !enabled {
        return;
    }

    match launch {
        HudLaunch::Native => println!("HUD: native window"),
        HudLaunch::Web => {
            if let Some(port) = config.hud_http_port {
                let hud_url = format!("http://{}:{port}", config.hud_host);
                println!("HUD: {hud_url}");
                if let Err(error) = open_browser(&hud_url) {
                    eprintln!("[warning] failed to open HUD in browser: {error}");
                }
            }
        }
    }
}

#[cfg(windows)]
fn open_browser(url: &str) -> Result<(), String> {
    open_browser_command(url)
        .spawn()
        .map(|_| ())
        .map_err(|error| error.to_string())
}

#[cfg(all(windows, target_os = "windows"))]
fn open_browser_command(url: &str) -> Command {
    let mut command = Command::new("cmd");
    command.args(["/C", "start", "", url]);
    command
}
