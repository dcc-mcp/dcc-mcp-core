//! Logging configuration using Rust `tracing` — replaces loguru.
//!
//! The subscriber is assembled exactly once per process:
//!
//! ```text
//! Registry
//!   ├── fmt::Layer  → stderr (always on)
//!   ├── reload::Layer<Option<FileLayer>>  → disabled initially
//!   └── TracyLayer  → local on-demand profiler (`tracy` feature only)
//! ```
//!
//! The reload layer lets [`crate::file_logging::init_file_logging`]
//! attach (or swap) a rolling-file layer **after** the subscriber has
//! already been installed by the Python module-init path in
//! `dcc_mcp_core._core`. See [`reload_handle`].

use crate::constants::{DEFAULT_LOG_LEVEL, ENV_LOG_LEVEL, LEGACY_ENV_LOG_LEVEL};
use std::sync::OnceLock;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::reload::{self, Handle};
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Layer};

#[cfg(feature = "tracy")]
use tracing_subscriber::fmt::format::DefaultFields;

#[cfg(feature = "tracy")]
const TRACY_TARGET: &str = "dcc_mcp::profiling";

#[cfg(feature = "tracy")]
fn is_tracy_profile(target: &str, is_span: bool) -> bool {
    is_span && target == TRACY_TARGET
}

#[cfg(feature = "tracy")]
fn is_tracy_metadata(metadata: &tracing::Metadata<'_>) -> bool {
    is_tracy_profile(metadata.target(), metadata.is_span())
}

#[cfg(feature = "tracy")]
fn enable_tracy_target(filter: EnvFilter) -> EnvFilter {
    filter.add_directive(
        format!("{TRACY_TARGET}=trace")
            .parse()
            .expect("static Tracy filter directive must be valid"),
    )
}

#[cfg(feature = "tracy")]
#[derive(Default)]
struct TracyConfig(DefaultFields);

#[cfg(feature = "tracy")]
impl tracing_tracy::Config for TracyConfig {
    type Formatter = DefaultFields;

    fn formatter(&self) -> &Self::Formatter {
        &self.0
    }

    fn stack_depth(&self, _metadata: &tracing::Metadata<'_>) -> u16 {
        0
    }

    fn format_fields_in_zone_name(&self) -> bool {
        false
    }
}

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
/// - an `EnvFilter` driven by `DCC_MCP_LOG_LEVEL` (fallback [`DEFAULT_LOG_LEVEL`]);
/// - a stderr `fmt::Layer` (thread names, targets on);
/// - a [`reload::Layer`] holding an `Option<BoxedLayer>` for dynamic
///   attachment of a rolling-file layer by
///   [`crate::file_logging::init_file_logging`].
/// - a target-filtered local Tracy layer when the `tracy` feature is enabled.
///
/// Safe to call multiple times — subsequent calls are no-ops thanks to
/// the internal [`std::sync::Once`].
pub fn init_logging() {
    INIT.call_once(|| {
        let filter = EnvFilter::try_from_env(ENV_LOG_LEVEL)
            .or_else(|_| EnvFilter::try_from_env(LEGACY_ENV_LOG_LEVEL))
            .unwrap_or_else(|_| EnvFilter::new(DEFAULT_LOG_LEVEL));

        #[cfg(feature = "tracy")]
        let filter = enable_tracy_target(filter);

        let fmt_layer = tracing_subscriber::fmt::layer()
            .with_target(true)
            .with_thread_names(true)
            .with_ansi(false)
            .with_writer(std::io::stderr);

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
        let subscriber = tracing_subscriber::registry()
            .with(file_layer)
            .with(filter)
            .with(fmt_layer);

        #[cfg(feature = "tracy")]
        let subscriber = subscriber.with(
            tracing_tracy::TracyLayer::new(TracyConfig::default())
                .with_filter(tracing_subscriber::filter::filter_fn(is_tracy_metadata)),
        );

        let _ = subscriber.try_init();
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
                "tracing subscriber not initialized — call dcc_mcp_logging::init_logging() first",
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

    #[cfg(feature = "tracy")]
    #[test]
    fn tracy_config_keeps_zone_names_stable_without_callstacks() {
        use tracing_tracy::Config as _;

        assert!(is_tracy_profile(TRACY_TARGET, true));
        assert!(!is_tracy_profile("dcc_mcp::gateway", true));
        assert!(!is_tracy_profile(TRACY_TARGET, false));
        assert!(
            enable_tracy_target(EnvFilter::new("off"))
                .to_string()
                .contains("dcc_mcp::profiling=trace")
        );

        let config = TracyConfig::default();
        assert!(!config.format_fields_in_zone_name());

        tracing::subscriber::with_default(tracing_subscriber::registry(), || {
            let span = tracing::info_span!(target: "dcc_mcp::profiling", "config_test");
            assert_eq!(config.stack_depth(span.metadata().unwrap()), 0);
        });
    }
}
