"""Regression checks for the Windows MSVC release contract."""

from pathlib import Path

ROOT = Path(__file__).parents[1]


def test_core_windows_wheel_jobs_pin_the_msvc_target() -> None:
    workflow = (ROOT / ".github" / "workflows" / "build-wheels.yml").read_text(encoding="utf-8")
    assert workflow.count("target: x86_64-pc-windows-msvc") == 2


def test_semantic_windows_wheels_pin_the_msvc_target() -> None:
    workflow = (ROOT / ".github" / "workflows" / "release.yml").read_text(encoding="utf-8")
    assert workflow.count("--target x86_64-pc-windows-msvc") == 2
    assert "Verify Windows uses the MSVC Rust target" in workflow


def test_approved_release_workflow_matches_the_msvc_contract() -> None:
    workflow = (ROOT / "scripts" / "ci" / "approved_release_workflow.yml").read_text(encoding="utf-8")
    assert workflow.count("--target x86_64-pc-windows-msvc") == 2
    assert "Verify Windows uses the MSVC Rust target" in workflow
