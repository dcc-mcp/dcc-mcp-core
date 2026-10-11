"""Line-ending gate contract tests.

The gate reads committed blobs, so these tests build throwaway git
repositories instead of writing into the repository under test.
"""

from __future__ import annotations

import importlib.util
from pathlib import Path
import shutil
import subprocess
import sys

import pytest

from conftest import REPO_ROOT

SCRIPT_PATH = REPO_ROOT / "scripts" / "ci" / "check_line_endings.py"

CORE_RELEASE_FIXTURE = "crates/dcc-mcp-attestation/tests/fixtures/core-release.json"

LF_SOURCE = b"import os\n\n\ndef main() -> int:\n    return 0\n"
CRLF_SOURCE = b"import os\r\n\r\n\r\ndef main() -> int:\r\n    return 0\r\n"
CRLF_COUNT = CRLF_SOURCE.count(bytes([13]))


def _load_gate_module():
    spec = importlib.util.spec_from_file_location("check_line_endings", SCRIPT_PATH)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    # NamedTuple resolves its string annotations through sys.modules, so the
    # module has to be registered before the class body executes.
    sys.modules["check_line_endings"] = module
    spec.loader.exec_module(module)
    return module


gate = _load_gate_module()


def _git(root: Path, *args: str) -> None:
    subprocess.run(["git", "-C", str(root), *args], check=True, capture_output=True)


def _init_repo(root: Path) -> Path:
    root.mkdir(parents=True, exist_ok=True)
    _git(root, "init", "-q")
    # Keep the fixture independent of the developer's global git config.
    _git(root, "config", "core.autocrlf", "false")
    _git(root, "config", "user.email", "ci@example.com")
    _git(root, "config", "user.name", "CI")
    return root


def _commit(root: Path, relative: str, data: bytes) -> None:
    target = root / relative
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_bytes(data)
    _git(root, "add", "--", relative)
    _git(root, "commit", "-q", "-m", f"add {relative}")


@pytest.fixture
def repo(tmp_path: Path) -> Path:
    if shutil.which("git") is None:
        pytest.skip("git is not available")
    return _init_repo(tmp_path / "repo")


def test_lf_blobs_pass(repo: Path) -> None:
    _commit(repo, "pkg/module.py", LF_SOURCE)
    _commit(repo, "pkg/nested/deep.py", LF_SOURCE)

    assert gate.find_cr_files(repo) == []
    assert gate.main(["--root", str(repo)]) == 0


def test_crlf_blob_is_reported(repo: Path) -> None:
    _commit(repo, "pkg/module.py", CRLF_SOURCE)

    violations = gate.find_cr_files(repo)

    assert [(item.path, item.cr_count) for item in violations] == [("pkg/module.py", CRLF_COUNT)]
    assert gate.main(["--root", str(repo)]) == 1


def test_crlf_blob_is_read_from_the_index_not_the_worktree(repo: Path) -> None:
    """A CRLF commit stays visible after the checkout is rewritten as LF."""
    _commit(repo, "pkg/module.py", CRLF_SOURCE)
    (repo / "pkg" / "module.py").write_bytes(LF_SOURCE)

    assert gate.find_cr_files(repo) != []


def test_untracked_crlf_file_is_ignored(repo: Path) -> None:
    _commit(repo, "pkg/module.py", LF_SOURCE)
    (repo / "pkg" / "untracked.py").write_bytes(CRLF_SOURCE)

    assert gate.find_cr_files(repo) == []
    assert gate.main(["--root", str(repo)]) == 0


def test_pattern_narrows_the_scope(repo: Path) -> None:
    _commit(repo, "notes.txt", b"one\r\ntwo\r\n")

    assert gate.find_cr_files(repo) == []
    assert gate.find_cr_files(repo, ("*.txt",)) != []
    assert gate.main(["--root", str(repo), "--pattern", "*.txt"]) == 1


def test_path_with_spaces_is_reported(repo: Path) -> None:
    _commit(repo, "pkg/with space.py", CRLF_SOURCE)

    violations = gate.find_cr_files(repo)

    assert [item.path for item in violations] == ["pkg/with space.py"]


def test_report_truncates_long_violation_lists(repo: Path) -> None:
    for index in range(4):
        _commit(repo, f"pkg/mod_{index}.py", CRLF_SOURCE)

    report = gate.format_report(gate.find_cr_files(repo), ("*.py",), max_reported=2)

    assert "4 tracked file(s)" in report
    assert "... and 2 more" in report
    assert "git add --renormalize" in report


def test_outside_a_git_repository_reports_a_tool_error(tmp_path: Path) -> None:
    assert gate.main(["--root", str(tmp_path / "not-a-repo")]) == 2


def _tracked_blob(relative: str) -> bytes | None:
    """Return the committed blob for ``relative``, or None when it is untracked."""
    for path, blob in gate.tracked_blobs(REPO_ROOT, (relative,)):
        if path == relative:
            return blob
    return None


def test_repository_covered_sources_are_lf() -> None:
    """Guard the repository itself: no tracked blob in DEFAULT_PATTERNS may carry a CR byte."""
    if shutil.which("git") is None:
        pytest.skip("git is not available")

    assert gate.find_cr_files(REPO_ROOT) == []


def _pinned_extensions(gitattributes: str) -> set[str]:
    """Return the extensions pinned to LF by ``*.ext ... text eol=lf`` rules.

    Tolerates a trailing comment and any attribute order: a gitattributes rule
    is a pattern followed by attributes, and ``#`` starts a comment that may
    sit on the same line as a rule.
    """
    pinned: set[str] = set()
    for raw_line in gitattributes.splitlines():
        line = raw_line.split("#", 1)[0].strip()
        if not line.startswith("*."):
            continue
        tokens = line.split()
        if len(tokens) >= 2 and "text" in tokens and "eol=lf" in tokens:
            pinned.add(tokens[0][2:])
    return pinned


def test_pinned_extensions_tolerates_comments_and_attribute_order() -> None:
    """A trailing comment or reordered attributes must not hide a pinned extension."""
    content = "\n".join(
        [
            "# a standalone comment",
            "*.py text eol=lf  # inline comment",
            "*.toml eol=lf text",
            "*.rs -text",
            "",
            "*.md text",
        ]
    )

    assert _pinned_extensions(content) == {"py", "toml"}


def test_every_gated_extension_is_pinned_to_lf_in_gitattributes() -> None:
    """DEFAULT_PATTERNS and .gitattributes must not drift apart."""
    pinned = _pinned_extensions((REPO_ROOT / ".gitattributes").read_text(encoding="utf-8"))

    for pattern in gate.DEFAULT_PATTERNS:
        assert pattern.startswith("*."), f"unexpected non-extension pattern: {pattern}"
        assert pattern[2:] in pinned, (
            f"{pattern} is gated by check_line_endings.py but has no `text eol=lf` rule "
            "in .gitattributes, so a CRLF commit would fail CI with no normalization path"
        )


def test_core_release_fixture_still_carries_cr_bytes() -> None:
    """The attestation fixture is read with include_bytes!; its CR bytes are payload."""
    if shutil.which("git") is None:
        pytest.skip("git is not available")

    blob = _tracked_blob(CORE_RELEASE_FIXTURE)

    assert blob is not None, f"{CORE_RELEASE_FIXTURE} must stay tracked"
    assert b"\r" in blob, (
        f"{CORE_RELEASE_FIXTURE} lost its CR bytes: a .gitattributes rule renormalized it, "
        "which breaks the attestation test that consumes the file with include_bytes!"
    )


def test_core_release_fixture_is_exempt_from_text_normalization() -> None:
    """`-text` must win for the fixture, so every exception has to stay last."""
    if shutil.which("git") is None:
        pytest.skip("git is not available")

    completed = subprocess.run(
        ["git", "-C", str(REPO_ROOT), "check-attr", "text", "--", CORE_RELEASE_FIXTURE],
        capture_output=True,
        text=True,
    )

    assert completed.returncode == 0, completed.stderr
    assert completed.stdout.strip().endswith("text: unset"), (
        f"the `-text` exemption no longer wins for {CORE_RELEASE_FIXTURE}: a `text eol=lf` "
        "rule now matches it. .gitattributes resolves last-match-wins, so move the `-text` "
        "exception below every `text eol=lf` rule that can match the same path."
    )
