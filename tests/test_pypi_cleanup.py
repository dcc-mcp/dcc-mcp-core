"""Regression tests for the PyPI release cleanup CLI."""

from __future__ import annotations

from pathlib import Path

import pytest
from scripts.release.pypi_cleanup import CleanupError
from scripts.release.pypi_cleanup import parse_csrf_token
from scripts.release.pypi_cleanup import parse_netscape_cookie_file
from scripts.release.pypi_cleanup import parse_release_versions
from scripts.release.pypi_cleanup import select_versions
from scripts.release.pypi_cleanup import version_key

RELEASES_HTML = """
<html>
<body>
<h2>Releases</h2>
<table class="table table--releases">
  <tr>
    <th scope="row"><a href="/manage/project/dcc-mcp-core/release/0.19.7/"
       title="Manage version">0.19.7</a></th>
  </tr>
  <tr>
    <th scope="row"><a href="/manage/project/dcc-mcp-core/release/0.19.94/"
       title="Manage version">0.19.94</a></th>
  </tr>
  <tr>
    <th scope="row"><a href="/manage/project/dcc-mcp-core/release/0.20.0/"
       title="Manage version">0.20.0</a></th>
  </tr>
  <tr>
    <th scope="row"><a href="/manage/project/dcc-mcp-core/release/0.20.6/"
       title="Manage version">0.20.6</a></th>
  </tr>
</table>
<form action="/account/reauthenticate/">
  <input type="hidden" name="csrf_token" value="csrf-abc123">
</form>
</body>
</html>
"""


def test_parse_release_versions_extracts_and_deduplicates() -> None:
    html = RELEASES_HTML + '<a href="/manage/project/dcc-mcp-core/release/0.19.7/">dup</a>'
    versions = parse_release_versions(html)
    assert versions == ["0.19.7", "0.19.94", "0.20.0", "0.20.6"]


def test_parse_csrf_token_finds_hidden_input() -> None:
    assert parse_csrf_token(RELEASES_HTML) == "csrf-abc123"


def test_parse_csrf_token_missing_raises() -> None:
    with pytest.raises(CleanupError, match="csrf_token"):
        parse_csrf_token("<html></html>")


def test_select_versions_delete_below() -> None:
    versions = ["0.19.7", "0.19.94", "0.20.0", "0.20.6", "0.21.1"]
    assert select_versions(versions, "0.20.0", None, None, None) == [
        "0.19.7",
        "0.19.94",
    ]
    assert select_versions(versions, "0.20.6", None, None, None) == [
        "0.19.7",
        "0.19.94",
        "0.20.0",
    ]


def test_select_versions_matching_and_exclude() -> None:
    versions = ["0.19.7", "0.19.94", "0.20.0", "0.20.6"]
    assert select_versions(versions, None, r"0\.19\..*", None, None) == [
        "0.19.7",
        "0.19.94",
    ]
    assert select_versions(versions, "0.20.0", None, r"0\.19\.7", None) == [
        "0.19.94",
    ]


def test_select_versions_max_deletes_caps() -> None:
    versions = ["0.19.7", "0.19.94", "0.20.0", "0.20.6"]
    assert select_versions(versions, "1.0.0", None, None, 2) == ["0.19.7", "0.19.94"]


def test_version_key_orders_numeric_segments() -> None:
    assert version_key("0.19.7") < version_key("0.19.94")
    assert version_key("0.19.9") < version_key("0.19.10")
    assert version_key("0.20.0") > version_key("0.19.94")
    assert version_key("0.19.94.post1") > version_key("0.19.94")


def test_parse_netscape_cookie_file_keeps_pypi_cookies(tmp_path: Path) -> None:
    cookies_file = tmp_path / "cookies.txt"
    cookies_file.write_text(
        "\n".join(
            [
                "# Netscape HTTP Cookie File",
                ".pypi.org\tTRUE\t/\tTRUE\t1893456000\tsession_id\tSESS123",
                ".pypi.org\tTRUE\t/\tTRUE\t1893456000\tcsrf_token\tCSRF123",
                ".example.com\tTRUE\t/\tTRUE\t1893456000\tsession_id\tOTHER",
            ]
        )
        + "\n",
        encoding="utf-8",
    )
    cookies = parse_netscape_cookie_file(cookies_file)
    assert cookies == {"session_id": "SESS123", "csrf_token": "CSRF123"}
