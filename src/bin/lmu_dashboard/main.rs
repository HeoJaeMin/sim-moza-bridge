mod contact;
mod demo;
mod engine;
mod lap;
mod model;
mod parser;
mod server;
mod store;
mod track;

#[cfg(windows)]
#[path = "../../adapters/shared_memory.rs"]
mod shared_memory;

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
use server::DashboardState;
use store::DashboardStore;

const LMU_MAPPING_NAME: &str = "LMU_Data";
const POLL_INTERVAL: Duration = Duration::from_millis(50);
#[cfg(windows)]
const WARNING_INTERVAL: Duration = Duration::from_secs(2);

#[derive(Clone, Debug, PartialEq)]
struct DashboardConfig {
    listen: SocketAddr,
    data_dir: PathBuf,
    demo: bool,
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
    let state = DashboardState::new(store.clone());
    let collector_state = state.clone();
    let demo = config.demo;
    tokio::spawn(async move {
        if demo {
            collect_demo(store, collector_state).await;
        } else {
            collect_lmu(store, collector_state).await;
        }
    });

    println!("LMU dashboard: http://{}", config.listen);
    println!("data: {}", config.data_dir.display());
    if config.listen.ip().is_unspecified() {
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

    if let Err(error) = server::serve(config.listen, state).await {
        eprintln!("[server-error] {error}");
        std::process::exit(1);
    }
}

async fn collect_demo(store: DashboardStore, state: DashboardState) {
    let source = DemoSource::new();
    let mut engine = match DashboardEngine::new(store, "demo") {
        Ok(engine) => engine,
        Err(error) => {
            eprintln!("[collector-error] {error}");
            return;
        }
    };
    let mut interval = tokio::time::interval(POLL_INTERVAL);
    loop {
        interval.tick().await;
        match engine.process(source.frame()) {
            Ok(update) => state.publish(update.live, update.trace).await,
            Err(error) => {
                let update = engine.disconnected(error);
                state.publish(update.live, update.trace).await;
            }
        }
    }
}

#[cfg(windows)]
async fn collect_lmu(store: DashboardStore, state: DashboardState) {
    let mut engine = match DashboardEngine::new(store, LMU_MAPPING_NAME) {
        Ok(engine) => engine,
        Err(error) => {
            eprintln!("[collector-error] {error}");
            return;
        }
    };
    let mut interval = tokio::time::interval(POLL_INTERVAL);
    let mut last_warning = Instant::now() - WARNING_INTERVAL;
    loop {
        interval.tick().await;
        let result = shared_memory::read_mapping(LMU_MAPPING_NAME, LMU_VIEW_SIZE)
            .and_then(|snapshot| parse_lmu_snapshot(&snapshot));
        match result.and_then(|frame| engine.process(frame)) {
            Ok(update) => state.publish(update.live, update.trace).await,
            Err(error) => {
                if last_warning.elapsed() >= WARNING_INTERVAL {
                    eprintln!("[adapter-warning] {error}");
                    last_warning = Instant::now();
                }
                let update = engine.disconnected(error);
                state.publish(update.live, update.trace).await;
            }
        }
    }
}

#[cfg(not(windows))]
async fn collect_lmu(_store: DashboardStore, _state: DashboardState) {}

fn parse_config<I>(arguments: I) -> Result<DashboardConfig, String>
where
    I: IntoIterator<Item = String>,
{
    let mut listen = "127.0.0.1:8787"
        .parse::<SocketAddr>()
        .expect("default dashboard address must be valid");
    let mut data_dir = PathBuf::from("lmu-dashboard-data");
    let mut demo = !cfg!(windows);
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
            unknown => return Err(format!("unknown option {unknown}")),
        }
    }
    Ok(DashboardConfig {
        listen,
        data_dir,
        demo,
    })
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
       --listen <address>  Web address (default: 127.0.0.1:8787)\n\
       --data-dir <path>   SQLite data directory (default: lmu-dashboard-data)\n\
       --demo              Run without LMU using generated race data\n\
       --live              Read LMU_Data on the Windows game PC\n\
       --help              Show this help\n\n\
     Tablet example: --listen 0.0.0.0:8787"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_dashboard_options() {
        let config = parse_config([
            "--listen".to_owned(),
            "0.0.0.0:9000".to_owned(),
            "--data-dir".to_owned(),
            "telemetry".to_owned(),
            "--demo".to_owned(),
        ])
        .unwrap();
        assert_eq!(config.listen, "0.0.0.0:9000".parse().unwrap());
        assert_eq!(config.data_dir, PathBuf::from("telemetry"));
        assert!(config.demo);
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
    }
}
