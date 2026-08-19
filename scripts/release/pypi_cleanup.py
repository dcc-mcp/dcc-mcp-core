#!/usr/bin/env python3
"""Build an auditable PyPI release-cleanup plan from public metadata.

PyPI has no supported deletion API. Deleting releases is permanent, can break
exact pins, and must be performed by a project Owner in the official PyPI
management UI. This tool deliberately does not read browser credentials or
submit private Warehouse forms. It inventories the public JSON API, applies a
bounded selection policy, and optionally writes a manifest containing every
selected file name, size, URL, and SHA-256 digest for operator review. When a
GitHub repository is supplied, the tool fails closed unless every selected
PyPI file has an exact-name GitHub Release asset with the same size and digest.

Example:
    python scripts/release/pypi_cleanup.py --package dcc-mcp-core \
        --delete-below 0.19.90 \
        --keep-version 0.19.3 --keep-version 0.19.4 \
        --keep-version 0.19.45 \
        --max-deletes 83 --expect-selected-count 83 \
        --expect-selected-total-bytes 9271523910 \
        --github-repository dcc-mcp/dcc-mcp-core \
        --output-json cleanup-plan.json

Set GH_TOKEN or GITHUB_TOKEN only when GitHub's anonymous API rate limit is
insufficient. Tokens are used for read-only Release metadata and never stored.

"""

from __future__ import annotations

import argparse
from dataclasses import asdict
from dataclasses import dataclass
from datetime import datetime
from datetime import timezone
import hashlib
import json
import os
from pathlib import Path
import re
import sys
import urllib.error
import urllib.parse
import urllib.request

PYPI_URL = "https://pypi.org"
GITHUB_API_URL = "https://api.github.com"
USER_AGENT = "dcc-mcp-pypi-cleanup-plan/2.0 (+https://github.com/dcc-mcp/dcc-mcp-core)"


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


def _distribution_file(version: str, payload: dict) -> DistributionFile:
    digests = payload.get("digests") or {}
    filename = payload.get("filename")
    size = payload.get("size")
    sha256 = digests.get("sha256") if isinstance(digests, dict) else None
    url = payload.get("url")
    parsed_url = urllib.parse.urlparse(url) if isinstance(url, str) else None
    valid = (
        isinstance(filename, str)
        and bool(filename)
        and Path(filename).name == filename
        and isinstance(size, int)
        and not isinstance(size, bool)
        and size > 0
        and isinstance(sha256, str)
        and re.fullmatch(r"[0-9a-fA-F]{64}", sha256) is not None
        and parsed_url is not None
        and parsed_url.scheme == "https"
        and parsed_url.hostname == "files.pythonhosted.org"
    )
    if not valid:
        raise CleanupError(f"release {version!r} has invalid file evidence")
    return DistributionFile(
        filename=filename,
        size=size,
        sha256=sha256.lower(),
        url=url,
    )


def _utc_now() -> str:
    """Return a compact UTC timestamp for manifest freshness evidence."""
    return datetime.now(timezone.utc).replace(microsecond=0).isoformat().replace("+00:00", "Z")


def parse_project_state(package: str, payload: dict) -> ProjectState:
    """Validate a project JSON payload and convert it into typed records."""
    if not isinstance(payload, dict):
        raise CleanupError("public PyPI response is missing release metadata")
    releases_payload = payload.get("releases")
    info_payload = payload.get("info")
    latest = info_payload.get("version") if isinstance(info_payload, dict) else None
    if not isinstance(releases_payload, dict) or not isinstance(latest, str) or not latest:
        raise CleanupError("public PyPI response is missing release metadata")
    releases = {}
    for version, files_payload in releases_payload.items():
        if not isinstance(files_payload, list) or any(not isinstance(item, dict) for item in files_payload):
            raise CleanupError(f"release {version!r} has an invalid files payload")
        files = tuple(_distribution_file(str(version), item) for item in files_payload)
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


def _github_repository_parts(repository: str) -> tuple[str, str]:
    parts = repository.split("/")
    if len(parts) != 2 or not all(re.fullmatch(r"[A-Za-z0-9_.-]+", part) for part in parts):
        raise CleanupError("GitHub repository must use the OWNER/REPO form")
    return parts[0], parts[1]


def fetch_github_releases(repository: str) -> list[dict]:
    """Read every public GitHub Release needed for backup verification."""
    owner, repo = _github_repository_parts(repository)
    releases = []
    for page in range(1, 101):
        url = (
            f"{GITHUB_API_URL}/repos/{urllib.parse.quote(owner, safe='')}/"
            f"{urllib.parse.quote(repo, safe='')}/releases?per_page=100&page={page}"
        )
        headers = {
            "Accept": "application/vnd.github+json",
            "User-Agent": USER_AGENT,
            "X-GitHub-Api-Version": "2022-11-28",
        }
        token = os.environ.get("GH_TOKEN") or os.environ.get("GITHUB_TOKEN")
        if token:
            headers["Authorization"] = f"Bearer {token}"
        request = urllib.request.Request(url, headers=headers)
        try:
            with urllib.request.urlopen(request, timeout=60) as response:
                payload = json.loads(response.read())
        except (urllib.error.URLError, json.JSONDecodeError) as exc:
            raise CleanupError(f"could not read public GitHub Releases for {repository!r}: {exc}") from exc
        if not isinstance(payload, list) or any(not isinstance(item, dict) for item in payload):
            raise CleanupError(f"public GitHub response for {repository!r} has invalid release metadata")
        releases.extend(payload)
        if len(payload) < 100:
            return releases
    raise CleanupError(f"public GitHub Releases for {repository!r} exceeded the 100-page safety bound")


def verify_github_backup(plan: dict, repository: str, releases: list[dict]) -> dict:
    """Fail closed unless every selected PyPI file has an identical GitHub asset."""
    _github_repository_parts(repository)
    releases_by_tag = {}
    for release in releases:
        tag = release.get("tag_name")
        if isinstance(tag, str) and tag and tag not in releases_by_tag:
            releases_by_tag[tag] = release

    issues = []
    release_evidence = []
    verified_files = 0
    for selected in plan["selected_releases"]:
        tag = f"v{selected['version']}"
        release = releases_by_tag.get(tag)
        if release is None:
            issues.append(f"{tag}: release is missing")
            continue
        if release.get("draft") is True:
            issues.append(f"{tag}: release is still a draft")
            continue
        release_url = release.get("html_url")
        assets_payload = release.get("assets")
        if not isinstance(release_url, str) or not release_url.startswith("https://github.com/"):
            issues.append(f"{tag}: release URL is invalid")
            continue
        if not isinstance(assets_payload, list) or any(not isinstance(item, dict) for item in assets_payload):
            issues.append(f"{tag}: asset metadata is invalid")
            continue

        assets_by_name = {}
        for asset in assets_payload:
            name = asset.get("name")
            if isinstance(name, str) and name and name not in assets_by_name:
                assets_by_name[name] = asset

        asset_evidence = []
        for expected in selected["files"]:
            filename = expected["filename"]
            asset = assets_by_name.get(filename)
            if asset is None:
                issues.append(f"{tag}/{filename}: asset is missing")
                continue
            digest = asset.get("digest")
            size = asset.get("size")
            expected_digest = f"sha256:{expected['sha256']}"
            if size != expected["size"]:
                issues.append(f"{tag}/{filename}: size mismatch (PyPI {expected['size']}, GitHub {size!r})")
                continue
            if not isinstance(digest, str) or digest.lower() != expected_digest:
                issues.append(f"{tag}/{filename}: digest mismatch (PyPI {expected_digest}, GitHub {digest!r})")
                continue
            asset_id = asset.get("id")
            download_url = asset.get("browser_download_url")
            if not isinstance(asset_id, int) or asset_id <= 0:
                issues.append(f"{tag}/{filename}: asset id is invalid")
                continue
            if not isinstance(download_url, str) or not download_url.startswith("https://github.com/"):
                issues.append(f"{tag}/{filename}: asset download URL is invalid")
                continue
            asset_evidence.append(
                {
                    "filename": filename,
                    "asset_id": asset_id,
                    "size": size,
                    "sha256": expected["sha256"],
                    "download_url": download_url,
                }
            )
            verified_files += 1
        release_evidence.append(
            {
                "version": selected["version"],
                "tag": tag,
                "release_url": release_url,
                "assets": asset_evidence,
            }
        )

    if issues:
        raise CleanupError("GitHub backup verification failed: " + "; ".join(issues))
    return {
        "status": "verified",
        "verified_at": _utc_now(),
        "repository": repository,
        "source": f"{GITHUB_API_URL}/repos/{repository}/releases",
        "release_count": len(release_evidence),
        "file_count": verified_files,
        "releases": release_evidence,
    }


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
        "schema_version": 2,
        "generated_at": _utc_now(),
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
    backup = plan.get("github_backup")
    if backup:
        print(
            f"GitHub backup: {backup['file_count']} file(s) across "
            f"{backup['release_count']} release(s) verified in {backup['repository']}"
        )
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
    parser.add_argument(
        "--expect-selected-count",
        type=int,
        metavar="N",
        help="fail unless the unbounded cleanup policy selects exactly N releases",
    )
    parser.add_argument(
        "--expect-selected-total-bytes",
        type=int,
        metavar="N",
        help="fail unless the selected releases contain exactly N stored bytes",
    )
    parser.add_argument(
        "--github-repository",
        metavar="OWNER/REPO",
        help="fail unless every selected file has an identical public GitHub Release asset",
    )
    parser.add_argument("--output-json", type=Path, metavar="PATH", help="write the complete cleanup manifest")
    args = parser.parse_args(argv)

    if not (args.delete_below or args.delete_matching):
        parser.error("one of --delete-below / --delete-matching is required")
    if args.max_deletes is not None and args.max_deletes <= 0:
        parser.error("--max-deletes must be positive")
    if args.expect_selected_count is not None and args.expect_selected_count <= 0:
        parser.error("--expect-selected-count must be positive")
    if args.expect_selected_total_bytes is not None and args.expect_selected_total_bytes <= 0:
        parser.error("--expect-selected-total-bytes must be positive")

    state = fetch_project_state(args.package)
    kept_versions = set(args.keep_version)
    unknown_kept = sorted(kept_versions - set(state.releases), key=version_key)
    if unknown_kept:
        raise CleanupError(f"explicitly kept versions are absent from PyPI: {unknown_kept}")
    matching = select_versions(
        list(state.releases),
        delete_below=args.delete_below,
        delete_matching=args.delete_matching,
        exclude_matching=args.exclude_matching,
        max_deletes=None,
        keep_versions=kept_versions,
    )
    if args.expect_selected_count is not None and len(matching) != args.expect_selected_count:
        raise CleanupError(
            f"expected {args.expect_selected_count} selected release(s), found {len(matching)} before the delete cap"
        )
    selected = matching[: args.max_deletes] if args.max_deletes is not None else matching
    if args.expect_selected_count is not None and len(selected) != args.expect_selected_count:
        raise CleanupError(
            f"expected {args.expect_selected_count} selected release(s), but the delete cap retained {len(selected)}"
        )
    if state.latest_version in selected:
        raise CleanupError(f"refusing to select current latest release {state.latest_version!r}")
    if not selected:
        print("Nothing matches the cleanup policy; nothing to do.")
        return 0
    selected_bytes = sum(state.releases[version].total_bytes for version in selected)
    if args.expect_selected_total_bytes is not None and selected_bytes != args.expect_selected_total_bytes:
        raise CleanupError(f"expected {args.expect_selected_total_bytes} selected byte(s), found {selected_bytes}")

    plan = build_plan(state, selected, kept_versions)
    if args.github_repository:
        releases = fetch_github_releases(args.github_repository)
        plan["github_backup"] = verify_github_backup(plan, args.github_repository, releases)
    _print_plan(plan)
    if args.output_json:
        manifest_text = json.dumps(plan, indent=2, sort_keys=True) + "\n"
        manifest_bytes = manifest_text.encode("utf-8")
        args.output_json.write_bytes(manifest_bytes)
        print(f"Manifest written to: {args.output_json}")
        print(f"Manifest SHA-256: {hashlib.sha256(manifest_bytes).hexdigest()}")
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except CleanupError as exc:
        print(f"::error::{exc}", file=sys.stderr)
        sys.exit(1)
