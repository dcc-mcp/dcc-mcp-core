#!/usr/bin/env python3
"""Gate a PyPI publish on the project's total-size budget.

PyPI enforces a default 10 GB total-size limit per project
(https://docs.pypi.org/project-management/storage-limits). When a project
is at the limit, the upload fails MID-STREAM: twine uploads distributions
one by one, so the release is left partially published. Both v0.19.0 and
v0.20.6 of dcc-mcp-core shipped only the cp37 wheels to PyPI this way; the
macOS abi3 wheel was rejected with '400 Project size too large', which
broke downstream consumers that resolve platform wheels from PyPI (for
example dcc-mcp-blender's addon packaging).

This preflight mirrors the 'skip-existing: true' behaviour of
pypa/gh-action-pypi-publish: artifacts whose filenames already exist on
PyPI are treated as skipped, and everything else counts against the budget.
It runs BEFORE the upload so a release either publishes completely or fails
loudly without touching PyPI at all.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path
import sys
from typing import Mapping
import urllib.request

DEFAULT_LIMIT_GB = 10
DEFAULT_WARN_HEADROOM_GB = 1.0
DEFAULT_JSON_URL = "https://pypi.org/pypi/{package}/json"
STORAGE_LIMITS_DOC = "https://docs.pypi.org/project-management/storage-limits"
MANAGE_RELEASES_URL = "https://pypi.org/manage/project/{package}/releases/"
GIB = 1024**3


class Artifact:
    """A single local distribution file that the publish step would upload."""

    def __init__(self, filename: str, size_bytes: int) -> None:
        self.filename = filename
        self.size_bytes = size_bytes


class SizeBudgetReport:
    """Projected PyPI storage state after the pending publish."""

    def __init__(
        self, package, limit_bytes, current_bytes, already_present, to_add, projected_bytes, warn_headroom_bytes
    ):
        self.package = package
        self.limit_bytes = limit_bytes
        self.current_bytes = current_bytes
        self.already_present = already_present
        self.to_add = to_add
        self.projected_bytes = projected_bytes
        self.warn_headroom_bytes = warn_headroom_bytes

    @property
    def over_budget(self) -> bool:
        return self.projected_bytes > self.limit_bytes

    @property
    def near_budget(self) -> bool:
        headroom = self.limit_bytes - self.projected_bytes
        return not self.over_budget and headroom <= self.warn_headroom_bytes


def parse_gb(value: str) -> float:
    """Parse a fractional gigabyte limit for argparse."""
    try:
        parsed = float(value)
    except ValueError as exc:
        raise argparse.ArgumentTypeError(f"expected a number of gigabytes, got {value!r}") from exc
    if parsed <= 0:
        raise argparse.ArgumentTypeError("limit must be greater than zero")
    return parsed


def fetch_pypi_files(package: str, json_url: str) -> dict:
    """Return {filename: size_bytes} for every file of *package* on PyPI."""
    url = json_url.format(package=package)
    try:
        with urllib.request.urlopen(url, timeout=60) as response:
            payload = json.loads(response.read())
    except Exception as exc:  # pragma: no cover - network path
        raise RuntimeError(f"could not fetch PyPI metadata for {package!r} from {url}: {exc}") from exc
    files = {}
    for release_files in payload.get("releases", {}).values():
        for file_info in release_files:
            name = str(file_info.get("filename", ""))
            size = int(file_info.get("size", 0))
            if name:
                files[name] = size
    return files


def collect_dist_artifacts(dist_dir: Path) -> list:
    """Return the wheels and sdists in *dist_dir* that twine would upload."""
    if not dist_dir.is_dir():
        raise FileNotFoundError(f"dist directory not found: {dist_dir}")
    artifacts = []
    for path in sorted(dist_dir.iterdir()):
        if not path.is_file():
            continue
        if not (path.name.endswith(".whl") or path.name.endswith(".tar.gz")):
            continue
        artifacts.append(Artifact(filename=path.name, size_bytes=path.stat().st_size))
    return artifacts


def build_report(
    package: str,
    pypi_files: Mapping[str, int],
    artifacts: list,
    limit_bytes: int,
    warn_headroom_bytes: int,
) -> SizeBudgetReport:
    """Project the storage state after uploading artifacts not yet on PyPI."""
    current_bytes = sum(pypi_files.values())
    already_present = []
    to_add = []
    for artifact in artifacts:
        if artifact.filename in pypi_files:
            already_present.append(artifact.filename)
        else:
            to_add.append(artifact)
    projected_bytes = current_bytes + sum(a.size_bytes for a in to_add)
    return SizeBudgetReport(
        package=package,
        limit_bytes=limit_bytes,
        current_bytes=current_bytes,
        already_present=already_present,
        to_add=to_add,
        projected_bytes=projected_bytes,
        warn_headroom_bytes=warn_headroom_bytes,
    )


def format_gib(size_bytes: int) -> str:
    """Format a byte count as a human-readable GiB value."""
    return f"{size_bytes / GIB:.2f} GiB"


def render_summary(report: SizeBudgetReport) -> list:
    """Return human-readable log lines describing the size projection."""
    lines = [
        f"PyPI project size budget: {report.package}",
        f"  limit:     {format_gib(report.limit_bytes)}",
        f"  current:   {format_gib(report.current_bytes)}",
        f"  already on PyPI (skipped): {len(report.already_present)} file(s)",
    ]
    for name in report.already_present:
        lines.append(f"    - {name}")
    lines.append(f"  adding:    {len(report.to_add)} file(s)")
    for artifact in report.to_add:
        lines.append(f"    - {artifact.filename} ({format_gib(artifact.size_bytes)})")
    lines.append(f"  projected: {format_gib(report.projected_bytes)}")
    if not report.to_add:
        lines.append("  nothing new to upload; PyPI already has every artifact")
    return lines


def render_failure(report: SizeBudgetReport) -> str:
    """Return the actionable ::error:: message for an over-budget publish."""
    projected = format_gib(report.projected_bytes)
    limit = format_gib(report.limit_bytes)
    manage = MANAGE_RELEASES_URL.format(package=report.package)
    return (
        f"::error::PyPI project {report.package!r} has no room for the pending "
        f"artifacts: projected {projected} exceeds the {limit} project size "
        "limit. Refusing to upload so the release is not left partially "
        f"published. Remediation: (1) free storage by deleting old releases at "
        f"{manage} (PyPI exposes no deletion API, so this is a maintainer "
        f"web-UI action) or request a limit increase per {STORAGE_LIMITS_DOC}; "
        "(2) re-run the Release workflow for the affected tag - the publish "
        "step uses skip-existing, so already-published files are skipped and "
        "only the missing distributions are uploaded."
    )


def render_warning(report: SizeBudgetReport) -> str:
    """Return the ::warning:: message for a publish that fits but is tight."""
    headroom = format_gib(report.limit_bytes - report.projected_bytes)
    limit = format_gib(report.limit_bytes)
    projected = format_gib(report.projected_bytes)
    return (
        f"::warning::PyPI project {report.package!r} will be within {headroom} "
        f"of its {limit} size limit after this publish (projected {projected}). "
        f"Plan to free storage or request a limit increase soon "
        f"({STORAGE_LIMITS_DOC})."
    )


def build_parser() -> argparse.ArgumentParser:
    """Build the CLI argument parser for the preflight check."""
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--package", required=True, help="PyPI project name to check")
    parser.add_argument("--dist-dir", required=True, help="directory holding the artifacts to publish")
    parser.add_argument(
        "--limit-gb",
        type=parse_gb,
        default=DEFAULT_LIMIT_GB,
        help="PyPI total-size limit in GiB (default: %(default)s)",
    )
    parser.add_argument(
        "--warn-headroom-gb",
        type=parse_gb,
        default=DEFAULT_WARN_HEADROOM_GB,
        help="warn when projected headroom is at most this many GiB (default: %(default)s)",
    )
    parser.add_argument(
        "--pypi-json-url",
        default=DEFAULT_JSON_URL,
        help="PyPI JSON API URL template with {package} placeholder",
    )
    return parser


def main(argv: list | None = None) -> int:
    """Run the preflight and exit 1 when the publish would exceed the budget."""
    parser = build_parser()
    args = parser.parse_args(argv)
    limit_bytes = int(args.limit_gb * GIB)
    warn_headroom_bytes = int(args.warn_headroom_gb * GIB)

    try:
        pypi_files = fetch_pypi_files(args.package, args.pypi_json_url)
        artifacts = collect_dist_artifacts(Path(args.dist_dir))
    except Exception as exc:  # pragma: no cover - network/fs error path
        print(f"::error::{exc}", file=sys.stderr)
        return 1

    report = build_report(
        package=args.package,
        pypi_files=pypi_files,
        artifacts=artifacts,
        limit_bytes=limit_bytes,
        warn_headroom_bytes=warn_headroom_bytes,
    )
    for line in render_summary(report):
        print(line)

    if report.over_budget:
        print(render_failure(report), file=sys.stderr)
        return 1
    if report.near_budget:
        print(render_warning(report), file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
