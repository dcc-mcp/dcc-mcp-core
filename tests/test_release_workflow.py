"""Release workflow structure tests."""

from __future__ import annotations

from types import SimpleNamespace

import pytest

from conftest import REPO_ROOT
from dcc_mcp_core import yaml_loads

RELEASE_WORKFLOW = REPO_ROOT / ".github" / "workflows" / "release.yml"
PYPI_ACTION = "pypa/gh-action-pypi-publish@release/v1"


def _release_jobs() -> dict:
    workflow = yaml_loads(RELEASE_WORKFLOW.read_text(encoding="utf-8"))
    return workflow["jobs"]


def _pypi_steps(job: dict) -> list[dict]:
    return [step for step in job.get("steps", []) if step.get("uses") == PYPI_ACTION]


def _ui_control_host_probe_contract() -> dict:
    build = _release_jobs()["build-binaries"]
    host_build = next(step for step in build["steps"] if step.get("name") == "Build and stage Windows UI Control host")
    run = host_build["run"]
    script = run.split("<<'PY'\n", 1)[1].rsplit("\nPY\n", 1)[0]
    namespace = {"__name__": "release_workflow_contract"}
    exec(compile(script, "release-ui-control-host-version-gate", "exec"), namespace)
    return namespace


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
    downloads = [step for step in safety["steps"] if step.get("uses") == "actions/download-artifact@v8"]
    download_patterns = {step["with"]["pattern"]: step["with"]["path"] for step in downloads}
    assert download_patterns["server-binary-*"] == "dist-binaries"
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


def test_release_workflow_builds_deployable_server_zip_per_platform() -> None:
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
    assert "--ui-control-host target/release/dcc-mcp-ui-control-host.exe" in run

    host_build = next(step for step in build["steps"] if step.get("name") == "Build and stage Windows UI Control host")
    assert host_build["if"] == "matrix.os == 'windows-latest'"
    assert host_build["env"] == {"RELEASE_VERSION": "${{ needs.release-please.outputs.version }}"}
    assert "vx just stage-ui-control-host" in host_build["run"]
    assert '[host_path, "--version"]' in host_build["run"]
    assert "requires_host_version_probe" in host_build["run"]
    assert 'python - "$RELEASE_VERSION"' in host_build["run"]
    assert "${{ needs.release-please.outputs.version }}" not in host_build["run"]
    assert host_build["run"].index("vx just stage-ui-control-host") < host_build["run"].index(
        'python - "$RELEASE_VERSION"'
    )
    assert host_build["run"].index('python - "$RELEASE_VERSION"') < host_build["run"].index('[host_path, "--version"]')
    assert host_build["run"].index('[host_path, "--version"]') < host_build["run"].index(
        "cp target/release/dcc-mcp-ui-control-host.exe"
    )

    host_inject = next(
        step for step in build["steps"] if step.get("name") == "Inject Windows UI Control host into server wheel"
    )
    assert host_inject["if"] == "matrix.os == 'windows-latest'"
    assert "scripts/release/inject_ui_control_host.py" in host_inject["run"]
    assert "target/release/dcc-mcp-ui-control-host.exe" in host_inject["run"]

    raw_upload = next(
        step
        for step in build["steps"]
        if step.get("uses") == "actions/upload-artifact@v4" and step["with"]["name"] == "server-binary-${{ matrix.os }}"
    )
    assert "${{ steps.server-bundle.outputs.bundle_path }}" in raw_upload["with"]["path"]

    release_upload = next(step for step in build["steps"] if step.get("uses") == "softprops/action-gh-release@v3")
    assert "${{ steps.server-bundle.outputs.bundle_path }}" in release_upload["with"]["files"]

    notify = next(step for step in jobs["publish"]["steps"] if step["name"] == "Notify Multica release-ready autopilot")
    assert r"^dcc-mcp-server-[0-9A-Za-z.+-]+-(linux-x86_64|windows-x86_64|macos-universal2)\.zip$" in notify["run"]


def test_release_workflow_skips_host_version_probe_for_01964_backfill() -> None:
    contract = _ui_control_host_probe_contract()

    def must_not_run(*_args, **_kwargs):
        pytest.fail("legacy Host must not be launched with --version")

    assert contract["verify_host_version"]("0.19.64", "legacy-host.exe", runner=must_not_run) is False


def test_release_workflow_requires_exact_host_version_from_01965() -> None:
    contract = _ui_control_host_probe_contract()
    calls = []

    def exact_runner(command, **kwargs):
        calls.append((command, kwargs))
        return SimpleNamespace(returncode=0, stdout=b"0.19.65\r\n")

    assert contract["verify_host_version"]("0.19.65", "new-host.exe", runner=exact_runner) is True
    assert calls[0][0] == ["new-host.exe", "--version"]
    assert calls[0][1]["timeout"] == 10.0

    def mismatched_runner(_command, **_kwargs):
        return SimpleNamespace(returncode=0, stdout=b"0.19.64\n")

    with pytest.raises(contract["VerificationError"], match=r"does not match release 0.19.65"):
        contract["verify_host_version"]("0.19.65", "new-host.exe", runner=mismatched_runner)


def test_release_workflow_compares_semver_without_lexical_ordering() -> None:
    contract = _ui_control_host_probe_contract()

    assert contract["requires_host_version_probe"]("0.19.100") is True
    assert contract["requires_host_version_probe"]("0.20.0") is True
    for invalid in (
        "0.19.65-alpha.1",
        "0.19.65+build.1",
        "0.19.65; echo injected",
        "00.19.65",
    ):
        with pytest.raises(contract["VerificationError"], match=r"stable X.Y.Z"):
            contract["requires_host_version_probe"](invalid)
