"""Release workflow structure tests."""

from __future__ import annotations

from conftest import REPO_ROOT
from dcc_mcp_core import yaml_loads

RELEASE_WORKFLOW = REPO_ROOT / ".github" / "workflows" / "release.yml"
PYPI_ACTION = "pypa/gh-action-pypi-publish@release/v1"


def _release_jobs() -> dict:
    workflow = yaml_loads(RELEASE_WORKFLOW.read_text(encoding="utf-8"))
    return workflow["jobs"]


def _pypi_steps(job: dict) -> list[dict]:
    return [step for step in job.get("steps", []) if step.get("uses") == PYPI_ACTION]


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
    assert safety["permissions"] == {"actions": "read", "contents": "write"}

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
