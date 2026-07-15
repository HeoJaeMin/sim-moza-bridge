mod adapters;
mod analysis;
mod auto_runtime;
mod bridge;
mod config;
mod detect;
mod f1;
mod games;
mod hud;
mod logging;
#[cfg(any(target_os = "macos", target_os = "windows"))]
mod native_hud;
mod telemetry;
mod telemetry_quality;
mod udp;

use games::ProtocolKind;
use hud::HudHandle;

fn main() {
    let config = match config::read_config() {
        Ok(config) => config,
        Err(config::ConfigError::Help(help)) => {
            println!("{help}");
            return;
        }
        Err(config::ConfigError::Message(error)) => {
            eprintln!("[startup-error] {error}");
            std::process::exit(1);
        }
    };

    let result = start(config);

    if let Err(error) = result {
        eprintln!("[startup-error] {error}");
        std::process::exit(1);
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn start(config: config::BridgeConfig) -> Result<(), String> {
    if config.headless {
        start_runtime(config)
    } else {
        native_hud::run(config)
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn start(config: config::BridgeConfig) -> Result<(), String> {
    start_runtime(config)
}

fn start_runtime(config: config::BridgeConfig) -> Result<(), String> {
    match config.game.protocol {
        ProtocolKind::Auto => auto_runtime::start_auto_runtime(config),
        ProtocolKind::F1_25 | ProtocolKind::OpaqueUdp => udp::start_udp_bridge(config),
        ProtocolKind::AssettoCorsaEvo => adapters::ace::start_ace_adapter(config),
        ProtocolKind::LeMansUltimate => adapters::lmu::start_lmu_adapter(config),
        ProtocolKind::AssettoCorsaRally => adapters::acr::start_acr_adapter(config),
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
pub(crate) fn start_runtime_with_hud(
    config: config::BridgeConfig,
    hud: Option<HudHandle>,
) -> Result<(), String> {
    match config.game.protocol {
        ProtocolKind::Auto => auto_runtime::start_auto_runtime_with_hud(config, hud),
        ProtocolKind::F1_25 | ProtocolKind::OpaqueUdp => {
            udp::start_udp_bridge_with_hud(config, hud)
        }
        ProtocolKind::AssettoCorsaEvo => adapters::ace::start_ace_adapter_with_hud(config, hud),
        ProtocolKind::LeMansUltimate => adapters::lmu::start_lmu_adapter_with_hud(config, hud),
        ProtocolKind::AssettoCorsaRally => adapters::acr::start_acr_adapter_with_hud(config, hud),
    }
}
