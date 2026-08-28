"""Release workflow structure tests."""

from __future__ import annotations

from conftest import REPO_ROOT
from dcc_mcp_core import yaml_loads

RELEASE_WORKFLOW = REPO_ROOT / ".github" / "workflows" / "release.yml"
PYPI_ACTION = "pypa/gh-action-pypi-publish@release/v1"
GITHUB_RELEASE_ACTION = "softprops/action-gh-release@v3"
CORE_BACKFILL_EXPRESSION = "github.event_name == 'workflow_dispatch' && inputs.release_tag != ''"


def _release_jobs() -> dict:
    workflow = yaml_loads(RELEASE_WORKFLOW.read_text(encoding="utf-8"))
    return workflow["jobs"]


def _pypi_steps(job: dict) -> list[dict]:
    return [step for step in job.get("steps", []) if step.get("uses") == PYPI_ACTION]


def _github_release_steps(jobs: dict) -> list[dict]:
    return [step for job in jobs.values() for step in job.get("steps", []) if step.get("uses") == GITHUB_RELEASE_ACTION]


def test_release_workflow_preserves_existing_github_release_assets() -> None:
    steps = _github_release_steps(_release_jobs())
    assert len(steps) == 3
    for step in steps:
        assert step["with"]["overwrite_files"] is False
        assert step["with"]["fail_on_unmatched_files"] is True


def test_release_workflow_manual_backfill_reuses_core_release_assets() -> None:
    build_wheels = _release_jobs()["build-wheels"]
    assert build_wheels["with"]["reuse-release-assets"] == (
        "${{ github.event_name == 'workflow_dispatch' && inputs.release_tag != '' }}"
    )


def test_manual_backfill_is_explicitly_core_only() -> None:
    jobs = _release_jobs()
    for job_id in ("build-admin-ui", "build-binaries", "build-semantic-wheels"):
        condition = jobs[job_id]["if"]
        assert f"!({CORE_BACKFILL_EXPRESSION})" in condition

    summary = jobs["publish"]["steps"][0]["run"]
    assert f'core_backfill="${{{{ {CORE_BACKFILL_EXPRESSION} }}}}"' in summary
    assert 'server" != "skipped"' in summary
    assert 'semantic" != "skipped"' in summary
    assert 'release_assets" != "skipped"' in summary


def test_release_workflow_publishes_each_pypi_project_in_its_own_job() -> None:
    jobs = _release_jobs()
    expected = {
        "publish-core-pypi": {
            "needs": ["release-please", "validate-release-version", "build-wheels"],
            "url": "https://pypi.org/p/dcc-mcp-core",
            "artifact_pattern": "wheels-*",
            "artifact_path": "dist",
            "packages_dir": "dist",
        },
        "publish-server-pypi": {
            "needs": ["release-please", "validate-release-version", "build-binaries"],
            "url": "https://pypi.org/p/dcc-mcp-server",
            "artifact_pattern": "server-wheel-*",
            "artifact_path": "dist-server",
            "packages_dir": "dist-server",
        },
        "publish-semantic-pypi": {
            "needs": ["release-please", "validate-release-version", "build-semantic-wheels"],
            "url": "https://pypi.org/p/dcc-mcp-core-semantic",
            "artifact_pattern": "semantic-wheel-*",
            "artifact_path": "dist-semantic",
            "packages_dir": "dist-semantic",
        },
    }

    for job_id, config in expected.items():
        job = jobs[job_id]
        assert job["runs-on"] == "ubuntu-latest"
        assert job["needs"] == config["needs"]
        assert job["environment"] == {"name": "pypi", "url": config["url"]}
        assert job["permissions"] == {
            "id-token": "write",
            "actions": "read",
            "contents": "read",
        }

        download = job["steps"][0]
        assert download["uses"] == "actions/download-artifact@v8"
        assert download["with"]["pattern"] == config["artifact_pattern"]
        assert download["with"]["path"] == config["artifact_path"]
        assert download["with"]["merge-multiple"] is True

        publish_steps = _pypi_steps(job)
        assert len(publish_steps) == 1
        publish = publish_steps[0]
        assert "continue-on-error" not in publish
        assert publish["with"] == {
            "packages-dir": config["packages_dir"],
            "verbose": True,
            "print-hash": True,
            "skip-existing": True,
        }

    assert sum(len(_pypi_steps(job)) for job in jobs.values()) == 3


def test_core_pypi_publish_validates_complete_distribution_set_before_upload() -> None:
    publish = _release_jobs()["publish-core-pypi"]
    steps = publish["steps"]
    validate_index = next(
        index for index, step in enumerate(steps) if "check_release_distribution_set.py" in step.get("run", "")
    )
    upload_index = next(index for index, step in enumerate(steps) if step.get("uses") == PYPI_ACTION)

    assert validate_index < upload_index
    assert "--dist-dir dist" in steps[validate_index]["run"]
    assert "--version" in steps[validate_index]["run"]


def test_release_workflow_keeps_github_release_safety_net_after_pypi_jobs() -> None:
    jobs = _release_jobs()
    safety = jobs["publish-github-release-assets"]
    assert safety["needs"] == [
        "release-please",
        "build-wheels",
        "build-binaries",
        "build-semantic-wheels",
        "publish-core-pypi",
        "publish-server-pypi",
        "publish-semantic-pypi",
    ]
    assert "always()" in safety["if"]
    assert safety["permissions"] == {
        "actions": "read",
        "contents": "write",
        "id-token": "write",
        "attestations": "write",
    }
    downloads = [step for step in safety["steps"] if step.get("uses") == "actions/download-artifact@v8"]
    download_patterns = {step["with"]["pattern"]: step["with"]["path"] for step in downloads}
    assert download_patterns["server-binary-*"] == "dist-binaries"
    attestation_steps = {step["id"]: step for step in safety["steps"] if step.get("uses") == "actions/attest@v4"}
    assert {step["with"]["subject-path"] for step in attestation_steps.values()} == {
        "dist-binaries/dcc-mcp-update-manifest-linux-x86_64.json",
        "dist-binaries/dcc-mcp-update-manifest-windows-x86_64.json",
        "dist-binaries/dcc-mcp-update-manifest-macos-universal2.json",
    }
    publish_bundles = next(
        step for step in safety["steps"] if step.get("name") == "Publish detached update-manifest bundles"
    )
    assert ".sigstore.json" in publish_bundles["run"]
    safety_upload = next(step for step in safety["steps"] if step.get("uses") == "softprops/action-gh-release@v3")
    assert "dist-binaries/*" in safety_upload["with"]["files"]

    summary = jobs["publish"]
    assert summary["needs"] == [
        "release-please",
        "publish-core-pypi",
        "publish-server-pypi",
        "publish-semantic-pypi",
        "publish-github-release-assets",
    ]
    assert "always()" in summary["if"]
    run = summary["steps"][0]["run"]
    assert "needs.publish-core-pypi.result" in run
    assert "needs.publish-server-pypi.result" in run
    assert "needs.publish-semantic-pypi.result" in run
    assert "needs.publish-github-release-assets.result" in run


def test_release_workflow_builds_deployable_zips_per_platform() -> None:
    jobs = _release_jobs()
    build = jobs["build-binaries"]
    includes = build["strategy"]["matrix"]["include"]
    assert [entry["platform"] for entry in includes] == [
        "linux-x86_64",
        "windows-x86_64",
        "macos-universal2",
    ]

    bundle = next(step for step in build["steps"] if step.get("id") == "server-bundle")
    run = bundle["run"]
    assert "scripts/release/build_server_bundle.py" in run
    assert '--version "${{ needs.release-please.outputs.version }}"' in run
    assert '--platform "${{ matrix.platform }}"' in run
    assert '--server-bin "${{ matrix.artifact_name }}"' in run
    assert '--cli-bin "${{ matrix.cli_artifact_name }}"' in run

    cli_bundle = next(step for step in build["steps"] if step.get("id") == "cli-bundle")
    cli_run = cli_bundle["run"]
    assert "scripts/release/build_standalone_bundle.py" in cli_run
    assert '--version "${{ needs.release-please.outputs.version }}"' in cli_run
    assert '--platform "${{ matrix.platform }}"' in cli_run
    assert "--binary-name dcc-mcp-cli" in cli_run
    assert '--binary-path "${{ matrix.cli_artifact_name }}"' in cli_run

    raw_upload = next(
        step
        for step in build["steps"]
        if step.get("uses") == "actions/upload-artifact@v4" and step["with"]["name"] == "server-binary-${{ matrix.os }}"
    )
    assert "${{ steps.server-bundle.outputs.bundle_path }}" in raw_upload["with"]["path"]
    assert "${{ steps.cli-bundle.outputs.bundle_path }}" in raw_upload["with"]["path"]

    release_upload = next(step for step in build["steps"] if step.get("uses") == "softprops/action-gh-release@v3")
    assert "${{ steps.server-bundle.outputs.bundle_path }}" in release_upload["with"]["files"]
    assert "${{ steps.cli-bundle.outputs.bundle_path }}" in release_upload["with"]["files"]

    notify = next(step for step in jobs["publish"]["steps"] if step["name"] == "Notify Multica release-ready autopilot")
    assert r"^dcc-mcp-server-[0-9A-Za-z.+-]+-(linux-x86_64|windows-x86_64|macos-universal2)\.zip$" in notify["run"]
    assert r"^dcc-mcp-cli-[0-9A-Za-z.+-]+-(linux-x86_64|windows-x86_64|macos-universal2)\.zip$" in notify["run"]
