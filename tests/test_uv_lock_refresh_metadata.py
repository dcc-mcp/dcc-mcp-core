"""Execute the lock refresh PR metadata boundary without GitHub access."""

from __future__ import annotations

import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import textwrap

import pytest

WORKFLOW_PATH = Path(__file__).resolve().parents[1] / ".github/workflows/uv-lock-refresh.yml"


def _metadata_script() -> str:
    workflow = WORKFLOW_PATH.read_text(encoding="utf-8")
    start = workflow.index('          existing="$(gh pr list')
    end = workflow.index("          # The pull request stops here", start)
    return "set -euo pipefail\n" + textwrap.dedent(workflow[start:end])


@pytest.fixture
def replay_metadata(tmp_path):
    if os.name == "nt" or not shutil.which("bash"):
        pytest.skip("The refresh workflow runs in Bash on Ubuntu")
    executable = tmp_path / "gh"
    executable.write_text(
        f"#!{sys.executable}\n"
        + textwrap.dedent(
            """\
            import json
            import os
            from pathlib import Path
            import sys

            args = sys.argv[1:]
            with Path(os.environ["GH_CALLS"]).open("a", encoding="utf-8") as log:
                log.write(json.dumps(args) + "\\n")
            if args[:2] == ["pr", "list"]:
                print(os.environ["EXISTING_PR"])
                sys.exit(int(os.environ["LOOKUP_STATUS"]))
            if args[:2] == ["api", "-X"]:
                sys.exit(int(os.environ["PATCH_STATUS"]))
            if args[:2] == ["pr", "create"]:
                sys.exit(0)
            sys.exit("unexpected gh call (GraphQL edit is forbidden)")
            """
        ),
        encoding="utf-8",
    )
    executable.chmod(0o755)

    def replay(
        *,
        existing="2765",
        title="chore(lock): refresh pins",
        body="@literal\ntrue\nnull",
        patch_status=0,
        lookup_status=0,
    ):
        calls_path = tmp_path / "calls.jsonl"
        body_file = tmp_path / "body with spaces.md"
        body_file.write_text(body, encoding="utf-8")
        env = {
            "PATH": str(tmp_path) + os.pathsep + os.environ["PATH"],
            "GH_CALLS": str(calls_path),
            "EXISTING_PR": existing,
            "PATCH_STATUS": str(patch_status),
            "LOOKUP_STATUS": str(lookup_status),
            "GITHUB_REPOSITORY": "dcc-mcp/dcc-mcp-core",
            "REFRESH_BRANCH": "bot/uv-lock-refresh",
            "refresh_title": title,
            "body_file": str(body_file),
        }
        result = subprocess.run(
            ["bash", "-c", _metadata_script()],
            cwd=tmp_path,
            env=env,
            capture_output=True,
            text=True,
            timeout=10,
        )
        calls = [json.loads(line) for line in calls_path.read_text(encoding="utf-8").splitlines()]
        return result, calls

    return replay


@pytest.mark.parametrize("title,body", [("@literal title", "@literal body\nquotes: '\" `$()"), ("true", "null")])
def test_existing_pr_is_patched_with_raw_fields(replay_metadata, title, body):
    result, calls = replay_metadata(title=title, body=body)
    assert result.returncode == 0, result.stderr
    assert len(calls) == 2
    assert calls[0][:2] == ["pr", "list"]
    assert calls[1] == [
        "api",
        "-X",
        "PATCH",
        "repos/dcc-mcp/dcc-mcp-core/pulls/2765",
        "-f",
        "title=" + title,
        "-f",
        "body=" + body,
    ]


def test_no_existing_pr_keeps_the_create_path(replay_metadata):
    result, calls = replay_metadata(existing="")
    assert result.returncode == 0, result.stderr
    assert len(calls) == 2
    assert calls[1][:8] == [
        "pr",
        "create",
        "--base",
        "main",
        "--head",
        "bot/uv-lock-refresh",
        "--title",
        "chore(lock): refresh pins",
    ]
    assert calls[1][-2] == "--body-file"


def test_patch_failure_stops_without_creating_a_duplicate(replay_metadata):
    result, calls = replay_metadata(patch_status=17)
    assert result.returncode == 17
    assert len(calls) == 2
    assert calls[-1][:3] == ["api", "-X", "PATCH"]


def test_lookup_failure_stops_without_creating_a_duplicate(replay_metadata):
    result, calls = replay_metadata(existing="", lookup_status=19)
    assert result.returncode == 19
    assert len(calls) == 1


def test_failure_notice_does_not_assume_the_guard_failed():
    workflow = WORKFLOW_PATH.read_text(encoding="utf-8")
    notice = workflow.split("      - name: Report a failed refresh", 1)[1]
    assert "The failing operation was stopped by the guard" not in notice
    assert "PR metadata" in notice
    assert "skipped" in notice
