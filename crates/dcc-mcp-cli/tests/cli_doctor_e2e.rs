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
