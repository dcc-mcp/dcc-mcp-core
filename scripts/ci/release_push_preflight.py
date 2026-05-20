#!/usr/bin/env python3
"""Classify push-triggered release workflow state before release-please runs."""

from __future__ import annotations

import json
import os
from pathlib import Path
import re
import subprocess
import sys
from typing import Any

SEMVER = r"[0-9]+(?:\.[0-9]+){2}(?:[-+][0-9A-Za-z.-]+)?"
TITLE_RE = re.compile(rf"^chore\(main\): release (?P<version>{SEMVER})$")


def fail(message: str) -> None:
    """Emit a GitHub Actions error and exit."""
    print(f"::error::{message}", file=sys.stderr)
    raise SystemExit(1)


def emit(outputs: dict[str, str]) -> None:
    """Write step outputs for GitHub Actions and echo them for logs."""
    output_path = os.environ.get("GITHUB_OUTPUT")
    lines = [f"{key}={value}" for key, value in outputs.items()]
    if output_path:
        with Path(output_path).open("a", encoding="utf-8") as handle:
            handle.write("\n".join(lines))
            handle.write("\n")
    for line in lines:
        print(line)


def load_manifest_version(path: Path) -> str:
    """Load the root package version from the release-please manifest."""
    manifest = json.loads(path.read_text(encoding="utf-8"))
    version = manifest.get(".")
    if not isinstance(version, str) or not version:
        fail('.release-please-manifest.json must contain a "." package version')
    return version


def associated_pulls(repo: str, sha: str) -> list[dict[str, Any]]:
    """Return pull requests associated with a commit SHA."""
    override = os.environ.get("DCC_RELEASE_PREFLIGHT_PULLS_JSON")
    if override is not None:
        return json.loads(override)
    result = subprocess.run(
        [
            "gh",
            "api",
            "-H",
            "Accept: application/vnd.github+json",
            f"repos/{repo}/commits/{sha}/pulls",
        ],
        check=False,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
    )
    if result.returncode != 0:
        fail(f"could not inspect associated pull requests: {result.stdout.strip()}")
    return json.loads(result.stdout or "[]")


def remote_release_exists(repo: str, tag_name: str) -> bool:
    """Return whether a GitHub Release already exists for the tag."""
    override = os.environ.get("DCC_RELEASE_PREFLIGHT_TAG_EXISTS")
    if override is not None:
        return override == "1"
    result = subprocess.run(
        ["gh", "release", "view", tag_name, "--repo", repo],
        check=False,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
    )
    if result.returncode == 0:
        return True
    if "release not found" in result.stdout.lower() or "not found" in result.stdout.lower():
        return False
    fail(f"could not check release {tag_name}: {result.stdout.strip()}")


def release_pr_from(pulls: list[dict[str, Any]]) -> dict[str, Any] | None:
    """Pick the release-please PR associated with the push commit, if any."""
    for pull in pulls:
        head_ref = str(pull.get("head", {}).get("ref", ""))
        title = str(pull.get("title", ""))
        if head_ref.startswith("release-please--branches--main") and TITLE_RE.match(title):
            return pull
    return None


def main() -> None:
    """Validate push-triggered release state and emit release decision hints."""
    repo = os.environ.get("GITHUB_REPOSITORY", "")
    sha = os.environ.get("GITHUB_SHA", "")
    if not repo or not sha:
        fail("GITHUB_REPOSITORY and GITHUB_SHA are required")

    manifest_version = load_manifest_version(Path(".release-please-manifest.json"))
    pull = release_pr_from(associated_pulls(repo, sha))
    if pull is None:
        emit(
            {
                "skip_release_please": "false",
                "release_created": "false",
                "tag_name": "",
                "version": "",
            }
        )
        return

    title = str(pull.get("title", ""))
    match = TITLE_RE.match(title)
    if match is None:
        emit({"skip_release_please": "false", "release_created": "false", "tag_name": "", "version": ""})
        return

    title_version = match.group("version")
    if title_version != manifest_version:
        number = pull.get("number", "<unknown>")
        fail(
            f"stale release PR #{number}: title has {title_version}, "
            f"manifest has {manifest_version}. Close/regenerate the release PR "
            "before merging so release-please does not create the wrong tag."
        )

    tag_name = f"v{manifest_version}"
    if remote_release_exists(repo, tag_name):
        print(f"::notice::Release {tag_name} already exists; treating this run as an asset backfill.")
        emit(
            {
                "skip_release_please": "true",
                "release_created": "true",
                "tag_name": tag_name,
                "version": manifest_version,
            }
        )
        return

    emit(
        {
            "skip_release_please": "false",
            "release_created": "false",
            "tag_name": "",
            "version": "",
        }
    )


if __name__ == "__main__":
    main()
