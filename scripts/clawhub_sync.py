r"""Publish listed skills to ClawHub (https://clawhub.ai/)."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
from pathlib import PurePosixPath
import re
import shutil
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
DEFAULT_CLI = os.environ.get("CLAWHUB_CLI_PACKAGE", "clawhub@0.23.1")
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
# ClawHub creates this asynchronously and excludes it from the source bundle fingerprint.
GENERATED_PUBLIC_FILES = {"skill-card.md"}
STABLE_SEMVER_RE = re.compile(r"^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$")
# Mirrors clawhub@0.23.1 packages/clawhub/src/skills.ts::buildSkillFingerprint.
NODE_FINGERPRINT_SCRIPT = r"""
const crypto = require("node:crypto");
const fs = require("node:fs");
const files = JSON.parse(fs.readFileSync(0, "utf8"));
const normalized = files
  .filter((file) => Boolean(file.path) && Boolean(file.sha256))
  .map((file) => ({ path: file.path, sha256: file.sha256 }))
  .sort((a, b) => a.path.localeCompare(b.path));
const payload = normalized.map((file) => `${file.path}:${file.sha256}`).join("\n");
process.stdout.write(crypto.createHash("sha256").update(payload).digest("hex"));
"""


def parse_args() -> argparse.Namespace:
    """Parse CLI flags for manifest path and dry-run mode."""
    parser = argparse.ArgumentParser(description="Publish skills from clawhub-skills.json")
    parser.add_argument(
        "--manifest",
        type=Path,
        default=MANIFEST,
        help="JSON manifest: [{path, slug, owner, version}]",
    )
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="Validate and print publish commands without uploading",
    )
    parser.add_argument(
        "--cli",
        default=DEFAULT_CLI,
        help="npm package for clawhub CLI (default: clawhub@0.23.1)",
    )
    return parser.parse_args()


def load_manifest(path: Path) -> list[dict[str, Any]]:
    """Load [{path, slug, owner, version}, ...] entries from the JSON manifest."""
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
    requested = os.environ.get("NPX", "npx")
    npx = shutil.which(requested) or requested
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


def clawhub_file_fingerprint(file_hashes: dict[str, str]) -> str:
    """Reproduce ClawHub 0.23.1's locale-aware buildSkillFingerprint."""
    requested = os.environ.get("NODE", "node").strip() or "node"
    node = shutil.which(requested) or requested
    files = [{"path": path, "sha256": digest} for path, digest in file_hashes.items()]
    try:
        proc = subprocess.run(
            [node, "-e", NODE_FINGERPRINT_SCRIPT],
            input=json.dumps(files, separators=(",", ":")),
            check=False,
            capture_output=True,
            text=True,
            timeout=20,
        )
    except (OSError, subprocess.TimeoutExpired) as exc:
        raise ValueError(f"could not run Node.js fingerprint helper: {exc}") from exc
    fingerprint = proc.stdout.strip().lower()
    if proc.returncode != 0:
        detail = proc.stderr.strip() or f"Node.js exited with code {proc.returncode}"
        raise ValueError(f"ClawHub fingerprint helper failed: {detail}")
    if re.fullmatch(r"[0-9a-f]{64}", fingerprint) is None:
        raise ValueError(f"ClawHub fingerprint helper returned invalid output: {fingerprint!r}")
    return fingerprint


def published_skill_release(slug: str, owner: str, version: str) -> dict[str, Any]:
    """Read one owner-qualified public version record from ClawHub."""
    query = urlencode({"ownerHandle": owner})
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


def published_file_mismatches(
    published: dict[str, Any],
    skill_dir: Path,
    *,
    location: str,
    expected_file_count: int | None = None,
    expected_fingerprint: str | None = None,
) -> list[str]:
    """Compare ClawHub's authoritative published file set with local bytes."""
    raw_files = published.get("files")
    files = raw_files if isinstance(raw_files, list) else []
    source_files = [
        item
        for item in files
        if not (
            isinstance(item, dict)
            and isinstance(item.get("path"), str)
            and item["path"].strip().lower() in GENERATED_PUBLIC_FILES
        )
    ]
    mismatches: list[str] = []
    if expected_file_count is not None and len(source_files) != expected_file_count:
        mismatches.append(f"{location} file count {len(source_files)}, expected {expected_file_count}")
    remote_hashes: dict[str, str] = {}
    for item in source_files:
        if (
            not isinstance(item, dict)
            or not isinstance(item.get("path"), str)
            or not isinstance(item.get("sha256"), str)
        ):
            mismatches.append(f"{location} contains malformed file metadata")
            continue
        relative = item["path"]
        if relative in remote_hashes:
            mismatches.append(f"{location} contains duplicate file: {relative}")
            continue
        remote_hash = item["sha256"].lower()
        if re.fullmatch(r"[0-9a-f]{64}", remote_hash) is None:
            mismatches.append(f"{location} contains invalid sha256 for: {relative}")
            continue
        remote_hashes[relative] = remote_hash

    if expected_fingerprint is not None:
        try:
            actual_fingerprint = clawhub_file_fingerprint(remote_hashes)
        except ValueError as exc:
            mismatches.append(f"{location} fingerprint validation failed: {exc}")
        else:
            if actual_fingerprint != expected_fingerprint:
                mismatches.append(f"{location} fingerprint {actual_fingerprint}, expected {expected_fingerprint}")

    for relative in REQUIRED_PUBLIC_FILES:
        if relative not in remote_hashes:
            mismatches.append(f"{location} file missing: {relative}")
    for relative, remote_hash in remote_hashes.items():
        posix_path = PurePosixPath(relative)
        parts = posix_path.parts
        if "\\" in relative or posix_path.is_absolute() or not parts or any(part in {"", ".", ".."} for part in parts):
            mismatches.append(f"{location} file has unsafe path: {relative}")
            continue
        local_path = skill_dir.joinpath(*parts)
        current = skill_dir
        has_symlink = False
        for part in parts:
            current = current / part
            if current.is_symlink():
                has_symlink = True
                break
        if has_symlink:
            mismatches.append(f"local file is a symbolic link: {relative}")
            continue
        try:
            local_path.resolve().relative_to(skill_dir.resolve())
        except ValueError:
            mismatches.append(f"local file escapes Skill root: {relative}")
            continue
        if not local_path.is_file():
            mismatches.append(f"local file missing: {relative}")
        elif remote_hash != file_sha256(local_path):
            mismatches.append(f"{location} file hash mismatch: {relative}")
    return mismatches


def public_file_mismatches(
    published: dict[str, Any],
    skill_dir: Path,
    expected_file_count: int | None = None,
    expected_fingerprint: str | None = None,
) -> list[str]:
    """Return mismatches between the public release and local Skill tree."""
    return published_file_mismatches(
        published,
        skill_dir,
        location="public",
        expected_file_count=expected_file_count,
        expected_fingerprint=expected_fingerprint,
    )


def publish_command(cli: str, skill_dir: Path, slug: str, owner: str, version: str) -> list[str]:
    """Build the pinned, non-interactive ClawHub Skill publish command."""
    return npx_cmd(
        cli,
        "--no-input",
        "skill",
        "publish",
        str(skill_dir),
        "--slug",
        slug,
        "--version",
        version,
        "--owner",
        owner,
        "--json",
    )


def preview_publish_metadata(
    cmd: list[str],
    *,
    slug: str,
    version: str,
    announce: bool = False,
) -> tuple[int, str] | None:
    """Return ClawHub's authoritative file count and content fingerprint."""
    preview_cmd = [*cmd, "--dry-run"]
    if announce:
        print("DRY-RUN:", " ".join(preview_cmd), flush=True)
    proc = subprocess.run(preview_cmd, check=False, capture_output=True, text=True)
    if announce or proc.returncode != 0:
        print_completed_process_output(proc)
    if proc.returncode != 0:
        return None
    try:
        payload = json.loads(proc.stdout)
        if not isinstance(payload, dict) or payload.get("ok") is not True:
            raise ValueError("preview response is not a successful JSON object")
        if payload.get("status") not in {"would-publish", "unchanged"}:
            raise ValueError(f"preview has unexpected status: {payload.get('status')!r}")
        if payload.get("slug") != slug or payload.get("version") != version:
            raise ValueError(
                f"preview identity mismatch: slug={payload.get('slug')!r}, version={payload.get('version')!r}"
            )
        file_count = payload.get("fileCount")
        fingerprint = payload.get("fingerprint")
        if not isinstance(file_count, int) or file_count < len(REQUIRED_PUBLIC_FILES):
            raise ValueError(f"preview has invalid fileCount: {file_count!r}")
        if not isinstance(fingerprint, str) or re.fullmatch(r"[0-9a-f]{64}", fingerprint) is None:
            raise ValueError(f"preview has invalid fingerprint: {fingerprint!r}")
    except (json.JSONDecodeError, ValueError) as exc:
        print(f"ClawHub publish preview validation failed for {slug}@{version}: {exc}", file=sys.stderr)
        return None
    print(f"ClawHub publish preview selected {file_count} file(s) for {slug}@{version}.")
    return file_count, fingerprint


def owner_inspect_command(cli: str, slug: str, owner: str, version: str) -> list[str]:
    """Build the authenticated, owner-qualified release inspection command."""
    return npx_cmd(
        cli,
        "--no-input",
        "inspect",
        f"@{owner}/{slug}",
        "--version",
        version,
        "--files",
        "--json",
    )


def parse_owner_release(
    raw: str,
    *,
    slug: str,
    owner: str,
    version: str,
) -> tuple[dict[str, Any], str]:
    """Parse and identity-check one authenticated ClawHub inspect response."""
    payload = json.loads(raw)
    if not isinstance(payload, dict):
        raise ValueError("ClawHub inspect response is not a JSON object")
    moderation = payload.get("moderation")
    moderation_label = "not reported"
    if isinstance(moderation, dict):
        raw_verdict = moderation.get("verdict")
        verdict = str(raw_verdict).strip().lower() if raw_verdict is not None else ""
        is_malware_blocked = moderation.get("isMalwareBlocked") is True
        if is_malware_blocked or verdict == "malicious":
            reasons = moderation.get("reasonCodes")
            raise ValueError(f"owner inspect reports a malware-blocked release: {reasons!r}")
        if verdict in {"clean", "suspicious"}:
            moderation_label = str(verdict)
        elif moderation.get("isSuspicious") is True:
            moderation_label = "suspicious"
    skill = payload.get("skill")
    publisher = payload.get("owner")
    published = payload.get("version")
    actual_slug = skill.get("slug") if isinstance(skill, dict) else None
    actual_owner = publisher.get("handle") if isinstance(publisher, dict) else None
    actual_version = published.get("version") if isinstance(published, dict) else None
    security = published.get("security") if isinstance(published, dict) else None
    if isinstance(security, dict):
        raw_security_status = security.get("status")
        security_status = str(raw_security_status).strip().lower() if raw_security_status is not None else ""
        if security_status in {"malicious", "blocked"}:
            raise ValueError(f"owner inspect reports security status {security_status!r}")
        if moderation_label == "not reported" and security_status:
            moderation_label = f"security:{security_status}"
    if actual_slug != slug:
        raise ValueError(f"owner inspect returned slug {actual_slug!r}, expected {slug!r}")
    if actual_owner != owner:
        raise ValueError(f"owner inspect returned owner {actual_owner!r}, expected {owner!r}")
    if actual_version != version:
        raise ValueError(f"owner inspect returned version {actual_version!r}, expected {version!r}")
    return published, moderation_label


def verify_owner_version(
    cli: str,
    slug: str,
    owner: str,
    expected_version: str,
    skill_dir: Path,
    expected_file_count: int,
    expected_fingerprint: str,
) -> bool:
    """Require the authenticated owner view to match every local Skill file."""
    cmd = owner_inspect_command(cli, slug, owner, expected_version)
    last_error = "owner-visible version was not checked"
    for attempt in range(1, MAX_RETRIES + 1):
        proc = subprocess.run(cmd, check=False, capture_output=True, text=True)
        if proc.returncode == 0:
            try:
                published, moderation_label = parse_owner_release(
                    proc.stdout,
                    slug=slug,
                    owner=owner,
                    version=expected_version,
                )
                mismatches = published_file_mismatches(
                    published,
                    skill_dir,
                    location="owner-visible",
                    expected_file_count=expected_file_count,
                    expected_fingerprint=expected_fingerprint,
                )
                if not mismatches:
                    print(
                        f"Verified @{owner}/{slug}@{expected_version} in the authenticated owner view "
                        f"(moderation: {moderation_label})."
                    )
                    return True
                last_error = "; ".join(mismatches)
            except (json.JSONDecodeError, ValueError) as exc:
                last_error = str(exc)
        else:
            output = "\n".join(part.strip() for part in (proc.stdout, proc.stderr) if part.strip())
            last_error = output or f"inspect exited with code {proc.returncode}"

        if attempt < MAX_RETRIES:
            wait = retry_delay(proc, attempt) if is_retryable(proc) else 2**attempt
            print(
                f"ClawHub owner verification pending for @{owner}/{slug}@{expected_version}; "
                f"retrying in {wait}s (attempt {attempt}/{MAX_RETRIES}) ...",
                flush=True,
            )
            time.sleep(wait)

    print(
        f"ClawHub owner verification failed for @{owner}/{slug}@{expected_version}: {last_error}",
        file=sys.stderr,
    )
    return False


def verify_published_version(
    slug: str,
    owner: str,
    expected_version: str,
    skill_dir: Path | None = None,
    expected_file_count: int | None = None,
    expected_fingerprint: str | None = None,
) -> bool | None:
    """Verify the public release, returning None while moderation hides it."""
    last_error = "public version was not checked"
    saw_public_record = False
    only_not_found = True
    for attempt in range(1, MAX_RETRIES + 1):
        retry_wait: int | None = None
        try:
            published = published_skill_release(slug, owner, expected_version)
            saw_public_record = True
            raw_version = published.get("version")
            actual_version = raw_version.strip() if isinstance(raw_version, str) else ""
            if actual_version == expected_version:
                mismatches = (
                    public_file_mismatches(
                        published,
                        skill_dir,
                        expected_file_count,
                        expected_fingerprint,
                    )
                    if skill_dir is not None
                    else []
                )
                if not mismatches:
                    print(f"Verified @{owner}/{slug}@{expected_version} on the public ClawHub API.")
                    return True
                last_error = "; ".join(mismatches)
            else:
                last_error = f"public endpoint returned {actual_version or '<missing>'}, expected {expected_version}"
        except HTTPError as exc:
            last_error = str(exc)
            retry_wait = http_retry_delay(exc, attempt)
            if exc.code != 404:
                only_not_found = False
        except OSError as exc:
            only_not_found = False
            last_error = str(exc)
        except ValueError as exc:
            only_not_found = False
            last_error = str(exc)

        if attempt < MAX_RETRIES:
            wait = retry_wait if retry_wait is not None else 2**attempt
            print(
                f"ClawHub public verification pending for @{owner}/{slug}@{expected_version}; "
                f"retrying in {wait}s (attempt {attempt}/{MAX_RETRIES}) ...",
                flush=True,
            )
            time.sleep(wait)

    if not saw_public_record and only_not_found:
        print(
            f"Owner-verified @{owner}/{slug}@{expected_version} is not publicly visible; "
            f"ClawHub review or moderation may still be pending ({last_error})."
        )
        return None
    print(
        f"ClawHub public verification failed for @{owner}/{slug}@{expected_version}: {last_error}",
        file=sys.stderr,
    )
    return False


def verify_uploaded_version(
    cli: str,
    slug: str,
    owner: str,
    expected_version: str,
    skill_dir: Path,
) -> bool:
    """Verify owner-visible bytes, then report public or pending-review state."""
    cmd = publish_command(cli, skill_dir, slug, owner, expected_version)
    preview = preview_publish_metadata(cmd, slug=slug, version=expected_version)
    if preview is None:
        return False
    expected_file_count, expected_fingerprint = preview
    if not verify_owner_version(
        cli,
        slug,
        owner,
        expected_version,
        skill_dir,
        expected_file_count,
        expected_fingerprint,
    ):
        return False
    public_status = verify_published_version(
        slug,
        owner,
        expected_version,
        skill_dir,
        expected_file_count,
        expected_fingerprint,
    )
    return public_status is not False


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
    declared_version = entry.get("version")
    if not rel or not slug or not owner or not declared_version:
        print(
            f"invalid manifest entry (need path + slug + owner + version): {entry}",
            file=sys.stderr,
        )
        return 1
    version = str(declared_version).strip()
    if STABLE_SEMVER_RE.fullmatch(version) is None:
        print(f"invalid manifest version for {slug}: {declared_version!r}", file=sys.stderr)
        return 1

    skill_dir = (REPO_ROOT / str(rel)).resolve()
    try:
        skill_dir.relative_to(REPO_ROOT)
    except ValueError:
        print(f"skill directory escapes repository root: {skill_dir}", file=sys.stderr)
        return 1
    if not skill_dir.is_dir():
        print(f"skill directory not found: {skill_dir}", file=sys.stderr)
        return 1

    metadata_version = skill_version(skill_dir)
    if metadata_version != version:
        print(
            f"manifest version {version!r} does not match SKILL.md version {metadata_version!r}: {skill_dir}",
            file=sys.stderr,
        )
        return 1
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

    cmd = publish_command(cli, skill_dir, str(slug), str(owner), version)
    if dry_run:
        preview = preview_publish_metadata(cmd, slug=str(slug), version=version, announce=True)
        return 0 if preview is not None else 1

    print(f"Publishing {slug}@{version} from {skill_dir} ...", flush=True)
    for attempt in range(1, MAX_RETRIES + 1):
        proc = subprocess.run(cmd, check=False, capture_output=True, text=True)
        print_completed_process_output(proc)
        if proc.returncode == 0:
            return 0 if verify_uploaded_version(cli, str(slug), str(owner), version, skill_dir) else 1
        if version_already_exists(proc):
            print(f"{slug}@{version} already exists on ClawHub; skipping.")
            return 0 if verify_uploaded_version(cli, str(slug), str(owner), version, skill_dir) else 1
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
