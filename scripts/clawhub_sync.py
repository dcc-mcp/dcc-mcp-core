r"""Publish listed skills to ClawHub (https://clawhub.ai/)."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import time
from typing import Any
from urllib.error import HTTPError
from urllib.parse import quote
from urllib.parse import urlencode
from urllib.request import Request
from urllib.request import urlopen

import dcc_mcp_core

REPO_ROOT = Path(__file__).resolve().parent.parent
MANIFEST = REPO_ROOT / ".github" / "clawhub-skills.json"
DEFAULT_CLI = os.environ.get("CLAWHUB_CLI_PACKAGE", "clawhub@0.17.0")
CLAWHUB_API_BASE = "https://clawhub.ai/api/v1"
CLAWHUB_LICENSE = "MIT-0"
MAX_RETRIES = 3
VERSION_EXISTS_RE = re.compile(r"\bVersion(?:\s+\S+)?\s+already exists\b")
RETRYABLE_RE = re.compile(
    r"\b(?:Embedding failed|Please try again|reset in \d+s)\b",
    re.IGNORECASE,
)
RESET_IN_RE = re.compile(r"\breset in (\d+)s\b", re.IGNORECASE)
REQUIRED_PUBLIC_FILES = ("SKILL.md", "agents/openai.yaml")


def parse_args() -> argparse.Namespace:
    """Parse CLI flags for manifest path and dry-run mode."""
    parser = argparse.ArgumentParser(description="Publish skills from clawhub-skills.json")
    parser.add_argument(
        "--manifest",
        type=Path,
        default=MANIFEST,
        help="JSON manifest: [{path, slug, owner}]",
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
    """Load [{path, slug, owner}, ...] entries from the JSON manifest."""
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


def is_retryable(proc: subprocess.CompletedProcess[str]) -> bool:
    """Return True when ClawHub output indicates a transient retryable failure."""
    output = "\n".join(part for part in (proc.stdout, proc.stderr) if part)
    return RETRYABLE_RE.search(output) is not None


def retry_delay(proc: subprocess.CompletedProcess[str], attempt: int) -> int:
    """Return a bounded retry delay, honoring a server-provided reset window."""
    output = "\n".join(part for part in (proc.stdout, proc.stderr) if part)
    match = RESET_IN_RE.search(output)
    if match is not None:
        return min(int(match.group(1)) + 1, 120)
    return 2**attempt


def http_retry_delay(error: HTTPError, attempt: int) -> int:
    """Honor numeric HTTP Retry-After while keeping waits bounded."""
    raw = error.headers.get("Retry-After") if error.headers is not None else None
    try:
        return min(max(int(raw), 1), 120)
    except (TypeError, ValueError):
        return 2**attempt


def file_sha256(path: Path) -> str:
    """Hash one local publish artifact without loading it fully into memory."""
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def published_skill_release(slug: str, owner: str, version: str) -> dict[str, Any]:
    """Read one owner-qualified public version record from ClawHub."""
    query = urlencode({"owner": owner})
    url = f"{CLAWHUB_API_BASE}/skills/{quote(slug, safe='')}/versions/{quote(version, safe='')}?{query}"
    request = Request(
        url,
        headers={
            "Accept": "application/json",
            "User-Agent": "dcc-mcp-core-clawhub-sync",
        },
    )
    with urlopen(request, timeout=20) as response:
        payload = json.load(response)
    published = payload.get("version") if isinstance(payload, dict) else None
    if not isinstance(published, dict):
        raise ValueError(f"ClawHub response has no version record: {url}")
    return published


def published_skill_version(slug: str, owner: str, version: str) -> str:
    """Read one owner-qualified public version number from ClawHub."""
    published = published_skill_release(slug, owner, version)
    published_version = published.get("version") if isinstance(published, dict) else None
    if not isinstance(published_version, str) or not published_version.strip():
        raise ValueError(f"ClawHub response has no version.version for @{owner}/{slug}@{version}")
    return published_version.strip()


def public_file_mismatches(published: dict[str, Any], skill_dir: Path) -> list[str]:
    """Return missing or hash-mismatched required public Skill artifacts."""
    raw_files = published.get("files")
    files = raw_files if isinstance(raw_files, list) else []
    remote_hashes = {
        item["path"]: item["sha256"].lower()
        for item in files
        if isinstance(item, dict) and isinstance(item.get("path"), str) and isinstance(item.get("sha256"), str)
    }
    mismatches: list[str] = []
    for relative in REQUIRED_PUBLIC_FILES:
        local_path = skill_dir / relative
        if not local_path.is_file():
            mismatches.append(f"local file missing: {relative}")
            continue
        remote_hash = remote_hashes.get(relative)
        if remote_hash is None:
            mismatches.append(f"public file missing: {relative}")
        elif remote_hash != file_sha256(local_path):
            mismatches.append(f"public file hash mismatch: {relative}")
    return mismatches


def verify_published_version(
    slug: str,
    owner: str,
    expected_version: str,
    skill_dir: Path | None = None,
) -> bool:
    """Require the expected version and key artifacts on the public API."""
    last_error = "public version was not checked"
    for attempt in range(1, MAX_RETRIES + 1):
        retry_wait: int | None = None
        try:
            published = published_skill_release(slug, owner, expected_version)
            raw_version = published.get("version")
            actual_version = raw_version.strip() if isinstance(raw_version, str) else ""
            if actual_version == expected_version:
                mismatches = public_file_mismatches(published, skill_dir) if skill_dir is not None else []
                if not mismatches:
                    print(f"Verified @{owner}/{slug}@{expected_version} on the public ClawHub API.")
                    return True
                last_error = "; ".join(mismatches)
            else:
                last_error = f"public endpoint returned {actual_version or '<missing>'}, expected {expected_version}"
        except HTTPError as exc:
            last_error = str(exc)
            retry_wait = http_retry_delay(exc, attempt)
        except (OSError, ValueError) as exc:
            last_error = str(exc)

        if attempt < MAX_RETRIES:
            wait = retry_wait if retry_wait is not None else 2**attempt
            print(
                f"ClawHub public verification pending for @{owner}/{slug}@{expected_version}; "
                f"retrying in {wait}s (attempt {attempt}/{MAX_RETRIES}) ...",
                flush=True,
            )
            time.sleep(wait)

    print(
        f"ClawHub public verification failed for @{owner}/{slug}@{expected_version}: {last_error}",
        file=sys.stderr,
    )
    return False


def publish_one(
    entry: dict[str, Any],
    *,
    dry_run: bool,
    cli: str,
) -> int:
    """Validate and publish one manifest entry; return process exit code."""
    rel = entry.get("path")
    slug = entry.get("slug")
    owner = entry.get("owner")
    if not rel or not slug or not owner:
        print(f"invalid manifest entry (need path + slug + owner): {entry}", file=sys.stderr)
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
        "--owner",
        str(owner),
        "--no-input",
    )
    if dry_run:
        print("DRY-RUN:", " ".join(cmd))
        return 0

    print(f"Publishing {slug}@{version} from {skill_dir} ...", flush=True)
    for attempt in range(1, MAX_RETRIES + 1):
        proc = subprocess.run(cmd, check=False, capture_output=True, text=True)
        print_completed_process_output(proc)
        if proc.returncode == 0:
            return 0 if verify_published_version(str(slug), str(owner), version, skill_dir) else 1
        if version_already_exists(proc):
            print(f"{slug}@{version} already exists on ClawHub; skipping.")
            return 0 if verify_published_version(str(slug), str(owner), version, skill_dir) else 1
        if attempt < MAX_RETRIES and is_retryable(proc):
            wait = retry_delay(proc, attempt)
            print(
                f"Transient ClawHub error for {slug}@{version}; "
                f"retrying in {wait}s (attempt {attempt}/{MAX_RETRIES}) ...",
                flush=True,
            )
            time.sleep(wait)
        else:
            return int(proc.returncode)
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
