use std::net::UdpSocket;
use std::time::{Duration, Instant};

use crate::bridge::TelemetryBridge;
use crate::config::BridgeConfig;
use crate::hud::HudHandle;
use crate::logging::{TelemetryRecorder, print_enabled_outputs};

const UDP_BUFFER_SIZE: usize = 65_535;

#[cfg_attr(any(target_os = "macos", target_os = "windows"), allow(dead_code))]
pub fn start_udp_bridge(config: BridgeConfig) -> Result<(), String> {
    run_udp_bridge(config, None)
}

#[cfg_attr(not(any(target_os = "macos", target_os = "windows")), allow(dead_code))]
pub fn start_udp_bridge_with_hud(
    config: BridgeConfig,
    hud: Option<HudHandle>,
) -> Result<(), String> {
    run_udp_bridge(config, hud)
}

fn run_udp_bridge(config: BridgeConfig, hud: Option<HudHandle>) -> Result<(), String> {
    let receiver = UdpSocket::bind(format!("{}:{}", config.listen_host, config.listen_port))
        .map_err(|error| format!("bind failed: {error}"))?;
    let sender =
        UdpSocket::bind("0.0.0.0:0").map_err(|error| format!("sender bind failed: {error}"))?;
    let target = format!("{}:{}", config.moza_host, config.moza_port);
    let mut bridge = TelemetryBridge::new(
        config.game,
        config.mode,
        config.fix_tyre_wear_order,
        config.f1_24_car_damage_compat,
    );
    let mut recorder = TelemetryRecorder::open(&config)?;
    let mut last_stats = Instant::now();
    let mut buffer = vec![0_u8; UDP_BUFFER_SIZE];

    println!(
        "{}\n{}\nf1_25_compat=on\ndebug={}",
        format_args!(
            "Sim MOZA Bridge listening on {}:{}",
            config.listen_host, config.listen_port
        ),
        if config.dry_run {
            "dry-run enabled; packets will not be forwarded".to_owned()
        } else {
            format!("forwarding to {target}")
        },
        config.debug
    );
    println!("game={} ({})", config.game.id, config.game.name);
    if !is_loopback_host(&config.listen_host) {
        eprintln!(
            "[warning] listening on non-loopback host {}; LAN clients can send UDP packets to this bridge",
            config.listen_host
        );
    }
    print_enabled_outputs(&config);
    print_optional_hud(hud.is_some());

    loop {
        let (size, _) = receiver
            .recv_from(&mut buffer)
            .map_err(|error| format!("receive failed: {error}"))?;
        let packet = &buffer[..size];

        let Some(result) = bridge.process(packet) else {
            continue;
        };

        if let Some(detected_game) = result.detected_game {
            println!(
                "[detect] game={} ({})",
                detected_game.id, detected_game.name
            );
        }
        if config.debug && result.patched {
            println!("[patch] packet remapped");
        }

        if let Some(hud) = &hud
            && !result.telemetry_update.is_empty()
        {
            hud.update(&result.telemetry_update);
        }

        if !result.telemetry_update.is_empty() {
            recorder.ingest(&result.telemetry_update, config.debug);
        }

        if !config.dry_run {
            sender
                .send_to(&result.packet, &target)
                .map_err(|error| format!("send failed: {error}"))?;
            bridge.mark_forwarded();
        }

        if config.debug && last_stats.elapsed() >= Duration::from_secs(1) {
            println!(
                "[stats] received={} forwarded={} patched={} ignored={} malformed={} packets={}",
                bridge.stats.received,
                bridge.stats.forwarded,
                bridge.stats.patched,
                bridge.stats.ignored,
                bridge.stats.malformed,
                bridge.packet_summary()
            );
            last_stats = Instant::now();
        }
    }
}

fn is_loopback_host(host: &str) -> bool {
    matches!(host, "127.0.0.1" | "localhost" | "::1")
}

fn print_optional_hud(enabled: bool) {
    if enabled {
        println!("HUD: native window");
    }
}
