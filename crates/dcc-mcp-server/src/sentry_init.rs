//! Sentry error monitoring (Rust backend).
//!
//! Initialises the Sentry SDK when `DCC_MCP_SENTRY_DSN` or local
//! `~/dcc-mcp/etc/sentry.json` config is set at startup.
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
//! | `DCC_MCP_ETC_DIR` | `~/dcc-mcp/etc` | Admin UI local config directory |

use std::path::PathBuf;

use serde::Deserialize;

const ENV_DCC_MCP_ETC_DIR: &str = "DCC_MCP_ETC_DIR";
const DEFAULT_SENTRY_CONFIG_FILE: &str = "sentry.json";

#[derive(Debug, Default, Deserialize)]
struct LocalSentryConfig {
    dsn: Option<String>,
    environment: Option<String>,
    release: Option<String>,
    sample_rate: Option<f32>,
}

#[derive(Debug)]
struct ResolvedSentryConfig {
    dsn: String,
    environment: String,
    release: String,
    sample_rate: f32,
}

/// Initialise the Sentry SDK if `DCC_MCP_SENTRY_DSN` is set.
///
/// Returns the client guard that must be held for the process lifetime.
/// Dropping the guard flushes pending events and shuts down the SDK.
pub fn init_sentry() -> Option<sentry::ClientInitGuard> {
    let config = resolved_sentry_config()?;

    let parsed_dsn: sentry::types::Dsn = match config.dsn.parse() {
        Ok(dsn) => dsn,
        Err(err) => {
            tracing::warn!(
                error = %err,
                "Sentry DSN is not valid; skipping Sentry init"
            );
            return None;
        }
    };

    let guard = sentry::init(sentry::ClientOptions {
        dsn: Some(parsed_dsn),
        release: Some(config.release.clone().into()),
        environment: Some(config.environment.clone().into()),
        sample_rate: config.sample_rate,
        ..Default::default()
    });

    tracing::info!(
        sentry_enabled = true,
        environment = %config.environment,
        sample_rate = config.sample_rate,
        "sentry initialised",
    );

    Some(guard)
}

fn resolved_sentry_config() -> Option<ResolvedSentryConfig> {
    let local = match read_local_sentry_config() {
        Ok(config) => config,
        Err(err) => {
            tracing::warn!(error = %err, "local Sentry config ignored");
            None
        }
    };

    let dsn = env_string("DCC_MCP_SENTRY_DSN").or_else(|| {
        local
            .as_ref()
            .and_then(|config| clean_string(config.dsn.as_deref()))
    })?;
    let environment = env_string("DCC_MCP_SENTRY_ENVIRONMENT")
        .or_else(|| {
            local
                .as_ref()
                .and_then(|config| clean_string(config.environment.as_deref()))
        })
        .unwrap_or_else(|| "production".into());
    let release = env_string("DCC_MCP_SENTRY_RELEASE")
        .or_else(|| {
            local
                .as_ref()
                .and_then(|config| clean_string(config.release.as_deref()))
        })
        .unwrap_or_else(|| env!("CARGO_PKG_VERSION").into());
    let sample_rate = env_string("DCC_MCP_SENTRY_SAMPLE_RATE")
        .and_then(|s| s.parse::<f32>().ok())
        .or_else(|| local.as_ref().and_then(|config| config.sample_rate))
        .filter(|r| r.is_finite() && *r >= 0.0 && *r <= 1.0)
        .unwrap_or(1.0);

    Some(ResolvedSentryConfig {
        dsn,
        environment,
        release,
        sample_rate,
    })
}

fn read_local_sentry_config() -> anyhow::Result<Option<LocalSentryConfig>> {
    let Some(path) = default_sentry_config_path() else {
        return Ok(None);
    };
    if !path.exists() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(&path)?;
    let config = serde_json::from_str(&raw)?;
    Ok(Some(config))
}

fn default_sentry_config_path() -> Option<PathBuf> {
    integration_etc_dir().map(|dir| dir.join(DEFAULT_SENTRY_CONFIG_FILE))
}

fn integration_etc_dir() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os(ENV_DCC_MCP_ETC_DIR).filter(|value| !value.is_empty()) {
        return Some(PathBuf::from(path));
    }
    home_dir().map(|home| home.join("dcc-mcp").join("etc"))
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            let drive = std::env::var_os("HOMEDRIVE")?;
            let path = std::env::var_os("HOMEPATH")?;
            Some(PathBuf::from(format!(
                "{}{}",
                drive.to_string_lossy(),
                path.to_string_lossy()
            )))
        })
        .or_else(|| {
            std::env::var_os("HOME")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
        })
}

fn env_string(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .and_then(|value| clean_string(Some(value.as_str())))
}

fn clean_string(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

#[cfg(all(test, feature = "sentry"))]
mod tests {
    use super::{
        DEFAULT_SENTRY_CONFIG_FILE, ENV_DCC_MCP_ETC_DIR, init_sentry, resolved_sentry_config,
    };
    use dcc_mcp_test_utils::{EnvVarGuard, EnvVarsGuard};
    use std::time::Duration;

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

    #[test]
    fn resolved_sentry_config_loads_default_local_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(DEFAULT_SENTRY_CONFIG_FILE),
            r#"{
  "dsn": "https://abc123@sentry.example/42",
  "environment": "studio",
  "release": "0.18.0",
  "sample_rate": 0.25
}"#,
        )
        .unwrap();
        let etc_dir = dir.path().to_string_lossy().to_string();
        let _guard = EnvVarsGuard::set(&[
            ("DCC_MCP_SENTRY_DSN", None),
            ("DCC_MCP_SENTRY_ENVIRONMENT", None),
            ("DCC_MCP_SENTRY_RELEASE", None),
            ("DCC_MCP_SENTRY_SAMPLE_RATE", None),
            (ENV_DCC_MCP_ETC_DIR, Some(&etc_dir)),
        ]);

        let config = resolved_sentry_config().unwrap();

        assert_eq!(config.dsn, "https://abc123@sentry.example/42");
        assert_eq!(config.environment, "studio");
        assert_eq!(config.release, "0.18.0");
        assert_eq!(config.sample_rate, 0.25);
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

        let _g = EnvVarsGuard::set(&[
            ("DCC_MCP_SENTRY_DSN", Some(&dsn)),
            ("DCC_MCP_SENTRY_ENVIRONMENT", Some("ci-e2e")),
            ("DCC_MCP_SENTRY_SAMPLE_RATE", Some("1.0")),
        ]);

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
