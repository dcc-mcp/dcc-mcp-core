"""Keep the pinned modern SDK check in the required full Linux lane."""

from __future__ import annotations

import re

from conftest import REPO_ROOT


def test_modern_sdk_gate_is_explicit_bounded_and_keeps_normal_rust_tests():
    workflow = (REPO_ROOT / ".github/workflows/ci.yml").read_text(encoding="utf-8")
    job = re.search(r"(?ms)^  rust-check:\n(.*?)(?=^  [a-zA-Z0-9_-]+:|\Z)", workflow).group(1)
    step = re.search(r"(?ms)^      - name: MCP 2026 official SDK interoperability\n(.*?)(?=^      - |\Z)", job)
    assert step is not None, "modern SDK must be a required CI step, not local-only opt-in"
    step = step.group(1)
    assert "if: matrix.rust-validation == 'full'" in step
    assert "timeout-minutes: 15" in step
    assert 'DCC_MCP_SDK_SMOKE: "1"' in step
    assert "vx npm --prefix tests/interop/mcp-2026 ci --ignore-scripts" in step
    assert "vx node tests/interop/mcp-2026/request-oracle.mjs" in step
    assert "--no-default-features --features mcp-2026-07-28" in step
    for target in ["stateless_response_contract", "stateless_request_boundary", "stateless_param_headers"]:
        assert f"--test {target}" in step
    assert "continue-on-error" not in step
    rust_tests = re.search(r"(?ms)^      - name: Rust tests\n(.*?)(?=^      - |\Z)", job)
    assert rust_tests is not None, "the normal Rust test gate must remain a separate step"
    rust_tests = rust_tests.group(1)
    assert "if: matrix.rust-validation == 'full'" in rust_tests
    assert "shell: bash" in rust_tests
    assert "set -o pipefail" in rust_tests
    assert re.search(r"(?m)^\s+vx just test-rust 2>&1 \| tee rust-test-results\.log$", rust_tests)
    assert "continue-on-error" not in rust_tests


def test_rust_terminal_diagnostics_preserve_names_without_enabling_success_output():
    workflow = (REPO_ROOT / ".github/workflows/ci.yml").read_text(encoding="utf-8")
    step = re.search(r"(?ms)^      - name: Rust tests\n(.*?)(?=^      - |\Z)", workflow).group(1)
    assert "NEXTEST_FINAL_STATUS_LEVEL: all" in step
    assert "NEXTEST_SUCCESS_OUTPUT" not in step
    assert "NEXTEST_RETRIES" not in step
    artifact = re.search(
        r"(?ms)^      - name: Upload Rust test diagnostics\n(.*?)(?=^      - |\Z)", workflow
    ).group(1)
    assert "if: always() && matrix.rust-validation == 'full'" in artifact
    assert "path: rust-test-results.log" in artifact
    assert "retention-days: 14" in artifact


def test_parameter_sdk_script_has_bounded_deadline_and_pinned_public_client():
    script = (REPO_ROOT / "tests/interop/mcp-2026/param-client.mjs").read_text(encoding="utf-8")
    assert "30_000" in script
    assert "process.exit(1)" in script
    assert "@modelcontextprotocol/client" in script
    assert "toolDefinition: definition" in script
    assert "Mcp-Param-Tenant" in script
