#!/usr/bin/env python3
"""Behavioural regression tests for the `guard-build-wheels-ran` release gate.

`tests/test_release_workflow_integrity.py` is tamper *evidence*: it proves the
approved snapshot still matches the workflow and that edits are detectable. It
says nothing about what the gate actually decides. This module is the missing
behaviour control.

The gap it closes is concrete. The gate's `run:` body branches on two inputs and
returns a verdict for each combination. Swapping its `success` and `skipped`
arms inverts the verdict while leaving every existing check green — the drift
check only asks for a refreshed snapshot, and a snapshot can be refreshed to
match any text, including an inverted one. Nothing in the repository would have
caught that.

So these tests execute the real script from the real workflow under bash and
assert the exit code for every state. The script is read out of
`.github/workflows/release.yml` rather than copied here, because a copy drifts
from its source in exactly the way this module exists to prevent: edit the
workflow, and the test either follows or fails.

Two properties must hold together, and each is only meaningful with the other:

* `created=true, result=skipped` exits non-zero — the defect is caught.
* `created=true, result=success` exits zero — the gate passes a healthy run.

Asserting only the first would be satisfied by a gate that fails everything.
"""

from __future__ import annotations

import os
import pathlib
import shutil
import subprocess
import sys

import pytest
import yaml

REPO_ROOT = pathlib.Path(__file__).resolve().parents[1]
RELEASE_WORKFLOW = REPO_ROOT / ".github" / "workflows" / "release.yml"
GUARD_JOB = "guard-build-wheels-ran"

needs_bash = pytest.mark.skipif(
    sys.platform == "win32" or shutil.which("bash") is None,
    reason="the bash proof needs POSIX argv handling; Windows bash builds mangle a multi-line -c argument",
)


def _guard_step() -> dict:
    """Load the guard step straight from the workflow under test."""
    workflow = yaml.safe_load(RELEASE_WORKFLOW.read_text(encoding="utf-8"))
    jobs = workflow.get("jobs") or {}
    assert GUARD_JOB in jobs, (
        f"{GUARD_JOB} is missing from {RELEASE_WORKFLOW.name}; "
        "an absent wheel chain is once again read as a passing one"
    )
    job = jobs[GUARD_JOB]
    steps = job.get("steps") or []
    assert steps, f"{GUARD_JOB} declares no steps"
    return steps[0]


def _run_guard(tmp_path: pathlib.Path, *, created: str, result: str, tag: str = "v1.2.3") -> subprocess.CompletedProcess:
    """Execute the guard's script with the given inputs and return the result."""
    step = _guard_step()
    script = step["run"]
    env = {key: str(value) for key, value in (step.get("env") or {}).items()}

    # The step must reach its inputs through the environment. Interpolating them
    # into the script body lets GitHub substitute text before bash runs, so a
    # tag name could execute as shell. Keeping them in `env:` is what makes the
    # injection proof below meaningful.
    assert "RELEASE_CREATED" in env, "release_created must arrive via env, not text interpolation"
    assert "BUILD_WHEELS_RESULT" in env, "build-wheels result must arrive via env, not text interpolation"
    assert "RELEASE_TAG" in env, "tag name must arrive via env, not text interpolation"
    for needle in ("${{", "}}"):
        assert needle not in script, f"guard script still interpolates {needle} into its body"

    env.update(RELEASE_CREATED=created, BUILD_WHEELS_RESULT=result, RELEASE_TAG=tag)

    # Write to a file instead of passing the script to `bash -c`: a multi-line
    # -c argument is re-parsed by some bash builds and the verdict stops being
    # the one the workflow would reach.
    script_path = tmp_path / "guard.sh"
    script_path.write_text(script, encoding="utf-8")
    return subprocess.run(
        ["bash", str(script_path)],
        capture_output=True,
        text=True,
        env={**os.environ, **env},
    )


@needs_bash
@pytest.mark.parametrize(
    ("created", "result"),
    [
        ("false", "skipped"),
        ("false", "success"),
        ("true", "success"),
    ],
)
def test_guard_passes_states_that_must_not_fail_the_run(
    tmp_path: pathlib.Path, created: str, result: str
) -> None:
    """No release, or a release whose wheels built: the gate stays quiet."""
    assert _run_guard(tmp_path, created=created, result=result).returncode == 0


@needs_bash
@pytest.mark.parametrize("result", ["skipped", "failure", "cancelled"])
def test_guard_fails_when_a_released_version_has_no_successful_wheel_chain(
    tmp_path: pathlib.Path, result: str
) -> None:
    """A version was released but no wheel job succeeded: fail the run, loudly."""
    completed = _run_guard(tmp_path, created="true", result=result)
    assert completed.returncode != 0
    assert "::error::" in completed.stdout


@needs_bash
def test_guard_does_not_cry_wolf_on_an_empty_release_created(tmp_path: pathlib.Path) -> None:
    """An unset `release_created` output is a skip, not a release."""
    assert _run_guard(tmp_path, created="", result="skipped").returncode == 0


@needs_bash
def test_the_two_decisive_states_differ(tmp_path: pathlib.Path) -> None:
    """The pair that a swapped-arm regression would silently invert.

    A gate that always fails satisfies the `skipped` assertion alone; a gate
    that always passes satisfies the `success` assertion alone. Only the pair
    pins the branch to the right arm.
    """
    released_skipped = _run_guard(tmp_path, created="true", result="skipped")
    released_success = _run_guard(tmp_path, created="true", result="success")

    assert released_skipped.returncode != 0, "a released version with no wheel chain must fail the run"
    assert released_success.returncode == 0, "a released version with a healthy wheel chain must pass"
    assert released_skipped.returncode != released_success.returncode


@needs_bash
def test_tag_name_is_not_executed_as_shell(tmp_path: pathlib.Path) -> None:
    """A crafted tag must be echoed, not run.

    `workflow_dispatch.release_tag` reaches `tag_name`, and the validation job
    only compares it against the supplied version. Reaching the script through
    `env:` keeps it data; interpolating it into the body would make it code.
    """
    payload = "v0.20.43$(echo PWNED_$(id -u))"
    completed = _run_guard(tmp_path, created="true", result="skipped", tag=payload)
    output = completed.stdout + completed.stderr

    assert "PWNED_0" not in output, "tag name was executed as shell"
    assert payload in output, "the tag should still be reported verbatim"


def test_guard_watches_both_upstream_jobs() -> None:
    """The gate needs the release decision and the wheel chain, and runs on always()."""
    job = yaml.safe_load(RELEASE_WORKFLOW.read_text(encoding="utf-8"))["jobs"][GUARD_JOB]
    assert set(job.get("needs") or []) == {"release-please", "build-wheels"}
    assert job.get("if") == "always()"
