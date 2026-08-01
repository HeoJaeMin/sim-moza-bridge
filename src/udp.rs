use std::io::ErrorKind;
use std::net::UdpSocket;
use std::time::{Duration, Instant};

use crate::bridge::TelemetryBridge;
use crate::config::BridgeConfig;
use crate::hud::HudHandle;
use crate::logging::{TelemetryRecorder, print_enabled_outputs};
use crate::runtime_control::{ShutdownToken, never_stop_token, shutdown_requested};

const UDP_BUFFER_SIZE: usize = 65_535;

#[cfg_attr(any(target_os = "macos", target_os = "windows"), allow(dead_code))]
pub fn start_udp_bridge(config: BridgeConfig) -> Result<(), String> {
    run_udp_bridge(config, None, never_stop_token())
}

pub fn start_udp_bridge_with_hud(
    config: BridgeConfig,
    hud: Option<HudHandle>,
    shutdown: ShutdownToken,
) -> Result<(), String> {
    run_udp_bridge(config, hud, shutdown)
}

fn run_udp_bridge(
    config: BridgeConfig,
    hud: Option<HudHandle>,
    shutdown: ShutdownToken,
) -> Result<(), String> {
    let receiver = UdpSocket::bind(format!("{}:{}", config.listen_host, config.listen_port))
        .map_err(|error| format!("bind failed: {error}"))?;
    receiver
        .set_read_timeout(Some(Duration::from_millis(50)))
        .map_err(|error| format!("failed to configure UDP receive timeout: {error}"))?;
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

    while !shutdown_requested(&shutdown) {
        let (size, _) = match receiver.recv_from(&mut buffer) {
            Ok(received) => received,
            Err(error) if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                continue;
            }
            Err(error) => return Err(format!("receive failed: {error}")),
        };
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
            recorder.ingest(config.game.id, &result.telemetry_update, config.debug);
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

    Ok(())
}

fn is_loopback_host(host: &str) -> bool {
    matches!(host, "127.0.0.1" | "localhost" | "::1")
}

fn print_optional_hud(enabled: bool) {
    if enabled {
        println!("HUD: native window");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bridge::BridgeMode;
    use crate::games::F1_25;
    use crate::runtime_control::{new_shutdown_token, request_shutdown};
    use std::sync::Arc;
    use std::thread;

    fn test_config() -> BridgeConfig {
        BridgeConfig {
            game: F1_25,
            listen_host: "127.0.0.1".to_owned(),
            listen_port: 0,
            moza_host: "127.0.0.1".to_owned(),
            moza_port: 22025,
            mode: BridgeMode::Passthrough,
            fix_tyre_wear_order: false,
            f1_24_car_damage_compat: false,
            input_log: None,
            corner_log: None,
            analysis_report: None,
            race_engineer: false,
            engineer_voice: false,
            engineer_log: None,
            engineer_state: None,
            engineer_history: None,
            engineer_trigger: None,
            engineer_hook: None,
            engineer_ai_hook: None,
            engineer_ai_task_id: None,
            engineer_radio_hook: None,
            dry_run: true,
            debug: false,
        }
    }

    #[test]
    fn idle_udp_runtime_stops_after_shutdown_is_requested() {
        let shutdown = new_shutdown_token();
        let worker_shutdown = Arc::clone(&shutdown);
        let worker = thread::spawn(move || run_udp_bridge(test_config(), None, worker_shutdown));

        thread::sleep(Duration::from_millis(20));
        request_shutdown(&shutdown);

        let deadline = Instant::now() + Duration::from_secs(1);
        while !worker.is_finished() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        assert!(worker.is_finished(), "idle UDP receive did not unblock");
        assert!(worker.join().unwrap().is_ok());
    }
}
