"""Reproduce and verify the generated-lock merge race repair.

The failure this module exists to prove fixed: ``release-please-lock-sync.yml``
generated correct lockfiles, release automation merged the pull request while
generation was still running, and ``preflight`` answered with "pull request is
no longer an open PR targeting main" and exit 1. The job is not a required
check, so the generated locks were lost silently and only resurfaced as red
lanes on ``main`` (0.20.35, repaired by hand in PR #2578).

The tests below execute the workflow's real "Resolve generated lock target"
step against a stubbed ``gh``, so the merged-before-push case is reproduced
rather than described.
"""

from __future__ import annotations

import json
import os
from pathlib import Path
import shutil
import stat
import subprocess
from types import SimpleNamespace

import pytest

from conftest import REPO_ROOT
from dcc_mcp_core import yaml_loads
from test_generated_lock_workflow_execution import _clean_environment

SYNC_WORKFLOW_PATH = REPO_ROOT / ".github" / "workflows" / "release-please-lock-sync.yml"
REPAIR_WORKFLOW_PATH = REPO_ROOT / ".github" / "workflows" / "generated-lock-repair.yml"
ACTION_PATH = REPO_ROOT / ".github" / "actions" / "generated-lock-repair" / "action.yml"

TARGET_STEP = "Resolve generated lock target"
ABORT_STEP = "Report an unrepairable generated lock target"
REVALIDATE_STEP = "Revalidate pull request identity and generated diff"
COMMIT_STEP = "Commit and push changes"
REPAIR_STEP = "Repair main after the merge race"

EVENT_HEAD_SHA = "a" * 40
ADVANCED_HEAD_SHA = "b" * 40


def _sync_steps():
    workflow = yaml_loads(SYNC_WORKFLOW_PATH.read_text(encoding="utf-8"))
    return workflow["jobs"]["sync-cargo-metadata"]["steps"]


def _step(name):
    step = next((step for step in _sync_steps() if step.get("name") == name), None)
    assert step is not None, f"workflow step {name!r} is missing"
    return step


def _action():
    return yaml_loads(ACTION_PATH.read_text(encoding="utf-8"))


def _write_executable(path: Path, body: str) -> None:
    path.write_text(body, encoding="utf-8")
    path.chmod(path.stat().st_mode | stat.S_IEXEC | stat.S_IXGRP | stat.S_IXOTH)


@pytest.fixture
def target_fixture(tmp_path: Path):
    """Run the real target-resolution step against a stubbed ``gh``."""
    bin_dir = tmp_path / "stub bin"
    bin_dir.mkdir()
    payload_file = tmp_path / "pull request payload.json"
    _write_executable(
        bin_dir / "gh",
        '#!/bin/sh\ncat "$STUB_PR_PAYLOAD"\n',
    )
    github_output = tmp_path / "github output"
    github_output.write_bytes(b"")
    script = tmp_path / "target step.sh"
    script.write_text(_step(TARGET_STEP)["run"], encoding="utf-8")

    env = _clean_environment(tmp_path)
    env["PATH"] = str(bin_dir) + os.pathsep + env["PATH"]
    env.update(
        {
            "GITHUB_REPOSITORY": "dcc-mcp/dcc-mcp-core",
            "PR_NUMBER": "2556",
            "PR_HEAD_SHA": EVENT_HEAD_SHA,
            "GITHUB_OUTPUT": str(github_output),
            "STUB_PR_PAYLOAD": str(payload_file),
        }
    )

    def run(**overrides):
        payload = {
            "state": "open",
            "merged": False,
            "base": {"ref": "main"},
            "head": {"sha": EVENT_HEAD_SHA},
        }
        payload.update(overrides)
        payload_file.write_text(json.dumps(payload), encoding="utf-8")
        github_output.write_bytes(b"")
        result = subprocess.run(
            ["bash", str(script)],
            cwd=tmp_path,
            env=env,
            capture_output=True,
            text=True,
            timeout=60,
        )
        outputs = {}
        for line in github_output.read_text(encoding="utf-8").splitlines():
            key, _, value = line.partition("=")
            outputs[key] = value
        return result, outputs

    return SimpleNamespace(run=run, workspace=tmp_path)


_TARGET_SHELL = pytest.mark.skipif(
    os.name == "nt" or shutil.which("bash") is None,
    reason="Git Bash resolves C:\\ paths through its own mount table, which the "
    "workflow's POSIX shell cannot see; the Linux CI lane executes these.",
)


@_TARGET_SHELL
def test_merged_pull_request_is_repaired_on_main_instead_of_abandoned(target_fixture) -> None:
    """The 0.20.35 race: the PR merged while the locks were being generated."""
    result, outputs = target_fixture.run(state="closed", merged=True)

    # This is the whole point of the fix: the run must not give up.
    assert result.returncode == 0, result.stdout + result.stderr
    assert outputs.get("target") == "main"
    assert "merged while the generated lock metadata was produced" in result.stdout


@_TARGET_SHELL
def test_open_pull_request_keeps_the_original_branch_path(target_fixture) -> None:
    result, outputs = target_fixture.run()

    assert result.returncode == 0, result.stdout + result.stderr
    assert outputs.get("target") == "pr"
    assert "reason" not in outputs


@_TARGET_SHELL
@pytest.mark.parametrize(
    "overrides,expected_reason",
    [
        ({"state": "closed", "merged": False}, "without being merged"),
        ({"base": {"ref": "release-0.20.x"}}, "no longer targets main"),
        ({"head": {"sha": ADVANCED_HEAD_SHA}}, "head advanced"),
    ],
)
def test_unrepairable_targets_fail_loudly(target_fixture, overrides, expected_reason) -> None:
    """A silent exit is what let the drift reach main, so every other case is loud."""
    result, outputs = target_fixture.run(**overrides)

    assert result.returncode == 0, result.stdout + result.stderr
    assert outputs.get("target") == "abort"
    assert expected_reason in outputs.get("reason", "")


def test_repair_and_original_paths_are_mutually_exclusive() -> None:
    """Exactly one write path may run, and the abort path must be a hard failure."""
    assert _step(REPAIR_STEP)["if"] == "steps.target.outputs.target == 'main'"
    assert _step(REVALIDATE_STEP)["if"] == "steps.target.outputs.target == 'pr'"
    assert _step(COMMIT_STEP)["if"] == "steps.target.outputs.target == 'pr'"

    abort_run = _step(ABORT_STEP)["run"]
    assert "::error::" in abort_run
    assert "exit 1" in abort_run
    assert _step(ABORT_STEP)["if"] == "steps.target.outputs.target == 'abort'"


def test_resolution_runs_after_generation_and_before_identity_revalidation() -> None:
    """The target must be chosen before any check that can fail on a merged PR."""
    names = [step.get("name") for step in _sync_steps()]
    assert names.index(TARGET_STEP) == names.index("Sync generated lock metadata") + 1
    assert names.index(TARGET_STEP) < names.index(REVALIDATE_STEP)
    assert names.index(TARGET_STEP) < names.index(COMMIT_STEP)


def test_repair_checks_out_the_base_branch_not_the_merged_head() -> None:
    """The generated locks now have to land on the branch the PR merged into."""
    checkout = _step("Checkout the merge target branch")
    assert checkout["uses"] == "actions/checkout@v6"
    assert checkout["with"]["path"] == "main-checkout"
    assert checkout["with"]["ref"] == "${{ github.event.pull_request.base.ref }}"
    assert checkout["with"]["persist-credentials"] is False
    assert _step(REPAIR_STEP)["with"]["workdir"] == "main-checkout"


def test_repair_shares_the_single_generated_lock_repair_action() -> None:
    """Both directions call one action, so the two paths cannot drift apart."""
    assert _step(REPAIR_STEP)["uses"] == "./.github/actions/generated-lock-repair"
    repair_workflow = yaml_loads(REPAIR_WORKFLOW_PATH.read_text(encoding="utf-8"))
    uses = [step.get("uses") for step in repair_workflow["jobs"]["repair"]["steps"]]
    assert uses.count("./.github/actions/generated-lock-repair") == 1


def test_repair_workflow_verifies_main_on_every_push_that_moves_a_version() -> None:
    """The merge-side backstop covers every trigger path, not just release-please."""
    repair_workflow = yaml_loads(REPAIR_WORKFLOW_PATH.read_text(encoding="utf-8"))
    on = repair_workflow["on"]

    assert on["push"]["branches"] == ["main"]
    assert "Cargo.toml" in on["push"]["paths"]
    assert "crates/**/Cargo.toml" in on["push"]["paths"]
    assert ".release-please-manifest.json" in on["push"]["paths"]
    assert "workflow_dispatch" in on
    # No branch protection and no required check: the repair only removes the
    # silence, it never gates a merge.
    assert repair_workflow["permissions"] == {"contents": "read"}


def test_repair_uses_a_non_releasing_commit_type() -> None:
    """`refactor` would be read as a bump signal; `chore(lock)` is release-invisible."""
    inputs = _action()["inputs"]
    assert inputs["title"]["default"] == "chore(lock): sync generated lock metadata"
    assert inputs["title"]["default"].startswith("chore(")
    assert "refactor" not in inputs["title"]["default"]
    assert inputs["branch"]["default"] == "bot/generated-lock-repair"


def test_repair_fails_closed_on_an_unexpected_generated_diff() -> None:
    """A surprising re-resolution is reported, never merged."""
    steps = {step.get("name"): step for step in _action()["runs"]["steps"]}
    guard = steps["Fail closed on an unexpected generated diff"]
    assert guard["run"] == "python -I scripts/ci/generated_lock_sync.py verify-diff"
    assert guard["if"] == "steps.generate.outputs.changed == 'true'"
    assert steps["Regenerate the committed lock outputs"]["id"] == "generate"


def test_repair_never_auto_merges() -> None:
    """`main` has no required checks, so auto-merge would land before CI finishes."""
    action_text = ACTION_PATH.read_text(encoding="utf-8")
    assert "gh pr merge" not in action_text
    assert "--auto-merge" not in action_text


def test_failed_repair_is_reported_instead_of_going_silent() -> None:
    """A failed repair restores the drift, so it has to leave a visible trail."""
    steps = {step.get("name"): step for step in _action()["runs"]["steps"]}
    report = steps["Report a failed repair"]
    assert report["if"] == "failure()"
    assert "gh issue create" in report["run"]
    assert report["env"]["NOTIFY_LABEL"] == "${{ inputs.notify-label }}"
    assert _action()["inputs"]["notify-label"]["default"] == "generated-lock-repair"


def test_every_input_a_step_reads_is_wired_into_its_environment() -> None:
    """Inputs reach bash only through env; an unwired one fails at runtime."""
    mapping = {
        "$REPAIR_BRANCH": "REPAIR_BRANCH",
        "$REPAIR_TITLE": "REPAIR_TITLE",
        "$NOTIFY_LABEL": "NOTIFY_LABEL",
    }
    checked = 0
    for step in _action()["runs"]["steps"]:
        run = step.get("run", "")
        env = step.get("env", {})
        for token, key in mapping.items():
            if token in run:
                checked += 1
                assert key in env, f"step {step.get('name')!r} reads {token} but never sets {key}"
    # Guards against the mapping going stale and the test passing vacuously.
    assert checked >= 4


def test_repair_action_pins_the_hakari_release_that_generated_the_committed_output() -> None:
    """A drifting cargo-hakari would rewrite workspace-hack with a new output shape."""
    steps = {step.get("name"): step for step in _action()["runs"]["steps"]}
    assert steps["Install pinned cargo-hakari"]["with"]["tool"] == "cargo-hakari@0.9.38"


@_TARGET_SHELL
def test_target_step_never_evals_api_controlled_values(target_fixture) -> None:
    """Branch names and titles come from the API and must not reach the shell as code."""
    result, outputs = target_fixture.run(head={"sha": "main; touch injected"})

    assert result.returncode == 0, result.stdout + result.stderr
    assert outputs.get("target") == "abort"
    assert "head advanced" in outputs.get("reason", "")
    # The hostile value was reported, never executed.
    assert not (target_fixture.workspace / "injected").exists()
