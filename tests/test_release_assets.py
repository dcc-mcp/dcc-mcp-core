"""Tests for published GitHub Release asset verification."""

from __future__ import annotations

import io
import json

import pytest
from scripts.ci import check_release_assets
from scripts.ci.check_release_assets import ReleaseAssetError
from scripts.ci.check_release_assets import verify_release_assets

VERSION = "0.20.34"
PLATFORMS = ("linux-x86_64", "macos-universal2", "windows-x86_64")


def _asset_payload(names):
    return {"tag_name": f"v{VERSION}", "assets": [{"name": name} for name in names]}


def _baseline_names(version=VERSION):
    """Mirror the v0.20.32-v0.20.33 asset set: 33 files."""
    names = [
        f"dcc_mcp_core-{version}-cp37-cp37m-manylinux_2_17_x86_64.manylinux2014_x86_64.whl",
        f"dcc_mcp_core-{version}-cp37-cp37m-win_amd64.whl",
        (f"dcc_mcp_core-{version}-cp38-abi3-macosx_10_12_x86_64.macosx_11_0_arm64.macosx_10_12_universal2.whl"),
        f"dcc_mcp_core-{version}-cp38-abi3-manylinux_2_17_x86_64.manylinux2014_x86_64.whl",
        f"dcc_mcp_core-{version}-cp38-abi3-win_amd64.whl",
        f"dcc_mcp_core-{version}-py3-none-any.whl",
        f"dcc_mcp_core-{version}.tar.gz",
        f"dcc_mcp_core_semantic-{version}-cp37-cp37m-manylinux_2_28_x86_64.whl",
        f"dcc_mcp_core_semantic-{version}-cp37-cp37m-win_amd64.whl",
        f"dcc_mcp_core_semantic-{version}-cp38-abi3-macosx_11_0_arm64.whl",
        f"dcc_mcp_core_semantic-{version}-cp38-abi3-manylinux_2_28_x86_64.whl",
        f"dcc_mcp_core_semantic-{version}-cp38-abi3-win_amd64.whl",
        f"dcc_mcp_server-{version}-py3-none-manylinux2014_x86_64.manylinux_2_17_x86_64.whl",
        f"dcc_mcp_server-{version}-py3-none-win_amd64.whl",
        f"dcc_mcp_server-{version}-py3-none-macosx_10_12_universal2.macosx_11_0_arm64.whl",
    ]
    for platform in PLATFORMS:
        suffix = ".exe" if platform == "windows-x86_64" else ""
        names.append(f"dcc-mcp-server-{platform}{suffix}")
        names.append(f"dcc-mcp-cli-{platform}{suffix}")
        names.append(f"dcc-mcp-server-{version}-{platform}.zip")
        names.append(f"dcc-mcp-cli-{version}-{platform}.zip")
        names.append(f"dcc-mcp-update-manifest-{platform}.json")
        names.append(f"dcc-mcp-update-manifest-{platform}.sigstore.json")
    return names


def test_baseline_release_passes() -> None:
    names = _baseline_names()

    evidence = verify_release_assets(_asset_payload(names), VERSION)

    assert len(names) == check_release_assets.DEFAULT_EXPECTED_COUNT
    assert evidence["asset_count"] == check_release_assets.DEFAULT_EXPECTED_COUNT
    assert evidence["missing"] == []
    assert evidence["unexpected"] == []


def test_empty_release_is_rejected() -> None:
    with pytest.raises(ReleaseAssetError, match="0 assets"):
        verify_release_assets(_asset_payload([]), VERSION)


def test_missing_python37_wheels_is_rejected() -> None:
    names = [name for name in _baseline_names() if "cp37" not in name]

    with pytest.raises(ReleaseAssetError, match="core-py37-manylinux"):
        verify_release_assets(_asset_payload(names), VERSION)


def test_partial_platform_leg_is_rejected() -> None:
    names = [name for name in _baseline_names() if "windows-x86_64" not in name]

    with pytest.raises(ReleaseAssetError, match="server-binary"):
        verify_release_assets(_asset_payload(names), VERSION)


def test_missing_attestation_bundle_is_rejected() -> None:
    names = [name for name in _baseline_names() if not name.endswith(".sigstore.json")]

    with pytest.raises(ReleaseAssetError, match="update-manifest-attestation"):
        verify_release_assets(_asset_payload(names), VERSION)


def test_extra_assets_are_reported_but_accepted() -> None:
    names = [*_baseline_names(), "unexpected-hotfix.whl"]

    evidence = verify_release_assets(_asset_payload(names), VERSION)

    assert evidence["unexpected"] == ["unexpected-hotfix.whl"]


def test_invalid_payload_shapes_are_rejected() -> None:
    with pytest.raises(ReleaseAssetError, match="not a JSON object"):
        verify_release_assets([], VERSION)
    with pytest.raises(ReleaseAssetError, match="no asset list"):
        verify_release_assets({"tag_name": f"v{VERSION}"}, VERSION)
    with pytest.raises(ReleaseAssetError, match="invalid release version"):
        verify_release_assets(_asset_payload([]), "not-a-version")


def test_main_reports_failure_without_raising(tmp_path) -> None:
    payload = tmp_path / "release.json"
    payload.write_text(json.dumps(_asset_payload([])), encoding="utf-8")

    assert check_release_assets.main(["--tag", f"v{VERSION}", "--version", VERSION, "--payload", str(payload)]) == 1


def test_main_reads_payload_from_stdin(monkeypatch, tmp_path) -> None:
    monkeypatch.setattr(
        "sys.stdin",
        io.StringIO(json.dumps(_asset_payload(_baseline_names()))),
    )

    assert check_release_assets.main(["--tag", f"v{VERSION}", "--version", VERSION, "--payload", "-"]) == 0
