use std::net::UdpSocket;
use std::process::Command;
use std::time::{Duration, Instant};

use crate::analysis::TelemetryAnalyzer;
use crate::bridge::TelemetryBridge;
use crate::config::BridgeConfig;
use crate::hud::{HudHandle, start_hud_server};
use crate::logging::{CornerLogger, InputLogger, write_analysis_report};

const UDP_BUFFER_SIZE: usize = 65_535;

pub fn start_udp_bridge(config: BridgeConfig) -> Result<(), String> {
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
    let mut input_logger = config
        .input_log
        .as_deref()
        .map(InputLogger::open)
        .transpose()?;
    let mut corner_logger = config
        .corner_log
        .as_deref()
        .map(CornerLogger::open)
        .transpose()?;
    let mut analyzer = if config.corner_log.is_some() || config.analysis_report.is_some() {
        Some(TelemetryAnalyzer::default())
    } else {
        None
    };
    let mut analysis_report = config.analysis_report.clone();
    let hud = start_optional_hud(&config)?;
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
    if let Some(path) = &config.input_log {
        println!("input logging enabled: {path}");
    }
    if let Some(path) = &config.corner_log {
        println!("corner trace logging enabled: {path}");
    }
    if let Some(path) = &config.analysis_report {
        println!("analysis report enabled: {path}");
    }
    if let Some(port) = config.hud_http_port {
        let hud_url = format!("http://{}:{port}", config.hud_host);
        println!("HUD: {hud_url}");
        if let Err(error) = open_browser(&hud_url) {
            eprintln!("[warning] failed to open HUD in browser: {error}");
        }
    }

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

        if let Some(sample) = &result.input_sample {
            let input_log_error = input_logger
                .as_mut()
                .and_then(|logger| logger.write(sample).err());
            if let Some(error) = input_log_error {
                eprintln!("[log-error] {error}; disabling input logging");
                input_logger = None;
            }
        }

        if let Some(hud) = &hud
            && !result.telemetry_update.is_empty()
        {
            hud.update(&result.telemetry_update);
        }

        if let Some(analyzer) = &mut analyzer
            && !result.telemetry_update.is_empty()
            && let Some(analysis) = analyzer.ingest(&result.telemetry_update)
        {
            let corner_log_error = corner_logger
                .as_mut()
                .and_then(|logger| logger.write(&analysis).err());
            if let Some(error) = corner_log_error {
                eprintln!("[log-error] {error}; disabling corner logging");
                corner_logger = None;
            }

            let report_error = analysis_report
                .as_deref()
                .and_then(|path| write_analysis_report(path, &analysis).err());
            if let Some(error) = report_error {
                eprintln!("[log-error] {error}; disabling analysis report writes");
                analysis_report = None;
            }

            if config.debug {
                println!(
                    "[analysis] lap={} clean={} samples={} recommendations={}",
                    analysis.lap_num,
                    analysis.clean,
                    analysis.sample_count,
                    analysis.recommendations.len()
                );
            }
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

fn start_optional_hud(config: &BridgeConfig) -> Result<Option<HudHandle>, String> {
    config
        .hud_http_port
        .map(|port| start_hud_server(&config.hud_host, port))
        .transpose()
}

fn is_loopback_host(host: &str) -> bool {
    matches!(host, "127.0.0.1" | "localhost" | "::1")
}

fn open_browser(url: &str) -> Result<(), String> {
    open_browser_command(url)
        .spawn()
        .map(|_| ())
        .map_err(|error| error.to_string())
}

#[cfg(target_os = "windows")]
fn open_browser_command(url: &str) -> Command {
    let mut command = Command::new("cmd");
    command.args(["/C", "start", "", url]);
    command
}

#[cfg(target_os = "macos")]
fn open_browser_command(url: &str) -> Command {
    let mut command = Command::new("open");
    command.arg(url);
    command
}

#[cfg(all(unix, not(target_os = "macos")))]
fn open_browser_command(url: &str) -> Command {
    let mut command = Command::new("xdg-open");
    command.arg(url);
    command
}
