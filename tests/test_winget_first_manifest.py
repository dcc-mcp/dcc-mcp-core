"""Regression tests for the first WinGet manifest renderer."""

from __future__ import annotations

import json
from pathlib import Path
import re

import pytest
from scripts.ci import winget_first_manifest as winget
from scripts.ci.winget_first_manifest import WingetManifestError

REPO_ROOT = Path(__file__).resolve().parents[1]
WORKFLOW = REPO_ROOT / ".github" / "workflows" / "winget-first-manifest.yml"

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


def test_an_api_style_url_is_rejected_instead_of_published() -> None:
    # A raw REST payload carries the download link in `browserDownloadUrl` and
    # the API link in `url`. Publishing the API link as InstallerUrl would be
    # silent, so the renderer has to refuse it rather than fall back.
    payload = release_payload(url="https://api.github.com/repos/o/r/releases/assets/383985289")

    with pytest.raises(WingetManifestError, match="not a release download link"):
        manifests(payload)


def test_a_download_url_with_a_query_string_is_accepted() -> None:
    payload = release_payload(url=f"https://example.invalid/{INSTALLER}?sig=abc")

    assert f"InstallerUrl: https://example.invalid/{INSTALLER}?sig=abc" in installer_text(payload)


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


def test_the_existence_guard_fails_closed_on_a_non_404_api_error() -> None:
    """The workflow's duplicate-submission guard must never fail open.

    `gh api` exits non-zero for a 404 (package absent, the only case allowed to
    continue) and equally for a 403 rate limit, a 401 or a network error. A
    guard that reads any non-zero exit as "absent" silently disables itself, so
    the workflow has to name 404 explicitly and abort on anything else.
    """
    yaml = pytest.importorskip("yaml")

    workflow = yaml.safe_load(WORKFLOW.read_text(encoding="utf-8"))
    step = next(
        s
        for s in workflow["jobs"]["submit-first-manifest"]["steps"]
        if s.get("name") == "Refuse to run once the package exists"
    )
    body = step["run"]

    assert "gh_status=$?" in body, "the guard must capture the gh api exit code"
    assert re.search(r"grep -q ['\"]HTTP 404['\"]", body), (
        "the guard must discriminate a confirmed 404 instead of trusting any non-zero exit"
    )
    # The abort branch has to come after the 200 branch and before continuing.
    assert "already exists in winget-pkgs" in body
    assert "refusing to submit" in body


def test_the_existence_guard_probes_a_control_path_before_trusting_a_404() -> None:
    """A 404 only means "absent" if the token can read the repository at all.

    This step reads microsoft/winget-pkgs, which is in another organisation. A
    token scoped only to this repository gets 404 for *every* path, including
    ones that certainly exist, so discriminating 404 would still let the guard
    become a silent no-op. Probing a package that must exist turns that into a
    loud failure.
    """
    yaml = pytest.importorskip("yaml")

    workflow = yaml.safe_load(WORKFLOW.read_text(encoding="utf-8"))
    step = next(
        s
        for s in workflow["jobs"]["submit-first-manifest"]["steps"]
        if s.get("name") == "Refuse to run once the package exists"
    )

    # The read must use the PAT, not the repo-scoped installation token.
    assert step["env"]["GH_TOKEN"] == "${{ secrets.WINGET_TOKEN }}", (
        "a cross-org read needs the PAT; github.token is scoped to this repository"
    )

    body = step["run"]
    assert re.search(r'^\s*control=".+"\s*$', body, re.MULTILINE), (
        "the guard must define a control path that is known to exist"
    )
    assert "control_status=$?" in body, "the guard must capture the control probe's exit code"
    assert "cannot read winget-pkgs" in body, "an unreadable control must abort the run"
    # The control has to be checked before the package path is trusted.
    assert body.index("control_status=$?") < body.index("gh_status=$?")


def test_write_manifests_creates_the_winget_pkgs_layout(tmp_path) -> None:
    written = winget.write_manifests(release_payload(), tmp_path)

    assert len(written) == 3
    for path in written:
        assert path.is_file()
        assert path.read_text(encoding="utf-8").endswith("\n")
        assert "\r\n" not in path.read_bytes().decode("utf-8")
    assert (tmp_path / "manifests" / "d" / "DccMcp" / "DccMcpCli" / VERSION).is_dir()


def _workflow_steps() -> list:
    yaml = pytest.importorskip("yaml")

    workflow = yaml.safe_load(WORKFLOW.read_text(encoding="utf-8"))
    return workflow["jobs"]["submit-first-manifest"]["steps"]


def test_no_free_text_input_is_interpolated_into_a_run_block() -> None:
    """`${{ }}` is expanded before the shell parses, so an input spliced into a
    `run:` block is shell code rather than data.

    `release_tag` is `type: string` free text, so it must reach the script
    through `env:` like every other value this workflow passes in. A boolean
    input cannot carry a payload, but routing all of them through `env:` keeps
    the rule checkable in one place instead of relying on per-input reasoning.
    """
    for step in _workflow_steps():
        body = step.get("run")
        if not body:
            continue
        offenders = re.findall(r"\$\{\{\s*inputs\.[^}]*\}\}", body)
        assert not offenders, (
            f"step {step.get('name')!r} interpolates {offenders} directly into a run block; "
            "pass it through env: instead"
        )


def test_the_summary_step_reaches_inputs_through_env() -> None:
    step = next(s for s in _workflow_steps() if s.get("name") == "Summary")

    assert step["env"]["RELEASE_TAG"] == "${{ inputs.release_tag }}"
    assert step["env"]["DRY_RUN"] == "${{ inputs.dry_run }}"
    body = step["run"]
    assert "release: $RELEASE_TAG" in body
    assert "dry run: $DRY_RUN" in body


def test_manifest_validation_tolerates_warnings_but_not_errors() -> None:
    """`winget validate` exits non-zero for warnings as well as errors.

    The yaml-language-server schema comment every manifest carries trips a
    cosmetic "schema header URL" warning on current runner images, so a valid
    set can exit 1. Judging by exit code alone rejects it. The failure marker
    is the locale-independent "Manifest Error:" line; a missing tool or empty
    output must also fail rather than read as a pass.
    """
    step = next(s for s in _workflow_steps() if s.get("name") == "Validate the manifests")
    body = step["run"]

    assert "Manifest Error:" in body, (
        "validation must fail on the locale-independent error marker, not on the exit code"
    )
    assert "Get-Command winget" in body, "a missing winget must fail instead of silently passing"
    assert "IsNullOrWhiteSpace" in body, "empty output must fail instead of silently passing"
    # The exit code must not be the sole or primary verdict.
    assert "exit $LASTEXITCODE" not in body
