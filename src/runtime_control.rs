use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

pub type ShutdownToken = Arc<AtomicBool>;

pub fn new_shutdown_token() -> ShutdownToken {
    Arc::new(AtomicBool::new(false))
}

pub fn never_stop_token() -> ShutdownToken {
    new_shutdown_token()
}

pub fn request_shutdown(token: &ShutdownToken) {
    token.store(true, Ordering::Release);
}

pub fn shutdown_requested(token: &ShutdownToken) -> bool {
    token.load(Ordering::Acquire)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shutdown_request_is_shared_across_token_clones() {
        let token = new_shutdown_token();
        let worker_token = Arc::clone(&token);

        assert!(!shutdown_requested(&worker_token));
        request_shutdown(&token);
        assert!(shutdown_requested(&worker_token));
    }
}
