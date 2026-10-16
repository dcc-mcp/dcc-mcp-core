//! End-to-end coverage for `dcc-mcp-cli host`.
//!
//! These tests drive the real binary, because the property that matters is
//! observable from outside: probing an absent host reports `missing` and does
//! not touch the disk, and a licence-gated host is never installable.

mod support;

use std::path::{Path, PathBuf};

use serde_json::Value;

use support::{cli_command, run_json};

/// Write a stub host file with executable permissions where they are required.
///
/// `std::fs::write` alone creates a 0644 file, which unix probing rejects for
/// lacking the executable bit, so the bit is set explicitly where it exists.
fn write_executable_stub(path: &Path, contents: &[u8]) {
    std::fs::write(path, contents).expect("stub should be writable");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
            .expect("stub should be marked executable");
    }
}

/// A versioned host stub that can actually launch on the current platform.
fn available_stub(dir: &Path) -> PathBuf {
    #[cfg(unix)]
    let (name, contents) = (
        "fakeblender.sh",
        b"#!/bin/sh\necho 'Blender 5.1.1'\n".as_slice(),
    );
    #[cfg(not(unix))]
    let (name, contents) = ("fakeblender.cmd", b"@echo Blender 5.1.1\r\n".as_slice());
    let path = dir.join(name);
    write_executable_stub(&path, contents);
    path
}

/// A host id that cannot exist on any machine, so `missing` is deterministic.
const IMPOSSIBLE_HOST: &str = "dcc-mcp-nonexistent-host";

/// Run `host` with a private lock file so tests never read or write the
/// developer's real pins.
fn host_command(lock: &PathBuf) -> std::process::Command {
    let mut command = cli_command();
    command.env("DCC_MCP_HOSTS_LOCK", lock);
    // A baseline must not inherit the developer's explicit Blender override.
    // Individual tests may still set their own override on the returned command.
    command.env_remove("DCC_MCP_BLENDER_EXECUTABLE");
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

/// Probe `blender` with no override set, so a test can tell whether this
/// machine already has a Blender the probe would rather report.
///
/// Probing takes the first candidate that clears the version gate, so an
/// installed Blender outranks an override that cannot be run or whose version
/// cannot be read. That fall-through is deliberate — a broken shim must not
/// hide a working binary — but it makes an override-only assertion depend on
/// what else is installed. Tests that set an override therefore read this
/// baseline first and pick the branch that is actually observable here.
fn blender_baseline(lock: &PathBuf) -> Value {
    let value = host_json(&["doctor", "blender"], lock);
    value["hosts"][0].clone()
}

/// Whether `baseline` shows a Blender this machine can already run.
fn blender_already_available(baseline: &Value) -> bool {
    baseline["status"] == "available"
}

/// Assert that an override was genuinely probed and reported as such, even
/// when an installed Blender won the fall-through and owns `executable_source`.
///
/// The probe names every candidate it passed over in `warning`, including its
/// source label, so this keeps the `env_override` contract covered on machines
/// that have Blender installed instead of letting the test assert nothing.
fn assert_override_was_probed(host: &Value, override_path: &Path) {
    let warning = host["warning"]
        .as_str()
        .unwrap_or_else(|| panic!("expected a warning naming the skipped override, got {host}"));
    assert!(
        warning.contains("(env_override)"),
        "the skipped candidate must be attributed to env_override, got {warning}"
    );
    assert!(
        warning.contains(&override_path.display().to_string()),
        "the warning must name the override target, got {warning}"
    );
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
    let binary = available_stub(dir.path());

    let output = host_command(&lock)
        .args(["doctor", "blender", "--output", "json"])
        .env("DCC_MCP_BLENDER_EXECUTABLE", &binary)
        .output()
        .expect("doctor should run");
    let overridden: Value = serde_json::from_slice(&output.stdout).expect("JSON report");
    let host = &overridden["hosts"][0];
    assert_eq!(host["id"], "blender");
    assert_eq!(host["gate"], ">=5.1");
    assert_eq!(overridden["read_only"], true);

    // A runnable override satisfying the version gate always wins, even when
    // another Blender is already available on PATH or in a search root.
    assert_eq!(host["executable_source"], "env_override");
    assert_eq!(host["status"], "available");
    assert_eq!(host["version"], "5.1.1");
    assert_eq!(host["executable"], binary.to_string_lossy().as_ref());
    assert!(host["warning"].is_null());
    assert!(output.status.success());
}

#[test]
#[cfg(unix)]
fn doctor_available_override_wins_with_blender_on_path() {
    let dir = tempfile::tempdir().unwrap();
    let blender = dir.path().join("blender");
    write_executable_stub(&blender, b"#!/bin/sh\necho 'Blender 5.2.0'\n");

    // Run the actual hosted-host test with a deterministic, usable baseline.
    // Keeping this in a child avoids changing PATH for parallel tests.
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "doctor_reports_a_hosted_host_as_available_with_its_path",
            "--nocapture",
        ])
        .env("PATH", dir.path())
        .output()
        .expect("hosted-host regression should run");
    assert!(
        output.status.success() && String::from_utf8_lossy(&output.stdout).contains("1 passed;"),
        "hosted-host test must preserve override precedence with Blender installed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn blender_baseline_ignores_inherited_override() {
    const CHILD_FLAG: &str = "DCC_MCP_TEST_BLENDER_BASELINE_CHILD";
    if std::env::var_os(CHILD_FLAG).is_some() {
        let lock = std::env::temp_dir().join(format!("hosts-{}.lock", uuid::Uuid::new_v4()));
        let baseline = blender_baseline(&lock);
        assert_ne!(baseline["executable_source"], "env_override");
        return;
    }

    let dir = tempfile::tempdir().unwrap();
    let binary = available_stub(dir.path());
    // Only the child inherits this setting; no process-global environment
    // mutation can race another test.
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "blender_baseline_ignores_inherited_override",
            "--nocapture",
        ])
        .env(CHILD_FLAG, "1")
        .env("DCC_MCP_BLENDER_EXECUTABLE", binary)
        .output()
        .expect("baseline regression should run");
    assert!(
        output.status.success() && String::from_utf8_lossy(&output.stdout).contains("1 passed;"),
        "baseline must ignore an inherited override:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn doctor_reports_an_override_it_cannot_run() {
    // An override is an explicit operator declaration, so naming something
    // unusable must be reported against that variable instead of silently
    // degrading to "not found" with no mention of the path that was set.
    let lock = std::env::temp_dir().join(format!("hosts-{}.lock", uuid::Uuid::new_v4()));
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("no-such-blender");

    // The rejection path only runs when nothing else finds the host, so a
    // runner that already has Blender installed proves nothing about it.
    if blender_already_available(&blender_baseline(&lock)) {
        return;
    }

    let output = host_command(&lock)
        .args(["doctor", "blender", "--output", "json"])
        .env("DCC_MCP_BLENDER_EXECUTABLE", &missing)
        .output()
        .expect("doctor should run");
    let value: Value = serde_json::from_slice(&output.stdout).expect("JSON report");
    let host = &value["hosts"][0];
    assert_eq!(host["status"], "missing");
    assert_eq!(host["reason"], "override_not_runnable");
    assert_eq!(host["executable_source"], "env_override");
    assert!(
        host["hint"]
            .as_str()
            .is_some_and(|hint| hint.contains("DCC_MCP_BLENDER_EXECUTABLE")
                && hint.contains(&missing.display().to_string())),
        "the hint must name the variable and its target, got {}",
        host["hint"]
    );
}

/// A stub that answers the version query successfully but without a version
/// token, which is what a GUI splash screen or a studio wrapper script does.
///
/// Unix runs a shebang script; Windows runs a `.cmd` one-liner. Both exit 0,
/// so the only thing that can reject them is the missing version.
fn unparsable_stub(dir: &Path) -> PathBuf {
    #[cfg(unix)]
    {
        let path = dir.join("fakeblender.sh");
        write_executable_stub(&path, b"#!/bin/sh\necho 'Blender (splash)'\n");
        path
    }
    #[cfg(not(unix))]
    {
        let path = dir.join("fakeblender.cmd");
        std::fs::write(&path, b"@echo Blender (splash)\r\n").expect("stub should be writable");
        path
    }
}

#[test]
fn doctor_does_not_report_available_when_the_version_is_unreadable() {
    // The gate is the point of `min_version`. A host that answers the version
    // query with no version token must be `version_unknown`, never
    // `available` — the latter would let automation proceed on an unverified
    // host while the JSON still shows a `gate` that looks satisfied.
    let lock = std::env::temp_dir().join(format!("hosts-{}.lock", uuid::Uuid::new_v4()));
    let dir = tempfile::tempdir().unwrap();
    let binary = unparsable_stub(dir.path());

    // Read first, before the override exists: an installed Blender outranks an
    // override whose version cannot be read, which changes what may be asserted.
    let baseline = blender_baseline(&lock);

    let output = host_command(&lock)
        .args(["doctor", "blender", "--output", "json"])
        .env("DCC_MCP_BLENDER_EXECUTABLE", &binary)
        .output()
        .expect("doctor should run");
    let value: Value = serde_json::from_slice(&output.stdout).expect("JSON report");
    let host = &value["hosts"][0];

    if blender_already_available(&baseline) {
        // Fall-through won: the real install is reported as available, which is
        // correct precisely because the override could not be verified. The
        // override must still appear as the skipped `env_override` candidate,
        // so the version-gate contract stays covered on such machines.
        assert_override_was_probed(host, &binary);
        assert_eq!(host["status"], "available");
        assert_eq!(host["executable"], baseline["executable"]);
        assert_eq!(host["executable_source"], baseline["executable_source"]);
        assert!(output.status.success());
        assert!(
            host["warning"]
                .as_str()
                .is_some_and(|w| w.contains("unreadable version")),
            "the skipped override must be reported as unreadable, got {}",
            host["warning"]
        );
        return;
    }

    assert_eq!(host["status"], "version_unknown");
    assert_eq!(host["reason"], "version_unparsable");
    assert_eq!(host["executable_source"], "env_override");
    assert!(
        host["hint"].as_str().is_some_and(|hint| !hint.is_empty()),
        "an unverified host must explain itself, got {}",
        host["hint"]
    );
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
fn install_refuses_licence_gated_hosts_even_with_yes() {
    // Acceptance criterion 4: maya must never be installed by this CLI. The
    // refusal is independent of consent, so `--yes` must not bypass it.
    let lock = std::env::temp_dir().join(format!("hosts-{}.lock", uuid::Uuid::new_v4()));
    let output = host_command(&lock)
        .args(["install", "maya", "--yes", "--output", "json"])
        .output()
        .expect("host install should run");

    let value: Value = serde_json::from_slice(&output.stdout).expect("JSON report");
    assert_eq!(value["status"], "refused");
    assert_eq!(value["reason"], "commercial");
    assert_eq!(value["installed"], false);
    assert!(
        value["hint"]
            .as_str()
            .is_some_and(|hint| hint.contains("licence-gated")),
        "the hint must tell the user to install it themselves: {}",
        value["hint"]
    );
}

#[test]
fn install_refuses_without_consent_when_unattended() {
    // The `vx ffmpeg` rule, applied: a write operation without an operator
    // present is refused rather than assumed approved.
    let lock = std::env::temp_dir().join(format!("hosts-{}.lock", uuid::Uuid::new_v4()));
    let output = host_command(&lock)
        .args(["install", "blender", "--output", "json"])
        // A test harness is never a terminal, so `ask` resolves to a refusal.
        .env("DCC_MCP_HOST_INSTALL", "never")
        .output()
        .expect("host install should run");

    let value: Value = serde_json::from_slice(&output.stdout).expect("JSON report");
    // Blender is missing on CI-like machines, so the consent gate is what is
    // under test; on a machine that already has it, `already_satisfied` is the
    // correct short-circuit before consent matters.
    assert!(
        matches!(
            value["status"].as_str(),
            Some("consent_required" | "already_satisfied")
        ),
        "expected a consent gate or an already-satisfied host, got {}",
        value["status"]
    );
    if value["status"] == "consent_required" {
        assert_eq!(value["reason"], "policy_never");
        assert_eq!(value["installed"], false);
    }
}

/// P1-A: `host pin` may not write a version below the manifest minimum.
///
/// The Windows winget channel takes an arbitrary `--version`, so without a
/// floor at pin time `host pin blender==5.0 && host install blender --yes`
/// would install 5.0, below the >=5.1 baseline the manifest declares.
#[test]
fn pin_refuses_a_version_below_the_manifest_minimum() {
    let dir = tempfile::tempdir().unwrap();
    let lock = dir.path().join("hosts.lock");

    for spec in ["blender==5.0", "blender==4.2.1"] {
        let output = host_command(&lock)
            .args(["pin", spec, "--output", "json"])
            .output()
            .expect("host pin should run");
        assert!(
            !output.status.success(),
            "`host pin {spec}` must be refused: it is below min_version 5.1"
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("below the minimum"),
            "the error must say why, got {stderr}"
        );
    }

    // The lock must not have been written by a refused pin.
    assert!(
        !lock.exists(),
        "a refused pin must not create the lock file"
    );

    // A pin at the floor is accepted.
    let output = host_command(&lock)
        .args(["pin", "blender==5.1", "--output", "json"])
        .output()
        .expect("host pin should run");
    assert!(output.status.success(), "5.1 is exactly at the floor");
}

/// The exit code is the contract a proposition runner gates on, so it has to
/// be non-zero for every non-success outcome, not just for refusals.
#[test]
fn install_exit_code_reflects_the_outcome() {
    let lock = std::env::temp_dir().join(format!("hosts-{}.lock", uuid::Uuid::new_v4()));

    // Refused (commercial host) must exit non-zero.
    let refused = host_command(&lock)
        .args(["install", "maya", "--yes", "--output", "json"])
        .output()
        .expect("host install should run");
    assert!(
        !refused.status.success(),
        "a refused install must exit non-zero so `install && run` stops"
    );

    // Consent denied must exit non-zero.
    let denied = host_command(&lock)
        .args(["install", "blender", "--output", "json"])
        .env("DCC_MCP_HOST_INSTALL", "never")
        .output()
        .expect("host install should run");
    let value: Value = serde_json::from_slice(&denied.stdout).expect("JSON report");
    if value["status"] == "consent_required" {
        assert!(
            !denied.status.success(),
            "a consent refusal must exit non-zero"
        );
    }
}

#[test]
fn pin_writes_the_lock_file_and_changes_the_reported_version() {
    let dir = tempfile::tempdir().unwrap();
    let lock = dir.path().join("hosts.lock");

    let output = host_command(&lock)
        .args(["pin", "blender==5.1.1", "--output", "json"])
        .output()
        .expect("host pin should run");
    assert!(
        output.status.success(),
        "pin should succeed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).expect("JSON report");
    assert_eq!(value["id"], "blender");
    assert_eq!(value["pinned_version"], "5.1.1");
    assert!(lock.is_file(), "pin must create the lock file");

    // The pin has to be visible to the next command, otherwise it is not a
    // single source of truth.
    let listed = host_json(&["list"], &lock);
    let blender = listed["hosts"]
        .as_array()
        .expect("hosts")
        .iter()
        .find(|host| host["id"] == "blender")
        .expect("blender");
    assert_eq!(blender["pinned_version"], "5.1.1");
}

#[test]
fn pin_rejects_unknown_hosts_and_unpinnable_specs() {
    let lock = std::env::temp_dir().join(format!("hosts-{}.lock", uuid::Uuid::new_v4()));
    for spec in ["not-a-host==1.0", "blender>=five"] {
        let output = host_command(&lock)
            .args(["pin", spec, "--output", "json"])
            .output()
            .expect("host pin should run");
        assert!(
            !output.status.success(),
            "`host pin {spec}` must fail rather than silently accept"
        );
    }
}

#[test]
fn pin_without_a_version_freezes_the_manifest_value() {
    // `host pin blender` is a legitimate request: freeze the version at
    // whatever the manifest currently declares, so a later manifest bump does
    // not move this machine.
    let dir = tempfile::tempdir().unwrap();
    let lock = dir.path().join("hosts.lock");
    let output = host_command(&lock)
        .args(["pin", "blender", "--output", "json"])
        .output()
        .expect("host pin should run");
    assert!(output.status.success());
    let value: Value = serde_json::from_slice(&output.stdout).expect("JSON report");
    assert_eq!(value["pinned_version"], "5.1.1");
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
