"""Release workflow structure tests."""

from __future__ import annotations

from conftest import REPO_ROOT
from dcc_mcp_core import yaml_loads

RELEASE_WORKFLOW = REPO_ROOT / ".github" / "workflows" / "release.yml"
PYPI_ACTION = "pypa/gh-action-pypi-publish"
GITHUB_RELEASE_ACTION = "softprops/action-gh-release@v3"
CORE_BACKFILL_EXPRESSION = "github.event_name == 'workflow_dispatch' && inputs.release_tag != ''"
ASSETS_BACKFILL_GUARD = " && inputs.backfill_assets != true"
# The default GITHUB_TOKEN is refused on the release-update call that precedes
# every asset upload, so the release PAT is handed to the action instead.
GITHUB_RELEASE_TOKEN = "${{ secrets.PERSONAL_ACCESS_TOKEN || github.token }}"
REUSE_RELEASE_ASSETS_EXPRESSION = (
    "${{ github.event_name == 'workflow_dispatch' && inputs.release_tag != ''" + ASSETS_BACKFILL_GUARD + " }}"
)
# Assets are only ever replaced for the explicit asset backfill. A normal
# release must overwrite nothing, or the safety-net upload in
# publish-github-release-assets would delete and re-upload every asset the
# per-platform jobs just attached.
OVERWRITE_FILES_EXPRESSION = "${{ github.event_name == 'workflow_dispatch' && inputs.backfill_assets == true }}"


def _release_jobs() -> dict:
    workflow = yaml_loads(RELEASE_WORKFLOW.read_text(encoding="utf-8"))
    return workflow["jobs"]


def _uses_action(step: dict, action: str) -> bool:
    """Report whether ``step`` invokes ``action``, whatever ref pins it.

    The publish action moved from the mutable ``release/v1`` tag to an immutable
    commit SHA with a ``# v1.14.2`` trailing comment, so the ref is not a stable
    identifier to compare against.
    """

    uses = str(step.get("uses") or "")
    return uses.split("#", 1)[0].strip().startswith(f"{action}@")


def _pypi_steps(job: dict) -> list[dict]:
    return [step for step in job.get("steps", []) if _uses_action(step, PYPI_ACTION)]


def _github_release_steps(jobs: dict) -> list[dict]:
    return [step for job in jobs.values() for step in job.get("steps", []) if step.get("uses") == GITHUB_RELEASE_ACTION]


def test_release_workflow_preserves_existing_github_release_assets() -> None:
    """Assets are preserved by default and replaced only for an asset backfill.

    `softprops/action-gh-release@v3` silently skips any same-named asset when
    `overwrite_files` is false and still exits 0, so an unconditional false
    turns the opt-in backfill into a no-op that reports success while the
    release keeps its old bytes. An unconditional true is just as wrong: the
    safety-net upload would delete and re-upload every asset the per-platform
    jobs just attached.
    """
    # One upload step per asset-producing job: build-binaries,
    # build-semantic-wheels and build-cli-wheels attach their own artefacts,
    # and publish-github-release-assets is the safety net.
    steps = _github_release_steps(_release_jobs())
    assert len(steps) == 4
    for step in steps:
        assert step["with"]["overwrite_files"] == OVERWRITE_FILES_EXPRESSION
        assert step["with"]["fail_on_unmatched_files"] is True


def test_release_workflow_uploads_assets_with_the_release_token() -> None:
    """Every Release upload must use the PAT, not the default GITHUB_TOKEN.

    softprops/action-gh-release@v3 starts each upload with
    PATCH /repos/{owner}/{repo}/releases/{release_id}. When the default token
    is refused there, the whole asset set is dropped while every build still
    reports success: v0.20.34 shipped with 0 assets.
    """
    steps = _github_release_steps(_release_jobs())
    assert len(steps) == 4
    for step in steps:
        assert step["with"]["token"] == GITHUB_RELEASE_TOKEN


def test_release_workflow_manual_backfill_reuses_core_release_assets() -> None:
    build_wheels = _release_jobs()["build-wheels"]
    assert build_wheels["with"]["reuse-release-assets"] == REUSE_RELEASE_ASSETS_EXPRESSION
    assert build_wheels["secrets"] == {"RELEASE_TOKEN": "${{ secrets.PERSONAL_ACCESS_TOKEN }}"}


def test_manual_backfill_is_explicitly_core_only() -> None:
    jobs = _release_jobs()
    for job_id in ("build-admin-ui", "build-binaries", "build-semantic-wheels", "build-cli-wheels"):
        condition = jobs[job_id]["if"]
        assert f"!({CORE_BACKFILL_EXPRESSION}{ASSETS_BACKFILL_GUARD})" in condition

    summary = jobs["publish"]["steps"][0]["run"]
    assert f'core_backfill="${{{{ {CORE_BACKFILL_EXPRESSION}{ASSETS_BACKFILL_GUARD} }}}}"' in summary
    assert 'server" != "skipped"' in summary
    assert 'semantic" != "skipped"' in summary
    assert 'release_assets" != "skipped"' in summary


def test_backfill_assets_input_defaults_to_core_only() -> None:
    """`backfill_assets` is the opt-in that widens a core-only backfill."""
    workflow = yaml_loads(RELEASE_WORKFLOW.read_text(encoding="utf-8"))
    backfill_assets = workflow["on"]["workflow_dispatch"]["inputs"]["backfill_assets"]
    assert backfill_assets["type"] == "boolean"
    assert backfill_assets["default"] is False
    assert backfill_assets["required"] is False


def test_release_workflow_verifies_the_published_asset_set() -> None:
    jobs = _release_jobs()
    verify = jobs["verify-release-assets"]

    assert "always()" in verify["if"]
    # The legacy core-only PyPI backfill deliberately leaves the existing
    # Release untouched, so it is the one route without the gate.
    assert f"!({CORE_BACKFILL_EXPRESSION}{ASSETS_BACKFILL_GUARD})" in verify["if"]
    assert verify["needs"] == ["release-please", "publish-github-release-assets"]
    # `publish` aggregates every publication route, so the gate is part of it.
    assert "verify-release-assets" in jobs["publish"]["needs"]
    assert 'assets_verified" != "success"' in jobs["publish"]["steps"][0]["run"]

    script = next(step for step in verify["steps"] if "check_release_assets.py" in step.get("run", ""))
    assert '--version "$RELEASE_VERSION"' in script["run"]


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
        # The dcc-mcp-cli wrapper wheels are built once for all platforms and
        # published from a single artefact, hence the non-glob pattern.
        "publish-cli-pypi": {
            "needs": ["release-please", "validate-release-version", "build-cli-wheels"],
            "url": "https://pypi.org/p/dcc-mcp-cli",
            "artifact_pattern": "cli-wheel-all",
            "artifact_path": "dist-cli",
            "packages_dir": "dist-cli",
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

    assert sum(len(_pypi_steps(job)) for job in jobs.values()) == 4


def test_core_pypi_publish_validates_complete_distribution_set_before_upload() -> None:
    publish = _release_jobs()["publish-core-pypi"]
    steps = publish["steps"]
    validate_index = next(
        index for index, step in enumerate(steps) if "check_release_distribution_set.py" in step.get("run", "")
    )
    upload_index = next(index for index, step in enumerate(steps) if _uses_action(step, PYPI_ACTION))

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
        "publish-cli-pypi",
        "publish-github-release-assets",
        "verify-release-assets",
    ]
    assert "always()" in summary["if"]
    run = summary["steps"][0]["run"]
    assert "needs.publish-core-pypi.result" in run
    assert "needs.publish-server-pypi.result" in run
    assert "needs.publish-semantic-pypi.result" in run
    assert "needs.publish-cli-pypi.result" in run
    assert "needs.publish-github-release-assets.result" in run
    # A new route must also be gated, or it can fail silently.
    assert 'cli" != "success"' in run
    assert 'cli" != "skipped"' in run


def test_release_workflow_builds_cli_wrapper_wheels_from_the_release_archives() -> None:
    """The PyPI wrapper wheels are derived from the archives build-binaries uploads.

    Nothing in that job compiles, so it runs once on a single runner and loops
    over the three platforms. It must depend on ``build-binaries`` (the source
    of the archives) and stamp each wheel with a platform tag before upload,
    or pip would resolve a Linux binary onto Windows.
    """
    jobs = _release_jobs()
    build = jobs["build-cli-wheels"]

    assert build["runs-on"] == "ubuntu-latest"
    assert build["needs"] == ["release-please", "build-binaries"]

    runs = "\n".join(step.get("run", "") for step in build["steps"])
    assert "scripts/release/build_cli_wrapper_wheel.py" in runs
    assert "scripts/release/cli_wheel_tags.py retag" in runs
    assert "scripts/release/cli_wheel_tags.py validate" in runs
    assert '--platform "$platform"' in runs
    for platform in ("linux-x86_64", "macos-universal2", "windows-x86_64"):
        assert platform in runs

    upload = next(step for step in build["steps"] if step.get("uses") == "actions/upload-artifact@v4")
    assert upload["with"]["name"] == "cli-wheel-all"
    assert upload["with"]["path"] == "dist-cli/*.whl"


def test_release_workflow_gives_every_platform_its_own_wheel_output_directory() -> None:
    """Each platform must build into a directory no other platform writes to.

    hatchling names every build ``dcc_mcp_cli-<version>-py3-none-any.whl``
    because the wrapper carries no compiled extension. Looping the three
    platforms over one output directory therefore overwrites the first wheel
    with the second, and the build script finds no *new* file and exits 1 with
    "expected exactly one wrapper wheel, got []" - which in turn skips
    ``publish-cli-pypi`` and fails every release. The wheel has to be retagged
    before it joins the shared directory, or the next build overwrites it by
    name.
    """
    build = _release_jobs()["build-cli-wheels"]
    build_step = next(step for step in build["steps"] if step.get("name") == "Build wrapper wheels")
    run = build_step["run"]

    assert 'out="$PWD/dist-cli-build/$platform"' in run
    assert '--out-dir "$out"' in run
    # Shared-directory builds would silently collide on the second platform.
    assert '--out-dir "$PWD/dist-cli"' not in run
    # Retag inside the loop, then move: the tag is what makes the names unique.
    assert 'cli_wheel_tags.py retag --wheel-dir "$out"' in run
    assert 'mv "$out"/*.whl "$PWD/dist-cli/"' in run


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
