"""Build-wheel workflow release publication tests."""

from __future__ import annotations

from fnmatch import fnmatch

from conftest import REPO_ROOT
from dcc_mcp_core import yaml_loads

BUILD_WHEELS_WORKFLOW = REPO_ROOT / ".github" / "workflows" / "build-wheels.yml"
BUILD_JOB_IDS = {
    "linux",
    "windows",
    "py37-lite",
    "linux-py37",
    "windows-py37",
    "macos-py37",
    "macos",
}
# The macOS cp37 wheel stays a build-verification spike rather than a release
# artefact, so it trails both `publish-release.needs` and the published artifact
# list. It is withheld by scope, not because it is broken: the lane builds it,
# passes the native_py37 contract, and pip-installs it into python.org 3.7.9 on
# macOS 10.15+ (pip compares the wheel tag against the running OS version, not
# the interpreter's MACOSX_DEPLOYMENT_TARGET). Promoting it to a release
# artefact is a product decision, not a bug fix.
PUBLISH_JOB_IDS = BUILD_JOB_IDS - {"macos-py37"}
PUBLISH_ARTIFACT_NAMES = {
    "wheels-linux-x86_64",
    "wheels-windows-x64",
    "wheels-py37-lite",
    "wheels-linux-x86_64-py37",
    "wheels-windows-x64-py37",
    "wheels-macos-universal2",
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
    assert set(publish["needs"]) == PUBLISH_JOB_IDS
    assert publish["if"] == "inputs.release-tag-name != ''"
    assert publish["permissions"] == {"actions": "read", "contents": "write"}

    # The macOS cp37 wheel is a build-verification spike, not a release
    # artefact, so `publish-release` downloads each published wheel by name
    # instead of globbing `wheels-*`. The spike is uploaded as
    # `spike-wheels-macos-x86_64-py37`, deliberately outside the `wheels-*`
    # namespace, so publication globs cannot sweep it into the distribution
    # set even if this allowlist is ever relaxed.
    downloads = [step for step in publish["steps"] if step.get("uses") == "actions/download-artifact@v8"]
    assert downloads
    downloaded_names = set()
    for step in downloads:
        with_ = step["with"]
        assert "pattern" not in with_
        assert with_["path"] == "dist"
        assert with_["merge-multiple"] is True
        downloaded_names.add(with_["name"])
    assert downloaded_names == PUBLISH_ARTIFACT_NAMES

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
    # The default GITHUB_TOKEN is refused on the release-update call that
    # precedes every asset upload, so release.yml passes the
    # release PAT through the reusable-workflow secret.
    assert upload["with"] == {
        "token": "${{ secrets.RELEASE_TOKEN || github.token }}",
        "tag_name": "${{ inputs.release-tag-name }}",
        "files": "dist/*",
        # Replacing a same-named asset is opt-in: the caller decides, because
        # only the asset backfill may overwrite an existing release.
        "overwrite_files": "${{ inputs.overwrite-release-assets }}",
        "fail_on_unmatched_files": True,
    }


def test_verification_spike_producer_stays_outside_publication_globs() -> None:
    jobs = _jobs()
    build = next(step for step in jobs["macos-py37"]["steps"] if step.get("uses") == "./.github/actions/build-wheel")
    artifact_name = build["with"]["artifact-name"]
    assert artifact_name not in PUBLISH_ARTIFACT_NAMES

    release = yaml_loads((REPO_ROOT / ".github" / "workflows" / "release.yml").read_text(encoding="utf-8"))
    for job_id in ("publish-core-pypi", "publish-github-release-assets"):
        download = next(
            step
            for step in release["jobs"][job_id]["steps"]
            if step.get("name") == "Download dcc-mcp-core wheel artefacts"
        )
        assert not fnmatch(artifact_name, download["with"]["pattern"]), job_id


def test_callable_workflow_declares_the_release_token_secret() -> None:
    """Reusable workflows do not inherit secrets, so it must be declared."""
    workflow = yaml_loads(BUILD_WHEELS_WORKFLOW.read_text(encoding="utf-8"))
    secrets = workflow["on"]["workflow_call"]["secrets"]
    assert secrets["RELEASE_TOKEN"]["required"] is False


def test_asset_overwrite_is_opt_in_on_every_trigger() -> None:
    """Both triggers default the overwrite input to false.

    `workflow_dispatch` matters as much as `workflow_call`: a manual run of the
    reusable workflow must not be able to clobber a published release by
    omission.
    """
    workflow = yaml_loads(BUILD_WHEELS_WORKFLOW.read_text(encoding="utf-8"))
    for trigger in ("workflow_dispatch", "workflow_call"):
        overwrite = workflow["on"][trigger]["inputs"]["overwrite-release-assets"]
        assert overwrite["type"] == "boolean"
        assert overwrite["default"] is False
        assert overwrite["required"] is False


def test_python37_runtime_smokes_share_version_and_ref_bound_dependency_preparation() -> None:
    smoke_steps = {
        "py37-lite": "Test py37-lite wheel",
        "linux-py37": "Test native Python 3.7 wheel with workflow smoke",
        "windows-py37": "Test native Python 3.7 wheel with workflow smoke",
        "macos-py37": "Test native Python 3.7 wheel with workflow smoke",
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


def test_abi3_backfill_uses_workflow_smoke_without_changing_tag_source() -> None:
    for job_id in ("linux", "windows", "macos"):
        steps = _jobs()[job_id]["steps"]
        assert steps[0]["with"]["ref"] == "${{ inputs.checkout-ref || github.ref }}"
        build_index = next(i for i, step in enumerate(steps) if step.get("uses") == "./.github/actions/build-wheel")
        assert steps[build_index]["with"]["test-wheel"] == "false"
        tooling_index = next(
            i for i, step in enumerate(steps) if step.get("name") == "Checkout workflow compatibility tooling"
        )
        tooling = steps[tooling_index]["with"]
        assert tooling["ref"] == "${{ github.workflow_sha }}"
        assert "tests/test_imports.py" in tooling["sparse-checkout"]
        smoke_index = next(
            i for i, step in enumerate(steps) if step.get("name") == "Test wheel with workflow import smoke"
        )
        assert build_index < tooling_index < smoke_index
        smoke = steps[smoke_index]["run"]
        assert "python -m venv test-env" in smoke
        assert "python -m pip install --force-reinstall dist/*.whl" in smoke
        assert "python -m pip check" in smoke
        assert "python .workflow-tools/tests/test_imports.py" in smoke
        assert "test-env/Scripts/activate" in smoke
        assert "test-env/bin/activate" in smoke
