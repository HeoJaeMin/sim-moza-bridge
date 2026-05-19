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

    if let Err(error) = udp::start_udp_bridge(config) {
        eprintln!("[startup-error] {error}");
        std::process::exit(1);
    }
}
