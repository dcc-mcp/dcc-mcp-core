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

#[cfg(all(test, feature = "sentry"))]
mod tests {
    use super::init_sentry;
    use std::sync::{Mutex, MutexGuard};
    use std::time::Duration;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    struct EnvVarGuard {
        key: &'static str,
        previous: Option<String>,
        _lock: MutexGuard<'static, ()>,
    }

    impl EnvVarGuard {
        fn set(key: &'static str, value: Option<&str>) -> Self {
            let lock = ENV_LOCK.lock().expect("env lock poisoned");
            let previous = std::env::var(key).ok();
            // SAFETY: serialized by ENV_LOCK; tests restore previous values on drop.
            unsafe {
                match value {
                    Some(v) => std::env::set_var(key, v),
                    None => std::env::remove_var(key),
                }
            }
            Self {
                key,
                previous,
                _lock: lock,
            }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            // SAFETY: serialized by ENV_LOCK held for the guard lifetime.
            unsafe {
                match &self.previous {
                    Some(v) => std::env::set_var(self.key, v),
                    None => std::env::remove_var(self.key),
                }
            }
        }
    }

    #[test]
    fn init_sentry_absent_dsn_returns_none() {
        let _guard = EnvVarGuard::set("DCC_MCP_SENTRY_DSN", None);
        assert!(init_sentry().is_none());
    }

    #[test]
    fn init_sentry_blank_dsn_returns_none() {
        let _guard = EnvVarGuard::set("DCC_MCP_SENTRY_DSN", Some("   "));
        assert!(init_sentry().is_none());
    }

    #[test]
    fn init_sentry_invalid_dsn_returns_none() {
        let _guard = EnvVarGuard::set("DCC_MCP_SENTRY_DSN", Some("not-a-valid-dsn"));
        assert!(init_sentry().is_none());
    }

    /// Validates that `init_sentry` with a well-formed DSN initialises the SDK
    /// and captures events without requiring a live ingest endpoint.
    ///
    /// Uses a syntactically valid DSN that points to a non-existent project.
    /// The SDK initialises anyway and assigns event ids; only the transport
    /// flush is expected to fail (no network reachability assertion).
    /// This test runs in every PR CI regardless of secret availability.
    #[test]
    fn init_sentry_valid_dsn_initialises_and_captures() {
        let _guard = EnvVarGuard::set(
            "DCC_MCP_SENTRY_DSN",
            Some("https://key@o0.ingest.sentry.io/0"),
        );
        let guard = init_sentry().expect("valid-format DSN should initialise Sentry SDK");

        let event_id =
            sentry::capture_message("unit test probe — no real DSN", sentry::Level::Info);
        assert!(
            !event_id.is_nil(),
            "Sentry SDK should assign a non-nil event id for captured messages"
        );

        // Flush may fail (no live ingest) — that is expected for a unit test.
        drop(guard);
    }

    /// Real Sentry ingest E2E — posts a probe event and flushes the transport.
    ///
    /// Skips when `DCC_MCP_SENTRY_DSN` is unset (local dev / PR CI without the
    /// secret). The dedicated `sentry-e2e` workflow job sets the secret from
    /// GitHub Actions and runs this test with `--test-threads=1`.
    #[test]
    fn sentry_real_ingest_e2e() {
        let dsn = match std::env::var("DCC_MCP_SENTRY_DSN") {
            Ok(v) if !v.trim().is_empty() => v,
            _ => {
                eprintln!("SKIP sentry_real_ingest_e2e: DCC_MCP_SENTRY_DSN not set");
                return;
            }
        };

        let _lock = ENV_LOCK.lock().expect("env lock poisoned");
        // SAFETY: serialized by ENV_LOCK; e2e probe restores env when the lock drops.
        unsafe {
            std::env::set_var("DCC_MCP_SENTRY_DSN", &dsn);
            std::env::set_var("DCC_MCP_SENTRY_ENVIRONMENT", "ci-e2e");
            std::env::set_var("DCC_MCP_SENTRY_SAMPLE_RATE", "1.0");
        }

        let guard = init_sentry().expect("valid DCC_MCP_SENTRY_DSN should initialise Sentry");
        let probe = format!("dcc-mcp-core sentry e2e probe {}", uuid::Uuid::new_v4());
        let event_id = sentry::capture_message(&probe, sentry::Level::Info);
        assert!(
            !event_id.is_nil(),
            "Sentry SDK should assign an event id for captured messages"
        );

        let flushed = guard.flush(Some(Duration::from_secs(15)));
        assert!(
            flushed,
            "Sentry transport flush should succeed for real ingest (dsn host reachable)"
        );
    }
}
