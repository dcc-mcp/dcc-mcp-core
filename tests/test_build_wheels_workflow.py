"""Build-wheel workflow release publication tests."""

from __future__ import annotations

from conftest import REPO_ROOT
from dcc_mcp_core import yaml_loads

BUILD_WHEELS_WORKFLOW = REPO_ROOT / ".github" / "workflows" / "build-wheels.yml"
BUILD_JOB_IDS = {
    "linux",
    "windows",
    "py37-lite",
    "linux-py37",
    "windows-py37",
    "macos",
}


def _jobs() -> dict:
    workflow = yaml_loads(BUILD_WHEELS_WORKFLOW.read_text(encoding="utf-8"))
    return workflow["jobs"]


def test_release_wheels_are_uploaded_once_after_every_build() -> None:
    jobs = _jobs()

    for job_id in BUILD_JOB_IDS:
        steps = jobs[job_id]["steps"]
        assert all(step.get("uses") != "softprops/action-gh-release@v3" for step in steps)

    publish = jobs["publish-release"]
    assert set(publish["needs"]) == BUILD_JOB_IDS
    assert publish["if"] == "inputs.release-tag-name != ''"
    assert publish["permissions"] == {"actions": "read", "contents": "write"}

    download = publish["steps"][0]
    assert download["uses"] == "actions/download-artifact@v8"
    assert download["with"] == {
        "pattern": "wheels-*",
        "path": "dist",
        "merge-multiple": True,
    }

    upload = publish["steps"][1]
    assert upload["env"] == {"GH_TOKEN": "${{ github.token }}"}
    command = upload["run"]
    assert 'gh release upload "${{ inputs.release-tag-name }}" dist/*.whl' in command
    assert '--repo "${{ github.repository }}"' in command
    assert command.endswith("--clobber")
