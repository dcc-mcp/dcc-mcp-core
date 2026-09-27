"""Regression tests for the first WinGet manifest renderer."""

from __future__ import annotations

import json

import pytest
from scripts.ci import winget_first_manifest as winget
from scripts.ci.winget_first_manifest import WingetManifestError

VERSION = "0.20.37"
SHA256 = "A" * 64
INSTALLER = f"dcc-mcp-cli-{VERSION}-windows-x86_64.zip"
URL = f"https://example.invalid/{INSTALLER}"


def release_payload(version: str = VERSION, **asset_overrides) -> dict:
    asset = {
        "name": f"dcc-mcp-cli-{version}-windows-x86_64.zip",
        "digest": f"sha256:{SHA256.lower()}",
        "url": f"https://example.invalid/dcc-mcp-cli-{version}-windows-x86_64.zip",
    }
    asset.update(asset_overrides)
    return {
        "tagName": f"v{version}",
        "publishedAt": "2026-09-28T01:20:00Z",
        "assets": [
            asset,
            {
                "name": f"dcc-mcp-cli-{version}-linux-x86_64.zip",
                "digest": "sha256:" + "b" * 64,
                "url": "https://example.invalid/linux.zip",
            },
        ],
    }


def manifests(payload: dict) -> dict:
    return winget.build_manifests(payload)


def installer_text(payload: dict) -> str:
    key = f"manifests/d/DccMcp/DccMcpCli/{payload['tagName'][1:]}/DccMcp.DccMcpCli.installer.yaml"
    return manifests(payload)[key]


def test_rendered_manifest_set_has_the_three_expected_files() -> None:
    payload = release_payload()

    rendered = manifests(payload)

    prefix = f"manifests/d/DccMcp/DccMcpCli/{VERSION}/DccMcp.DccMcpCli"
    assert sorted(rendered) == sorted([f"{prefix}.yaml", f"{prefix}.installer.yaml", f"{prefix}.locale.en-US.yaml"])


def test_installer_manifest_pins_the_windows_zip_and_its_nested_exe() -> None:
    text = installer_text(release_payload())

    assert "InstallerType: zip" in text
    assert "NestedInstallerType: portable" in text
    assert f"RelativeFilePath: {winget.RELATIVE_FILE_PATH}" in text
    assert f"PortableCommandAlias: {winget.PORTABLE_COMMAND_ALIAS}" in text
    assert "Architecture: x64" in text
    assert f"InstallerSha256: {SHA256}" in text
    assert f"InstallerUrl: {URL}" in text


def test_release_date_comes_from_the_release_not_from_today() -> None:
    payload = release_payload()
    payload["publishedAt"] = "2026-10-05T09:08:07Z"

    assert "ReleaseDate: 2026-10-05" in installer_text(payload)


@pytest.mark.parametrize(
    "version",
    ["0.20.36", "0.20.10", "0.19.99", "0.9.0"],
)
def test_versions_below_the_links_symlink_fix_are_refused(version: str) -> None:
    with pytest.raises(WingetManifestError, match="predates the WinGet Links symlink fix"):
        manifests(release_payload(version))


@pytest.mark.parametrize("version", ["0.20.37", "0.20.38", "0.21.0", "1.0.0"])
def test_versions_at_or_above_the_floor_are_accepted(version: str) -> None:
    assert manifests(release_payload(version))


def test_a_missing_windows_asset_is_an_error_not_a_silent_skip() -> None:
    payload = release_payload()
    payload["assets"] = [a for a in payload["assets"] if "windows" not in a["name"]]

    with pytest.raises(WingetManifestError, match="has no"):
        manifests(payload)


def test_a_release_without_a_digest_is_refused() -> None:
    payload = release_payload(digest=None)

    with pytest.raises(WingetManifestError, match="no digest"):
        manifests(payload)


def test_a_short_or_missing_sha256_digest_is_refused() -> None:
    payload = release_payload(digest="sha256:deadbeef")

    with pytest.raises(WingetManifestError, match="unreadable digest"):
        manifests(payload)


def test_a_non_sha256_digest_is_refused() -> None:
    payload = release_payload(digest="sha1:" + "c" * 40)

    with pytest.raises(WingetManifestError, match="unreadable digest"):
        manifests(payload)


def test_duplicate_windows_assets_are_refused() -> None:
    payload = release_payload()
    payload["assets"].append(dict(payload["assets"][0]))

    with pytest.raises(WingetManifestError, match="2 "):
        manifests(payload)


def test_an_unparsable_tag_is_refused() -> None:
    payload = release_payload()
    payload["tagName"] = "0.20.37"

    with pytest.raises(WingetManifestError, match="unexpected release tag"):
        manifests(payload)


def test_an_unparsable_version_is_refused() -> None:
    payload = release_payload()
    payload["tagName"] = "v0.20.37-rc1"

    with pytest.raises(WingetManifestError, match="unexpected package version"):
        manifests(payload)


def test_manifests_are_valid_yaml_with_the_expected_identity() -> None:
    yaml = pytest.importorskip("yaml")

    payload = release_payload()
    for text in manifests(payload).values():
        document = yaml.safe_load(text)
        assert document["PackageIdentifier"] == "DccMcp.DccMcpCli"
        assert document["PackageVersion"] == VERSION

    installer = yaml.safe_load(
        manifests(payload)[f"manifests/d/DccMcp/DccMcpCli/{VERSION}/DccMcp.DccMcpCli.installer.yaml"]
    )
    assert installer["Installers"][0]["InstallerSha256"] == SHA256
    assert installer["Installers"][0]["Architecture"] == "x64"


def test_scalars_that_would_be_ambiguous_in_yaml_are_quoted() -> None:
    assert winget._scalar("DCC MCP CLI") == "DCC MCP CLI"
    assert winget._scalar("true") == json.dumps("true")
    assert winget._scalar("a: b") == json.dumps("a: b")
    assert winget._scalar("- item") == json.dumps("- item")
    assert winget._scalar(" padded") == json.dumps(" padded")


def test_write_manifests_creates_the_winget_pkgs_layout(tmp_path) -> None:
    written = winget.write_manifests(release_payload(), tmp_path)

    assert len(written) == 3
    for path in written:
        assert path.is_file()
        assert path.read_text(encoding="utf-8").endswith("\n")
        assert "\r\n" not in path.read_bytes().decode("utf-8")
    assert (tmp_path / "manifests" / "d" / "DccMcp" / "DccMcpCli" / VERSION).is_dir()
