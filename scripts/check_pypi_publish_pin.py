#!/usr/bin/env python3
"""Assert every ``pypa/gh-action-pypi-publish`` ref in the org is a 40-hex SHA.

PIP-3715 guards the PIP-3627 rollout. Unifying 48 repositories on one immutable
commit SHA is worth nothing if nothing notices a later revert to the mutable
``release/v1`` tag, so this scanner asserts the *shape* of the ref and nothing
else.

Shape-only, by deliberate decision: the SHA **value** is never asserted. Pinning
a value would mean editing an assertion in every repository each time the action
is bumped, which is exactly the scope expansion this check is meant to avoid.
Bumping the action therefore requires no change here at all.

Read-only. Python 3.7 compatible, standard library only.

Usage::

    python scripts/check_pypi_publish_pin.py                  # whole org
    python scripts/check_pypi_publish_pin.py --json           # machine output
    python scripts/check_pypi_publish_pin.py --repos a,b      # subset
    python scripts/check_pypi_publish_pin.py --ref <branch>   # drill on one ref

Auth: uses ``$GITHUB_TOKEN`` when set, otherwise ``gh auth token``.

Exit codes: 0 = clean, 1 = at least one bad ref, 2 = the scan itself failed.
"""

from __future__ import annotations

import argparse
import base64
import binascii
import json
import os
import re
import subprocess
import sys
import urllib.error
import urllib.parse
import urllib.request
from concurrent.futures import ThreadPoolExecutor

DEFAULT_ORG = "dcc-mcp"
PUBLISH_ACTION = "pypa/gh-action-pypi-publish"
WORKFLOWS_DIR = ".github/workflows"
API_ROOT = "https://api.github.com"

# ``uses:`` lines carry an optional ``# v1.14.2`` trailing comment, so the ref
# runs to the next piece of YAML whitespace rather than to end of line.
REF_RE = re.compile(re.escape(PUBLISH_ACTION) + r"@([^\s#\"']+)", re.IGNORECASE)
HEX40_RE = re.compile(r"^[0-9a-f]{40}$")


class ScanError(RuntimeError):
    """Raised when the scan cannot complete, as opposed to finding bad refs."""


def _token():
    """Return a GitHub token from the environment or from ``gh auth token``."""
    token = (os.environ.get("GITHUB_TOKEN") or "").strip()
    if token:
        return token
    try:
        completed = subprocess.run(
            ["gh", "auth", "token"], stdout=subprocess.PIPE, stderr=subprocess.PIPE
        )
    except OSError as error:
        raise ScanError("cannot read a GitHub token: {}".format(error))
    token = completed.stdout.decode("utf-8", "replace").strip()
    if completed.returncode != 0 or not token:
        raise ScanError("set GITHUB_TOKEN or run `gh auth login` first")
    return token


def _get(path, token):
    """GET an API path (without the host) and return ``(json_body, headers)``."""
    request = urllib.request.Request(urllib.parse.urljoin(API_ROOT + "/", path.lstrip("/")))
    request.add_header("Authorization", "token " + token)
    request.add_header("Accept", "application/vnd.github+json")
    request.add_header("User-Agent", "pip-3715-publish-pin-check")
    try:
        with urllib.request.urlopen(request, timeout=60) as response:
            raw = response.read()
            headers = dict(response.headers.items())
    except urllib.error.HTTPError as error:
        if error.code in (403, 429):
            raise ScanError("GitHub rate limit or forbidden on {}".format(path))
        # A missing .github/workflows directory or an empty repo. Not a violation.
        if error.code == 404:
            raise NotFound(path)
        raise ScanError("GET {} failed with HTTP {}".format(path, error.code))
    except urllib.error.URLError as error:
        raise ScanError("GET {} failed: {}".format(path, error.reason))
    if not raw:
        return None, headers
    try:
        return json.loads(raw.decode("utf-8", "replace")), headers
    except ValueError as error:
        raise ScanError("GET {} returned invalid JSON: {}".format(path, error))


class NotFound(Exception):
    """Raised for a 404 so callers can treat "no such path" as empty."""


def _get_paginated(path, token):
    """Follow ``Link: rel="next"`` and return the concatenated list payload."""
    items = []
    next_path = path
    while next_path:
        body, headers = _get(next_path, token)
        if isinstance(body, list):
            items.extend(body)
        next_path = None
        for link in (headers.get("Link") or "").split(","):
            parts = [chunk.strip() for chunk in link.split(";")]
            if len(parts) == 2 and parts[1] == 'rel="next"':
                next_path = parts[0].strip("<>")
    return items


def list_repos(org, token):
    """Return non-archived repos as ``[(name, default_branch), ...]``."""
    path = "/orgs/{}/repos?type=all&per_page=100&sort=full_name".format(
        urllib.parse.quote(org)
    )
    repos = []
    for entry in _get_paginated(path, token):
        if entry.get("archived"):
            continue
        repos.append((entry["name"], entry.get("default_branch") or "main"))
    return repos


def list_workflow_paths(org, repo, ref, token):
    """List workflow files on ``ref``; empty list when the repo has none."""
    path = "/repos/{}/{}/contents/{}?ref={}".format(
        urllib.parse.quote(org), urllib.parse.quote(repo), WORKFLOWS_DIR, urllib.parse.quote(ref)
    )
    try:
        payload = _get(path, token)[0]
    except NotFound:
        return []
    if not isinstance(payload, list):
        return []
    return [
        entry["path"]
        for entry in payload
        if entry.get("type") == "file" and str(entry.get("name", "")).endswith((".yml", ".yaml"))
    ]


def fetch_file(org, repo, path, ref, token):
    """Return the decoded text of ``path`` on ``ref``, or None if unreadable."""
    api_path = "/repos/{}/{}/contents/{}?ref={}".format(
        urllib.parse.quote(org), urllib.parse.quote(repo), urllib.parse.quote(path),
        urllib.parse.quote(ref),
    )
    try:
        payload = _get(api_path, token)[0]
    except NotFound:
        return None
    content = payload.get("content") if isinstance(payload, dict) else None
    if not content:
        return None
    try:
        return base64.b64decode(content).decode("utf-8", "replace")
    except (binascii.Error, ValueError):
        return None


def scan_text(text):
    """Return ``(line_number, raw_line, ref)`` for every publish action ref."""
    found = []
    for number, line in enumerate(text.splitlines(), start=1):
        stripped = line.strip()
        # A commented-out ``uses:`` line is documentation, not an invocation.
        if stripped.startswith("#"):
            continue
        for action_ref in REF_RE.findall(line):
            found.append((number, stripped, action_ref))
    return found


def _scan_repo(org, repo, ref, token):
    """Scan one repo; return ``(repo, saw_publish, histogram, findings)``."""
    histogram = {}
    findings = []
    for path in list_workflow_paths(org, repo, ref, token):
        text = fetch_file(org, repo, path, ref, token)
        if not text:
            continue
        for number, raw_line, action_ref in scan_text(text):
            histogram[action_ref] = histogram.get(action_ref, 0) + 1
            if not HEX40_RE.match(action_ref):
                findings.append(
                    {
                        "repo": repo,
                        "ref": ref,
                        "path": path,
                        "line": number,
                        "found": action_ref,
                        "line_text": raw_line,
                    }
                )
    return repo, bool(histogram), histogram, findings


def scan_org(org, repos=None, ref_override=None, jobs=8, progress=None):
    """Scan every repo in ``org``; return the report dict."""
    token = _token()
    if repos:
        targets = [(name, ref_override or "main") for name in repos]
    else:
        targets = list_repos(org, token)
        if progress:
            progress("enumerated {} non-archived repos in {}".format(len(targets), org))
        if ref_override:
            targets = [(name, ref_override) for name, _ in targets]

    histogram = {}
    findings = []
    repos_with_publish = 0

    def worker(target):
        repo, ref = target
        return _scan_repo(org, repo, ref, token)

    with ThreadPoolExecutor(max_workers=max(1, jobs)) as pool:
        for index, (repo, saw_publish, repo_histogram, repo_findings) in enumerate(
            pool.map(worker, targets), start=1
        ):
            if progress:
                progress("[{}/{}] {}".format(index, len(targets), repo))
            if saw_publish:
                repos_with_publish += 1
            for action_ref, count in repo_histogram.items():
                histogram[action_ref] = histogram.get(action_ref, 0) + count
            findings.extend(repo_findings)

    findings.sort(key=lambda item: (item["repo"], item["path"], item["line"]))
    return {
        "org": org,
        "scanned_ref": ref_override or "default branch",
        "repos_scanned": len(targets),
        "repos_with_publish_step": repos_with_publish,
        "refs_found": sum(histogram.values()),
        "distinct_refs": dict(sorted(histogram.items(), key=lambda kv: (-kv[1], kv[0]))),
        "violations": findings,
        "ok": not findings,
    }


def render_text(report):
    lines = [
        "org={} scanned_ref={} repos_scanned={}".format(
            report["org"], report["scanned_ref"], report["repos_scanned"]
        ),
        "repos_with_publish_step={} refs_found={}".format(
            report["repos_with_publish_step"], report["refs_found"]
        ),
    ]
    if report["distinct_refs"]:
        lines.append("distinct refs (informational, the SHA value is NOT asserted):")
        for ref, count in report["distinct_refs"].items():
            lines.append("  {} x {}".format(count, ref))
    if not report["violations"]:
        lines.append("OK: every {} ref is a 40-hex SHA.".format(PUBLISH_ACTION))
        return "\n".join(lines)
    lines.append("FAIL: {} ref(s) are not a 40-hex SHA:".format(len(report["violations"])))
    for finding in report["violations"]:
        lines.append(
            "  {repo}@{ref} {path}:{line} got {found!r}".format(**finding)
        )
    return "\n".join(lines)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--org", default=DEFAULT_ORG, help="GitHub organization (default: %(default)s)")
    parser.add_argument("--repos", default="", help="comma-separated repo names instead of the whole org")
    parser.add_argument("--ref", default=None, help="scan this git ref instead of each repo's default branch")
    parser.add_argument("--jobs", type=int, default=8, help="parallel repo fetches (default: %(default)s)")
    parser.add_argument("--json", action="store_true", dest="as_json", help="emit the report as JSON")
    parser.add_argument("--quiet", action="store_true", help="suppress per-repo progress on stderr")
    args = parser.parse_args(argv)

    repo_list = [item.strip() for item in args.repos.split(",") if item.strip()] or None
    progress = None
    if not args.quiet:
        progress = lambda message: print(message, file=sys.stderr)
    try:
        report = scan_org(
            args.org, repos=repo_list, ref_override=args.ref, jobs=args.jobs, progress=progress
        )
    except ScanError as error:
        print("SCAN ERROR: {}".format(error), file=sys.stderr)
        return 2

    if args.as_json:
        print(json.dumps(report, indent=2))
    else:
        print(render_text(report))
    return 1 if report["violations"] else 0


if __name__ == "__main__":
    sys.exit(main())
