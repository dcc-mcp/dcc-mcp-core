"""P1 coverage for the marketplace-publish-extension CLI entry point and git helper.

Covers ``main`` (the JSON contract printed on stdout, the exit codes, and the
``marketplace.json`` side effects) and ``_git_commit_and_push`` (commit message
shape, command sequence, and failure handling) without pushing anywhere real.
"""

from __future__ import annotations

import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys

import pytest

from conftest import REPO_ROOT

_SCRIPT = REPO_ROOT / "skills" / "marketplace-publish-extension" / "scripts" / "publish.py"
_SPEC = importlib.util.spec_from_file_location("marketplace_publish_cli", _SCRIPT)
assert _SPEC is not None and _SPEC.loader is not None
_PUBLISH = importlib.util.module_from_spec(_SPEC)
_SPEC.loader.exec_module(_PUBLISH)

_GIT_REF = "a" * 40

_SKILL_MD = """---
name: maya-pipeline-tools
description: >-
  Publish Maya pipeline tools.
metadata:
  dcc-mcp:
    dcc: maya
---

# Maya Pipeline Tools
"""


def _make_extension(tmp_path: Path, name: str = "maya-pipeline-tools") -> Path:
    ext_dir = tmp_path / "extensions" / name
    ext_dir.mkdir(parents=True)
    (ext_dir / "SKILL.md").write_text(_SKILL_MD.replace("maya-pipeline-tools", name), encoding="utf-8")
    return ext_dir


def _run_main(monkeypatch, capsys, *args: str) -> dict:
    monkeypatch.setattr(sys, "argv", ["publish.py", *args])
    _PUBLISH.main()
    return json.loads(capsys.readouterr().out)


def _run_main_expecting_failure(monkeypatch, capsys, *args: str) -> dict:
    monkeypatch.setattr(sys, "argv", ["publish.py", *args])
    with pytest.raises(SystemExit) as excinfo:
        _PUBLISH.main()
    assert excinfo.value.code == 1
    return json.loads(capsys.readouterr().out)


# ── main(): success paths ─────────────────────────────────────────────────────


def test_main_creates_a_new_entry_and_writes_the_catalog(tmp_path: Path, monkeypatch, capsys) -> None:
    ext_dir = _make_extension(tmp_path)
    catalog_path = tmp_path / "marketplace.json"

    result = _run_main(
        monkeypatch,
        capsys,
        "--extension_dir",
        str(ext_dir),
        "--marketplace_source",
        str(catalog_path),
        "--install_url",
        "https://github.com/dcc-mcp/maya-pipeline-tools.git",
        "--install_ref",
        _GIT_REF,
    )

    assert result["success"] is True
    assert result["message"] == "Successfully created marketplace entry: maya-pipeline-tools"
    context = result["context"]
    assert context["action"] == "created"
    assert context["was_updated"] is False
    assert context["total_entries"] == 1
    assert context["entry"]["dcc"] == ["maya"]
    assert Path(context["marketplace_path"]) == catalog_path.resolve()

    persisted = json.loads(catalog_path.read_text(encoding="utf-8"))
    assert persisted["entries"][0]["name"] == "maya-pipeline-tools"


def test_main_updates_an_existing_entry_on_a_second_run(tmp_path: Path, monkeypatch, capsys) -> None:
    ext_dir = _make_extension(tmp_path)
    catalog_path = tmp_path / "marketplace.json"
    argv = [
        "--extension_dir",
        str(ext_dir),
        "--marketplace_source",
        str(catalog_path),
        "--install_url",
        "https://github.com/dcc-mcp/maya-pipeline-tools.git",
        "--install_ref",
        _GIT_REF,
    ]

    _run_main(monkeypatch, capsys, *argv)
    result = _run_main(monkeypatch, capsys, *argv, "--version", "2.0.0")

    assert result["message"] == "Successfully updated marketplace entry: maya-pipeline-tools"
    context = result["context"]
    assert context["action"] == "updated"
    assert context["was_updated"] is True
    assert context["total_entries"] == 1
    assert context["entry"]["version"] == "2.0.0"


def test_main_keeps_unrelated_entries(tmp_path: Path, monkeypatch, capsys) -> None:
    ext_dir = _make_extension(tmp_path)
    catalog_path = tmp_path / "marketplace.json"
    catalog_path.write_text(json.dumps({"version": "1", "entries": [{"name": "unrelated"}]}), encoding="utf-8")

    result = _run_main(
        monkeypatch,
        capsys,
        "--extension_dir",
        str(ext_dir),
        "--marketplace_source",
        str(catalog_path),
        "--install_url",
        "https://github.com/dcc-mcp/maya-pipeline-tools.git",
        "--install_ref",
        _GIT_REF,
    )

    assert result["context"]["total_entries"] == 2
    names = [entry["name"] for entry in json.loads(catalog_path.read_text(encoding="utf-8"))["entries"]]
    assert names == ["unrelated", "maya-pipeline-tools"]


def test_main_accepts_a_directory_as_marketplace_source(tmp_path: Path, monkeypatch, capsys) -> None:
    ext_dir = _make_extension(tmp_path)
    marketplace_dir = tmp_path / "marketplace"
    marketplace_dir.mkdir()

    result = _run_main(
        monkeypatch,
        capsys,
        "--extension_dir",
        str(ext_dir),
        "--marketplace_source",
        str(marketplace_dir),
        "--install_url",
        "https://github.com/dcc-mcp/maya-pipeline-tools.git",
        "--install_ref",
        _GIT_REF,
    )

    assert result["success"] is True
    assert (marketplace_dir / "marketplace.json").is_file()


def test_main_reports_a_commit_error_when_the_catalog_is_not_a_git_repo(tmp_path: Path, monkeypatch, capsys) -> None:
    ext_dir = _make_extension(tmp_path)
    catalog_path = tmp_path / "marketplace.json"

    result = _run_main(
        monkeypatch,
        capsys,
        "--extension_dir",
        str(ext_dir),
        "--marketplace_source",
        str(catalog_path),
        "--install_url",
        "https://github.com/dcc-mcp/maya-pipeline-tools.git",
        "--install_ref",
        _GIT_REF,
        "--commit",
    )

    assert result["success"] is True
    assert "not a git repository" in result["context"]["commit_error"]
    assert "git" not in result["context"]


# ── main(): failure paths ─────────────────────────────────────────────────────


def test_main_fails_when_the_extension_directory_is_missing(tmp_path: Path, monkeypatch, capsys) -> None:
    result = _run_main_expecting_failure(
        monkeypatch,
        capsys,
        "--extension_dir",
        str(tmp_path / "absent"),
        "--marketplace_source",
        str(tmp_path / "marketplace.json"),
        "--install_url",
        "https://github.com/dcc-mcp/example.git",
        "--install_ref",
        _GIT_REF,
    )

    assert result["success"] is False
    assert "Extension directory not found" in result["message"]


def test_main_fails_when_skill_md_is_unreadable(tmp_path: Path, monkeypatch, capsys) -> None:
    ext_dir = tmp_path / "extensions" / "broken"
    ext_dir.mkdir(parents=True)

    result = _run_main_expecting_failure(
        monkeypatch,
        capsys,
        "--extension_dir",
        str(ext_dir),
        "--marketplace_source",
        str(tmp_path / "marketplace.json"),
        "--install_url",
        "https://github.com/dcc-mcp/example.git",
        "--install_ref",
        _GIT_REF,
    )

    assert result["success"] is False
    assert "SKILL.md not found" in result["message"]


def test_main_fails_on_a_git_install_without_a_full_object_id(tmp_path: Path, monkeypatch, capsys) -> None:
    ext_dir = _make_extension(tmp_path)

    result = _run_main_expecting_failure(
        monkeypatch,
        capsys,
        "--extension_dir",
        str(ext_dir),
        "--marketplace_source",
        str(tmp_path / "marketplace.json"),
        "--install_url",
        "https://github.com/dcc-mcp/example.git",
        "--install_ref",
        "main",
    )

    assert result["success"] is False
    assert "Unexpected error" in result["message"]
    assert "40-character commit" in result["message"]


def test_main_fails_when_the_marketplace_source_is_a_github_slug(tmp_path: Path, monkeypatch, capsys) -> None:
    ext_dir = _make_extension(tmp_path)

    result = _run_main_expecting_failure(
        monkeypatch,
        capsys,
        "--extension_dir",
        str(ext_dir),
        "--marketplace_source",
        "dcc-mcp/marketplace",
        "--install_url",
        "https://github.com/dcc-mcp/example.git",
        "--install_ref",
        _GIT_REF,
    )

    assert result["success"] is False
    assert "looks like a GitHub slug" in result["message"]


# ── _git_commit_and_push ──────────────────────────────────────────────────────


class _Completed:
    """Stand-in for ``subprocess.CompletedProcess``."""

    def __init__(self, returncode: int = 0, stdout: str = "", stderr: str = "", args=None) -> None:
        self.returncode = returncode
        self.stdout = stdout
        self.stderr = stderr
        self.args = args if args is not None else ["git"]


class _GitRecorder:
    """Records git invocations and replays scripted results."""

    def __init__(self, *results: _Completed) -> None:
        self.calls: list[list[str]] = []
        self._results = list(results)

    def __call__(self, cmd, **kwargs):
        self.calls.append(list(cmd))
        if self._results:
            return self._results.pop(0)
        return _Completed()


@pytest.mark.parametrize(
    ("was_updated", "expected_message"),
    [
        pytest.param(False, "Add marketplace entry: maya-pipeline-tools", id="add"),
        pytest.param(True, "Update marketplace entry: maya-pipeline-tools", id="update"),
    ],
)
def test_git_commit_and_push_uses_the_expected_commands_and_message(
    tmp_path: Path, monkeypatch, was_updated: bool, expected_message: str
) -> None:
    recorder = _GitRecorder()
    monkeypatch.setattr(_PUBLISH.subprocess, "run", recorder)

    result = _PUBLISH._git_commit_and_push(tmp_path, "marketplace.json", "maya-pipeline-tools", was_updated)

    assert result == {
        "committed": True,
        "message": expected_message,
        "push_success": True,
        "stdout": "",
    }
    assert recorder.calls == [
        ["git", "-C", str(tmp_path), "add", "marketplace.json"],
        ["git", "-C", str(tmp_path), "commit", "-m", expected_message],
        ["git", "-C", str(tmp_path), "push", "origin"],
    ]


def test_git_commit_and_push_reports_nothing_to_commit_without_pushing(tmp_path: Path, monkeypatch) -> None:
    recorder = _GitRecorder(_Completed(), _Completed(1, "On branch main\nnothing to commit, working tree clean\n", ""))
    monkeypatch.setattr(_PUBLISH.subprocess, "run", recorder)

    result = _PUBLISH._git_commit_and_push(tmp_path, "marketplace.json", "maya-pipeline-tools", False)

    assert result == {"committed": False, "reason": "no changes to commit"}
    assert [call[3] for call in recorder.calls] == ["add", "commit"]


def test_git_commit_and_push_reports_a_commit_failure(tmp_path: Path, monkeypatch) -> None:
    recorder = _GitRecorder(_Completed(), _Completed(1, "", "fatal: unable to commit\n"))
    monkeypatch.setattr(_PUBLISH.subprocess, "run", recorder)

    result = _PUBLISH._git_commit_and_push(tmp_path, "marketplace.json", "maya-pipeline-tools", False)

    assert result["committed"] is False
    assert "git command failed" in result["error"]
    assert result["stderr"] == "fatal: unable to commit"


def test_git_commit_and_push_reports_a_push_failure(tmp_path: Path, monkeypatch) -> None:
    recorder = _GitRecorder(_Completed(), _Completed(), _Completed(1, "", "fatal: could not read Username\n"))
    monkeypatch.setattr(_PUBLISH.subprocess, "run", recorder)

    result = _PUBLISH._git_commit_and_push(tmp_path, "marketplace.json", "maya-pipeline-tools", True)

    assert result["committed"] is False
    assert "git command failed" in result["error"]
    assert result["stderr"] == "fatal: could not read Username"


@pytest.mark.skipif(shutil.which("git") is None, reason="git is not available")
def test_git_commit_and_push_against_a_real_repo_with_no_changes(tmp_path: Path, monkeypatch) -> None:
    """A clean tree must be reported as 'nothing to commit' instead of raising."""
    # The setup below creates a real commit, so it must not inherit the ambient
    # git configuration: a global/system `commit.gpgsign`, `core.hooksPath`, or
    # `init.templateDir` would fail this test for reasons unrelated to the code
    # under test. System and global config are detached, and identity, signing,
    # and line endings are passed per invocation.
    monkeypatch.setenv("LC_ALL", "C")
    monkeypatch.setenv("LANG", "C")
    monkeypatch.setenv("GIT_CONFIG_NOSYSTEM", "1")
    monkeypatch.setenv("GIT_CONFIG_GLOBAL", os.devnull)

    def git(*args: str) -> None:
        subprocess.run(
            [
                "git",
                "-C",
                str(tmp_path),
                "-c",
                "user.name=loonghao",
                "-c",
                "user.email=hal.long@outlook.com",
                "-c",
                "commit.gpgsign=false",
                "-c",
                "core.autocrlf=false",
                *args,
            ],
            check=True,
            capture_output=True,
            timeout=30,
        )

    git("init", "--quiet")
    catalog = tmp_path / "marketplace.json"
    catalog.write_text(json.dumps({"version": "1", "entries": []}), encoding="utf-8")
    git("add", "marketplace.json")
    git("commit", "--quiet", "--no-verify", "-m", "init catalog")

    result = _PUBLISH._git_commit_and_push(tmp_path, "marketplace.json", "maya-pipeline-tools", False)

    assert result == {"committed": False, "reason": "no changes to commit"}
