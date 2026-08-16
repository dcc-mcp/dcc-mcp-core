#!/usr/bin/env python3
"""Maintainer CLI that deletes old PyPI releases through the warehouse web flow.

PyPI exposes no deletion API; project owners can only delete releases through
the pypi.org web UI, which additionally requires a fresh 30-minute reauth
('require_reauth=True' on the deletion endpoint). This CLI automates that
flow for a maintainer on their own machine:

1. Reuse the already-logged-in browser session (Chrome cookies on Windows,
   or an exported Netscape cookie file) so login/captcha is never involved.
2. Optionally re-authenticate with the account password so the 30-minute
   deletion window is open.
3. POST the same 'confirm_delete_version' form the web UI submits and
   verify each deletion against the releases page and the PyPI JSON API.

Intended for cleaning up expired/dev releases that consumed the project's
10 GB PyPI storage budget. Deletion is irreversible: yank instead unless the
goal is freeing storage.

Usage::

    python scripts/release/pypi_cleanup.py --package dcc-mcp-core \
        --cookies-from-chrome --delete-below 0.20.0 --dry-run

    python scripts/release/pypi_cleanup.py --package dcc-mcp-core \
        --cookies-from-chrome --delete-below 0.20.0 --yes
"""

from __future__ import annotations

import argparse
import base64
import getpass
import http.cookiejar
import json
import os
from pathlib import Path
import re
import shutil
import sqlite3
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.parse
import urllib.request

PYPI_URL = "https://pypi.org"
REAUTH_PATH = "/account/reauthenticate/"
RELEASES_PATH = "/manage/project/{package}/releases/"
RELEASE_PATH = "/manage/project/{package}/release/{version}/"
USER_AGENT = "dcc-mcp-pypi-cleanup/1.0 (+https://github.com/dcc-mcp/dcc-mcp-core)"

# Cookie names warehouse needs: the signed Pyramid session plus the CSRF token.
REQUIRED_COOKIES = ("session_id", "csrf_token")


class CleanupError(Exception):
    """Fatal, user-facing cleanup failure."""


class HttpClient:
    """Small stdlib HTTP client with a persistent cookie jar."""

    def __init__(self, cookies: dict[str, str]) -> None:
        jar = http.cookiejar.CookieJar()
        for name, value in cookies.items():
            cookie = http.cookiejar.Cookie(
                version=0,
                name=name,
                value=value,
                port=None,
                port_specified=False,
                domain="pypi.org",
                domain_specified=True,
                domain_initial_dot=name != "session_id",
                path="/",
                path_specified=True,
                secure=True,
                expires=None,
                discard=False,
                comment=None,
                comment_url=None,
                rest={},
            )
            jar.set_cookie(cookie)
        self.opener = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(jar))
        self.opener.addheaders = [
            ("User-Agent", USER_AGENT),
            ("Accept-Language", "en-US,en;q=0.9"),
        ]

    def request(
        self,
        url: str,
        data: dict | None = None,
        referer: str | None = None,
    ) -> tuple[int, str, str]:
        """Return (status, final_url, body) following redirects."""
        headers = {}
        if referer:
            headers["Referer"] = referer
        payload = None
        if data is not None:
            payload = urllib.parse.urlencode(data).encode("utf-8")
            headers["Content-Type"] = "application/x-www-form-urlencoded"
        request = urllib.request.Request(url, data=payload, headers=headers)
        last_error = None
        for attempt in range(4):
            try:
                with self.opener.open(request, timeout=60) as response:
                    return response.status, response.geturl(), response.read().decode("utf-8", "replace")
            except urllib.error.HTTPError as exc:
                body = exc.read().decode("utf-8", "replace")
                if exc.code == 429 or exc.code >= 500:
                    last_error = exc
                    time.sleep(2**attempt)
                    continue
                return exc.code, exc.geturl(), body
            except urllib.error.URLError as exc:
                last_error = exc
                time.sleep(2**attempt)
        raise CleanupError(f"request failed after retries: {last_error}")


def parse_netscape_cookie_file(path: Path) -> dict[str, str]:
    """Parse a Netscape cookies.txt export and keep the pypi.org cookies."""
    cookies: dict[str, str] = {}
    for line in path.read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        if "=" in line and not line.startswith("http"):
            name, _, value = line.partition("=")
            if name.strip() in REQUIRED_COOKIES:
                cookies[name.strip()] = value.strip()
            continue
        parts = line.split("	")
        if len(parts) < 7 or not parts[0].endswith("pypi.org"):
            continue
        name, value = parts[5], parts[6]
        if name in REQUIRED_COOKIES:
            cookies[name] = value
    return cookies


def _chrome_cookie_dir(profile: str | None) -> Path:
    base = Path(os.environ.get("LOCALAPPDATA", "")) / "Google" / "Chrome" / "User Data"
    if not base.exists():
        raise CleanupError(f"Chrome User Data directory not found: {base}")
    return base / (profile or "Default")


def _decrypt_chrome_values(values_b64: dict[str, str]) -> dict[str, str | None]:
    """DPAPI-decrypt Chrome v10 cookie values through Windows PowerShell."""
    payload = json.dumps([{"name": name, "value_b64": value} for name, value in values_b64.items()])
    script = (
        "$ErrorActionPreference='Stop';"
        "Add-Type -AssemblyName System.Security;"
        "$items = [System.Text.Encoding]::UTF8.GetString("
        "[System.Convert]::FromBase64String('{}')) | ConvertFrom-Json;"
        "$out = @{{}};"
        "foreach ($item in $items) {{"
        "  try {{"
        "    $bytes = [Convert]::FromBase64String($item.value_b64);"
        "    $plain = [System.Security.Cryptography.ProtectedData]::Unprotect("
        "$bytes, $null, 'CurrentUser');"
        "    $out[$item.name] = [System.Text.Encoding]::UTF8.GetString($plain);"
        "  }} catch {{ $out[$item.name] = $null; }}"
        "}};"
        "$out | ConvertTo-Json -Compress"
    ).format(base64.b64encode(payload.encode("utf-8")).decode("ascii"))
    result = subprocess.run(
        ["powershell.exe", "-NoProfile", "-NonInteractive", "-Command", script],
        capture_output=True,
        text=True,
        timeout=120,
        check=False,
    )
    if result.returncode != 0:
        raise CleanupError(f"PowerShell cookie decryption failed: {result.stderr.strip() or result.stdout.strip()}")
    try:
        return json.loads(result.stdout.strip())
    except json.JSONDecodeError as exc:
        raise CleanupError(f"could not parse PowerShell decryption output: {result.stdout.strip()[:200]}") from exc


def load_chrome_cookies(profile: str | None) -> dict[str, str]:
    """Extract pypi.org session cookies from a local Chrome profile."""
    if os.name != "nt":
        raise CleanupError("--cookies-from-chrome is Windows-only; use --cookie-file elsewhere")
    cookie_dir = _chrome_cookie_dir(profile)
    db_path = cookie_dir / "Network" / "Cookies"
    if not db_path.exists():
        raise CleanupError(f"Chrome cookie database not found: {db_path}. Open pypi.org in Chrome and log in first.")
    encrypted: dict[str, str] = {}
    with tempfile.TemporaryDirectory() as tmp:
        copy_path = Path(tmp) / "cookies.sqlite"
        shutil.copy2(db_path, copy_path)
        connection = sqlite3.connect(str(copy_path))
        try:
            rows = connection.execute(
                "SELECT name, encrypted_value FROM cookies WHERE host_key LIKE '%pypi.org' AND name IN (?, ?)",
                REQUIRED_COOKIES,
            ).fetchall()
        finally:
            connection.close()
        for name, value in rows:
            if isinstance(value, bytes) and value.startswith(b"v20"):
                raise CleanupError(
                    "Chrome uses app-bound cookie encryption (v20), which this "
                    "tool cannot decrypt. Export a Netscape cookies.txt from "
                    "the browser and pass --cookie-file instead."
                )
            if isinstance(value, bytes):
                encrypted[name] = base64.b64encode(value).decode("ascii")
    decrypted = _decrypt_chrome_values(encrypted)
    cookies: dict[str, str] = {}
    for name, value in decrypted.items():
        if value:
            cookies[name] = value
    missing = [name for name in REQUIRED_COOKIES if name not in cookies]
    if missing:
        raise CleanupError(
            f"missing pypi.org cookies in the Chrome profile: {missing}. Make sure "
            "you are logged in to pypi.org in that profile."
        )
    return cookies


RELEASE_LINK_RE = re.compile(r'href="/manage/project/[^"/]+/release/([^"/]+)/"')
CSRF_RE = re.compile(r'name="csrf_token"[^>]*value="([^"]+)"')


def parse_release_versions(html: str) -> list[str]:
    """Return the release versions listed on the manage releases page."""
    seen: list[str] = []
    for match in RELEASE_LINK_RE.finditer(html):
        version = urllib.parse.unquote(match.group(1))
        if version not in seen:
            seen.append(version)
    return seen


def parse_csrf_token(html: str) -> str:
    """Return the CSRF token rendered in the page form."""
    match = CSRF_RE.search(html)
    if not match:
        raise CleanupError("could not find csrf_token in the manage page")
    return match.group(1)


def version_key(version: str) -> tuple:
    """Ordering key for PEP 440-ish versions (release segments numeric)."""
    head = re.split(r"[-+]", version)[0]
    parts = [part for part in re.split(r"[._]", head) if part]
    numbers = []
    for part in parts:
        if part.isdigit():
            numbers.append((0, int(part)))
        else:
            numbers.append((1, 0, part))
    return tuple(numbers)


def select_versions(
    versions: list[str],
    delete_below: str | None,
    delete_matching: str | None,
    exclude_matching: str | None,
    max_deletes: int | None,
) -> list[str]:
    """Apply the deletion policy and return versions to delete, sorted."""
    selected = []
    below_key = version_key(delete_below) if delete_below else None
    match_re = re.compile(delete_matching) if delete_matching else None
    exclude_re = re.compile(exclude_matching) if exclude_matching else None
    for version in versions:
        if below_key is not None and version_key(version) >= below_key:
            continue
        if match_re is not None and not match_re.fullmatch(version):
            continue
        if exclude_re is not None and exclude_re.fullmatch(version):
            continue
        selected.append(version)
    selected.sort(key=version_key)
    if max_deletes is not None:
        selected = selected[:max_deletes]
    return selected


def fetch_release_sizes(package: str) -> dict[str, int]:
    """Return {version: total_bytes} from the PyPI JSON API."""
    url = f"https://pypi.org/pypi/{package}/json"
    with urllib.request.urlopen(url, timeout=60) as response:
        payload = json.loads(response.read())
    sizes: dict[str, int] = {}
    for version, files in payload.get("releases", {}).items():
        sizes[version] = sum(int(file_info.get("size", 0)) for file_info in files)
    return sizes


def reauthenticate(client: HttpClient, package: str, csrf: str, password: str) -> None:
    """Open the 30-minute deletion window by re-authenticating."""
    status, _url, body = client.request(
        PYPI_URL + REAUTH_PATH,
        data={
            "csrf_token": csrf,
            "password": password,
            "next_route": "manage.project.releases",
            "next_route_matchdict": json.dumps({"project_name": package}),
            "next_route_query": "{}",
        },
        referer=PYPI_URL + RELEASES_PATH.format(package=package),
    )
    if status != 303:
        hint = "invalid password" if "Invalid password" in body else "reauth rejected"
        raise CleanupError(
            f"re-authentication failed (status {status}, {hint}). Deletion requires a "
            "fresh password entry within the last 30 minutes."
        )


def delete_release(client: HttpClient, package: str, version: str, csrf: str) -> bool:
    """Submit the warehouse delete form and verify the version disappeared."""
    release_url = PYPI_URL + RELEASE_PATH.format(package=package, version=urllib.parse.quote(version))
    status, _url, body = client.request(
        release_url,
        data={"csrf_token": csrf, "confirm_delete_version": version},
        referer=release_url,
    )
    if status >= 400:
        raise CleanupError(f"delete request for {version!r} failed with HTTP {status}")
    if "Could not delete release" in body:
        raise CleanupError(f"warehouse refused to delete {version!r}: {body[:300]}")
    status, _url, body = client.request(PYPI_URL + RELEASES_PATH.format(package=package))
    versions = parse_release_versions(body)
    return version not in versions


def main(argv: list | None = None) -> int:
    """Run the cleanup CLI and exit 1 when any deletion fails."""
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--package", required=True, help="PyPI project name")
    source = parser.add_mutually_exclusive_group(required=True)
    source.add_argument(
        "--cookies-from-chrome",
        nargs="?",
        const="Default",
        metavar="PROFILE",
        help="read session cookies from a local Chrome profile (default: Default)",
    )
    source.add_argument(
        "--cookie-file",
        metavar="PATH",
        help="cookie file for pypi.org (Netscape cookies.txt or name=value lines)",
    )
    source.add_argument(
        "--session-cookie",
        metavar="VALUE",
        help="paste the pypi.org session_id cookie value directly",
    )
    source.add_argument(
        "--csrf-cookie",
        metavar="VALUE",
        help="paste the pypi.org csrf_token cookie value directly",
    )
    parser.add_argument(
        "--delete-below",
        metavar="VERSION",
        help="delete releases strictly older than VERSION (e.g. 0.20.0)",
    )
    parser.add_argument(
        "--delete-matching",
        metavar="REGEX",
        help="delete releases whose version fully matches REGEX",
    )
    parser.add_argument(
        "--exclude-matching",
        metavar="REGEX",
        help="never delete releases matching REGEX",
    )
    parser.add_argument("--max-deletes", type=int, metavar="N")
    parser.add_argument(
        "--delay-seconds",
        type=float,
        default=1.5,
        help="pause between deletions (default: %(default)s)",
    )
    parser.add_argument(
        "--dry-run",
        action="store_true",
        default=True,
        help="list what would be deleted without changing anything (default)",
    )
    parser.add_argument(
        "--yes",
        action="store_true",
        help="actually perform the deletions",
    )
    parser.add_argument(
        "--password",
        metavar="VALUE",
        help="PyPI password for re-authentication; prompted when omitted",
    )
    parser.add_argument(
        "--password-file",
        metavar="PATH",
        help="read the PyPI password from the first line of PATH "
        "(keeps it out of command lines and shell history)",
    )
    args = parser.parse_args(argv)

    if not (args.delete_below or args.delete_matching):
        parser.error("one of --delete-below / --delete-matching is required")

    if args.cookies_from_chrome:
        cookies = load_chrome_cookies(args.cookies_from_chrome)
    elif args.cookie_file:
        cookies = parse_netscape_cookie_file(Path(args.cookie_file))
    else:
        cookies = {}
    if args.session_cookie:
        cookies["session_id"] = args.session_cookie
    if args.csrf_cookie:
        cookies["csrf_token"] = args.csrf_cookie
    if "session_id" not in cookies:
        raise CleanupError("no session_id cookie for pypi.org found")

    client = HttpClient(cookies)
    status, url, html = client.request(PYPI_URL + RELEASES_PATH.format(package=args.package))
    if "/account/login/" in url or status == 401:
        raise CleanupError("session is not authenticated on pypi.org")
    csrf = parse_csrf_token(html)
    versions = parse_release_versions(html)
    if not versions:
        raise CleanupError("no releases found on the manage releases page")

    selected = select_versions(
        versions,
        delete_below=args.delete_below,
        delete_matching=args.delete_matching,
        exclude_matching=args.exclude_matching,
        max_deletes=args.max_deletes,
    )
    if not selected:
        print("Nothing matches the deletion policy; nothing to do.")
        return 0

    sizes = fetch_release_sizes(args.package)
    freed = sum(sizes.get(version, 0) for version in selected)
    print(
        f"Package: {args.package} — {len(versions)} release(s) on PyPI, {len(selected)} selected for deletion "
        f"(~{freed / (1024 * 1024):.2f} MiB)"
    )
    for version in selected:
        size = sizes.get(version, 0)
        print(f"  - {version} ({size / (1024 * 1024):.2f} MiB)")
    if args.dry_run:
        print("DRY RUN — nothing was deleted. Pass --yes to execute.")
        return 0
    if not args.yes:
        return 0

    password = args.password
    if password is None and args.password_file:
        password = Path(args.password_file).read_text(encoding="utf-8").splitlines()[0].strip()
    if password is None:
        password = os.environ.get("PYPI_CLEANUP_PASSWORD")
    if password is None:
        password = getpass.getpass("PyPI password (for 30-minute reauth): ")
    reauthenticate(client, args.package, csrf, password)
    print("Re-authenticated; deletion window is open for 30 minutes.")

    deleted = 0
    failed = 0
    for index, version in enumerate(selected, start=1):
        try:
            ok = delete_release(client, args.package, version, csrf)
        except CleanupError as exc:
            failed += 1
            print(f"[{index}/{len(selected)}] {version} FAILED: {exc}")
            continue
        if ok:
            deleted += 1
            print(f"[{index}/{len(selected)}] {version} deleted")
        else:
            failed += 1
            print(f"[{index}/{len(selected)}] {version} still listed after delete (verify manually)")
        time.sleep(args.delay_seconds)

    remaining = fetch_release_sizes(args.package)
    gone = sorted(set(sizes) - set(remaining))
    print(f"Done: {deleted} deleted, {failed} failed. PyPI JSON API confirms {len(gone)} release(s) removed.")
    return 1 if failed else 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except CleanupError as exc:
        print(f"::error::{exc}", file=sys.stderr)
        sys.exit(1)
