"""Release workflow structure tests."""

from __future__ import annotations

from conftest import REPO_ROOT
from dcc_mcp_core import yaml_loads

RELEASE_WORKFLOW = REPO_ROOT / ".github" / "workflows" / "release.yml"
PYPI_ACTION = "pypa/gh-action-pypi-publish"
GITHUB_RELEASE_ACTION = "softprops/action-gh-release@v3"
CORE_BACKFILL_EXPRESSION = "github.event_name == 'workflow_dispatch' && inputs.release_tag != ''"
ASSETS_BACKFILL_GUARD = " && inputs.backfill_assets != true"
GITHUB_RELEASE_TOKEN = "${{ secrets.PERSONAL_ACCESS_TOKEN || github.token }}"
REUSE_RELEASE_ASSETS_EXPRESSION = (
    "${{ github.event_name == 'workflow_dispatch' && inputs.release_tag != ''" + ASSETS_BACKFILL_GUARD + " }}"
)
OVERWRITE_FILES_EXPRESSION = "${{ github.event_name == 'workflow_dispatch' && inputs.backfill_assets == true }}"


def _release_jobs() -> dict:
    workflow = yaml_loads(RELEASE_WORKFLOW.read_text(encoding="utf-8"))
    return workflow["jobs"]


def _uses_action(step: dict, action: str) -> bool:
    uses = str(step.get("uses") or "")
    return uses.split("#", 1)[0].strip().startswith(f"{action}@")


def _pypi_steps(job: dict) -> list[dict]:
    return [step for step in job.get("steps", []) if _uses_action(step, PYPI_ACTION)]


def _github_release_steps(jobs: dict) -> list[dict]:
    return [step for job in jobs.values() for step in job.get("steps", []) if step.get("uses") == GITHUB_RELEASE_ACTION]


def test_release_workflow_preserves_existing_github_release_assets() -> None:
    steps = _github_release_steps(_release_jobs())
    assert len(steps) == 4
    for step in steps:
        assert step["with"]["overwrite_files"] == OVERWRITE_FILES_EXPRESSION
        assert step["with"]["fail_on_unmatched_files"] is True


def test_release_workflow_uploads_assets_with_the_release_token() -> None:
    steps = _github_release_steps(_release_jobs())
    assert len(steps) == 4
    for step in steps:
        assert step["with"]["token"] == GITHUB_RELEASE_TOKEN


def test_release_workflow_manual_backfill_reuses_core_release_assets() -> None:
    build_wheels = _release_jobs()["build-wheels"]
    assert build_wheels["with"]["reuse-release-assets"] == REUSE_RELEASE_ASSETS_EXPRESSION
    assert build_wheels["with"]["overwrite-release-assets"] == OVERWRITE_FILES_EXPRESSION
    assert build_wheels["secrets"] == {"RELEASE_TOKEN": "${{ secrets.PERSONAL_ACCESS_TOKEN }}"}


def test_release_workflow_publishes_all_distributions_in_separate_jobs() -> None:
    jobs = _release_jobs()
    expected = {
        "publish-core-pypi": ("build-wheels", "wheels-*", "dist"),
        "publish-server-pypi": ("build-binaries", "server-wheel-*", "dist-server"),
        "publish-semantic-pypi": ("build-semantic-wheels", "semantic-wheel-*", "dist-semantic"),
        "publish-cli-pypi": ("build-cli-wheels", "cli-wheel-all", "dist-cli"),
    }
    for job_id, (build_job, artifact_pattern, artifact_path) in expected.items():
        job = jobs[job_id]
        assert build_job in job["needs"]
        assert "validate-release-version" in job["needs"]
        assert "validate-release-notices" in job["needs"]
        assert job["environment"]["name"] == "pypi"
        download = job["steps"][0]
        assert download["uses"] == "actions/download-artifact@v8"
        assert download["with"]["pattern"] == artifact_pattern
        assert download["with"]["path"] == artifact_path
        publish_steps = _pypi_steps(job)
        assert len(publish_steps) == 1
        assert publish_steps[0]["with"]["packages-dir"] == artifact_path
        assert publish_steps[0]["with"]["skip-existing"] is True


def test_release_workflow_keeps_the_github_release_safety_net() -> None:
    jobs = _release_jobs()
    safety = jobs["publish-github-release-assets"]
    assert "build-cli-wheels" in safety["needs"]
    assert "validate-release-notices" in safety["needs"]
    assert "always()" in safety["if"]
    downloads = [step for step in safety["steps"] if step.get("uses") == "actions/download-artifact@v8"]
    download_patterns = {step["with"]["pattern"]: step["with"]["path"] for step in downloads}
    assert download_patterns["server-binary-*"] == "dist-binaries"

    summary = jobs["publish"]
    assert "publish-cli-pypi" in summary["needs"]
    assert "publish-github-release-assets" in summary["needs"]
    assert "always()" in summary["if"]


def test_release_workflow_builds_server_and_cli_archives_for_all_platforms() -> None:
    jobs = _release_jobs()
    build = jobs["build-binaries"]
    assert [entry["platform"] for entry in build["strategy"]["matrix"]["include"]] == [
        "linux-x86_64",
        "windows-x86_64",
        "macos-universal2",
    ]
    server_bundle = next(step for step in build["steps"] if step.get("id") == "server-bundle")
    server_run = server_bundle["run"]
    assert "scripts/release/build_server_bundle.py" in server_run
    assert "$RELEASE_VERSION" in server_run
    assert "$MATRIX_PLATFORM" in server_run
    cli_bundle = next(step for step in build["steps"] if step.get("id") == "cli-bundle")
    assert "scripts/release/build_standalone_bundle.py" in cli_bundle["run"]
    assert "--binary-name dcc-mcp-cli" in cli_bundle["run"]
    assert '"$MATRIX_CLI_ARTIFACT_NAME"' in cli_bundle["run"]
