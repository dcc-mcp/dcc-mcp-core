//! Logging configuration using Rust `tracing` — replaces loguru.

use crate::constants::{DEFAULT_LOG_LEVEL, ENV_LOG_LEVEL};
use tracing_subscriber::EnvFilter;

static INIT: std::sync::Once = std::sync::Once::new();

/// Initialize the tracing subscriber (called once from Python module init).
///
/// Falls back to [`DEFAULT_LOG_LEVEL`] when the `ENV_LOG_LEVEL` environment
/// variable is not set or cannot be parsed.
pub fn init_logging() {
    INIT.call_once(|| {
        let filter = EnvFilter::try_from_env(ENV_LOG_LEVEL)
            .unwrap_or_else(|_| EnvFilter::new(DEFAULT_LOG_LEVEL));

        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_target(true)
            .with_thread_names(true)
            .try_init()
            .ok(); // Ignore if already initialized
    });
}
