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
    reuse = jobs["reuse-release-assets"]
    assert reuse["if"] == "needs.classify-release-assets.outputs.mode == 'reuse'"
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


def test_manual_backfill_rebuilds_from_tag_when_release_has_no_core_assets() -> None:
    jobs = _jobs()
    classifier = jobs["classify-release-assets"]
    assert classifier["if"] == "inputs.reuse-release-assets == true"
    assert classifier["permissions"] == {"contents": "read"}
    assert classifier["outputs"] == {"mode": "${{ steps.classify.outputs.mode }}"}
    classifier_commands = "\n".join(step.get("run", "") for step in classifier["steps"])
    assert "gh release view" in classifier_commands
    assert "--classify-assets" in classifier_commands
    assert 'if [ "$CHECKOUT_REF" != "$RELEASE_TAG" ]' in classifier_commands
    release_read = next(
        step for step in classifier["steps"] if step.get("name") == "Read dcc-mcp-core GitHub Release assets"
    )
    assert release_read["env"]["CHECKOUT_REF"] == "${{ inputs.checkout-ref }}"
    classifier_checkout = next(step for step in classifier["steps"] if step.get("uses") == "actions/checkout@v6")
    assert classifier_checkout["with"]["ref"] == "${{ github.workflow_sha }}"

    build_condition = (
        "always() && (inputs.reuse-release-assets != true || needs.classify-release-assets.outputs.mode == 'rebuild')"
    )
    for job_id in BUILD_JOB_IDS:
        assert jobs[job_id]["needs"] == ["classify-release-assets"]
        assert jobs[job_id]["if"] == build_condition

    reuse = jobs["reuse-release-assets"]
    assert reuse["needs"] == ["classify-release-assets"]
    assert reuse["if"] == "needs.classify-release-assets.outputs.mode == 'reuse'"


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

    validate_index = next(
        index
        for index, step in enumerate(publish["steps"])
        if "check_release_distribution_set.py" in step.get("run", "")
    )
    upload_index = next(
        index for index, step in enumerate(publish["steps"]) if step.get("uses") == "softprops/action-gh-release@v3"
    )
    assert validate_index < upload_index
    validation = publish["steps"][validate_index]
    assert "--dist-dir dist" in validation["run"]
    assert "--version" in validation["run"]

    upload = publish["steps"][upload_index]
    assert upload["uses"] == "softprops/action-gh-release@v3"
    assert upload["with"] == {
        "tag_name": "${{ inputs.release-tag-name }}",
        "files": "dist/*",
        "overwrite_files": False,
        "fail_on_unmatched_files": True,
    }


def test_python37_runtime_smokes_share_version_and_ref_bound_dependency_preparation() -> None:
    smoke_steps = {
        "py37-lite": "Test py37-lite wheel",
        "linux-py37": "Test native Python 3.7 wheel with workflow smoke",
        "windows-py37": "Test native Python 3.7 wheel with workflow smoke",
    }

    for job_id, smoke_step_name in smoke_steps.items():
        steps = _jobs()[job_id]["steps"]
        smoke_index = next(index for index, step in enumerate(steps) if step.get("name") == smoke_step_name)
        smoke = steps[smoke_index]
        assert "python37_wheel_smoke.py" in smoke["run"]
        assert '--checkout-ref "$CHECKOUT_REF"' in smoke["run"]
        assert smoke["env"]["CHECKOUT_REF"] == "${{ inputs.checkout-ref || github.ref }}"
        assert "--profile " + ("lite_py37" if job_id == "py37-lite" else "native_py37") in smoke["run"]
        assert "if [[ -f" not in smoke["run"]
        assert all("test_issue_2388_zero_typing_extensions.py" not in step.get("run", "") for step in steps)
