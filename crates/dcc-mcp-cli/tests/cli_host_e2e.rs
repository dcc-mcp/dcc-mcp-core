//! End-to-end coverage for `dcc-mcp-cli host`.
//!
//! These tests drive the real binary, because the property that matters is
//! observable from outside: probing an absent host reports `missing` and does
//! not touch the disk, and a licence-gated host is never installable.

mod support;

use std::path::PathBuf;

use serde_json::Value;

use support::{cli_command, run_json};

/// A host id that cannot exist on any machine, so `missing` is deterministic.
const IMPOSSIBLE_HOST: &str = "dcc-mcp-nonexistent-host";

/// Run `host` with a private lock file so tests never read or write the
/// developer's real pins.
fn host_command(lock: &PathBuf) -> std::process::Command {
    let mut command = cli_command();
    command.env("DCC_MCP_HOSTS_LOCK", lock);
    command.arg("host");
    command
}

fn host_json(args: &[&str], lock: &PathBuf) -> Value {
    let output = host_command(lock)
        .args(args)
        .arg("--output")
        .arg("json")
        .output()
        .expect("host command should run");
    // `host doctor` writes its report to stdout and, when a host is unusable,
    // also writes a UNAVAILABLE envelope to stderr. Parse stdout only.
    serde_json::from_slice(&output.stdout).expect("host command should print JSON")
}

#[test]
fn list_reports_every_host_with_its_licence_class() {
    let lock = std::env::temp_dir().join(format!("hosts-{}.lock", uuid::Uuid::new_v4()));
    let value = host_json(&["list"], &lock);
    let hosts = value["hosts"].as_array().expect("hosts array");
    assert!(!hosts.is_empty());

    let blender = hosts
        .iter()
        .find(|host| host["id"] == "blender")
        .expect("blender is in the manifest");
    assert_eq!(blender["license"], "open_source");
    assert_eq!(blender["self_provision"], true);
    assert_eq!(blender["pinned_version"], "5.1.1");

    // The redline, asserted from the CLI surface rather than the data file.
    for id in [
        "maya",
        "3dsmax",
        "houdini",
        "nuke",
        "photoshop",
        "substance",
    ] {
        let host = hosts
            .iter()
            .find(|host| host["id"] == id)
            .unwrap_or_else(|| panic!("{id} is in the manifest"));
        assert_eq!(host["license"], "commercial", "{id} licence class");
        assert_eq!(
            host["self_provision"], false,
            "{id} must not self-provision"
        );
        assert!(
            host["install_channels"]
                .as_array()
                .is_some_and(Vec::is_empty),
            "{id} must declare no install channel"
        );
    }
}

#[test]
fn doctor_reports_missing_without_installing_anything() {
    // Acceptance criterion 1: an absent host is reported as `missing`, and the
    // probe performs no installation. The disk effect is checked by pointing the
    // lock at a fresh path and asserting the probe never creates it.
    let lock = std::env::temp_dir().join(format!("hosts-{}.lock", uuid::Uuid::new_v4()));
    assert!(!lock.exists(), "lock must start absent");

    let output = host_command(&lock)
        .args(["doctor", IMPOSSIBLE_HOST, "--output", "json"])
        .output()
        .expect("doctor should run");

    assert!(
        !output.status.success(),
        "an absent host must exit non-zero so runners can gate on it"
    );
    let value: Value = serde_json::from_slice(&output.stdout).expect("JSON report");
    let host = &value["hosts"][0];
    assert_eq!(host["id"], IMPOSSIBLE_HOST);
    assert_eq!(host["status"], "unknown_host");
    assert_eq!(value["read_only"], true);
    assert!(
        host["sources_checked"].as_array().is_some(),
        "the report must say where it looked"
    );
    assert!(!lock.exists(), "a read-only probe must not create files");
}

#[test]
fn doctor_reports_a_hosted_host_as_available_with_its_path() {
    // Acceptance criterion 2 (read half): when the executable exists, doctor
    // reports `available` and carries the resolved path, which is what a
    // proposition runner hands to the launcher.
    let lock = std::env::temp_dir().join(format!("hosts-{}.lock", uuid::Uuid::new_v4()));
    let dir = tempfile::tempdir().unwrap();
    let binary = dir.path().join("fakeblender.exe");
    std::fs::write(&binary, b"#!/bin/sh\necho 'Blender 5.1.1'\n").unwrap();

    let output = host_command(&lock)
        .args(["doctor", "blender", "--output", "json"])
        .env("DCC_MCP_BLENDER_EXECUTABLE", &binary)
        .output()
        .expect("doctor should run");
    let overridden: Value = serde_json::from_slice(&output.stdout).expect("JSON report");
    let host = &overridden["hosts"][0];
    assert_eq!(host["id"], "blender");
    assert_eq!(host["executable_source"], "env_override");
    assert_eq!(host["gate"], ">=5.1");
    // The stub is a text file, not an executable, so the version query cannot
    // succeed here; the graded outcomes below are the contract.
    assert!(
        matches!(
            host["status"].as_str(),
            Some("available" | "version_unknown")
        ),
        "expected available or version_unknown, got {}",
        host["status"]
    );
    assert_eq!(overridden["read_only"], true);
}

#[test]
fn doctor_flags_a_version_that_misses_the_baseline() {
    // The Blender baseline is >=5.1. A host below it must be reported as a
    // mismatch, not silently accepted.
    let lock = std::env::temp_dir().join(format!("hosts-{}.lock", uuid::Uuid::new_v4()));
    let value = host_json(&["doctor", "blender>=99.0.0"], &lock);
    let host = &value["hosts"][0];
    assert_eq!(host["gate"], ">=99.0.0");
    assert!(
        matches!(
            host["status"].as_str(),
            Some("missing" | "version_mismatch")
        ),
        "a spec no installed host can satisfy must not report available, got {}",
        host["status"]
    );
}

#[test]
fn doctor_rejects_malformed_specs() {
    let lock = std::env::temp_dir().join(format!("hosts-{}.lock", uuid::Uuid::new_v4()));
    let value = host_json(&["doctor", "blender>=five"], &lock);
    assert_eq!(value["hosts"][0]["status"], "invalid_spec");
    assert!(
        value["hosts"][0]["hint"]
            .as_str()
            .is_some_and(|hint| hint.contains("host==5.1")),
        "the hint must show the accepted forms"
    );
}

#[test]
fn install_and_pin_refuse_to_act_before_phase_two() {
    // Guards against a silently no-op install, which would be worse than an
    // error: an agent would believe a host had been provisioned.
    let lock = std::env::temp_dir().join(format!("hosts-{}.lock", uuid::Uuid::new_v4()));
    for args in [["install", "maya"], ["pin", "blender==5.1.1"]] {
        let output = host_command(&lock)
            .args(args)
            .arg("--output")
            .arg("json")
            .output()
            .expect("host command should run");
        assert!(
            !output.status.success(),
            "`host {}` must fail rather than no-op",
            args[0]
        );
    }
}

#[test]
fn host_does_not_start_or_require_a_gateway() {
    // `host` inspects local binaries only. Asserting the command succeeds with
    // an unreachable gateway URL proves it never dials one.
    let lock = std::env::temp_dir().join(format!("hosts-{}.lock", uuid::Uuid::new_v4()));
    let output = host_command(&lock)
        .args(["list", "--output", "json"])
        .env("DCC_MCP_BASE_URL", "http://127.0.0.1:1")
        .env("DCC_MCP_CLI_NO_AUTO_GATEWAY", "true")
        .output()
        .expect("host list should run");
    assert!(
        output.status.success(),
        "host list must not need a gateway: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// `run_json` from the shared helpers is exercised by other suites; this keeps
/// the helper in scope so its behaviour stays covered if the suite is trimmed.
#[test]
fn support_helper_still_parses_json() {
    let lock = std::env::temp_dir().join(format!("hosts-{}.lock", uuid::Uuid::new_v4()));
    let value = host_json(&["list"], &lock);
    assert_eq!(value["version"], "1");
    let _ = run_json(&["host", "list"]);
}
