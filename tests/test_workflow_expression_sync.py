"""Workflow expression sync gate contract tests.

The gate exists because a GitHub Actions expression and the golden-test
literal that pins it used to drift apart, and the mismatch only surfaced
after the full test matrix had already run. These tests pin the gate's own
behaviour so it cannot quietly become a green stub: a check that always
passes is worse than no check at all.

The real-repository case is asserted too, so a binding that stops resolving
(for example after a workflow key is renamed) fails here instead of silently
narrowing the gate's coverage.
"""

from __future__ import annotations

import importlib.util
from pathlib import Path
import sys
import textwrap

import pytest

from conftest import REPO_ROOT

SCRIPT_PATH = REPO_ROOT / "scripts" / "ci" / "check_workflow_expression_sync.py"


def _load_gate_module():
    spec = importlib.util.spec_from_file_location("check_workflow_expression_sync", SCRIPT_PATH)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    # NamedTuple resolves its string annotations through sys.modules, so the
    # module has to be registered before the class body executes.
    sys.modules["check_workflow_expression_sync"] = module
    spec.loader.exec_module(module)
    return module


gate = _load_gate_module()

_REAL_BINDINGS = gate.BINDINGS


@pytest.fixture(autouse=True)
def _restore_bindings():
    """Isolate each test's binding overrides.

    The gate reads the module-level ``BINDINGS`` table, so a test that swaps
    it to point at a fixture repository would otherwise leak into every later
    test -- including the one that checks the real repository.
    """
    gate.BINDINGS = _REAL_BINDINGS
    yield
    gate.BINDINGS = _REAL_BINDINGS


def _write(root: Path, relative: str, content: str) -> None:
    path = root / relative
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(textwrap.dedent(content), encoding="utf-8")


# A minimal but structurally real pair: one workflow, one golden test.
CLEAN_WORKFLOW = """\
    on:
      workflow_dispatch:
    jobs:
      upload:
        steps:
          - uses: softprops/action-gh-release@v3
            with:
              overwrite_files: ${{ inputs.overwrite-release-assets }}
    """

CLEAN_TEST = """\
    OVERWRITE_FILES_EXPRESSION = "${{ inputs.overwrite-release-assets }}"

    def test_pins_the_expression() -> None:
        assert workflow_value == OVERWRITE_FILES_EXPRESSION
    """


def _clean_repo(tmp_path: Path) -> Path:
    _write(tmp_path, ".github/workflows/release.yml", CLEAN_WORKFLOW)
    _write(tmp_path, "tests/test_release_workflow.py", CLEAN_TEST)
    return tmp_path


def _single_binding() -> tuple:
    return (
        gate.Binding(
            workflow=".github/workflows/release.yml",
            path="jobs.upload.steps.with.overwrite_files",
            test_file="tests/test_release_workflow.py",
            test_name="OVERWRITE_FILES_EXPRESSION",
            uses="softprops/action-gh-release@v3",
        ),
    )


def test_gate_passes_when_both_sides_agree(tmp_path: Path) -> None:
    root = _clean_repo(tmp_path)
    gate.BINDINGS = _single_binding()

    mismatches, errors = gate.check(root)

    assert (mismatches, errors) == ([], [])


def test_gate_fails_when_the_workflow_drifts(tmp_path: Path) -> None:
    """A workflow-only edit must turn the gate red."""
    root = _clean_repo(tmp_path)
    _write(
        root,
        ".github/workflows/release.yml",
        CLEAN_WORKFLOW.replace("inputs.overwrite-release-assets", "inputs.overwrite-everything"),
    )
    gate.BINDINGS = _single_binding()

    mismatches, errors = gate.check(root)

    assert errors == []
    assert len(mismatches) == 1
    assert mismatches[0].actual == "${{ inputs.overwrite-everything }}"


def test_gate_fails_when_only_the_test_drifts(tmp_path: Path) -> None:
    """A test-only edit must also turn the gate red.

    This is the direction a hard-coded expectation table would miss: the
    expected value is read out of the test, so moving the test moves the gate.
    """
    root = _clean_repo(tmp_path)
    _write(
        root,
        "tests/test_release_workflow.py",
        CLEAN_TEST.replace("inputs.overwrite-release-assets", "inputs.overwrite-release-assets-v2"),
    )
    gate.BINDINGS = _single_binding()

    mismatches, errors = gate.check(root)

    assert errors == []
    assert len(mismatches) == 1
    assert mismatches[0].expected == "${{ inputs.overwrite-release-assets-v2 }}"


def test_report_names_both_sides_of_the_mismatch(tmp_path: Path) -> None:
    """The failure must locate the workflow line and the test line."""
    root = _clean_repo(tmp_path)
    _write(
        root,
        ".github/workflows/release.yml",
        CLEAN_WORKFLOW.replace("inputs.overwrite-release-assets", "inputs.overwrite-everything"),
    )
    gate.BINDINGS = _single_binding()

    mismatches, errors = gate.check(root)
    report = gate.format_report(root, mismatches, errors)

    assert ".github/workflows/release.yml:" in report
    assert "jobs.upload.steps.with.overwrite_files" in report
    assert "tests/test_release_workflow.py:" in report
    assert "OVERWRITE_FILES_EXPRESSION" in report


def test_unresolvable_binding_is_reported_not_ignored(tmp_path: Path) -> None:
    """A binding that cannot be resolved must fail, never pass silently."""
    root = _clean_repo(tmp_path)
    gate.BINDINGS = (
        gate.Binding(
            workflow=".github/workflows/release.yml",
            path="jobs.upload.steps.with.does-not-exist",
            test_file="tests/test_release_workflow.py",
            test_name="OVERWRITE_FILES_EXPRESSION",
            uses="softprops/action-gh-release@v3",
        ),
    )

    mismatches, errors = gate.check(root)

    assert mismatches == []
    assert len(errors) == 1
    assert "does-not-exist" in errors[0].reason


def test_constant_follows_references_to_other_constants(tmp_path: Path) -> None:
    """A composite literal built from several constants still resolves."""
    root = _clean_repo(tmp_path)
    _write(
        root,
        "tests/test_release_workflow.py",
        """\
        GUARD = " && inputs.backfill_assets != true"
        FULL = "${{ github.event_name == 'workflow_dispatch'" + GUARD + " }}"
        """,
    )
    _write(
        root,
        ".github/workflows/release.yml",
        """\
        jobs:
          upload:
            with:
              reuse: ${{ github.event_name == 'workflow_dispatch' && inputs.backfill_assets != true }}
        """,
    )
    gate.BINDINGS = (
        gate.Binding(
            workflow=".github/workflows/release.yml",
            path="jobs.upload.with.reuse",
            test_file="tests/test_release_workflow.py",
            test_name="FULL",
        ),
    )

    mismatches, errors = gate.check(root)

    assert (mismatches, errors) == ([], [])


def test_dict_assertion_literals_are_read_from_the_test(tmp_path: Path) -> None:
    """Literals inside an ``assert x["with"] == {...}`` are read, not restated."""
    root = _clean_repo(tmp_path)
    _write(
        root,
        "tests/test_release_workflow.py",
        """\
        def test_upload() -> None:
            assert upload["with"] == {
                "overwrite_files": "${{ inputs.overwrite-release-assets }}",
            }
        """,
    )
    gate.BINDINGS = (
        gate.Binding(
            workflow=".github/workflows/release.yml",
            path="jobs.upload.steps.with.overwrite_files",
            test_file="tests/test_release_workflow.py",
            test_name="overwrite_files@with",
            uses="softprops/action-gh-release@v3",
        ),
    )

    mismatches, errors = gate.check(root)

    assert (mismatches, errors) == ([], [])


def test_exit_code_is_one_on_drift_and_zero_when_clean(tmp_path: Path) -> None:
    root = _clean_repo(tmp_path)
    gate.BINDINGS = _single_binding()

    assert gate.main(["--root", str(root)]) == 0

    _write(
        root,
        ".github/workflows/release.yml",
        CLEAN_WORKFLOW.replace("inputs.overwrite-release-assets", "inputs.overwrite-everything"),
    )
    assert gate.main(["--root", str(root)]) == 1


def test_real_repository_bindings_all_resolve() -> None:
    """Every binding must resolve against the real repo.

    Guards the gate against silently losing coverage: if a workflow key is
    renamed, the binding stops resolving and the gate would otherwise report
    nothing while checking nothing.
    """
    mismatches, errors = gate.check(REPO_ROOT)

    assert errors == [], f"unresolvable bindings: {[error.reason for error in errors]}"
    assert mismatches == [], f"drifted bindings: {[mismatch.binding.path for mismatch in mismatches]}"


def test_bindings_are_unique() -> None:
    """Two bindings for the same path would let one mask the other."""
    seen = set()
    for binding in gate.BINDINGS:
        key = (binding.workflow, binding.path, binding.test_file, binding.test_name)
        assert key not in seen, f"duplicate binding: {key}"
        seen.add(key)


def test_every_binding_pins_an_expression() -> None:
    """A binding that does not pin an expression is out of scope for this gate."""
    for binding in gate.BINDINGS:
        ok, value, _line, reason = gate.read_test_literal(REPO_ROOT, binding)
        assert ok, f"{binding.test_name}: {reason}"
        assert "${{" in value, f"{binding.test_name} does not pin an expression: {value!r}"


def test_every_pinned_expression_has_a_binding() -> None:
    """The reverse of the check above: no pinned expression may be unbound.

    Without this, adding a new golden constant while forgetting its binding
    would silently narrow the gate -- neither the gate nor any test would go
    red. This is the guard that makes a coverage gap visible.
    """
    unbound = gate.unbound_pinned_constants(REPO_ROOT)

    assert unbound == [], (
        "pinned constants with no binding; add a Binding for each, or record it "
        f"in KNOWN_UNBOUND_PINNED if it cannot be resolved by path: {unbound}"
    )


def test_completeness_guard_detects_an_unbound_constant(tmp_path: Path) -> None:
    """The guard itself must fail, not pass vacuously."""
    (tmp_path / "tests").mkdir()
    (tmp_path / "tests" / "test_release_workflow.py").write_text(
        textwrap.dedent(
            """\
            UNBOUND_EXPRESSION = "${{ github.event_name == 'push' }}"
            """
        ),
        encoding="utf-8",
    )

    unbound = gate.unbound_pinned_constants(tmp_path)

    assert ("tests/test_release_workflow.py", "UNBOUND_EXPRESSION") in unbound


def test_github_annotations_put_attributes_before_the_message() -> None:
    """GitHub parses ``::error file=...,line=...,title=...::message``.

    Attributes placed after the message make the first comma-segment an
    unknown key, so ``file`` / ``line`` / ``title`` are dropped and the
    annotation attaches to no file. This is the regression that hid the
    location from the CI log.
    """
    binding = gate.Binding(
        workflow=".github/workflows/release.yml",
        path="jobs.upload.with.reuse",
        test_file="tests/test_release_workflow.py",
        test_name="X",
    )
    mismatch = gate.Mismatch(
        binding=binding,
        workflow_line=700,
        test_line=16,
        actual="a",
        expected="b, c",
        reason="does not match",
    )
    annotation = gate.format_github_annotations(REPO_ROOT, [mismatch], [])

    assert annotation.startswith("::error file=.github/workflows/release.yml,line=700,title=")
    # The message must come after the '::' separator, and a comma inside it
    # must not be read as an attribute separator.
    _head, sep, _message = annotation.partition("::")
    assert sep == "::"
    assert "b%2C c" in annotation


def test_annotation_mode_still_prints_the_text_report(tmp_path: Path, capsys) -> None:
    """The location must not depend on the CI runner parsing annotations."""
    root = _clean_repo(tmp_path)
    gate.BINDINGS = _single_binding()
    _write(
        root,
        ".github/workflows/release.yml",
        CLEAN_WORKFLOW.replace("inputs.overwrite-release-assets", "inputs.overwrite-everything"),
    )

    exit_code = gate.main(["--root", str(root), "--format", "github"])
    captured = capsys.readouterr()

    assert exit_code == 1
    # Both the plain-text location report and the annotation are emitted.
    assert "jobs.upload.steps.with.overwrite_files" in captured.out
    assert "::error file=" in captured.out
