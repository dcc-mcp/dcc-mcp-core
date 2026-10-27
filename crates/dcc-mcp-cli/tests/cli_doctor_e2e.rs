//! End-to-end coverage for `dcc-mcp-cli doctor` adapter import probes.
//!
//! Kept separate from `cli_e2e.rs` so that file stays under the repository's
//! Rust test-file line limit.

mod support;

use dcc_mcp_transport::discovery::file_registry::FileRegistry;
use dcc_mcp_transport::discovery::types::ServiceEntry;
use tempfile::{NamedTempFile, TempDir};

use support::*;

#[test]
fn explicit_registry_commands_do_not_require_user_home() {
    let fixture = spawn_gateway_fixture();
    let port = fixture.base_url.rsplit(':').next().unwrap();
    let registry = TempDir::new().unwrap();
    let profiles = NamedTempFile::new().unwrap();
    let registry_s = registry.path().to_str().unwrap();
    let profiles_s = profiles.path().to_str().unwrap();
    for args in [
        vec!["doctor", "--gateway-port", port],
        vec!["gateway", "status", "--port", port],
        vec!["gateway", "daemon", "status", "--port", port],
    ] {
        let mut command = vec![
            "--no-update-maintenance",
            "--auto-gateway-bin",
            env!("CARGO_BIN_EXE_dcc-mcp-cli"),
        ];
        command.extend(args);
        command.extend(["--registry-dir", registry_s]);
        let result = run_json_with_env_removed(
            &command,
            &[
                (
                    if cfg!(windows) { "USERPROFILE" } else { "HOME" },
                    "relative-home",
                ),
                ("DCC_MCP_GATEWAY_PROFILES_FILE", profiles_s),
                ("DCC_MCP_PYTHON_EXECUTABLE", ""),
            ],
            &[
                "DCC_MCP_REGISTRY_DIR",
                "DCC_MCP_GATEWAY_PROFILE",
                "DCC_MCP_BASE_URL",
            ],
        );
        if let Some(local) = result.get("local") {
            assert_eq!(local["registry_dir"], registry_s);
        } else {
            assert_eq!(result["healthy"], true);
        }
    }
}

#[tokio::test]
async fn doctor_healthy_gateway_does_not_repair_registry_to_recover_missing_pidfile() {
    let registry = TempDir::new().unwrap();
    let path = registry.path().join("services.json");
    std::fs::write(&path, "private-invalid-json").unwrap();
    std::fs::create_dir(registry.path().join("services.lock")).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let app = axum::Router::new().route("/health", axum::routing::get(|| async { "ok" }));
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let profiles = NamedTempFile::new().unwrap();
    let registry_s = registry.path().to_string_lossy().to_string();
    let profiles_s = profiles.path().to_string_lossy().to_string();
    let doctor = tokio::task::spawn_blocking(move || {
        run_json_with_env(
            &[
                "--auto-gateway-bin",
                env!("CARGO_BIN_EXE_dcc-mcp-cli"),
                "doctor",
                "--gateway-port",
                &port.to_string(),
            ],
            &[
                ("DCC_MCP_REGISTRY_DIR", &registry_s),
                ("DCC_MCP_GATEWAY_PROFILES_FILE", &profiles_s),
                ("DCC_MCP_PYTHON_EXECUTABLE", ""),
            ],
        )
    })
    .await
    .unwrap();
    task.abort();
    assert_eq!(doctor["gateway"]["status"]["healthy"], true);
    assert_eq!(doctor["gateway"]["status"]["pid"], serde_json::Value::Null);
    assert_eq!(doctor["local"]["inventory"]["ok"], false);
    assert_eq!(
        std::fs::read_to_string(path).unwrap(),
        "private-invalid-json"
    );
    assert_eq!(std::fs::read_dir(registry.path()).unwrap().count(), 2);
}

#[test]
fn doctor_registry_snapshot_does_not_create_prune_or_quarantine() {
    for content in [None, Some("invalid-private-registry"), Some("\0\0")] {
        let root = TempDir::new().unwrap();
        let registry = root.path().join("registry");
        if let Some(content) = content {
            std::fs::create_dir(&registry).unwrap();
            std::fs::write(registry.join("services.json"), content).unwrap();
        }
        let profiles = NamedTempFile::new().unwrap();
        let registry_s = registry.to_string_lossy().to_string();
        let profiles_s = profiles.path().to_string_lossy().to_string();
        let port = unused_loopback_port().to_string();
        let doctor = run_json_with_env(
            &[
                "--auto-gateway-bin",
                env!("CARGO_BIN_EXE_dcc-mcp-cli"),
                "doctor",
                "--gateway-port",
                &port,
            ],
            &[
                ("DCC_MCP_REGISTRY_DIR", &registry_s),
                ("DCC_MCP_GATEWAY_PROFILES_FILE", &profiles_s),
                ("DCC_MCP_PYTHON_EXECUTABLE", ""),
            ],
        );
        let inventory = &doctor["local"]["inventory"];
        assert_eq!(inventory["mode"], "read_only_snapshot");
        if let Some(content) = content {
            assert_eq!(inventory["ok"], false);
            assert_eq!(inventory["operation"], "read_registry_snapshot");
            assert_eq!(inventory["os_error"], serde_json::Value::Null);
            assert!(
                !inventory["error"]
                    .as_str()
                    .unwrap()
                    .contains("invalid-private")
            );
            assert_eq!(
                std::fs::read_to_string(registry.join("services.json")).unwrap(),
                content
            );
            assert_eq!(std::fs::read_dir(&registry).unwrap().count(), 1);
        } else {
            assert_eq!(inventory["ok"], true);
            assert_eq!(inventory["total"], 0);
            assert!(!registry.exists());
        }
    }
}

/// A registered adapter with no configured interpreter must not be reported as
/// broken: an unrun probe is not evidence of a failing adapter.
#[test]
fn doctor_adapter_imports_skip_without_an_interpreter() {
    let port = unused_loopback_port();
    let port_s = port.to_string();
    let registry = TempDir::new().unwrap();
    let registry_s = registry.path().to_string_lossy().to_string();
    let profiles = NamedTempFile::new().unwrap();
    let profiles_s = profiles.path().to_string_lossy().to_string();
    let cli_bin = env!("CARGO_BIN_EXE_dcc-mcp-cli");
    let file_registry = FileRegistry::new(registry.path()).unwrap();
    file_registry
        .register(ServiceEntry::new("maya", "127.0.0.1", 18080))
        .unwrap();

    let envs = [
        ("DCC_MCP_REGISTRY_DIR", registry_s.as_str()),
        ("DCC_MCP_GATEWAY_PROFILES_FILE", profiles_s.as_str()),
        ("DCC_MCP_PYTHON_EXECUTABLE", ""),
    ];

    let doctor = run_json_with_env(
        &[
            "--auto-gateway-bin",
            cli_bin,
            "doctor",
            "--gateway-port",
            &port_s,
        ],
        &envs,
    );

    assert_eq!(doctor["local"]["inventory"]["total"], 1);
    // The registered DCC type is observed, so the probe is attempted.
    assert_eq!(doctor["adapter_imports"]["total"], 1);
    let probe = &doctor["adapter_imports"]["probes"][0];
    assert_eq!(probe["dcc_type"], "maya");
    assert_eq!(probe["distribution"], "dcc-mcp-maya");
    assert_eq!(probe["module"], "dcc_mcp_maya");
    // No interpreter configured means "not checked", never "broken".
    assert_eq!(probe["status"], "unavailable");
    assert_eq!(probe["reason"], "no_interpreter");
    assert_eq!(doctor["adapter_imports"]["failures"], 0);
    assert_eq!(doctor["status"], "ok");
}

/// Locate a real interpreter, so the test can skip cleanly when none exists.
fn real_python() -> Option<String> {
    for var in ["PYTHON", "PYTHON3"] {
        if let Some(path) = std::env::var(var)
            .ok()
            .filter(|path| !path.trim().is_empty())
        {
            return Some(path);
        }
    }
    for candidate in ["python3", "python"] {
        // `continue`, not `?`: a candidate that cannot be spawned at all (no
        // such file) must not prevent trying the remaining candidates.
        let Ok(probe) = std::process::Command::new(candidate)
            .arg("-c")
            .arg("import sys; sys.stdout.write(sys.executable)")
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .output()
        else {
            continue;
        };
        if probe.status.success() {
            let path = String::from_utf8_lossy(&probe.stdout).trim().to_string();
            if !path.is_empty() {
                return Some(path);
            }
        }
    }
    None
}

/// The headline capability of PIP-3530, on a real interpreter: distribution
/// metadata present but the module gone (an editable install whose source
/// checkout was deleted) must be graded `unimportable`, not silently `ok`.
#[test]
fn doctor_flags_unimportable_adapter_on_a_real_interpreter() {
    let Some(python) = real_python() else {
        eprintln!("skipping: no python interpreter available");
        return;
    };

    // A fake site-packages holding only `*.dist-info`: the module is absent,
    // which is exactly the broken-editable-install shape. The synthetic dcc
    // type keeps the probe independent of what the runner has installed.
    let site = TempDir::new().unwrap();
    let dist_info = site.path().join("dcc_mcp_mayaprobe-9.9.9.dist-info");
    std::fs::create_dir_all(&dist_info).unwrap();
    std::fs::write(
        dist_info.join("METADATA"),
        "Metadata-Version: 2.1\nName: dcc-mcp-mayaprobe\nVersion: 9.9.9\n",
    )
    .unwrap();

    let port = unused_loopback_port();
    let port_s = port.to_string();
    let registry = TempDir::new().unwrap();
    let registry_s = registry.path().to_string_lossy().to_string();
    let profiles = NamedTempFile::new().unwrap();
    let profiles_s = profiles.path().to_string_lossy().to_string();
    let file_registry = FileRegistry::new(registry.path()).unwrap();
    file_registry
        .register(ServiceEntry::new("mayaprobe", "127.0.0.1", 18080))
        .unwrap();

    let output = cli_command()
        .args([
            "--output",
            "json",
            "doctor",
            "--registry-dir",
            &registry_s,
            "--gateway-port",
            &port_s,
            "--adapter-python",
            &format!("mayaprobe={python}"),
        ])
        .env("DCC_MCP_REGISTRY_DIR", &registry_s)
        .env("DCC_MCP_GATEWAY_PROFILES_FILE", &profiles_s)
        .env("PYTHONPATH", site.path())
        .env_remove("DCC_MCP_PYTHON_EXECUTABLE")
        .output()
        .unwrap();

    let value: serde_json::Value = serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|err| panic!("doctor did not emit JSON: {err}"));
    let probe = &value["adapter_imports"]["probes"][0];

    assert_eq!(probe["distribution"], "dcc-mcp-mayaprobe");
    assert_eq!(probe["module"], "dcc_mcp_mayaprobe");
    // Distribution metadata is visible, but the import fails.
    assert_eq!(probe["dist_version"], "9.9.9");
    assert_eq!(probe["imported"], false);
    assert_eq!(probe["error_type"], "ModuleNotFoundError");
    assert_eq!(probe["status"], "unimportable");

    // The whole point of PIP-3530: a broken install must not look healthy.
    assert_eq!(value["adapter_imports"]["failures"], 1);
    assert_eq!(value["status"], "degraded");
    assert!(
        !output.status.success(),
        "doctor must exit non-zero for a broken adapter install"
    );
}

/// An unlaunchable interpreter is also "not checked", not a broken adapter.
#[test]
fn doctor_adapter_imports_report_unlaunchable_interpreters() {
    let port = unused_loopback_port();
    let port_s = port.to_string();
    let registry = TempDir::new().unwrap();
    let registry_s = registry.path().to_string_lossy().to_string();
    let profiles = NamedTempFile::new().unwrap();
    let profiles_s = profiles.path().to_string_lossy().to_string();
    let cli_bin = env!("CARGO_BIN_EXE_dcc-mcp-cli");
    let missing = registry.path().join("no-such-python");
    let missing_s = missing.to_string_lossy().to_string();

    let envs = [
        ("DCC_MCP_REGISTRY_DIR", registry_s.as_str()),
        ("DCC_MCP_GATEWAY_PROFILES_FILE", profiles_s.as_str()),
    ];

    let doctor = run_json_with_env(
        &[
            "--auto-gateway-bin",
            cli_bin,
            "doctor",
            "--gateway-port",
            &port_s,
            "--adapter-python",
            &format!("maya={missing_s}"),
        ],
        &envs,
    );

    assert_eq!(doctor["adapter_imports"]["total"], 1);
    let probe = &doctor["adapter_imports"]["probes"][0];
    assert_eq!(probe["dcc_type"], "maya");
    assert_eq!(probe["python_source"], "cli_override");
    assert_eq!(probe["status"], "unavailable");
    assert!(
        probe["reason"]
            .as_str()
            .unwrap()
            .starts_with("interpreter_not_launchable"),
        "reason was {}",
        probe["reason"]
    );
    assert_eq!(doctor["adapter_imports"]["failures"], 0);
    assert_eq!(doctor["status"], "ok");
}
