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
    match config::read_config().and_then(udp::start_udp_bridge) {
        Ok(()) => {}
        Err(error) => {
            eprintln!("[startup-error] {error}");
            std::process::exit(1);
        }
    }
}
