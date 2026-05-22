#![cfg_attr(not(windows), allow(dead_code))]

use std::io;
use std::net::UdpSocket;
use std::thread;
use std::time::{Duration, Instant};

use crate::bridge::TelemetryBridge;
use crate::config::BridgeConfig;
use crate::games::{ACE, F1_25, LMU};
use crate::hud::HudHandle;
use crate::logging::{TelemetryRecorder, print_enabled_outputs};
use crate::telemetry::TelemetryUpdate;

const UDP_BUFFER_SIZE: usize = 65_535;
const LOOP_INTERVAL: Duration = Duration::from_millis(16);
const UDP_PRIORITY_WINDOW: Duration = Duration::from_secs(2);
const SHARED_MEMORY_POLL_INTERVAL: Duration = Duration::from_millis(50);
const WARNING_INTERVAL: Duration = Duration::from_secs(2);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RuntimeSource {
    Waiting,
    F1,
    Lmu,
    Ace,
}

#[cfg_attr(any(target_os = "macos", target_os = "windows"), allow(dead_code))]
pub fn start_auto_runtime(config: BridgeConfig) -> Result<(), String> {
    run_auto_runtime(config, None)
}

pub fn start_auto_runtime_with_hud(
    config: BridgeConfig,
    hud: Option<HudHandle>,
) -> Result<(), String> {
    run_auto_runtime(config, hud)
}

fn run_auto_runtime(config: BridgeConfig, hud: Option<HudHandle>) -> Result<(), String> {
    let receiver = UdpSocket::bind(format!("{}:{}", config.listen_host, config.listen_port))
        .map_err(|error| format!("bind failed: {error}"))?;
    receiver
        .set_nonblocking(true)
        .map_err(|error| format!("failed to enable nonblocking UDP receive: {error}"))?;
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
    let mut buffer = vec![0_u8; UDP_BUFFER_SIZE];
    let mut source = RuntimeSource::Waiting;
    let mut frame_identifier = 0_u32;
    let mut last_udp = Instant::now() - UDP_PRIORITY_WINDOW;
    let mut last_shared_memory_poll = Instant::now() - SHARED_MEMORY_POLL_INTERVAL;
    let mut last_lmu_warning = Instant::now() - WARNING_INTERVAL;
    let mut last_ace_warning = Instant::now() - WARNING_INTERVAL;
    let mut last_stats = Instant::now();

    println!(
        "{}\n{}\nf1_25_compat=on\ndebug={}",
        format_args!(
            "Sim MOZA Bridge auto runtime listening on {}:{}",
            config.listen_host, config.listen_port
        ),
        if config.dry_run {
            "dry-run enabled; packets will not be forwarded".to_owned()
        } else {
            format!("forwarding F1 UDP to {target}")
        },
        config.debug
    );
    println!("game=auto (F1 UDP + LMU/ACE shared-memory supervisor)");
    if !is_loopback_host(&config.listen_host) {
        eprintln!(
            "[warning] listening on non-loopback host {}; LAN clients can send UDP packets to this bridge",
            config.listen_host
        );
    }
    print_enabled_outputs(&config);
    print_optional_hud(hud.is_some());

    loop {
        while let Some(update) = receive_udp_update(
            &receiver,
            &sender,
            &target,
            &mut bridge,
            &config,
            &mut buffer,
        )? {
            last_udp = Instant::now();
            set_source(&mut source, RuntimeSource::F1);
            if let Some(hud) = &hud
                && !update.is_empty()
            {
                hud.update(&update);
            }
            if !update.is_empty() {
                recorder.ingest(&update, config.debug);
            }
        }

        let udp_recent = last_udp.elapsed() < UDP_PRIORITY_WINDOW;
        if !udp_recent && last_shared_memory_poll.elapsed() >= SHARED_MEMORY_POLL_INTERVAL {
            frame_identifier = frame_identifier.wrapping_add(1);
            if try_shared_memory_update(
                &hud,
                &mut source,
                &mut frame_identifier,
                &mut last_lmu_warning,
                &mut last_ace_warning,
            )? {
                last_shared_memory_poll = Instant::now();
            } else {
                set_source(&mut source, RuntimeSource::Waiting);
                last_shared_memory_poll = Instant::now();
            }
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

        thread::sleep(LOOP_INTERVAL);
    }
}

fn receive_udp_update(
    receiver: &UdpSocket,
    sender: &UdpSocket,
    target: &str,
    bridge: &mut TelemetryBridge,
    config: &BridgeConfig,
    buffer: &mut [u8],
) -> Result<Option<TelemetryUpdate>, String> {
    let packet = match receiver.recv_from(buffer) {
        Ok((size, _)) => &buffer[..size],
        Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(None),
        Err(error) => return Err(format!("receive failed: {error}")),
    };

    let Some(result) = bridge.process(packet) else {
        return Ok(Some(TelemetryUpdate::default()));
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

    if !config.dry_run {
        sender
            .send_to(&result.packet, target)
            .map_err(|error| format!("send failed: {error}"))?;
        bridge.mark_forwarded();
    }

    Ok(Some(result.telemetry_update))
}

fn try_shared_memory_update(
    hud: &Option<HudHandle>,
    source: &mut RuntimeSource,
    frame_identifier: &mut u32,
    last_lmu_warning: &mut Instant,
    last_ace_warning: &mut Instant,
) -> Result<bool, String> {
    #[cfg(not(windows))]
    {
        let _ = hud;
        let _ = source;
        let _ = frame_identifier;
        let _ = last_lmu_warning;
        let _ = last_ace_warning;
        Ok(false)
    }

    #[cfg(windows)]
    {
        if crate::adapters::lmu_mapping_exists() {
            set_source(source, RuntimeSource::Lmu);
            match crate::adapters::read_lmu_update(*frame_identifier) {
                Ok(Some(update)) => {
                    if let Some(hud) = hud {
                        hud.update(&update);
                    }
                    return Ok(true);
                }
                Ok(None) => return Ok(true),
                Err(error) => {
                    warn_periodically(last_lmu_warning, &format!("[adapter-warning] {error}"));
                    return Ok(false);
                }
            }
        }

        if crate::adapters::ace_mapping_exists() {
            set_source(source, RuntimeSource::Ace);
            match crate::adapters::read_ace_update(*frame_identifier) {
                Ok(Some(update)) => {
                    if let Some(hud) = hud {
                        hud.update(&update);
                    }
                    return Ok(true);
                }
                Ok(None) => return Ok(true),
                Err(error) => {
                    warn_periodically(last_ace_warning, &format!("[adapter-warning] {error}"));
                    return Ok(false);
                }
            }
        }

        Ok(false)
    }
}

fn set_source(current: &mut RuntimeSource, next: RuntimeSource) {
    if *current == next {
        return;
    }

    *current = next;
    match next {
        RuntimeSource::Waiting => println!("[source] waiting for F1 UDP, LMU, or ACE telemetry"),
        RuntimeSource::F1 => println!("[source] {}", F1_25.name),
        RuntimeSource::Lmu => println!("[source] {}", LMU.name),
        RuntimeSource::Ace => println!("[source] {}", ACE.name),
    }
}

fn warn_periodically(last_warning: &mut Instant, message: &str) {
    if last_warning.elapsed() >= WARNING_INTERVAL {
        eprintln!("{message}");
        *last_warning = Instant::now();
    }
}

fn print_optional_hud(enabled: bool) {
    if enabled {
        println!("HUD: native window");
    }
}

fn is_loopback_host(host: &str) -> bool {
    matches!(host, "127.0.0.1" | "localhost" | "::1")
}
