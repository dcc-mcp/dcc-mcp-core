//! Logging configuration using Rust `tracing` — replaces loguru.

use tracing_subscriber::EnvFilter;

static INIT: std::sync::Once = std::sync::Once::new();

/// Initialize the tracing subscriber (called once from Python module init).
pub fn init_logging() {
    INIT.call_once(|| {
        let filter = EnvFilter::try_from_env("MCP_LOG_LEVEL")
            .unwrap_or_else(|_| EnvFilter::new("warn"));

        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_target(true)
            .with_thread_names(true)
            .try_init()
            .ok(); // Ignore if already initialized
    });
}
