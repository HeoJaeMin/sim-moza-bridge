mod coach;
mod contact;
mod demo;
mod engine;
mod lap;
mod model;
mod parser;
mod server;
mod store;
mod track;

use std::env;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;
#[cfg(windows)]
use std::time::Instant;

use demo::DemoSource;
use engine::DashboardEngine;
#[cfg(windows)]
use parser::{LMU_VIEW_SIZE, parse_lmu_snapshot};
use server::{CollectorCommand, DashboardState};
use sim_moza_bridge::{shared_memory, telemetry_core, telemetry_quality};
use store::{DashboardStore, PersistenceWorker};
use tokio::sync::{mpsc, watch};

const LMU_MAPPING_NAME: &str = "LMU_Data";
#[cfg(windows)]
const LMU_SCORING_COUNT_OFFSET: usize = 1_736;
#[cfg(windows)]
const LMU_SCORING_VEHICLES_OFFSET: usize = 2_192;
#[cfg(windows)]
const LMU_SCORING_VEHICLE_SIZE: usize = 584;
#[cfg(windows)]
const LMU_TELEMETRY_OFFSET: usize = 128_464;
#[cfg(windows)]
const LMU_MAX_VEHICLES: usize = 104;
#[cfg(windows)]
const LMU_STABILITY_MARKERS: [shared_memory::StabilityMarker; 3] = [
    shared_memory::StabilityMarker::new(1_700, 8),
    shared_memory::StabilityMarker::new(1_736, 4),
    shared_memory::StabilityMarker::new(LMU_TELEMETRY_OFFSET, 4),
];
#[cfg(windows)]
const LMU_ACTIVE_VEHICLE_REGIONS: [shared_memory::CountedStabilityRegion; 2] = [
    shared_memory::CountedStabilityRegion::new(
        LMU_SCORING_COUNT_OFFSET,
        LMU_SCORING_VEHICLES_OFFSET,
        LMU_SCORING_VEHICLE_SIZE,
        LMU_MAX_VEHICLES,
    ),
    shared_memory::CountedStabilityRegion::new(
        LMU_TELEMETRY_OFFSET,
        LMU_TELEMETRY_OFFSET + 4,
        (LMU_VIEW_SIZE - (LMU_TELEMETRY_OFFSET + 4)) / LMU_MAX_VEHICLES,
        LMU_MAX_VEHICLES,
    ),
];
const POLL_INTERVAL: Duration = Duration::from_millis(50);
#[cfg(windows)]
const WARNING_INTERVAL: Duration = Duration::from_secs(2);

#[derive(Clone, Debug, PartialEq)]
struct DashboardConfig {
    listen: SocketAddr,
    data_dir: PathBuf,
    demo: bool,
    coach_report: Option<PathBuf>,
    allow_remote: bool,
    raw_retention_days: Option<u64>,
    analysis_retention_days: Option<u64>,
}

#[tokio::main]
async fn main() {
    if env::args().any(|argument| matches!(argument.as_str(), "--help" | "-h")) {
        println!("{}", help_text());
        return;
    }
    let config = match parse_config(env::args().skip(1)) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("[startup-error] {error}\n\n{}", help_text());
            std::process::exit(1);
        }
    };

    #[cfg(not(windows))]
    if !config.demo {
        eprintln!(
            "[startup-error] live LMU shared memory requires Windows; use --demo on this platform"
        );
        std::process::exit(1);
    }
    let store = match DashboardStore::open(&config.data_dir) {
        Ok(store) => store,
        Err(error) => {
            eprintln!("[startup-error] {error}");
            std::process::exit(1);
        }
    };
    if let Err(error) =
        store.apply_retention(config.raw_retention_days, config.analysis_retention_days)
    {
        eprintln!("[startup-error] {error}");
        std::process::exit(1);
    }

    let (shutdown_sender, shutdown_receiver) = watch::channel(false);
    let (control_sender, control_receiver) = mpsc::unbounded_channel();
    let state =
        DashboardState::new(store.clone()).with_control(control_sender, shutdown_sender.clone());
    if let Some(report_path) = config.coach_report.as_ref() {
        println!("LMU session coaching: {}", report_path.display());
    }
    let coach_task = tokio::spawn(coach::run(
        store.clone(),
        config.coach_report.clone(),
        state.clone(),
        shutdown_receiver.clone(),
    ));
    let collector_state = state.clone();
    let collector_shutdown = shutdown_receiver.clone();
    let demo = config.demo;
    let collector_task = tokio::spawn(async move {
        if demo {
            collect_demo(store, collector_state, collector_shutdown, control_receiver).await;
        } else {
            collect_lmu(store, collector_state, collector_shutdown, control_receiver).await;
        }
    });

    println!("LMU dashboard: http://{}", config.listen);
    println!("data: {}", config.data_dir.display());
    if !config.listen.ip().is_loopback() {
        println!(
            "LAN mode enabled; open this PC's LAN IP on port {} and use a trusted network",
            config.listen.port()
        );
    }
    if config.demo {
        println!("source: demo (use --live on the Windows game PC for LMU_Data)");
    } else {
        println!("source: {LMU_MAPPING_NAME}; waiting for Le Mans Ultimate");
    }

    let server_shutdown = shutdown_receiver.clone();
    let mut server_task = tokio::spawn(server::serve(config.listen, state, server_shutdown));
    let server_result = tokio::select! {
        result = &mut server_task => result,
        signal = tokio::signal::ctrl_c() => {
            if let Err(error) = signal {
                eprintln!("[shutdown-error] failed to listen for Ctrl-C: {error}");
            }
            let _ = shutdown_sender.send(true);
            server_task.await
        }
    };
    let _ = shutdown_sender.send(true);
    if let Err(error) = collector_task.await {
        eprintln!("[shutdown-error] collector task stopped unexpectedly: {error}");
    }
    if let Err(error) = coach_task.await {
        eprintln!("[shutdown-error] coaching task stopped unexpectedly: {error}");
    }
    let server_result = server_result
        .map_err(|error| format!("server task stopped unexpectedly: {error}"))
        .and_then(|result| result);
    if let Err(error) = server_result {
        eprintln!("[server-error] {error}");
        std::process::exit(1);
    }
}

async fn collect_demo(
    store: DashboardStore,
    state: DashboardState,
    mut shutdown: watch::Receiver<bool>,
    mut control: mpsc::UnboundedReceiver<CollectorCommand>,
) {
    let worker = PersistenceWorker::start(store.clone());
    let source = DemoSource::new();
    let mut engine = match DashboardEngine::new(store, worker.queue(), "demo") {
        Ok(engine) => engine,
        Err(error) => {
            eprintln!("[collector-error] {error}");
            let _ = worker.shutdown().await;
            return;
        }
    };
    let mut interval = tokio::time::interval(POLL_INTERVAL);
    let mut manually_paused = false;
    let mut shutdown_acknowledge = None;
    loop {
        tokio::select! {
            _ = interval.tick(), if !manually_paused => {
                match engine.process(source.frame()) {
                    Ok(update) => state.publish(update.live, update.trace).await,
                    Err(error) => {
                        let update = engine.disconnected(error);
                        state.publish(update.live, update.trace).await;
                    }
                }
            }
            command = control.recv() => {
                match command {
                    Some(CollectorCommand::Pause(acknowledge)) => {
                        let result = if manually_paused {
                            Ok(())
                        } else {
                            let (update, flush) = engine.pause_and_flush();
                            manually_paused = true;
                            state.publish(update.live, update.trace).await;
                            flush
                        };
                        let _ = acknowledge.send(result);
                    }
                    Some(CollectorCommand::Resume(acknowledge)) => {
                        if manually_paused {
                            manually_paused = false;
                            let update = engine.resume();
                            state.publish(update.live, update.trace).await;
                        }
                        let _ = acknowledge.send(Ok(()));
                    }
                    Some(CollectorCommand::Shutdown(acknowledge)) => {
                        shutdown_acknowledge = Some(acknowledge);
                        break;
                    }
                    None => break,
                }
            }
            result = shutdown.changed() => {
                if result.is_err() || *shutdown.borrow() {
                    break;
                }
            }
        }
    }
    let result = finish_collection(&mut engine, worker).await;
    if let Some(acknowledge) = shutdown_acknowledge {
        let _ = acknowledge.send(result.clone());
    }
    if let Err(error) = result {
        eprintln!("[shutdown-error] {error}");
    }
}

#[cfg(windows)]
async fn collect_lmu(
    store: DashboardStore,
    state: DashboardState,
    mut shutdown: watch::Receiver<bool>,
    mut control: mpsc::UnboundedReceiver<CollectorCommand>,
) {
    let worker = PersistenceWorker::start(store.clone());
    let mut engine = match DashboardEngine::new(store, worker.queue(), LMU_MAPPING_NAME) {
        Ok(engine) => engine,
        Err(error) => {
            eprintln!("[collector-error] {error}");
            let _ = worker.shutdown().await;
            return;
        }
    };
    let mut interval = tokio::time::interval(POLL_INTERVAL);
    let mut last_warning = Instant::now() - WARNING_INTERVAL;
    let mut reader = None;
    let mut manually_paused = false;
    let mut shutdown_acknowledge = None;
    loop {
        tokio::select! {
            _ = interval.tick(), if !manually_paused => {
                let snapshot = (|| {
                    if reader.is_none() {
                        reader = Some(shared_memory::SharedMemoryReader::open(
                            LMU_MAPPING_NAME,
                            LMU_VIEW_SIZE,
                        )?);
                    }
                    reader
                        .as_ref()
                        .expect("LMU shared-memory reader was initialized")
                        .read_consistent_counted(
                            &LMU_STABILITY_MARKERS,
                            &LMU_ACTIVE_VEHICLE_REGIONS,
                        )
                })();
                let result = snapshot.and_then(|snapshot| parse_lmu_snapshot(&snapshot));
                match result.and_then(|frame| engine.process(frame)) {
                    Ok(update) => state.publish(update.live, update.trace).await,
                    Err(error) => {
                        let inconsistent = reader
                            .as_ref()
                            .is_some_and(|reader| reader.stats().inconsistent_reads > 0);
                        reader = None;
                        if last_warning.elapsed() >= WARNING_INTERVAL {
                            eprintln!("[adapter-warning] {error}");
                            last_warning = Instant::now();
                        }
                        if shared_memory::is_stalled_error(&error) {
                            let update = engine.stalled(error);
                            state.publish(update.live, update.trace).await;
                            continue;
                        }
                        let update = if inconsistent {
                            engine.inconsistent_snapshot(error)
                        } else {
                            engine.disconnected(error)
                        };
                        state.publish(update.live, update.trace).await;
                    }
                }
            }
            command = control.recv() => {
                match command {
                    Some(CollectorCommand::Pause(acknowledge)) => {
                        let result = if manually_paused {
                            Ok(())
                        } else {
                            let (update, flush) = engine.pause_and_flush();
                            manually_paused = true;
                            reader = None;
                            state.publish(update.live, update.trace).await;
                            flush
                        };
                        let _ = acknowledge.send(result);
                    }
                    Some(CollectorCommand::Resume(acknowledge)) => {
                        if manually_paused {
                            manually_paused = false;
                            let update = engine.resume();
                            state.publish(update.live, update.trace).await;
                        }
                        let _ = acknowledge.send(Ok(()));
                    }
                    Some(CollectorCommand::Shutdown(acknowledge)) => {
                        shutdown_acknowledge = Some(acknowledge);
                        break;
                    }
                    None => break,
                }
            }
            result = shutdown.changed() => {
                if result.is_err() || *shutdown.borrow() {
                    break;
                }
            }
        }
    }
    let result = finish_collection(&mut engine, worker).await;
    if let Some(acknowledge) = shutdown_acknowledge {
        let _ = acknowledge.send(result.clone());
    }
    if let Err(error) = result {
        eprintln!("[shutdown-error] {error}");
    }
}

#[cfg(not(windows))]
async fn collect_lmu(
    _store: DashboardStore,
    _state: DashboardState,
    _shutdown: watch::Receiver<bool>,
    _control: mpsc::UnboundedReceiver<CollectorCommand>,
) {
}

async fn finish_collection(
    engine: &mut DashboardEngine,
    worker: PersistenceWorker,
) -> Result<(), String> {
    let enqueue = engine
        .prepare_shutdown()
        .map_err(|error| format!("failed to enqueue final dashboard data: {error}"));
    let flush = worker
        .shutdown()
        .await
        .map_err(|error| format!("failed to flush dashboard data: {error}"));
    enqueue.and(flush)
}

fn parse_config<I>(arguments: I) -> Result<DashboardConfig, String>
where
    I: IntoIterator<Item = String>,
{
    let mut listen = "127.0.0.1:8787"
        .parse::<SocketAddr>()
        .expect("default dashboard address must be valid");
    let mut data_dir = PathBuf::from("lmu-dashboard-data");
    let mut demo = !cfg!(windows);
    let mut coach_report = None;
    let mut allow_remote = false;
    let mut raw_retention_days = None;
    let mut analysis_retention_days = None;
    let mut iterator = arguments.into_iter();
    while let Some(argument) = iterator.next() {
        match argument.as_str() {
            "--listen" => {
                let value = next_value(&mut iterator, "--listen")?;
                listen = value
                    .parse::<SocketAddr>()
                    .map_err(|_| "--listen must be an address such as 127.0.0.1:8787".to_owned())?;
            }
            "--data-dir" => {
                data_dir = PathBuf::from(next_value(&mut iterator, "--data-dir")?);
            }
            "--demo" => demo = true,
            "--live" => demo = false,
            "--coach-report" => {
                coach_report = Some(PathBuf::from(next_value(&mut iterator, "--coach-report")?));
            }
            "--allow-remote" => allow_remote = true,
            "--raw-retention-days" => {
                raw_retention_days = Some(parse_retention_days(
                    &next_value(&mut iterator, "--raw-retention-days")?,
                    "--raw-retention-days",
                )?);
            }
            "--analysis-retention-days" => {
                analysis_retention_days = Some(parse_retention_days(
                    &next_value(&mut iterator, "--analysis-retention-days")?,
                    "--analysis-retention-days",
                )?);
            }
            unknown => return Err(format!("unknown option {unknown}")),
        }
    }
    if !listen.ip().is_loopback() && !allow_remote {
        return Err(
            "non-loopback --listen requires --allow-remote; use it only on a trusted network"
                .to_owned(),
        );
    }
    if let (Some(raw), Some(analysis)) = (raw_retention_days, analysis_retention_days)
        && analysis < raw
    {
        return Err(
            "--analysis-retention-days must be greater than or equal to --raw-retention-days"
                .to_owned(),
        );
    }
    Ok(DashboardConfig {
        listen,
        data_dir,
        demo,
        coach_report,
        allow_remote,
        raw_retention_days,
        analysis_retention_days,
    })
}

fn parse_retention_days(value: &str, option: &str) -> Result<u64, String> {
    value
        .parse::<u64>()
        .ok()
        .filter(|days| *days > 0)
        .ok_or_else(|| format!("{option} must be a positive number of days"))
}

fn next_value<I>(iterator: &mut I, option: &str) -> Result<String, String>
where
    I: Iterator<Item = String>,
{
    iterator
        .next()
        .filter(|value| !value.starts_with("--"))
        .ok_or_else(|| format!("{option} requires a value"))
}

fn help_text() -> &'static str {
    "Usage: lmu-dashboard [options]\n\n\
     Options:\n\
       --listen <address>             Web address (default: 127.0.0.1:8787)\n\
       --data-dir <path>              SQLite data directory (default: lmu-dashboard-data)\n\
       --demo                         Run without LMU using generated race data\n\
       --live                         Read LMU_Data on the Windows game PC\n\
       --coach-report <path>          Write LMU session coaching Markdown and JSON\n\
       --raw-retention-days <days>    Remove raw samples after this many days (default: keep)\n\
       --analysis-retention-days <d>  Remove summaries after this many days (default: keep)\n\
       --allow-remote                 Required for non-loopback --listen addresses\n\
       --help                         Show this help\n\n\
     Tablet example: --listen 0.0.0.0:8787 --allow-remote"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_dashboard_options_and_explicit_remote_access() {
        let config = parse_config([
            "--listen".to_owned(),
            "0.0.0.0:9000".to_owned(),
            "--allow-remote".to_owned(),
            "--data-dir".to_owned(),
            "telemetry".to_owned(),
            "--demo".to_owned(),
            "--raw-retention-days".to_owned(),
            "30".to_owned(),
            "--analysis-retention-days".to_owned(),
            "180".to_owned(),
        ])
        .unwrap();
        assert_eq!(config.listen, "0.0.0.0:9000".parse().unwrap());
        assert_eq!(config.data_dir, PathBuf::from("telemetry"));
        assert!(config.demo);
        assert!(config.allow_remote);
        assert_eq!(config.raw_retention_days, Some(30));
        assert_eq!(config.analysis_retention_days, Some(180));
    }

    #[test]
    fn parses_coaching_report() {
        let config = parse_config([
            "--live".to_owned(),
            "--coach-report".to_owned(),
            "reports/qualifying.md".to_owned(),
        ])
        .unwrap();

        assert!(!config.demo);
        assert_eq!(
            config.coach_report,
            Some(PathBuf::from("reports/qualifying.md"))
        );
        assert_eq!(config.raw_retention_days, None);
    }

    #[test]
    fn rejects_implicit_remote_bind_and_invalid_retention() {
        assert!(
            parse_config(["--listen".to_owned(), "0.0.0.0:9000".to_owned()])
                .unwrap_err()
                .contains("requires --allow-remote")
        );
        assert!(
            parse_config([
                "--raw-retention-days".to_owned(),
                "30".to_owned(),
                "--analysis-retention-days".to_owned(),
                "7".to_owned(),
            ])
            .unwrap_err()
            .contains("greater than or equal")
        );
        assert!(
            parse_config(["--raw-retention-days".to_owned(), "0".to_owned()])
                .unwrap_err()
                .contains("positive")
        );
    }

    #[test]
    fn rejects_invalid_or_missing_addresses() {
        assert!(
            parse_config(["--listen".to_owned(), "localhost".to_owned()])
                .unwrap_err()
                .contains("must be an address")
        );
        assert!(
            parse_config(["--data-dir".to_owned()])
                .unwrap_err()
                .contains("requires a value")
        );
        assert!(
            parse_config(["--coach-report".to_owned()])
                .unwrap_err()
                .contains("requires a value")
        );
    }
}
