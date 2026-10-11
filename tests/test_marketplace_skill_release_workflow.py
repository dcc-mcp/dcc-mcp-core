"""Guard the marketplace release upload's assets and prerequisite checks."""

# Import future modules
from __future__ import annotations

# Import built-in modules
from pathlib import Path
import shlex

# Import third-party modules
import yaml

WORKFLOW = Path(__file__).resolve().parents[1] / ".github" / "workflows" / "marketplace-skill-release.yml"


def _jobs() -> dict:
    return yaml.safe_load(WORKFLOW.read_text(encoding="utf-8"))["jobs"]


def test_cross_os_pack_step_reads_version_under_bash() -> None:
    pack = _jobs()["pack"]
    assert "windows-latest" in pack["strategy"]["matrix"]["os"]
    step = next(step for step in pack["steps"] if step.get("name") == "Pack skill bundles")
    assert step["shell"] == "bash"
    assert step["env"]["VERSION"] == "${{ steps.version.outputs.value }}"
    assert '--version "$VERSION"' in step["run"]


def test_upload_publishes_matching_manifest_and_archives() -> None:
    steps = _jobs()["upload"]["steps"]
    downloads = [step["with"] for step in steps if step.get("uses", "").startswith("actions/download-artifact@")]
    assert {"name": "marketplace-skill-bundles-ubuntu-latest", "path": "dist/marketplace"} in downloads
    assert {"name": "marketplace-manifest-ubuntu-latest", "path": "dist/marketplace"} in downloads
    upload = next(step for step in steps if "gh release upload" in step.get("run", ""))
    command = shlex.split(upload["run"])
    assert command == [
        "gh",
        "release",
        "upload",
        "v$VERSION",
        "dist/marketplace/*.zip",
        "dist/marketplace/manifest.json",
        "--clobber",
    ]
    assert upload["env"]["VERSION"] == "${{ needs.pack.outputs.version }}"


def test_upload_waits_for_drift_gate_and_only_allows_disabled_skip() -> None:
    upload = _jobs()["upload"]
    assert set(upload["needs"]) == {"pack", "cross-os", "version-drift"}
    # always() is required to evaluate this condition when the optional gate
    # is skipped. A failed/cancelled gate, or an unexpected skip while enabled,
    # must never be accepted as permission to overwrite published assets.
    condition = " ".join(upload["if"].split())
    assert condition == (
        "${{ always() && inputs.upload_release "
        "&& needs.pack.result == 'success' "
        "&& needs.cross-os.result == 'success' "
        "&& (needs.version-drift.result == 'success' "
        "|| (!inputs.check_version_drift && needs.version-drift.result == 'skipped')) }}"
    )


def test_drift_baseline_still_uses_matching_release_tag() -> None:
    steps = _jobs()["version-drift"]["steps"]
    compare = next(step for step in steps if "gh" in step.get("run", ""))
    assert '["gh", "release", "view", "v" + version, "--json", "assets"]' in compare["run"]
    assert '["gh", "release", "download", "v" + version,' in compare["run"]
