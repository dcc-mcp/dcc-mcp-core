r"""Publish listed skills to ClawHub (https://clawhub.ai/)."""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import sys
from typing import Any

import dcc_mcp_core

REPO_ROOT = Path(__file__).resolve().parent.parent
MANIFEST = REPO_ROOT / ".github" / "clawhub-skills.json"
DEFAULT_CLI = os.environ.get("CLAWHUB_CLI_PACKAGE", "clawhub@0.17.0")
CLAWHUB_LICENSE = "MIT-0"
VERSION_EXISTS_RE = re.compile(r"\bVersion(?:\s+\S+)?\s+already exists\b")


def parse_args() -> argparse.Namespace:
    """Parse CLI flags for manifest path and dry-run mode."""
    parser = argparse.ArgumentParser(description="Publish skills from clawhub-skills.json")
    parser.add_argument(
        "--manifest",
        type=Path,
        default=MANIFEST,
        help="JSON manifest: [{path, slug}]",
    )
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="Validate and print publish commands without uploading",
    )
    parser.add_argument(
        "--cli",
        default=DEFAULT_CLI,
        help="npm package for clawhub CLI (default: clawhub@0.17.0)",
    )
    return parser.parse_args()


def load_manifest(path: Path) -> list[dict[str, Any]]:
    """Load [{path, slug}, ...] entries from the JSON manifest."""
    data = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(data, list):
        raise ValueError(f"manifest must be a JSON array: {path}")
    return data


def skill_version(skill_dir: Path) -> str:
    """Return version string from SKILL.md metadata."""
    meta = dcc_mcp_core.parse_skill_md(str(skill_dir))
    if meta is None:
        raise ValueError(f"failed to parse SKILL.md: {skill_dir}")
    version = (meta.version or "").strip()
    if not version:
        raise ValueError(f"missing version in SKILL.md metadata: {skill_dir}")
    return version


def skill_license(skill_dir: Path) -> str:
    """Return the top-level SKILL.md license identifier."""
    skill_md = skill_dir / "SKILL.md"
    lines = skill_md.read_text(encoding="utf-8").splitlines()
    if not lines or lines[0].strip() != "---":
        raise ValueError(f"missing YAML frontmatter in {skill_md}")
    for line in lines[1:]:
        stripped = line.strip()
        if stripped == "---":
            break
        if stripped.startswith("license:"):
            return stripped.split(":", 1)[1].strip().strip("'\"")
    raise ValueError(f"missing top-level license in {skill_md}")


def npx_cmd(cli: str, *args: str) -> list[str]:
    """Build an npx invocation argv list."""
    npx = os.environ.get("NPX", "npx")
    return [npx, cli, *args]


def print_completed_process_output(proc: subprocess.CompletedProcess[str]) -> None:
    """Forward captured child-process output to the current process streams."""
    if proc.stdout:
        print(proc.stdout, end="")
    if proc.stderr:
        print(proc.stderr, end="", file=sys.stderr)


def version_already_exists(proc: subprocess.CompletedProcess[str]) -> bool:
    """Return True when ClawHub reports that the immutable version exists."""
    output = "\n".join(part for part in (proc.stdout, proc.stderr) if part)
    return VERSION_EXISTS_RE.search(output) is not None


def publish_one(
    entry: dict[str, Any],
    *,
    dry_run: bool,
    cli: str,
) -> int:
    """Validate and publish one manifest entry; return process exit code."""
    rel = entry.get("path")
    slug = entry.get("slug")
    if not rel or not slug:
        print(f"invalid manifest entry (need path + slug): {entry}", file=sys.stderr)
        return 1

    skill_dir = (REPO_ROOT / str(rel)).resolve()
    if not skill_dir.is_dir():
        print(f"skill directory not found: {skill_dir}", file=sys.stderr)
        return 1

    version = skill_version(skill_dir)
    license_id = skill_license(skill_dir)
    if license_id != CLAWHUB_LICENSE:
        print(
            f"ClawHub publishes skills under {CLAWHUB_LICENSE}; "
            f"set 'license: {CLAWHUB_LICENSE}' in {skill_dir / 'SKILL.md'} "
            f"(found {license_id!r}).",
            file=sys.stderr,
        )
        return 1

    report = dcc_mcp_core.validate_skill(str(skill_dir))
    if not report.is_clean:
        print(f"validate_skill failed for {skill_dir}:", file=sys.stderr)
        for issue in report.issues:
            print(f"  - {issue}", file=sys.stderr)
        return 1

    cmd = npx_cmd(
        cli,
        "publish",
        str(skill_dir),
        "--slug",
        str(slug),
        "--version",
        version,
        "--no-input",
    )
    if dry_run:
        print("DRY-RUN:", " ".join(cmd))
        return 0

    print(f"Publishing {slug}@{version} from {skill_dir} ...", flush=True)
    proc = subprocess.run(cmd, check=False, capture_output=True, text=True)
    print_completed_process_output(proc)
    if proc.returncode != 0 and version_already_exists(proc):
        print(f"{slug}@{version} already exists on ClawHub; skipping.")
        return 0
    return int(proc.returncode)


def main() -> int:
    """Publish every skill in the manifest."""
    args = parse_args()
    manifest_path = args.manifest.resolve()
    if not manifest_path.is_file():
        print(f"manifest not found: {manifest_path}", file=sys.stderr)
        return 1

    entries = load_manifest(manifest_path)
    if not entries:
        print("manifest is empty", file=sys.stderr)
        return 1

    rc = 0
    for entry in entries:
        rc = max(rc, publish_one(entry, dry_run=args.dry_run, cli=args.cli))
    return rc


if __name__ == "__main__":
    raise SystemExit(main())
