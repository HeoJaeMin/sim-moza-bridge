use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

static SHUTDOWN_REQUESTED: AtomicBool = AtomicBool::new(false);
static CTRL_C_HANDLER: OnceLock<Result<(), String>> = OnceLock::new();

pub fn install_ctrl_c_handler() -> Result<(), String> {
    CTRL_C_HANDLER
        .get_or_init(|| {
            ctrlc::set_handler(request_shutdown)
                .map_err(|error| format!("failed to install Ctrl-C handler: {error}"))
        })
        .clone()
}

pub fn reset() {
    SHUTDOWN_REQUESTED.store(false, Ordering::SeqCst);
}

pub fn request_shutdown() {
    SHUTDOWN_REQUESTED.store(true, Ordering::SeqCst);
}

pub fn shutdown_requested() -> bool {
    SHUTDOWN_REQUESTED.load(Ordering::SeqCst)
}
