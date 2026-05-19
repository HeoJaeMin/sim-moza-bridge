use std::net::UdpSocket;
use std::time::{Duration, Instant};

use crate::analysis::TelemetryAnalyzer;
use crate::bridge::TelemetryBridge;
use crate::config::BridgeConfig;
use crate::hud::{HudHandle, start_hud_server};
use crate::logging::{CornerLogger, InputLogger, write_analysis_report};

pub fn start_udp_bridge(config: BridgeConfig) -> Result<(), String> {
    let receiver = UdpSocket::bind(format!("{}:{}", config.listen_host, config.listen_port))
        .map_err(|error| format!("bind failed: {error}"))?;
    let sender =
        UdpSocket::bind("0.0.0.0:0").map_err(|error| format!("sender bind failed: {error}"))?;
    let target = format!("{}:{}", config.moza_host, config.moza_port);
    let mut bridge = TelemetryBridge::new(config.game, config.mode, config.fix_tyre_wear_order);
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
    let hud = start_optional_hud(&config)?;
    let mut last_stats = Instant::now();
    let mut buffer = vec![0_u8; 4096];

    println!(
        "{}\n{}\nmode={:?}\nfixTyreWearOrder={}",
        format_args!(
            "Sim MOZA Bridge listening on {}:{}",
            config.listen_host, config.listen_port
        ),
        if config.dry_run {
            "dry-run enabled; packets will not be forwarded".to_owned()
        } else {
            format!("forwarding to {target}")
        },
        config.mode,
        config.fix_tyre_wear_order
    );
    println!("game={} ({})", config.game.id, config.game.name);
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
        println!("HUD: http://{}:{port}", config.hud_host);
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
        if config.verbose && result.patched {
            println!("[patch] packet remapped");
        }

        if let Some(sample) = &result.input_sample {
            if let Some(logger) = &mut input_logger {
                logger.write(sample)?;
            }
            if let Some(hud) = &hud {
                hud.update(sample.clone());
            }
        }

        if let Some(analyzer) = &mut analyzer {
            if !result.telemetry_update.is_empty() {
                if let Some(analysis) = analyzer.ingest(&result.telemetry_update) {
                    if let Some(logger) = &mut corner_logger {
                        logger.write(&analysis)?;
                    }
                    if let Some(path) = &config.analysis_report {
                        write_analysis_report(path, &analysis)?;
                    }
                    if config.verbose {
                        println!(
                            "[analysis] lap={} clean={} samples={} recommendations={}",
                            analysis.lap_num,
                            analysis.clean,
                            analysis.sample_count,
                            analysis.recommendations.len()
                        );
                    }
                }
            }
        }

        if !config.dry_run {
            sender
                .send_to(&result.packet, &target)
                .map_err(|error| format!("send failed: {error}"))?;
            bridge.mark_forwarded();
        }

        if config.verbose && last_stats.elapsed() >= Duration::from_secs(1) {
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
