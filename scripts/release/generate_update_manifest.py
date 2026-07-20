#!/usr/bin/env python3
"""Generate the update manifest JSON for the release.

Produces a platform-specific manifest that maps binary names to their
version, download URL, and SHA-256 checksum. The gateway's update-check
endpoint fetches this manifest via ``DCC_MCP_UPDATE_MANIFEST_URL``.

Output: ``dcc-mcp-update-manifest-<platform>.json``
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import sys


def _github_error(message: str) -> None:
    print(f"::error::{message}", file=sys.stderr)


def _sha256(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        while True:
            chunk = f.read(8192)
            if not chunk:
                break
            h.update(chunk)
    return h.hexdigest()


def generate_manifest(
    *,
    version: str,
    platform: str,
    release_tag: str,
    repo: str,
    assets: dict[str, Path],
    out_dir: Path,
) -> Path:
    """Generate the update manifest file.

    Args:
        version: Release version (e.g. ``0.19.0``).
        platform: Platform label (e.g. ``linux-x86_64``).
        release_tag: GitHub release tag (e.g. ``v0.19.0``).
        repo: GitHub repository (e.g. ``dcc-mcp/dcc-mcp-core``).
        assets: Mapping of logical binary names to their filesystem paths.
        out_dir: Output directory for the manifest.

    Returns:
        Path to the generated manifest file.

    """
    manifest: dict[str, dict] = {}

    removed_helper = "dcc-mcp-capture-helper.exe"
    if any(path.name.lower() == removed_helper for path in assets.values()):
        raise ValueError(f"removed runtime asset is forbidden: {removed_helper}")

    for binary_name, asset_path in assets.items():
        if not asset_path.is_file():
            raise FileNotFoundError(f"Asset not found for {binary_name}: {asset_path}")

        asset_filename = asset_path.name
        download_url = f"https://github.com/{repo}/releases/download/{release_tag}/{asset_filename}"
        sha256_digest = _sha256(asset_path)

        manifest[binary_name] = {
            "version": version,
            "url": download_url,
            "sha256": sha256_digest,
            "release_notes": None,
        }

    out_dir.mkdir(parents=True, exist_ok=True)
    manifest_path = out_dir / f"dcc-mcp-update-manifest-{platform}.json"
    manifest_path.write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")

    return manifest_path


def main() -> int:
    """Run the update manifest generator."""
    parser = argparse.ArgumentParser()
    parser.add_argument("--version", required=True)
    parser.add_argument("--platform", required=True)
    parser.add_argument("--release-tag", required=True)
    parser.add_argument("--repo", default="dcc-mcp/dcc-mcp-core")
    parser.add_argument("--server-bin", type=Path, required=True)
    parser.add_argument("--cli-bin", type=Path, required=True)
    parser.add_argument("--ui-control-host", type=Path)
    parser.add_argument("--out-dir", type=Path, default=Path())
    args = parser.parse_args()

    assets = {
        "dcc-mcp-server": args.server_bin,
        "dcc-mcp-cli": args.cli_bin,
    }
    is_windows = args.platform.startswith("windows-")
    if is_windows and args.ui_control_host is None:
        _github_error("Windows update manifests require --ui-control-host")
        return 1
    if not is_windows and args.ui_control_host is not None:
        _github_error("Non-Windows update manifests must not include --ui-control-host")
        return 1
    if args.ui_control_host is not None:
        assets["dcc-mcp-ui-control-host"] = args.ui_control_host
    try:
        manifest_path = generate_manifest(
            version=args.version,
            platform=args.platform,
            release_tag=args.release_tag,
            repo=args.repo,
            assets=assets,
            out_dir=args.out_dir,
        )
    except Exception as exc:
        _github_error(str(exc))
        raise SystemExit(1) from exc

    manifest_output = manifest_path.as_posix()
    github_output = os.environ.get("GITHUB_OUTPUT")
    if github_output:
        with Path(github_output).open("a", encoding="utf-8") as fh:
            fh.write(f"manifest_path={manifest_output}\n")
    print(manifest_output)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
