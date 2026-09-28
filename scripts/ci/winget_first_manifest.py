#!/usr/bin/env python3
"""Render the first WinGet manifest set for ``DccMcp.DccMcpCli``.

``vedantmgoyal9/winget-releaser`` can only add a version to a package that
already exists in ``microsoft/winget-pkgs``, so the very first manifest has to
be produced some other way. Komac's ``new`` command could do it interactively,
but it prompts for the nested files of a zip installer even under ``--dry-run``
and therefore cannot run on a runner.

This module renders the three manifests directly instead, from facts read off a
published GitHub Release. The shape of the output was captured from Komac
v2.16.0 and verified with ``winget validate``.

Offline by design: the release facts arrive as JSON produced by
``gh release view --json tagName,publishedAt,assets``, so every rule below is
testable without network access.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path
import re
import sys

PACKAGE_IDENTIFIER = "DccMcp.DccMcpCli"

# The package lives under manifests/<first letter lower>/<publisher>/<name>.
PACKAGE_PATH = "d/DccMcp/DccMcpCli"

# crates/dcc-mcp-cli/src/application/current_exe.rs landed after the v0.20.36
# tag, so v0.20.36 still fails `components ensure` when it is started through
# the WinGet Links\ symlink. v0.20.37 is the first usable version.
MIN_VERSION = (0, 20, 37)

MANIFEST_VERSION = "1.12.0"
DEFAULT_LOCALE = "en-US"

PUBLISHER = "DCC MCP"
PUBLISHER_URL = "https://github.com/dcc-mcp"
PUBLISHER_SUPPORT_URL = "https://github.com/dcc-mcp/dcc-mcp-core/issues"
PACKAGE_NAME = "DCC MCP CLI"
PACKAGE_URL = "https://github.com/dcc-mcp/dcc-mcp-core"
LICENSE = "MIT"
LICENSE_URL = "https://github.com/dcc-mcp/dcc-mcp-core/blob/main/LICENSE"
SHORT_DESCRIPTION = "Command line client for the DCC Model Context Protocol"
DESCRIPTION = (
    "Command line client for the DCC Model Context Protocol (MCP) ecosystem. Manages "
    "adapters, skills and MCP server components for Maya, Blender, Houdini, 3ds Max, "
    "Nuke and Unreal Engine."
)
MONIKER = "dcc-mcp-cli"
TAGS = [
    "ai-agents",
    "automation",
    "blender",
    "dcc",
    "dcc-mcp",
    "houdini",
    "maya",
    "mcp",
    "model-context-protocol",
    "photoshop",
    "pyo3",
    "python",
    "rust",
]

# Only the unversioned portable zip for the CLI is published to WinGet, and the
# zip holds a single `dcc-mcp-cli.exe` at its root (verified against v0.20.36).
INSTALLER_ASSET_TEMPLATE = "dcc-mcp-cli-{version}-windows-x86_64.zip"
RELATIVE_FILE_PATH = "dcc-mcp-cli.exe"
PORTABLE_COMMAND_ALIAS = "dcc-mcp-cli"
ARCHITECTURE = "x64"
INSTALLER_TYPE = "zip"
NESTED_INSTALLER_TYPE = "portable"
UPGRADE_BEHAVIOR = "install"

_DIGEST_RE = re.compile(r"^sha256:([0-9a-f]{64})$")
_VERSION_RE = re.compile(r"^\d+\.\d+\.\d+$")


class WingetManifestError(ValueError):
    """Raised when a first WinGet manifest set cannot be derived."""


def parse_version(version: str) -> tuple:
    """Parse a ``major.minor.patch`` version into a comparable tuple."""
    if not _VERSION_RE.match(version):
        raise WingetManifestError(f"unexpected package version {version!r}")
    return tuple(int(part) for part in version.split("."))


def check_version_floor(version: str) -> None:
    r"""Reject versions that predate the WinGet Links\ symlink fix."""
    parsed = parse_version(version)
    if parsed < MIN_VERSION:
        floor = ".".join(str(part) for part in MIN_VERSION)
        raise WingetManifestError(
            f"{version} predates the WinGet Links symlink fix; the first manifest must target {floor} or newer"
        )


def select_installer_asset(assets: list, version: str) -> dict:
    """Return the single Windows CLI zip asset for ``version``."""
    expected = INSTALLER_ASSET_TEMPLATE.format(version=version)
    matches = [asset for asset in assets if asset.get("name") == expected]
    if not matches:
        names = ", ".join(sorted(str(asset.get("name")) for asset in assets))
        raise WingetManifestError(f"release {version} has no {expected} asset (found: {names or 'none'})")
    if len(matches) > 1:
        raise WingetManifestError(f"release {version} has {len(matches)} {expected} assets")
    return matches[0]


def installer_sha256(asset: dict) -> str:
    """Read the upper-case ``InstallerSha256`` from an asset digest."""
    digest = asset.get("digest")
    if not isinstance(digest, str):
        raise WingetManifestError(f"asset {asset.get('name')!r} has no digest; refusing to guess its SHA256")
    match = _DIGEST_RE.match(digest.strip())
    if not match:
        raise WingetManifestError(f"asset {asset.get('name')!r} has an unreadable digest {digest!r}")
    return match.group(1).upper()


def installer_url(asset: dict) -> str:
    """Return the immutable download URL of an asset.

    Only ``url`` is read. There is no ``browserDownloadUrl`` fallback: that key
    never appears in ``gh release view --json assets`` output, so the fallback
    would be dead code here, while on a raw REST payload the two fields swap
    roles and ``url`` is the API link. Requiring the URL to end with the asset
    name turns that mix-up into an error instead of shipping the API URL as
    ``InstallerUrl``.
    """
    name = asset.get("name")
    url = asset.get("url")
    if not isinstance(url, str) or not url:
        raise WingetManifestError(f"asset {name!r} has no download URL")
    if isinstance(name, str) and name:
        tail = url.split("?")[0].split("#")[0].rstrip("/")
        if not tail.endswith("/" + name):
            raise WingetManifestError(f"asset {name!r} has a URL that is not a release download link: {url!r}")
    return url


def release_date(published_at: str) -> str:
    """Return the ``YYYY-MM-DD`` prefix of a release timestamp."""
    if not isinstance(published_at, str) or len(published_at) < 10:
        raise WingetManifestError(f"unreadable release timestamp {published_at!r}")
    return published_at[:10]


def _scalar(value: str) -> str:
    """Quote a YAML scalar only when plain style would be ambiguous."""
    if value == "" or value.strip() != value:
        return json.dumps(value)
    if value[0] in "&*?|-<>=!%@`#{}[],:'\"":
        return json.dumps(value)
    if ": " in value or value.endswith(":") or " #" in value:
        return json.dumps(value)
    if value.lower() in ("true", "false", "null", "yes", "no", "on", "off", "~"):
        return json.dumps(value)
    return value


def render_version_manifest(version: str) -> str:
    """Render the singleton version manifest."""
    return (
        f"# yaml-language-server: "
        f"$schema=https://aka.ms/winget-manifest.version.{MANIFEST_VERSION}.schema.json\n"
        f"\n"
        f"PackageIdentifier: {PACKAGE_IDENTIFIER}\n"
        f"PackageVersion: {version}\n"
        f"DefaultLocale: {DEFAULT_LOCALE}\n"
        f"ManifestType: version\n"
        f"ManifestVersion: {MANIFEST_VERSION}\n"
    )


def render_installer_manifest(version: str, url: str, sha256: str, date: str) -> str:
    """Render the installer manifest for the portable Windows CLI zip."""
    return (
        f"# yaml-language-server: "
        f"$schema=https://aka.ms/winget-manifest.installer.{MANIFEST_VERSION}.schema.json\n"
        f"\n"
        f"PackageIdentifier: {PACKAGE_IDENTIFIER}\n"
        f"PackageVersion: {version}\n"
        f"InstallerType: {INSTALLER_TYPE}\n"
        f"NestedInstallerType: {NESTED_INSTALLER_TYPE}\n"
        f"NestedInstallerFiles:\n"
        f"- RelativeFilePath: {RELATIVE_FILE_PATH}\n"
        f"  PortableCommandAlias: {PORTABLE_COMMAND_ALIAS}\n"
        f"UpgradeBehavior: {UPGRADE_BEHAVIOR}\n"
        f"ReleaseDate: {date}\n"
        f"Installers:\n"
        f"- Architecture: {ARCHITECTURE}\n"
        f"  InstallerUrl: {url}\n"
        f"  InstallerSha256: {sha256}\n"
        f"ManifestType: installer\n"
        f"ManifestVersion: {MANIFEST_VERSION}\n"
    )


def render_locale_manifest(version: str, release_notes_url: str) -> str:
    """Render the default-locale manifest carrying the store metadata."""
    lines = [
        f"# yaml-language-server: $schema=https://aka.ms/winget-manifest.defaultLocale.{MANIFEST_VERSION}.schema.json",
        "",
        f"PackageIdentifier: {PACKAGE_IDENTIFIER}",
        f"PackageVersion: {version}",
        f"PackageLocale: {DEFAULT_LOCALE}",
        f"Publisher: {_scalar(PUBLISHER)}",
        f"PublisherUrl: {PUBLISHER_URL}",
        f"PublisherSupportUrl: {PUBLISHER_SUPPORT_URL}",
        f"PackageName: {_scalar(PACKAGE_NAME)}",
        f"PackageUrl: {PACKAGE_URL}",
        f"License: {LICENSE}",
        f"LicenseUrl: {LICENSE_URL}",
        f"ShortDescription: {_scalar(SHORT_DESCRIPTION)}",
        f"Description: {_scalar(DESCRIPTION)}",
        f"Moniker: {MONIKER}",
        "Tags:",
    ]
    lines.extend(f"- {tag}" for tag in TAGS)
    lines.extend(
        [
            f"ReleaseNotesUrl: {release_notes_url}",
            "ManifestType: defaultLocale",
            f"ManifestVersion: {MANIFEST_VERSION}",
            "",
        ]
    )
    return "\n".join(lines)


def build_manifests(release: dict) -> dict:
    """Return ``{relative path: manifest text}`` for a GitHub Release payload."""
    tag = release.get("tagName")
    if not isinstance(tag, str) or not tag.startswith("v"):
        raise WingetManifestError(f"unexpected release tag {tag!r}")
    version = tag[1:]
    check_version_floor(version)

    assets = release.get("assets")
    if not isinstance(assets, list):
        raise WingetManifestError("release payload has no assets list")

    asset = select_installer_asset(assets, version)
    url = installer_url(asset)
    sha256 = installer_sha256(asset)
    date = release_date(release.get("publishedAt", ""))
    release_notes_url = f"{PACKAGE_URL}/releases/tag/{tag}"

    directory = f"manifests/{PACKAGE_PATH}/{version}"
    return {
        f"{directory}/{PACKAGE_IDENTIFIER}.yaml": render_version_manifest(version),
        f"{directory}/{PACKAGE_IDENTIFIER}.installer.yaml": render_installer_manifest(version, url, sha256, date),
        f"{directory}/{PACKAGE_IDENTIFIER}.locale.{DEFAULT_LOCALE}.yaml": render_locale_manifest(
            version, release_notes_url
        ),
    }


def write_manifests(release: dict, output_dir: Path) -> list:
    """Write the manifest set under ``output_dir`` and return the written paths."""
    written = []
    for relative, text in sorted(build_manifests(release).items()):
        path = output_dir / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        # write_bytes, not write_text: `newline=` was only added to
        # Path.write_text in Python 3.10 and this repository supports 3.7.
        # Encoding explicitly also keeps the manifests LF-ended on Windows,
        # where a text-mode write would translate them to CRLF.
        path.write_bytes(text.encode("utf-8"))
        written.append(path)
    return written


def _parse_args(argv: list) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--release-json", required=True, type=Path, help="gh release view JSON")
    parser.add_argument("--output-dir", required=True, type=Path, help="manifest root directory")
    return parser.parse_args(argv)


def main(argv: list | None = None) -> int:
    """Render a manifest set from a release payload passed on the command line."""
    args = _parse_args(sys.argv[1:] if argv is None else argv)
    try:
        release = json.loads(args.release_json.read_text(encoding="utf-8"))
    except (OSError, ValueError) as error:
        print(f"error: cannot read {args.release_json}: {error}", file=sys.stderr)
        return 2
    try:
        written = write_manifests(release, args.output_dir)
    except WingetManifestError as error:
        print(f"error: {error}", file=sys.stderr)
        return 1
    for path in written:
        print(path)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
