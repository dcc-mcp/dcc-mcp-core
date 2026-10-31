//! Regression coverage for the `--timeout-secs` / `--wait-timeout-secs` help
//! wording reported in dcc-mcp/dcc-mcp-core#2857.
//!
//! The top-level `--help` once described `--timeout-secs` as a
//! "Global timeout in seconds for all operations", while `call --help`
//! described the same flag as a per-request timeout ("Increase for renders
//! and other long-running sync tools"). Those contradictory descriptions
//! mislead long-running render/sync calls. The issue asks for unified wording
//! and an explicit statement in `call --help` that `--wait-timeout-secs` is
//! independent of `--timeout-secs`.
//!
//! These tests render the real CLI help and lock that contract in. They only
//! assert help text; no runtime semantics and no default values are changed.

use std::process::Command;

fn help_text(args: &[&str]) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_dcc-mcp-cli"))
        .args(args)
        .env("DCC_MCP_INSTALL_OFFLINE", "true")
        .output()
        .expect("spawn dcc-mcp-cli for --help");
    assert!(
        output.status.success(),
        "{} --help should exit 0; stderr: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("help output should be UTF-8")
}

/// Collapse wrapping/indentation so assertions match phrases, not line breaks.
fn one_line(s: &str) -> String {
    s.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

#[test]
fn timeout_help_is_not_described_as_a_global_all_operations_budget() {
    let top = one_line(&help_text(&["--help"]));
    assert!(
        !top.contains("all operations"),
        "top-level --help must not describe --timeout-secs as a global \
         all-operations timeout (issue #2857): {top}"
    );
    let call = one_line(&help_text(&["call", "--help"]));
    assert!(
        !call.contains("increase for renders"),
        "call --help must not carry the old contradictory wording (issue #2857): {call}"
    );
}

#[test]
fn timeout_help_unifies_per_request_wording_and_states_wait_independence() {
    let top = one_line(&help_text(&["--help"]));
    let call = one_line(&help_text(&["call", "--help"]));

    for (label, help) in [("top-level", &top), ("call", &call)] {
        assert!(
            help.contains("per request") || help.contains("per-request"),
            "{label} --help must describe --timeout-secs as a per-request timeout: {help}"
        );
        assert!(
            help.contains("--wait-timeout-secs") && help.contains("independen"),
            "{label} --help must state that --wait-timeout-secs is an independent \
             total job-polling budget (issue #2857): {help}"
        );
    }

    // The independent polling budget keeps its own documented flag + default.
    assert!(
        call.contains("maximum total time to wait for an asynchronous job"),
        "call --help must document --wait-timeout-secs as the total wait budget: {call}"
    );
    assert!(
        call.contains("default: 600"),
        "call --help --wait-timeout-secs default must remain 600: {call}"
    );
}
