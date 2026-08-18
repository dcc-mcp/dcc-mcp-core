#!/usr/bin/env python3
"""Build an auditable PyPI release-cleanup plan from public metadata.

PyPI has no supported deletion API. Deleting releases is permanent, can break
exact pins, and must be performed by a project Owner in the official PyPI
management UI. This tool deliberately does not read browser credentials or
submit private Warehouse forms. It inventories the public JSON API, applies a
bounded selection policy, and optionally writes a manifest containing every
selected file name, size, URL, and SHA-256 digest for operator review.

Example:
    python scripts/release/pypi_cleanup.py --package dcc-mcp-core \
        --delete-below 0.19.90 \
        --keep-version 0.19.3 --keep-version 0.19.4 \
        --keep-version 0.19.45 --output-json cleanup-plan.json

"""

from __future__ import annotations

import argparse
from dataclasses import asdict
from dataclasses import dataclass
import json
from pathlib import Path
import re
import sys
import urllib.error
import urllib.parse
import urllib.request

PYPI_URL = "https://pypi.org"
USER_AGENT = "dcc-mcp-pypi-cleanup-plan/1.0 (+https://github.com/dcc-mcp/dcc-mcp-core)"


class CleanupError(Exception):
    """Fatal, user-facing cleanup planning failure."""


@dataclass(frozen=True)
class DistributionFile:
    """One immutable distribution file reported by the PyPI JSON API."""

    filename: str
    size: int
    sha256: str
    url: str


@dataclass(frozen=True)
class ReleaseRecord:
    """Public files and total storage for one release."""

    version: str
    files: tuple[DistributionFile, ...]

    @property
    def total_bytes(self) -> int:
        """Return the release's total stored bytes."""
        return sum(item.size for item in self.files)


@dataclass(frozen=True)
class ProjectState:
    """Public PyPI project state used to construct a cleanup plan."""

    package: str
    latest_version: str
    releases: dict[str, ReleaseRecord]

    @property
    def total_bytes(self) -> int:
        """Return the project's total stored bytes."""
        return sum(release.total_bytes for release in self.releases.values())


def version_key(version: str) -> tuple:
    """Return a deterministic ordering key for this project's release tags."""
    head = re.split(r"[-+]", version)[0]
    parts = [part for part in re.split(r"[._]", head) if part]
    key = []
    for part in parts:
        key.append((0, int(part)) if part.isdigit() else (1, 0, part))
    return tuple(key)


def select_versions(
    versions: list[str],
    delete_below: str | None,
    delete_matching: str | None,
    exclude_matching: str | None,
    max_deletes: int | None,
    keep_versions: set[str] | None = None,
) -> list[str]:
    """Apply a cleanup policy and return selected versions in order."""
    kept = keep_versions or set()
    below_key = version_key(delete_below) if delete_below else None
    match_re = re.compile(delete_matching) if delete_matching else None
    exclude_re = re.compile(exclude_matching) if exclude_matching else None
    selected = []
    for version in versions:
        if version in kept:
            continue
        if below_key is not None and version_key(version) >= below_key:
            continue
        if match_re is not None and not match_re.fullmatch(version):
            continue
        if exclude_re is not None and exclude_re.fullmatch(version):
            continue
        selected.append(version)
    selected.sort(key=version_key)
    return selected[:max_deletes] if max_deletes is not None else selected


def _distribution_file(payload: dict) -> DistributionFile:
    digests = payload.get("digests") or {}
    return DistributionFile(
        filename=str(payload.get("filename") or ""),
        size=int(payload.get("size") or 0),
        sha256=str(digests.get("sha256") or ""),
        url=str(payload.get("url") or ""),
    )


def parse_project_state(package: str, payload: dict) -> ProjectState:
    """Validate a project JSON payload and convert it into typed records."""
    releases_payload = payload.get("releases")
    latest = payload.get("info", {}).get("version")
    if not isinstance(releases_payload, dict) or not isinstance(latest, str) or not latest:
        raise CleanupError("public PyPI response is missing release metadata")
    releases = {}
    for version, files_payload in releases_payload.items():
        if not isinstance(files_payload, list):
            raise CleanupError(f"release {version!r} has an invalid files payload")
        files = tuple(_distribution_file(item) for item in files_payload if isinstance(item, dict))
        releases[str(version)] = ReleaseRecord(version=str(version), files=files)
    return ProjectState(package=package, latest_version=latest, releases=releases)


def fetch_project_state(package: str) -> ProjectState:
    """Read the current project inventory from PyPI's public JSON API."""
    encoded_package = urllib.parse.quote(package, safe="")
    request = urllib.request.Request(
        f"{PYPI_URL}/pypi/{encoded_package}/json",
        headers={"User-Agent": USER_AGENT},
    )
    try:
        with urllib.request.urlopen(request, timeout=60) as response:
            payload = json.loads(response.read())
    except (urllib.error.URLError, json.JSONDecodeError) as exc:
        raise CleanupError(f"could not read public PyPI state for {package!r}: {exc}") from exc
    return parse_project_state(package, payload)


def build_plan(state: ProjectState, selected_versions: list[str], kept_versions: set[str]) -> dict:
    """Return a stable JSON-serializable cleanup manifest."""
    selected = []
    for version in selected_versions:
        release = state.releases[version]
        selected.append(
            {
                "version": version,
                "total_bytes": release.total_bytes,
                "files": [asdict(item) for item in release.files],
            }
        )
    selected_bytes = sum(item["total_bytes"] for item in selected)
    return {
        "schema_version": 1,
        "package": state.package,
        "source": f"{PYPI_URL}/pypi/{urllib.parse.quote(state.package, safe='')}/json",
        "management_url": f"{PYPI_URL}/manage/project/{urllib.parse.quote(state.package, safe='')}/releases/",
        "latest_version": state.latest_version,
        "release_count": len(state.releases),
        "current_total_bytes": state.total_bytes,
        "selected_release_count": len(selected),
        "selected_total_bytes": selected_bytes,
        "projected_total_bytes": state.total_bytes - selected_bytes,
        "kept_versions": sorted(kept_versions, key=version_key),
        "selected_releases": selected,
        "warning": "Deletion is permanent. Execute only in the official PyPI Owner management UI.",
    }


def _print_plan(plan: dict) -> None:
    selected = plan["selected_releases"]
    selected_bytes = plan["selected_total_bytes"]
    print(
        f"Package: {plan['package']} — {plan['release_count']} release(s), {plan['current_total_bytes']} stored byte(s)"
    )
    print(f"Selected: {len(selected)} release(s), {selected_bytes} byte(s) ({selected_bytes / (1024**3):.3f} GiB)")
    print(f"Projected storage: {plan['projected_total_bytes']} byte(s)")
    if plan["kept_versions"]:
        print(f"Explicitly kept: {', '.join(plan['kept_versions'])}")
    for release in selected:
        print(
            f"  - {release['version']} ({release['total_bytes']} bytes, {release['total_bytes'] / (1024**2):.2f} MiB)"
        )
    print(f"Review and execute manually: {plan['management_url']}")
    print("DRY RUN — this tool never deletes releases or reads PyPI credentials.")


def main(argv: list[str] | None = None) -> int:
    """Build and print a public-data-only cleanup plan."""
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--package", required=True, help="PyPI project name")
    parser.add_argument("--delete-below", metavar="VERSION", help="select releases strictly older than VERSION")
    parser.add_argument("--delete-matching", metavar="REGEX", help="select releases whose version fully matches REGEX")
    parser.add_argument("--exclude-matching", metavar="REGEX", help="never select releases matching REGEX")
    parser.add_argument(
        "--keep-version", action="append", default=[], help="never select this exact version; repeatable"
    )
    parser.add_argument("--max-deletes", type=int, metavar="N", help="cap the number of selected releases")
    parser.add_argument("--output-json", type=Path, metavar="PATH", help="write the complete cleanup manifest")
    args = parser.parse_args(argv)

    if not (args.delete_below or args.delete_matching):
        parser.error("one of --delete-below / --delete-matching is required")
    if args.max_deletes is not None and args.max_deletes <= 0:
        parser.error("--max-deletes must be positive")

    state = fetch_project_state(args.package)
    kept_versions = set(args.keep_version)
    unknown_kept = sorted(kept_versions - set(state.releases), key=version_key)
    if unknown_kept:
        raise CleanupError(f"explicitly kept versions are absent from PyPI: {unknown_kept}")
    selected = select_versions(
        list(state.releases),
        delete_below=args.delete_below,
        delete_matching=args.delete_matching,
        exclude_matching=args.exclude_matching,
        max_deletes=args.max_deletes,
        keep_versions=kept_versions,
    )
    if state.latest_version in selected:
        raise CleanupError(f"refusing to select current latest release {state.latest_version!r}")
    if not selected:
        print("Nothing matches the cleanup policy; nothing to do.")
        return 0

    plan = build_plan(state, selected, kept_versions)
    _print_plan(plan)
    if args.output_json:
        args.output_json.write_text(json.dumps(plan, indent=2, sort_keys=True) + "\n", encoding="utf-8")
        print(f"Manifest written to: {args.output_json}")
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except CleanupError as exc:
        print(f"::error::{exc}", file=sys.stderr)
        sys.exit(1)
