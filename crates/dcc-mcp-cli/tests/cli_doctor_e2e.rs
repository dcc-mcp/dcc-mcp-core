//! End-to-end coverage for `dcc-mcp-cli doctor` adapter import probes.
//!
//! Kept separate from `cli_e2e.rs` so that file stays under the repository's
//! Rust test-file line limit.

mod support;

use dcc_mcp_transport::discovery::file_registry::FileRegistry;
use dcc_mcp_transport::discovery::types::ServiceEntry;
use tempfile::{NamedTempFile, TempDir};

use support::*;

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
        let probe = std::process::Command::new(candidate)
            .arg("-c")
            .arg("import sys; sys.stdout.write(sys.executable)")
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .output()
            .ok()?;
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
