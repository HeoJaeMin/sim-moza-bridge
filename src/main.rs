mod adapters;
mod analysis;
mod bridge;
mod config;
mod detect;
mod f1;
mod games;
mod hud;
mod logging;
mod telemetry;
mod udp;

use games::ProtocolKind;

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

    let result = match config.game.protocol {
        ProtocolKind::Auto => adapters::start_detected_adapter(config.clone())
            .unwrap_or_else(|| udp::start_udp_bridge(config)),
        ProtocolKind::F1_25 | ProtocolKind::OpaqueUdp => udp::start_udp_bridge(config),
        ProtocolKind::AssettoCorsaEvo => adapters::ace::start_ace_adapter(config),
        ProtocolKind::LeMansUltimate => adapters::lmu::start_lmu_adapter(config),
        ProtocolKind::AssettoCorsaRally => {
            Err("Assetto Corsa Rally adapter is not implemented yet".to_owned())
        }
    };

    if let Err(error) = result {
        eprintln!("[startup-error] {error}");
        std::process::exit(1);
    }
}
