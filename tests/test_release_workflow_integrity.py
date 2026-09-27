"""Tests for the release workflow digest drift check.

The release workflow carries PyPI Trusted Publishing credentials, so the check
that binds it to a snapshot has to fail closed: every test here either proves a
real change is caught or that a cosmetic one is not, and none of them may pass
by asserting nothing.

This module is the rollout copy of the check and is deliberately
configuration-free: it locates the script and the workflow by path relative to
this file, so moving it to another repository needs no edits. It also loads the
script through ``importlib`` rather than importing it as a package, because the
repositories this lands in disagree about ``sys.path`` -- some add the project
root to ``pythonpath``, one removes it again in ``conftest.py`` to keep a
top-level plugin directory from shadowing a real module. A path-based load is
the only form that behaves identically in all of them.
"""

from __future__ import annotations

import copy
import importlib.util
import pathlib
import shutil
import subprocess
import sys

import pytest
import yaml

REPO_ROOT = pathlib.Path(__file__).resolve().parents[1]
CHECK_SCRIPT = REPO_ROOT / "scripts" / "ci" / "check_release_workflow_digest.py"


def _load_check_module():
    """Load the check script from its path, whatever the repo does to sys.path."""
    spec = importlib.util.spec_from_file_location("check_release_workflow_digest", CHECK_SCRIPT)
    if spec is None or spec.loader is None:  # pragma: no cover - defensive
        raise RuntimeError(f"cannot load the release workflow check from {CHECK_SCRIPT}")
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


_check = _load_check_module()
APPROVED_SNAPSHOT = _check.APPROVED_SNAPSHOT
RELEASE_WORKFLOW = _check.RELEASE_WORKFLOW
DriftError = _check.DriftError
main = _check.main
release_workflow_digest = _check.release_workflow_digest
StrictWorkflowLoader = _check.StrictWorkflowLoader

MINIMAL_WORKFLOW = """name: Release

on:
  push:
    branches: [main]

permissions: {}

jobs:
  publish:
    runs-on: ubuntu-latest
    permissions:
      id-token: write
    steps:
      - run: echo publish
"""

# A `run:` block scalar is a shell script, so its whitespace is content. The
# two pairs below differ only inside that block, in ways YAML parsing preserves
# and bash then executes differently.
CONTINUATION_WORKFLOW = (
    "name: Release\n"
    "jobs:\n"
    "  build:\n"
    "    runs-on: ubuntu-latest\n"
    "    steps:\n"
    "      - run: |\n"
    "          echo one \\\n"
    "            two\n"
)

# One trailing space after the line-continuation backslash: bash then reads
# `<space>` as the escaped character, ends the command at the newline, and
# runs `two` as its own command.
CONTINUATION_TAMPERED = CONTINUATION_WORKFLOW.replace("echo one \\\n", "echo one \\ \n")

HEREDOC_WORKFLOW = (
    "name: Release\n"
    "jobs:\n"
    "  build:\n"
    "    runs-on: ubuntu-latest\n"
    "    steps:\n"
    "      - run: |\n"
    "          cat > out.txt <<'EOF'\n"
    "          line1\n"
    "          line2\n"
    "          EOF\n"
)

# One blank line inside the heredoc body: the script writes a different file.
HEREDOC_TAMPERED = HEREDOC_WORKFLOW.replace("          line1\n", "          line1\n\n")


# Same document as MINIMAL_WORKFLOW, different bytes: key order, comments,
# blank lines, CRLF endings and quoting style all differ.
COSMETIC_VARIANT = (
    "# leading comment\r\n"
    "\r\n"
    "name: Release\r\n"
    "permissions: {}\r\n"
    "jobs:\r\n"
    "  publish:\r\n"
    "    steps:\n"
    "      # a comment inside the step\n"
    "      - run: echo publish\n"
    "\n"
    "    permissions:\n"
    "      id-token: write\n"
    "    runs-on: ubuntu-latest\r\n"
    "on:\r\n"
    "  push:\r\n"
    "    branches: ['main']\r\n"
)

# GitHub permission scopes. The real-workflow tests below add one that the
# workflow does not already grant, so the mutation is always a real change.
PERMISSION_SCOPES = (
    "packages",
    "attestations",
    "contents",
    "actions",
    "checks",
    "deployments",
    "discussions",
    "issues",
    "pages",
    "pull-requests",
    "repository-projects",
    "security-events",
    "statuses",
)


def _write(path: pathlib.Path, text: str) -> pathlib.Path:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(text.encode("utf-8"))
    return path


def _run(monkeypatch, capsys, workflow: pathlib.Path, snapshot: pathlib.Path, *extra: str):
    monkeypatch.setattr(
        sys,
        "argv",
        ["check_release_workflow_digest", "--workflow", str(workflow), "--snapshot", str(snapshot), *extra],
    )
    code = main()
    captured = capsys.readouterr()
    return code, captured.out, captured.err


def _real_release_document():
    """Parse the repository's own release workflow with the strict loader."""
    return yaml.load(RELEASE_WORKFLOW.read_text(encoding="utf-8"), Loader=StrictWorkflowLoader)


def _dump(document: dict, sort_keys: bool = False) -> str:
    return yaml.safe_dump(document, sort_keys=sort_keys, default_flow_style=False, allow_unicode=True, width=4096)


def _write_document(tmp_path: pathlib.Path, document: dict, name: str) -> pathlib.Path:
    return _write(tmp_path / name, _dump(document))


def _job_with_permissions(document: dict) -> str:
    jobs = document.get("jobs") or {}
    for name, job in jobs.items():
        if isinstance(job, dict) and isinstance(job.get("permissions"), dict):
            return name
    raise AssertionError("the release workflow must grant job-level permissions")


def _job_with_most_steps(document: dict) -> str:
    jobs = document.get("jobs") or {}
    stepped = {name: job for name, job in jobs.items() if isinstance(job, dict) and job.get("steps")}
    if not stepped:
        raise AssertionError("the release workflow must contain at least one job with steps")
    return max(stepped, key=lambda name: len(stepped[name]["steps"]))


def test_committed_release_workflow_matches_the_approved_snapshot(monkeypatch, capsys):
    code, out, err = _run(monkeypatch, capsys, RELEASE_WORKFLOW, APPROVED_SNAPSHOT)

    assert code == 0, err
    assert "integrity ok" in out


def test_snapshot_digest_equals_release_workflow_digest():
    # The invariant the CI job enforces, asserted directly so a broken argparse
    # path cannot hide a drifting snapshot.
    assert release_workflow_digest(RELEASE_WORKFLOW) == release_workflow_digest(APPROVED_SNAPSHOT)


def test_structural_drift_is_detected(tmp_path, monkeypatch, capsys):
    snapshot = _write(tmp_path / "approved.yml", MINIMAL_WORKFLOW)
    drifted = _write(
        tmp_path / "release.yml", MINIMAL_WORKFLOW.replace("permissions: {}", "permissions:\n  contents: write")
    )

    code, out, err = _run(monkeypatch, capsys, drifted, snapshot)

    assert code == 1
    assert "drifted" in err
    assert release_workflow_digest(snapshot) in err
    assert release_workflow_digest(drifted) in err
    assert out == ""


def test_dropping_a_publish_step_is_detected(tmp_path, monkeypatch, capsys):
    snapshot = _write(tmp_path / "approved.yml", MINIMAL_WORKFLOW)
    drifted = _write(tmp_path / "release.yml", MINIMAL_WORKFLOW.replace("      - run: echo publish\n", ""))

    code, _, err = _run(monkeypatch, capsys, drifted, snapshot)

    assert code == 1
    assert "drifted" in err


def test_cosmetic_edits_do_not_move_the_digest(tmp_path):
    original = _write(tmp_path / "original.yml", MINIMAL_WORKFLOW)
    cosmetic = _write(tmp_path / "cosmetic.yml", COSMETIC_VARIANT)

    assert original.read_bytes() != cosmetic.read_bytes()
    assert release_workflow_digest(original) == release_workflow_digest(cosmetic)


def test_cosmetic_drift_does_not_fail_the_check(tmp_path, monkeypatch, capsys):
    snapshot = _write(tmp_path / "approved.yml", MINIMAL_WORKFLOW)
    cosmetic = _write(tmp_path / "release.yml", COSMETIC_VARIANT)

    code, out, err = _run(monkeypatch, capsys, cosmetic, snapshot)

    assert code == 0, err
    assert "integrity ok" in out


def test_missing_snapshot_fails_closed(tmp_path, monkeypatch, capsys):
    workflow = _write(tmp_path / "release.yml", MINIMAL_WORKFLOW)

    code, _, err = _run(monkeypatch, capsys, workflow, tmp_path / "does-not-exist.yml")

    assert code == 1
    assert "unavailable" in err
    assert "does-not-exist.yml" in err


def test_missing_workflow_fails_closed(tmp_path, monkeypatch, capsys):
    snapshot = _write(tmp_path / "approved.yml", MINIMAL_WORKFLOW)

    code, _, err = _run(monkeypatch, capsys, tmp_path / "does-not-exist.yml", snapshot)

    assert code == 1
    assert "unavailable" in err


def test_yaml_aliases_are_rejected(tmp_path):
    aliased = _write(
        tmp_path / "aliased.yml",
        "name: Release\npermissions: &perms\n  contents: write\njobs:\n  publish:\n    permissions: *perms\n",
    )

    with pytest.raises(DriftError, match="aliases"):
        release_workflow_digest(aliased)


def test_duplicate_mapping_keys_are_rejected(tmp_path):
    duplicated = _write(
        tmp_path / "duplicated.yml",
        "name: Release\nname: Release Again\npermissions: {}\n",
    )

    with pytest.raises(DriftError, match="duplicate"):
        release_workflow_digest(duplicated)


def test_unsupported_value_types_are_rejected(tmp_path):
    # An unquoted ISO date parses as a datetime.date, which the canonical form
    # refuses rather than silently serialising in a PyYAML-specific way.
    dated = _write(tmp_path / "dated.yml", "name: Release\ncreated: 2026-09-26\n")

    with pytest.raises(DriftError, match="unsupported value type"):
        release_workflow_digest(dated)


def test_update_snapshot_refreshes_the_record(tmp_path, monkeypatch, capsys):
    workflow = _write(tmp_path / "release.yml", MINIMAL_WORKFLOW)
    snapshot = tmp_path / "approved.yml"

    code, out, _ = _run(monkeypatch, capsys, workflow, snapshot, "--update-snapshot")
    assert code == 0
    assert snapshot.is_file()
    assert release_workflow_digest(snapshot) in out

    code, out, err = _run(monkeypatch, capsys, workflow, snapshot)
    assert code == 0, err

    _write(workflow, MINIMAL_WORKFLOW.replace("permissions: {}", "permissions:\n  contents: write"))
    code, _, err = _run(monkeypatch, capsys, workflow, snapshot)
    assert code == 1
    assert "drifted" in err


def test_print_digest_emits_the_canonical_digest(tmp_path, monkeypatch, capsys):
    workflow = _write(tmp_path / "release.yml", MINIMAL_WORKFLOW)

    code, out, _ = _run(monkeypatch, capsys, workflow, tmp_path / "unused.yml", "--print-digest")

    assert code == 0
    assert out.strip() == release_workflow_digest(workflow)
    assert len(out.strip()) == 64


def test_trailing_space_after_a_line_continuation_is_detected(tmp_path, monkeypatch, capsys):
    r"""A space after a `\` continuation changes what bash runs.

    The trailing space is invisible to YAML, so only the digest can catch it.
    """
    original = _write(tmp_path / "original.yml", CONTINUATION_WORKFLOW)
    tampered = _write(tmp_path / "tampered.yml", CONTINUATION_TAMPERED)

    assert tampered.read_text() != original.read_text()
    assert "\\\n" in yaml.safe_load(original.read_text())["jobs"]["build"]["steps"][0]["run"]
    assert release_workflow_digest(original) != release_workflow_digest(tampered)

    code, _, err = _run(monkeypatch, capsys, tampered, original)
    assert code == 1
    assert "drifted" in err


def test_blank_line_inside_a_heredoc_is_detected(tmp_path, monkeypatch, capsys):
    """A blank line inside a heredoc body changes the file the script writes."""
    original = _write(tmp_path / "original.yml", HEREDOC_WORKFLOW)
    tampered = _write(tmp_path / "tampered.yml", HEREDOC_TAMPERED)

    assert release_workflow_digest(original) != release_workflow_digest(tampered)

    code, _, err = _run(monkeypatch, capsys, tampered, original)
    assert code == 1
    assert "drifted" in err


def test_comment_line_inside_a_run_block_is_detected(tmp_path):
    """Inside a `run` block even a comment is script content, so it moves the digest."""
    original = _write(tmp_path / "original.yml", HEREDOC_WORKFLOW)
    commented = _write(
        tmp_path / "commented.yml",
        HEREDOC_WORKFLOW.replace("          line1\n", "          # a shell comment\n          line1\n"),
    )

    assert release_workflow_digest(original) != release_workflow_digest(commented)


@pytest.mark.skipif(
    sys.platform == "win32" or shutil.which("bash") is None,
    reason="the bash proof needs POSIX argv handling; Windows bash builds mangle a multi-line -c argument",
)
def test_the_continuation_tamper_actually_changes_what_bash_runs():
    """Evidence that the P2 finding is a real defect, not a digest curiosity.

    Running the two scripts under bash shows the trailing space really does
    split one command into two. The digest assertions above do not depend on
    this test: they hold on every platform, with or without bash.
    """

    def _exit_code(workflow_text: str) -> int:
        script = yaml.safe_load(workflow_text)["jobs"]["build"]["steps"][0]["run"]
        return subprocess.run(["bash", "-c", script], capture_output=True).returncode

    assert _exit_code(CONTINUATION_WORKFLOW) == 0
    assert _exit_code(CONTINUATION_TAMPERED) != 0


# ---------------------------------------------------------------------------
# Proofs against the repository's own release workflow
#
# The tests above use a synthetic workflow, so they prove the digest behaves.
# They do not prove it protects *this* repository: a workflow whose jobs,
# permissions and steps the check silently failed to bind would still pass
# them. The three tests below drive the real `.github/workflows/release.yml`,
# so they fail if the workflow's shape stops matching what the check claims to
# cover.
# ---------------------------------------------------------------------------


def test_real_release_workflow_grants_a_permission_the_snapshot_records():
    """Precondition guard: the two mutation proofs need a permission block."""
    document = _real_release_document()
    job = document["jobs"][_job_with_permissions(document)]

    assert "permissions" in job
    assert job["permissions"]


def test_real_release_workflow_permission_change_is_detected(tmp_path, monkeypatch, capsys):
    """Adding a scope to a real job's `permissions` fails the check."""
    document = _real_release_document()
    job_name = _job_with_permissions(document)
    snapshot = _write_document(tmp_path, document, "approved.yml")

    mutated = copy.deepcopy(document)
    granted = mutated["jobs"][job_name]["permissions"]
    extra_scope = next(scope for scope in PERMISSION_SCOPES if scope not in granted)
    granted[extra_scope] = "write"
    drifted = _write_document(tmp_path, mutated, "release.yml")

    assert release_workflow_digest(snapshot) != release_workflow_digest(drifted)

    code, _, err = _run(monkeypatch, capsys, drifted, snapshot)
    assert code == 1
    assert "drifted" in err


def test_real_release_workflow_step_change_is_detected(tmp_path, monkeypatch, capsys):
    """Dropping a step from the real workflow fails the check."""
    document = _real_release_document()
    job_name = _job_with_most_steps(document)
    snapshot = _write_document(tmp_path, document, "approved.yml")

    mutated = copy.deepcopy(document)
    steps = mutated["jobs"][job_name]["steps"]
    assert len(steps) > 1
    steps.pop()
    drifted = _write_document(tmp_path, mutated, "release.yml")

    assert release_workflow_digest(snapshot) != release_workflow_digest(drifted)

    code, _, err = _run(monkeypatch, capsys, drifted, snapshot)
    assert code == 1
    assert "drifted" in err


def test_real_release_workflow_cosmetic_reformat_is_not_reported(tmp_path, monkeypatch, capsys):
    """Reformatting the real workflow -- reordering keys, CRLF, a comment -- is not drift."""
    document = _real_release_document()
    snapshot = _write_document(tmp_path, document, "approved.yml")

    reformatted = _write(
        tmp_path / "release.yml",
        "# cosmetic-only reformat: key order, quoting and line endings differ\n" + _dump(document, sort_keys=True),
    )
    reformatted.write_bytes(reformatted.read_bytes().replace(b"\n", b"\r\n"))

    assert snapshot.read_bytes() != reformatted.read_bytes()
    assert release_workflow_digest(snapshot) == release_workflow_digest(reformatted)

    code, out, err = _run(monkeypatch, capsys, reformatted, snapshot)
    assert code == 0, err
    assert "integrity ok" in out
