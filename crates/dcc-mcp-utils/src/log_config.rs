//! Logging configuration using Rust `tracing` — replaces loguru.
//!
//! The subscriber is assembled exactly once per process:
//!
//! ```text
//! Registry
//!   ├── fmt::Layer  → stderr (always on)
//!   └── reload::Layer<Option<FileLayer>>  → disabled initially
//! ```
//!
//! The reload layer lets [`crate::file_logging::init_file_logging`]
//! attach (or swap) a rolling-file layer **after** the subscriber has
//! already been installed by the Python module-init path in
//! `dcc_mcp_core._core`. See [`reload_handle`].

use crate::constants::{DEFAULT_LOG_LEVEL, ENV_LOG_LEVEL};
use std::sync::OnceLock;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::reload::{self, Handle};
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Layer};

/// Type-erased subscriber-agnostic layer installed behind the reload handle.
///
/// We keep it boxed so `file_logging` can hand us any combination of
/// `fmt::Layer` variants (plain, JSON, custom writers) without the caller
/// having to name the exact generic parameters.
pub type BoxedLayer<S> = Box<dyn Layer<S> + Send + Sync + 'static>;

/// Default subscriber type used across the crate.
type DefaultSubscriber = tracing_subscriber::Registry;

/// Handle for swapping the optional file-logging layer at runtime.
type FileLayerReloadHandle = Handle<Option<BoxedLayer<DefaultSubscriber>>, DefaultSubscriber>;

static INIT: std::sync::Once = std::sync::Once::new();
static RELOAD_HANDLE: OnceLock<FileLayerReloadHandle> = OnceLock::new();

/// Initialize the tracing subscriber (called once from Python module init).
///
/// Installs:
/// - an `EnvFilter` driven by `MCP_LOG_LEVEL` (fallback [`DEFAULT_LOG_LEVEL`]);
/// - a stderr `fmt::Layer` (thread names, targets on);
/// - a [`reload::Layer`] holding an `Option<BoxedLayer>` for dynamic
///   attachment of a rolling-file layer by
///   [`crate::file_logging::init_file_logging`].
///
/// Safe to call multiple times — subsequent calls are no-ops thanks to
/// the internal [`std::sync::Once`].
pub fn init_logging() {
    INIT.call_once(|| {
        let filter = EnvFilter::try_from_env(ENV_LOG_LEVEL)
            .unwrap_or_else(|_| EnvFilter::new(DEFAULT_LOG_LEVEL));

        let fmt_layer = tracing_subscriber::fmt::layer()
            .with_target(true)
            .with_thread_names(true);

        // The slot is `None` until a caller opts into file logging.
        let (file_layer, handle) =
            reload::Layer::<Option<BoxedLayer<DefaultSubscriber>>, _>::new(None);

        let _ = RELOAD_HANDLE.set(handle);

        // `try_init` swallows the "global default already set" error so
        // repeated calls (e.g. from embedded hosts that re-import the
        // Python module) stay silent.
        //
        // Layer order matters: `reload::Layer<_, Registry>` is fixed to
        // `Layer<Registry>` so it MUST be attached directly on top of
        // `Registry`. Generic layers (`EnvFilter`, `fmt::Layer`) are
        // composed above it.
        let _ = tracing_subscriber::registry()
            .with(file_layer)
            .with(filter)
            .with(fmt_layer)
            .try_init();
    });
}

/// Access the reload handle for the optional file-logging layer.
///
/// Returns `None` when [`init_logging`] has not yet run. Callers that
/// want to guarantee availability should call [`init_logging`] first
/// (it's idempotent).
pub fn reload_handle() -> Option<&'static FileLayerReloadHandle> {
    RELOAD_HANDLE.get()
}

/// Install (or swap) a file-logging layer specialized for the default subscriber.
///
/// This is the variant used by [`crate::file_logging`]. Passing `None`
/// disables file logging without touching the console layer.
///
/// # Errors
/// - [`FileLayerInstallError::NotInitialized`] if [`init_logging`] hasn't run.
/// - [`FileLayerInstallError::Reload`] if `reload::Handle::reload` fails.
pub fn install_file_layer_boxed(
    layer: Option<BoxedLayer<DefaultSubscriber>>,
) -> Result<(), FileLayerInstallError> {
    let handle = RELOAD_HANDLE
        .get()
        .ok_or(FileLayerInstallError::NotInitialized)?;
    handle
        .reload(layer)
        .map_err(|e| FileLayerInstallError::Reload(e.to_string()))
}

/// Errors produced when swapping the file-logging layer.
#[derive(Debug)]
#[non_exhaustive]
pub enum FileLayerInstallError {
    /// [`init_logging`] has not been called yet — no reload handle exists.
    NotInitialized,
    /// The tracing-subscriber reload mechanism rejected the swap.
    Reload(String),
}

impl std::fmt::Display for FileLayerInstallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotInitialized => f.write_str(
                "tracing subscriber not initialized — call dcc_mcp_utils::log_config::init_logging() first",
            ),
            Self::Reload(msg) => write!(f, "failed to reload file-logging layer: {msg}"),
        }
    }
}

impl std::error::Error for FileLayerInstallError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_init_logging_is_idempotent() {
        init_logging();
        init_logging();
        assert!(reload_handle().is_some());
    }
}
