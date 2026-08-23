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


def test_manual_backfill_reuses_verified_github_release_assets() -> None:
    workflow = yaml_loads(BUILD_WHEELS_WORKFLOW.read_text(encoding="utf-8"))
    reuse_input = workflow["on"]["workflow_call"]["inputs"]["reuse-release-assets"]
    assert reuse_input == {
        "description": "Reuse immutable dcc-mcp-core distributions from an existing GitHub Release",
        "required": False,
        "type": "boolean",
        "default": False,
    }

    jobs = workflow["jobs"]
    for job_id in BUILD_JOB_IDS:
        assert jobs[job_id]["if"] == "inputs.reuse-release-assets != true"

    reuse = jobs["reuse-release-assets"]
    assert reuse["if"] == "inputs.reuse-release-assets == true"
    assert reuse["permissions"] == {"contents": "read"}
    commands = "\n".join(step.get("run", "") for step in reuse["steps"])
    assert "gh release download" in commands
    assert "dcc_mcp_core-${RELEASE_VERSION}-*" in commands
    assert "dcc_mcp_core-${RELEASE_VERSION}.tar.gz" in commands
    assert "check_release_distribution_set.py" in commands
    checkout = next(step for step in reuse["steps"] if step.get("uses") == "actions/checkout@v6")
    assert checkout["with"]["ref"] == "${{ github.workflow_sha }}"
    assert "compatibility" in checkout["with"]["sparse-checkout"]
    assert "scripts/ci" in checkout["with"]["sparse-checkout"]
    upload = next(step for step in reuse["steps"] if step.get("uses") == "actions/upload-artifact@v7")
    assert upload["with"] == {
        "name": "wheels-release-backfill",
        "path": "dist/",
        "retention-days": 30,
    }


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
    assert upload["uses"] == "softprops/action-gh-release@v3"
    assert upload["with"] == {
        "tag_name": "${{ inputs.release-tag-name }}",
        "files": "dist/*",
        "overwrite_files": False,
        "fail_on_unmatched_files": True,
    }


def test_python37_runtime_smokes_provision_workflow_owned_typing_extensions() -> None:
    smoke_steps = {
        "py37-lite": "Test py37-lite wheel",
        "linux-py37": "Test native Python 3.7 wheel with workflow smoke",
        "windows-py37": "Test native Python 3.7 wheel with workflow smoke",
    }

    for job_id, smoke_step_name in smoke_steps.items():
        steps = _jobs()[job_id]["steps"]
        provision_index, provision = next(
            (index, step)
            for index, step in enumerate(steps)
            if step.get("name") == "Provision Python 3.7 smoke dependencies"
        )
        smoke_index = next(index for index, step in enumerate(steps) if step.get("name") == smoke_step_name)

        assert provision_index < smoke_index
        command = provision["run"]
        assert ".workflow-tools/compatibility/python.json" in command
        assert '["test_toolchain"]["typing_extensions"]' in command
        assert '"typing-extensions==${TYPING_EXTENSIONS_VERSION}"' in command
