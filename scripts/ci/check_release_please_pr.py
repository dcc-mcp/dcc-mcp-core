#!/usr/bin/env python3
"""Validate release-please PR metadata before it can be merged."""

from __future__ import annotations

import json
import os
from pathlib import Path
import re
import subprocess
import sys

SEMVER = r"[0-9]+(?:\.[0-9]+){2}(?:[-+][0-9A-Za-z.-]+)?"
TITLE_RE = re.compile(rf"^chore\(main\): release (?P<version>{SEMVER})$")
CHANGELOG_RE = re.compile(rf"^## \[(?P<version>{SEMVER})\]")


def fail(message: str) -> None:
    """Emit a GitHub Actions error and exit."""
    print(f"::error::{message}", file=sys.stderr)
    raise SystemExit(1)


def first_changelog_version(path: Path) -> str | None:
    """Return the first release heading version from a changelog."""
    if not path.exists():
        return None
    for line in path.read_text(encoding="utf-8").splitlines():
        match = CHANGELOG_RE.match(line.strip())
        if match:
            return match.group("version")
    return None


def remote_tag_exists(repo: str, tag_name: str) -> bool:
    """Return whether a tag ref exists on GitHub."""
    result = subprocess.run(
        ["gh", "api", f"repos/{repo}/git/ref/tags/{tag_name}"],
        check=False,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
    )
    if result.returncode == 0:
        return True
    if "Not Found" in result.stdout or '"status":"404"' in result.stdout:
        return False
    fail(f"could not check remote tag {tag_name}: {result.stdout.strip()}")


def main() -> None:
    """Validate the current checkout against pull request metadata."""
    head_ref = os.environ.get("PR_HEAD_REF", "")
    title = os.environ.get("PR_TITLE", "").strip()
    if not head_ref.startswith("release-please--branches--main"):
        print("Not a release-please branch; skipping.")
        return

    manifest_path = Path(".release-please-manifest.json")
    if not manifest_path.exists():
        fail(".release-please-manifest.json is missing")
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    manifest_version = manifest.get(".")
    if not isinstance(manifest_version, str) or not manifest_version:
        fail('.release-please-manifest.json must contain a "." package version')

    title_match = TITLE_RE.match(title)
    if not title_match:
        fail(f"release PR title must match 'chore(main): release X.Y.Z', got: {title!r}")
    title_version = title_match.group("version")
    if title_version != manifest_version:
        fail(
            "release PR title/version drift: "
            f"title has {title_version}, manifest has {manifest_version}. "
            "Close the stale PR and let release-please regenerate it."
        )

    changelog_version = first_changelog_version(Path("CHANGELOG.md"))
    if changelog_version and changelog_version != manifest_version:
        fail(
            f"CHANGELOG top release/version drift: CHANGELOG has {changelog_version}, manifest has {manifest_version}."
        )

    if os.environ.get("DCC_RELEASE_GUARD_SKIP_REMOTE") != "1":
        repo = os.environ.get("GITHUB_REPOSITORY", "")
        if not repo:
            fail("GITHUB_REPOSITORY is required for remote tag validation")
        tag_name = f"v{manifest_version}"
        if remote_tag_exists(repo, tag_name):
            fail(
                f"remote tag {tag_name} already exists while this release PR is still open. "
                "This PR is stale; close it and let release-please regenerate from main."
            )

    print(f"Release PR metadata is consistent for {manifest_version}.")


if __name__ == "__main__":
    main()
