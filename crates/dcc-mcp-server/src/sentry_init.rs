//! Sentry error monitoring (Rust backend).
//!
//! Initialises the Sentry SDK when `DCC_MCP_SENTRY_DSN` is set at startup.
//! Panics are automatically captured and dispatched to the configured Sentry
//! project. Use `sentry::capture_error` or `sentry::capture_message` for
//! explicit instrumentation points.
//!
//! ## Environment variables
//!
//! | Variable | Default | Purpose |
//! |----------|---------|---------|
//! | `DCC_MCP_SENTRY_DSN` | (disabled) | Sentry project DSN |
//! | `DCC_MCP_SENTRY_ENVIRONMENT` | `production` | Environment tag |
//! | `DCC_MCP_SENTRY_RELEASE` | crate version | Release identifier |
//! | `DCC_MCP_SENTRY_SAMPLE_RATE` | `1.0` | Error sample rate (0.0–1.0) |

/// Initialise the Sentry SDK if `DCC_MCP_SENTRY_DSN` is set.
///
/// Returns the client guard that must be held for the process lifetime.
/// Dropping the guard flushes pending events and shuts down the SDK.
pub fn init_sentry() -> Option<sentry::ClientInitGuard> {
    let dsn = std::env::var("DCC_MCP_SENTRY_DSN")
        .ok()
        .filter(|s| !s.trim().is_empty())?;

    let environment = std::env::var("DCC_MCP_SENTRY_ENVIRONMENT")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "production".into());

    let release = std::env::var("DCC_MCP_SENTRY_RELEASE")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| env!("CARGO_PKG_VERSION").into());

    let sample_rate = std::env::var("DCC_MCP_SENTRY_SAMPLE_RATE")
        .ok()
        .and_then(|s| s.parse::<f32>().ok())
        .filter(|r| r.is_finite() && *r >= 0.0 && *r <= 1.0)
        .unwrap_or(1.0);

    let parsed_dsn: sentry::types::Dsn = match dsn.parse() {
        Ok(dsn) => dsn,
        Err(err) => {
            tracing::warn!(
                error = %err,
                "DCC_MCP_SENTRY_DSN is not a valid Sentry DSN; skipping Sentry init"
            );
            return None;
        }
    };

    let guard = sentry::init(sentry::ClientOptions {
        dsn: Some(parsed_dsn),
        release: Some(release.into()),
        environment: Some(environment.into()),
        sample_rate,
        ..Default::default()
    });

    tracing::info!(
        sentry_enabled = true,
        environment = %std::env::var("DCC_MCP_SENTRY_ENVIRONMENT").unwrap_or_else(|_| "production".into()),
        sample_rate,
        "sentry initialised",
    );

    Some(guard)
}
