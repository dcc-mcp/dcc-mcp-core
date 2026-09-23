#!/usr/bin/env python3
"""Assert a published GitHub Release actually carries its artefact set.

Workflow job conclusions are not evidence that a release shipped. Three
distinct failure modes all look healthy in the Actions UI:

* **Short circuit** — the 2026-09-21 and 2026-09-22 batch windows reported
  ``success`` because ``release-please`` created no release and every other
  job was skipped.
* **Silent upload failure** — the v0.20.34 window cut tag and Release, but
  every ``softprops/action-gh-release`` upload step was refused with HTTP 403
  ``Resource not accessible by integration`` and the safety-net job was
  skipped by its ``if``, so the Release shipped with **zero** assets.
* **Partial upload** — a matrix leg fails after other legs already attached
  their files, leaving an incomplete but non-empty Release.

Reading the Release asset list back from the API and asserting the expected
set is the only check that separates those three outcomes from a real
release. It is deliberately run as its own job with ``always()`` so it also
fires when the upload jobs were skipped rather than failed.

The Python 3.7 requirement groups are part of the expected set on purpose:
the project still declares Python 3.7 support for the embedded DCC hosts
that cannot upgrade, so a Release without the ``cp37`` wheels is a failed
release, not a degraded one.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path
import re
import sys

DEFAULT_EXPECTED_COUNT = 33

_PLATFORMS = ("linux-x86_64", "macos-universal2", "windows-x86_64")
_PLATFORM_ALTERNATION = "|".join(_PLATFORMS)
_VERSION_RE = re.compile(r"\d+\.\d+\.\d+(?:[A-Za-z0-9.+-]*)")


class ReleaseAssetError(ValueError):
    """Raised when a published Release is missing part of its asset set."""


def _platform_pattern(prefix: str, version: str, suffix: str) -> str:
    """Build an anchored regex for one ``<prefix>[-<version>]-<platform><suffix>`` family."""
    infix = f"-{version}" if version else ""
    return rf"^{prefix}{infix}-({_PLATFORM_ALTERNATION}){suffix}$"


def required_asset_groups(version: str) -> tuple:
    """Return ``(label, pattern, minimum_count)`` triples for one release.

    The minimum counts sum to :data:`DEFAULT_EXPECTED_COUNT`, the asset count
    every ``dcc-mcp-core`` Release has shipped since v0.20.32.
    """
    if not _VERSION_RE.fullmatch(version):
        raise ReleaseAssetError(f"invalid release version {version!r}")
    escaped = re.escape(version)
    return (
        # dcc-mcp-core: abi3 wheels cover Python 3.8-3.14, cp37 wheels cover the
        # embedded DCC hosts (Maya 2022, Blender 2.83).
        ("core-py37-manylinux", rf"^dcc_mcp_core-{escaped}-cp37-cp37m-manylinux\S+\.whl$", 1),
        ("core-py37-windows", rf"^dcc_mcp_core-{escaped}-cp37-cp37m-win_amd64\.whl$", 1),
        ("core-abi3", rf"^dcc_mcp_core-{escaped}-cp38-abi3-\S+\.whl$", 3),
        ("core-lite", rf"^dcc_mcp_core-{escaped}-py3-none-any\.whl$", 1),
        ("core-sdist", rf"^dcc_mcp_core-{escaped}\.tar\.gz$", 1),
        # dcc-mcp-core-semantic: opt-in native embeddings, same two tiers.
        ("semantic-py37", rf"^dcc_mcp_core_semantic-{escaped}-cp37-cp37m-\S+\.whl$", 2),
        ("semantic-abi3", rf"^dcc_mcp_core_semantic-{escaped}-cp38-abi3-\S+\.whl$", 3),
        ("server-wheel", rf"^dcc_mcp_server-{escaped}-py3-none-\S+\.whl$", 3),
        # Standalone server / CLI binaries, their versioned bundles, and the
        # update manifests (plus one Sigstore attestation per manifest).
        ("server-binary", rf"^dcc-mcp-server-({_PLATFORM_ALTERNATION})(\.exe)?$", 3),
        ("cli-binary", rf"^dcc-mcp-cli-({_PLATFORM_ALTERNATION})(\.exe)?$", 3),
        ("server-bundle", _platform_pattern("dcc-mcp-server", escaped, r"\.zip"), 3),
        ("cli-bundle", _platform_pattern("dcc-mcp-cli", escaped, r"\.zip"), 3),
        ("update-manifest", _platform_pattern("dcc-mcp-update-manifest", "", r"\.json"), 3),
        (
            "update-manifest-attestation",
            _platform_pattern("dcc-mcp-update-manifest", "", r"\.sigstore\.json"),
            3,
        ),
    )


def _asset_names(payload: object) -> list:
    if not isinstance(payload, dict):
        raise ReleaseAssetError("GitHub Release payload is not a JSON object")
    assets = payload.get("assets")
    if not isinstance(assets, list):
        raise ReleaseAssetError("GitHub Release payload has no asset list")
    names = []
    for asset in assets:
        if not isinstance(asset, dict):
            raise ReleaseAssetError("GitHub Release asset JSON is invalid")
        name = asset.get("name")
        if not isinstance(name, str) or not name:
            raise ReleaseAssetError("GitHub Release asset has a missing or invalid name")
        if Path(name).name != name:
            raise ReleaseAssetError(f"GitHub Release asset name is not a bare filename: {name!r}")
        names.append(name)
    return names


def verify_release_assets(payload: object, version: str, expected_count: int = DEFAULT_EXPECTED_COUNT) -> dict:
    """Verify a Release asset set and return evidence.

    Raises :class:`ReleaseAssetError` when the Release carries no assets at
    all or misses any required group. A Release that carries *more* assets
    than expected is reported but not rejected: an extra artefact never makes
    a release unshippable.
    """
    if not _VERSION_RE.fullmatch(version or ""):
        raise ReleaseAssetError(f"invalid release version {version!r}")
    names = _asset_names(payload)
    if not names:
        raise ReleaseAssetError("GitHub Release has 0 assets; nothing was published for this tag")

    remaining = list(names)
    satisfied = []
    missing = []
    for label, pattern, minimum in required_asset_groups(version):
        compiled = re.compile(pattern)
        matched = [name for name in remaining if compiled.match(name)]
        for name in matched:
            remaining.remove(name)
        satisfied.append({"group": label, "required": minimum, "matched": len(matched)})
        if len(matched) < minimum:
            missing.append({"group": label, "required": minimum, "matched": len(matched)})

    evidence = {
        "tag": payload.get("tag_name") if isinstance(payload, dict) else None,
        "version": version,
        "asset_count": len(names),
        "expected_count": expected_count,
        "groups": satisfied,
        "missing": missing,
        "unexpected": sorted(remaining),
    }
    if missing:
        detail = ", ".join(
            f"{item['group']} (expected {item['required']}, found {item['matched']})" for item in missing
        )
        raise ReleaseAssetError(f"GitHub Release is incomplete ({len(names)} assets): missing asset groups: {detail}")
    if len(names) < expected_count:
        raise ReleaseAssetError(f"GitHub Release carries {len(names)} assets, expected at least {expected_count}")
    return evidence


def _load_payload(path: Path) -> object:
    if str(path) == "-":
        return json.load(sys.stdin)
    if not path.is_file():
        raise ReleaseAssetError(f"release payload file does not exist: {path}")
    return json.loads(path.read_text(encoding="utf-8"))


def main(argv=None) -> int:
    """Run the Release asset gate and return a process exit code."""
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--tag", required=True, help="Release tag, for log messages only")
    parser.add_argument("--version", required=True, help="Release version without the v prefix")
    parser.add_argument(
        "--payload",
        required=True,
        type=Path,
        help="Path to the `gh api releases/tags/<tag>` JSON payload, or - for stdin",
    )
    parser.add_argument(
        "--expected-count",
        type=int,
        default=DEFAULT_EXPECTED_COUNT,
        help=f"Minimum asset total (default: {DEFAULT_EXPECTED_COUNT})",
    )
    args = parser.parse_args(argv)

    try:
        evidence = verify_release_assets(_load_payload(args.payload), args.version, args.expected_count)
    except ReleaseAssetError as error:
        print(f"::error::{error}")
        return 1

    print(f"{args.tag}: {evidence['asset_count']} assets verified")
    for group in evidence["groups"]:
        print(f"  - {group['group']}: {group['matched']}/{group['required']}")
    if evidence["unexpected"]:
        print("::warning::Release carries unexpected extra assets: " + ", ".join(evidence["unexpected"]))
    return 0


if __name__ == "__main__":
    sys.exit(main())
