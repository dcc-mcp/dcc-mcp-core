"""Release-please PR guard behavior tests."""

from __future__ import annotations

import importlib.util
import json
from pathlib import Path
from types import SimpleNamespace

import pytest

REPO_ROOT = Path(__file__).resolve().parents[1]
SCRIPT_PATH = REPO_ROOT / "scripts" / "ci" / "check_release_please_pr.py"


def _load_guard_module():
    spec = importlib.util.spec_from_file_location("check_release_please_pr", SCRIPT_PATH)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def test_current_release_pr_head_uses_exact_open_branch_filter(monkeypatch: pytest.MonkeyPatch) -> None:
    guard = _load_guard_module()
    calls: list[list[str]] = []

    def fake_run(command: list[str], **_kwargs):
        calls.append(command)
        return SimpleNamespace(
            returncode=0,
            stdout=json.dumps(
                [
                    {
                        "head": {
                            "ref": "release-please--branches--main--components--dcc-mcp-core",
                            "sha": "current-head",
                        }
                    }
                ]
            ),
        )

    monkeypatch.setattr(guard.subprocess, "run", fake_run)

    assert (
        guard.current_release_pr_head(
            "dcc-mcp/dcc-mcp-core",
            "release-please--branches--main--components--dcc-mcp-core",
        )
        == "current-head"
    )
    assert calls == [
        [
            "gh",
            "api",
            "repos/dcc-mcp/dcc-mcp-core/pulls",
            "--method",
            "GET",
            "-f",
            "state=open",
            "-f",
            "head=dcc-mcp:release-please--branches--main--components--dcc-mcp-core",
            "-f",
            "base=main",
            "-f",
            "per_page=100",
        ]
    ]


@pytest.mark.parametrize("current_sha", ["new-head", None])
def test_workflow_run_skips_superseded_or_closed_release_pr_heads(
    monkeypatch: pytest.MonkeyPatch,
    current_sha: str | None,
) -> None:
    guard = _load_guard_module()
    monkeypatch.setenv("GITHUB_EVENT_NAME", "workflow_run")
    monkeypatch.setenv("GITHUB_REPOSITORY", "dcc-mcp/dcc-mcp-core")
    monkeypatch.setenv("PR_HEAD_REF", "release-please--branches--main--components--dcc-mcp-core")
    monkeypatch.setenv("PR_HEAD_SHA", "old-head")
    monkeypatch.setattr(guard, "current_release_pr_head", lambda _repo, _head_ref: current_sha)

    def fail_if_checked(_repo: str, _sha: str) -> None:
        pytest.fail("superseded or closed release PR heads must not be checked")

    monkeypatch.setattr(guard, "check_ci_status", fail_if_checked)

    guard.main()


def test_workflow_run_checks_the_current_release_pr_head(monkeypatch: pytest.MonkeyPatch) -> None:
    guard = _load_guard_module()
    monkeypatch.setenv("GITHUB_EVENT_NAME", "workflow_run")
    monkeypatch.setenv("GITHUB_REPOSITORY", "dcc-mcp/dcc-mcp-core")
    monkeypatch.setenv("PR_HEAD_REF", "release-please--branches--main--components--dcc-mcp-core")
    monkeypatch.setenv("PR_HEAD_SHA", "current-head")
    monkeypatch.setattr(guard, "current_release_pr_head", lambda _repo, _head_ref: "current-head")
    checked: list[tuple[str, str]] = []
    monkeypatch.setattr(guard, "check_ci_status", lambda repo, sha: checked.append((repo, sha)))

    guard.main()

    assert checked == [("dcc-mcp/dcc-mcp-core", "current-head")]
