use super::*;
#[cfg(feature = "telemetry")]
use std::sync::{Mutex, MutexGuard};

#[cfg(feature = "telemetry")]
static OTLP_ENV_LOCK: Mutex<()> = Mutex::new(());

#[cfg(feature = "telemetry")]
struct EnvVarsGuard {
    previous: Vec<(&'static str, Option<String>)>,
    _lock: MutexGuard<'static, ()>,
}

#[cfg(feature = "telemetry")]
impl EnvVarsGuard {
    fn set(vars: &[(&'static str, Option<&str>)]) -> Self {
        let lock = OTLP_ENV_LOCK.lock().expect("env lock poisoned");
        let previous = vars
            .iter()
            .map(|(key, _)| (*key, std::env::var(key).ok()))
            .collect::<Vec<_>>();
        // SAFETY: serialized by OTLP_ENV_LOCK; tests restore previous values on drop.
        unsafe {
            for (key, value) in vars {
                match value {
                    Some(v) => std::env::set_var(key, v),
                    None => std::env::remove_var(key),
                }
            }
        }
        Self {
            previous,
            _lock: lock,
        }
    }
}

#[cfg(feature = "telemetry")]
impl Drop for EnvVarsGuard {
    fn drop(&mut self) {
        // SAFETY: serialized by OTLP_ENV_LOCK held for the guard lifetime.
        unsafe {
            for (key, value) in &self.previous {
                match value {
                    Some(v) => std::env::set_var(key, v),
                    None => std::env::remove_var(key),
                }
            }
        }
    }
}

#[test]
fn no_log_file_disables_default_file_logging() {
    let opts = FileLoggingCliOptions {
        no_log_file: true,
        ..FileLoggingCliOptions::default()
    };

    assert!(!should_enable_file_logging(&opts, false));
}

#[test]
fn parsed_no_log_file_has_no_implicit_retention_override() {
    let args = Args::try_parse_from([
        "dcc-mcp-server",
        "--no-log-file",
        "--gateway-port",
        "0",
        "--no-bridge",
    ])
    .expect("valid CLI args");
    let opts = FileLoggingCliOptions::from(&args.server);

    assert!(opts.log_retention_days.is_none());
    assert!(opts.log_max_total_size_mb.is_none());
    assert!(!should_enable_file_logging(&opts, false));
}

#[test]
fn explicit_log_option_overrides_no_log_file() {
    let opts = FileLoggingCliOptions {
        no_log_file: true,
        log_retention_days: Some(3),
        ..FileLoggingCliOptions::default()
    };

    assert!(should_enable_file_logging(&opts, false));
}

#[test]
fn env_logging_option_overrides_no_log_file() {
    let opts = FileLoggingCliOptions {
        no_log_file: true,
        ..FileLoggingCliOptions::default()
    };

    assert!(should_enable_file_logging(&opts, true));
}

#[cfg(feature = "telemetry")]
#[test]
fn resolved_otlp_config_loads_default_local_file() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join(DEFAULT_OTLP_CONFIG_FILE),
        r#"{
  "endpoint": "http://collector.local:4317",
  "service_name": "dcc-mcp-gateway",
  "headers": "authorization=Bearer token"
}"#,
    )
    .unwrap();
    let etc_dir = dir.path().to_string_lossy().to_string();
    let _env = EnvVarsGuard::set(&[
        ("OTEL_EXPORTER_OTLP_ENDPOINT", None),
        ("OTEL_SERVICE_NAME", None),
        ("OTEL_EXPORTER_OTLP_HEADERS", None),
        (ENV_DCC_MCP_ETC_DIR, Some(&etc_dir)),
    ]);

    let config = resolved_otlp_config();

    assert_eq!(
        config.endpoint.as_deref(),
        Some("http://collector.local:4317")
    );
    assert_eq!(config.service_name, "dcc-mcp-gateway");
    assert_eq!(
        config.headers.as_deref(),
        Some("authorization=Bearer token")
    );
    assert_eq!(
        parse_otlp_headers(config.headers.as_deref().unwrap())
            .get("authorization")
            .map(String::as_str),
        Some("Bearer token")
    );
}

#[cfg(all(feature = "gateway-auto", feature = "gateway-daemon"))]
#[test]
fn resolve_registry_dir_prefers_explicit_path() {
    let explicit = PathBuf::from(r"C:\dcc-mcp\registry");

    assert_eq!(resolve_registry_dir(Some(&explicit)), explicit);
}

#[cfg(all(feature = "gateway-auto", feature = "gateway-daemon"))]
#[test]
fn server_gateway_daemon_guardian_runs_only_in_daemon_backed_mode() {
    let mut args =
        Args::try_parse_from(["dcc-mcp-server", "--app", "maya"]).expect("valid CLI args");
    assert!(
        should_start_gateway_daemon_guardian(&args.server),
        "implicit auto mode should keep a daemon guardian alive"
    );

    args = Args::try_parse_from(["dcc-mcp-server", "auto", "--app", "houdini"])
        .expect("valid CLI args");
    let Some(SubCmd::Auto(auto_args)) = args.command else {
        panic!("expected auto subcommand");
    };
    assert!(
        should_start_gateway_daemon_guardian(&auto_args),
        "explicit auto mode should keep a daemon guardian alive"
    );

    let args = Args::try_parse_from(["dcc-mcp-server", "--app", "maya", "--gateway-port", "0"])
        .expect("valid CLI args");
    assert!(
        !should_start_gateway_daemon_guardian(&args.server),
        "--gateway-port 0 disables gateway participation"
    );

    let args = Args::try_parse_from(["dcc-mcp-server", "--app", "maya", "--no-ensure-gateway"])
        .expect("valid CLI args");
    assert!(
        !should_start_gateway_daemon_guardian(&args.server),
        "--no-ensure-gateway opts out of daemon launch and guardian"
    );

    let args = Args::try_parse_from([
        "dcc-mcp-server",
        "--app",
        "maya",
        "--legacy-gateway-election",
    ])
    .expect("valid CLI args");
    assert!(
        !should_start_gateway_daemon_guardian(&args.server),
        "legacy embedded election owns its own gateway loop"
    );

    let parsed = Args::try_parse_from([
        "dcc-mcp-server",
        "serve",
        "--app",
        "maya",
        "--no-auto-gateway",
    ])
    .expect("valid CLI args");
    let Some(SubCmd::Serve(serve)) = parsed.command else {
        panic!("expected serve subcommand");
    };
    assert!(
        !should_start_gateway_daemon_guardian(&serve.into_server_args()),
        "serve --no-auto-gateway must not start a daemon guardian"
    );
}

#[cfg(all(feature = "gateway-auto", feature = "gateway-daemon"))]
#[test]
fn server_gateway_daemon_options_preserve_cli_identity() {
    let args = Args::try_parse_from([
        "dcc-mcp-server",
        "--app",
        "maya",
        "--server-name",
        "maya-prod",
        "--host",
        "127.0.0.2",
        "--gateway-host",
        "0.0.0.0",
        "--gateway-name",
        "studio-gateway",
        "--gateway-remote-host",
        "10.0.0.10",
        "--gateway-remote-port",
        "59766",
        "--registry-dir",
        r"C:\dcc-mcp\registry",
    ])
    .expect("valid CLI args");
    let registry_dir_path = args.server.registry_dir.as_deref().map(PathBuf::from);

    let opts = build_server_gateway_daemon_options(&args.server, registry_dir_path.as_ref());
    assert_eq!(opts.host, "0.0.0.0");
    assert_eq!(opts.port, 9765);
    assert_eq!(opts.name.as_deref(), Some("studio-gateway"));
    assert_eq!(opts.registry_dir, PathBuf::from(r"C:\dcc-mcp\registry"));
    assert_eq!(opts.remote_host, "10.0.0.10");
    assert_eq!(opts.remote_port, 59766);

    let args = Args::try_parse_from(["dcc-mcp-server", "--server-name", "houdini-lookdev"])
        .expect("valid CLI args");
    let opts = build_server_gateway_daemon_options(&args.server, None);
    assert_eq!(opts.host, "127.0.0.1");
    assert_eq!(opts.name.as_deref(), Some("gateway-for-houdini-lookdev"));
}

#[cfg(all(feature = "gateway-auto", feature = "gateway-daemon"))]
#[test]
fn server_registration_metadata_reports_gateway_guardian_mode() {
    let args = Args::try_parse_from(["dcc-mcp-server", "--app", "maya"]).expect("valid CLI args");
    let mut entry = ServiceEntry::new("maya", "127.0.0.1", 18812);

    stamp_server_gateway_runtime_metadata(&mut entry, &args.server);

    assert_eq!(
        entry
            .metadata
            .get(GATEWAY_RUNTIME_MODE_METADATA_KEY)
            .map(String::as_str),
        Some("daemon-backed")
    );
    assert_eq!(
        entry
            .metadata
            .get(GATEWAY_GUARDIAN_ENABLED_METADATA_KEY)
            .map(String::as_str),
        Some("true")
    );
    assert_eq!(
        entry
            .metadata
            .get(GATEWAY_RECOVERY_DRIVER_METADATA_KEY)
            .map(String::as_str),
        Some(GATEWAY_RECOVERY_DRIVER_DAEMON_GUARDIAN)
    );
    assert_eq!(
        entry
            .metadata
            .get(REGISTRATION_REFRESH_MODE_METADATA_KEY)
            .map(String::as_str),
        Some(REGISTRATION_REFRESH_MODE_FILE_REGISTRY_HEARTBEAT)
    );
    assert_eq!(
        entry
            .metadata
            .get(SERVER_BINARY_VERSION_METADATA_KEY)
            .map(String::as_str),
        Some(env!("CARGO_PKG_VERSION"))
    );

    let args = Args::try_parse_from(["dcc-mcp-server", "--app", "maya", "--gateway-port", "0"])
        .expect("valid CLI args");
    stamp_server_gateway_runtime_metadata(&mut entry, &args.server);
    assert_eq!(
        entry
            .metadata
            .get(GATEWAY_RUNTIME_MODE_METADATA_KEY)
            .map(String::as_str),
        Some("not_configured")
    );
    assert_eq!(
        entry
            .metadata
            .get(GATEWAY_GUARDIAN_ENABLED_METADATA_KEY)
            .map(String::as_str),
        Some("false")
    );
    assert_eq!(
        entry
            .metadata
            .get(GATEWAY_RECOVERY_DRIVER_METADATA_KEY)
            .map(String::as_str),
        Some(GATEWAY_RECOVERY_DRIVER_NONE)
    );

    let args = Args::try_parse_from([
        "dcc-mcp-server",
        "--app",
        "maya",
        "--legacy-gateway-election",
    ])
    .expect("valid CLI args");
    stamp_server_gateway_runtime_metadata(&mut entry, &args.server);
    assert_eq!(
        entry
            .metadata
            .get(GATEWAY_RECOVERY_DRIVER_METADATA_KEY)
            .map(String::as_str),
        Some(GATEWAY_RECOVERY_DRIVER_EMBEDDED_ELECTION)
    );
}

// ── Cross-DCC regression: gateway runtime mode (PIP-488) ──────────────────
//
// Verify that daemon-backed registration stamps the right metadata for at
// least two DCC families so the admin UI, diagnostics, and agent tools
// do not encode Maya-only assumptions.

#[cfg(all(feature = "gateway-auto", feature = "gateway-daemon"))]
#[test]
fn multi_dcc_daemon_registration_metadata_is_consistent() {
    // Maya and Blender: both in default daemon-backed mode must stamp the
    // same metadata keys with DCC-appropriate values.
    for dcc in &["maya", "blender"] {
        let args = Args::try_parse_from(["dcc-mcp-server", "--app", dcc])
            .unwrap_or_else(|_| panic!("valid CLI args for {dcc}"));
        let mut entry = ServiceEntry::new(*dcc, "127.0.0.1", 18812);

        stamp_server_gateway_runtime_metadata(&mut entry, &args.server);

        assert_eq!(
            entry
                .metadata
                .get(GATEWAY_RUNTIME_MODE_METADATA_KEY)
                .map(String::as_str),
            Some("daemon-backed"),
            "{dcc}: default mode must be daemon-backed"
        );
        assert_eq!(
            entry
                .metadata
                .get(GATEWAY_GUARDIAN_ENABLED_METADATA_KEY)
                .map(String::as_str),
            Some("true"),
            "{dcc}: guardian must be enabled by default"
        );
        assert_eq!(
            entry
                .metadata
                .get(GATEWAY_RECOVERY_DRIVER_METADATA_KEY)
                .map(String::as_str),
            Some(GATEWAY_RECOVERY_DRIVER_DAEMON_GUARDIAN),
            "{dcc}: recovery driver must be daemon_guardian"
        );
    }
}

#[cfg(all(feature = "gateway-auto", feature = "gateway-daemon"))]
#[test]
fn multi_dcc_legacy_fallback_is_explicit_opt_in() {
    // Photoshop and ZBrush: `--legacy-gateway-election` must consistently
    // disable the daemon and stamp embedded_election across DCC families.
    for dcc in &["photoshop", "zbrush"] {
        let args =
            Args::try_parse_from(["dcc-mcp-server", "--app", dcc, "--legacy-gateway-election"])
                .unwrap_or_else(|_| panic!("valid CLI args for {dcc}"));
        let mut entry = ServiceEntry::new(*dcc, "127.0.0.1", 18813);

        stamp_server_gateway_runtime_metadata(&mut entry, &args.server);

        assert_eq!(
            entry
                .metadata
                .get(GATEWAY_RUNTIME_MODE_METADATA_KEY)
                .map(String::as_str),
            Some("embedded-fallback"),
            "{dcc}: legacy mode must stamp embedded-fallback"
        );
        assert_eq!(
            entry
                .metadata
                .get(GATEWAY_GUARDIAN_ENABLED_METADATA_KEY)
                .map(String::as_str),
            Some("false"),
            "{dcc}: guardian must be disabled in legacy mode"
        );
        assert_eq!(
            entry
                .metadata
                .get(GATEWAY_RECOVERY_DRIVER_METADATA_KEY)
                .map(String::as_str),
            Some(GATEWAY_RECOVERY_DRIVER_EMBEDDED_ELECTION),
            "{dcc}: recovery driver must be embedded_election in legacy mode"
        );
    }
}

#[cfg(all(feature = "gateway-auto", feature = "gateway-daemon"))]
#[test]
fn multi_dcc_gateway_port_zero_disables_guardian() {
    for dcc in &["maya", "houdini"] {
        let args = Args::try_parse_from(["dcc-mcp-server", "--app", dcc, "--gateway-port", "0"])
            .unwrap_or_else(|_| panic!("valid CLI args for {dcc}"));
        let mut entry = ServiceEntry::new(*dcc, "127.0.0.1", 18814);

        stamp_server_gateway_runtime_metadata(&mut entry, &args.server);

        assert_eq!(
            entry
                .metadata
                .get(GATEWAY_RUNTIME_MODE_METADATA_KEY)
                .map(String::as_str),
            Some("not_configured"),
            "{dcc}: gateway_port=0 must stamp not_configured"
        );
        assert_eq!(
            entry
                .metadata
                .get(GATEWAY_RECOVERY_DRIVER_METADATA_KEY)
                .map(String::as_str),
            Some(GATEWAY_RECOVERY_DRIVER_NONE),
            "{dcc}: recovery driver must be none when port is 0"
        );
    }
}
