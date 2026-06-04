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
/// Returns `true` when Sentry was successfully initialised.
pub fn init_sentry() -> bool {
    let Some(dsn) = std::env::var("DCC_MCP_SENTRY_DSN")
        .ok()
        .filter(|s| !s.trim().is_empty())
    else {
        return false;
    };

    let environment: std::borrow::Cow<'static, str> = std::env::var("DCC_MCP_SENTRY_ENVIRONMENT")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .map(Into::into)
        .unwrap_or_else(|| "production".into());

    let release: std::borrow::Cow<'static, str> = std::env::var("DCC_MCP_SENTRY_RELEASE")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .map(Into::into)
        .unwrap_or_else(|| env!("CARGO_PKG_VERSION").into());

    let sample_rate: f32 = std::env::var("DCC_MCP_SENTRY_SAMPLE_RATE")
        .ok()
        .and_then(|s| s.parse::<f32>().ok())
        .filter(|r| r.is_finite() && *r >= 0.0 && *r <= 1.0)
        .unwrap_or(1.0);

    let dsn_log = dsn.clone();

    let guard = sentry::init(sentry::ClientOptions {
        dsn: Some(
            dsn.parse()
                .expect("DCC_MCP_SENTRY_DSN must be a valid Sentry DSN"),
        ),
        release: Some(release),
        environment: Some(environment),
        traces_sample_rate: sample_rate,
        ..Default::default()
    });

    // Leak the guard so Sentry stays alive for the process lifetime.
    std::mem::forget(guard);

    tracing::info!(
        dsn = %dsn_log,
        sample_rate,
        "sentry initialised",
    );

    true
}
