"""Regression tests for the public-data-only PyPI cleanup planner."""

from __future__ import annotations

import json
from pathlib import Path

import pytest
from scripts.release import pypi_cleanup
from scripts.release.pypi_cleanup import CleanupError
from scripts.release.pypi_cleanup import build_plan
from scripts.release.pypi_cleanup import parse_project_state
from scripts.release.pypi_cleanup import select_versions
from scripts.release.pypi_cleanup import version_key


def project_payload() -> dict:
    """Return a compact PyPI JSON fixture with immutable file evidence."""
    return {
        "info": {"version": "0.20.7"},
        "releases": {
            "0.19.3": [
                {
                    "filename": "dcc_mcp_core-0.19.3.tar.gz",
                    "size": 11,
                    "url": "https://files.pythonhosted.org/0.19.3.tar.gz",
                    "digests": {"sha256": "a" * 64},
                }
            ],
            "0.19.4": [
                {
                    "filename": "dcc_mcp_core-0.19.4.tar.gz",
                    "size": 13,
                    "url": "https://files.pythonhosted.org/0.19.4.tar.gz",
                    "digests": {"sha256": "b" * 64},
                }
            ],
            "0.19.45": [
                {
                    "filename": "dcc_mcp_core-0.19.45.tar.gz",
                    "size": 17,
                    "url": "https://files.pythonhosted.org/0.19.45.tar.gz",
                    "digests": {"sha256": "c" * 64},
                }
            ],
            "0.19.89": [
                {
                    "filename": "dcc_mcp_core-0.19.89.tar.gz",
                    "size": 19,
                    "url": "https://files.pythonhosted.org/0.19.89.tar.gz",
                    "digests": {"sha256": "d" * 64},
                }
            ],
            "0.19.90": [
                {
                    "filename": "dcc_mcp_core-0.19.90.tar.gz",
                    "size": 23,
                    "url": "https://files.pythonhosted.org/0.19.90.tar.gz",
                    "digests": {"sha256": "e" * 64},
                }
            ],
            "0.20.7": [
                {
                    "filename": "dcc_mcp_core-0.20.7.tar.gz",
                    "size": 29,
                    "url": "https://files.pythonhosted.org/0.20.7.tar.gz",
                    "digests": {"sha256": "f" * 64},
                }
            ],
        },
    }


def test_parse_project_state_preserves_file_evidence() -> None:
    state = parse_project_state("dcc-mcp-core", project_payload())
    assert state.latest_version == "0.20.7"
    assert state.total_bytes == 112
    release = state.releases["0.19.89"]
    assert release.total_bytes == 19
    assert release.files[0].filename == "dcc_mcp_core-0.19.89.tar.gz"
    assert release.files[0].sha256 == "d" * 64


def test_parse_project_state_rejects_missing_metadata() -> None:
    with pytest.raises(CleanupError, match="missing release metadata"):
        parse_project_state("dcc-mcp-core", {"info": {}, "releases": {}})


def test_select_versions_delete_below_with_explicit_anchors() -> None:
    versions = ["0.19.3", "0.19.4", "0.19.45", "0.19.89", "0.19.90", "0.20.7"]
    selected = select_versions(
        versions,
        delete_below="0.19.90",
        delete_matching=None,
        exclude_matching=None,
        max_deletes=None,
        keep_versions={"0.19.3", "0.19.4", "0.19.45"},
    )
    assert selected == ["0.19.89"]


def test_select_versions_matching_exclude_and_cap() -> None:
    versions = ["0.19.7", "0.19.45", "0.19.89", "0.20.0"]
    selected = select_versions(
        versions,
        delete_below=None,
        delete_matching=r"0\.19\..*",
        exclude_matching=r"0\.19\.45",
        max_deletes=1,
    )
    assert selected == ["0.19.7"]


def test_version_key_orders_numeric_segments() -> None:
    assert version_key("0.19.7") < version_key("0.19.94")
    assert version_key("0.19.9") < version_key("0.19.10")
    assert version_key("0.20.0") > version_key("0.19.94")
    assert version_key("0.19.94.post1") > version_key("0.19.94")


def test_build_plan_includes_projected_storage_and_hashes() -> None:
    state = parse_project_state("dcc-mcp-core", project_payload())
    plan = build_plan(state, ["0.19.89"], {"0.19.3", "0.19.4", "0.19.45"})
    assert plan["selected_release_count"] == 1
    assert plan["selected_total_bytes"] == 19
    assert plan["projected_total_bytes"] == 93
    assert plan["selected_releases"][0]["files"][0]["sha256"] == "d" * 64
    assert plan["management_url"].endswith("/manage/project/dcc-mcp-core/releases/")


def test_main_writes_auditable_manifest(monkeypatch: pytest.MonkeyPatch, tmp_path: Path, capsys) -> None:
    state = parse_project_state("dcc-mcp-core", project_payload())
    monkeypatch.setattr(pypi_cleanup, "fetch_project_state", lambda _package: state)
    output = tmp_path / "cleanup-plan.json"

    result = pypi_cleanup.main(
        [
            "--package",
            "dcc-mcp-core",
            "--delete-below",
            "0.19.90",
            "--keep-version",
            "0.19.3",
            "--keep-version",
            "0.19.4",
            "--keep-version",
            "0.19.45",
            "--output-json",
            str(output),
        ]
    )

    assert result == 0
    manifest = json.loads(output.read_text(encoding="utf-8"))
    assert [item["version"] for item in manifest["selected_releases"]] == ["0.19.89"]
    stdout = capsys.readouterr().out
    assert "DRY RUN" in stdout
    assert "never deletes releases" in stdout


def test_main_rejects_unknown_keep_anchor(monkeypatch: pytest.MonkeyPatch) -> None:
    state = parse_project_state("dcc-mcp-core", project_payload())
    monkeypatch.setattr(pypi_cleanup, "fetch_project_state", lambda _package: state)
    with pytest.raises(CleanupError, match="absent from PyPI"):
        pypi_cleanup.main(
            [
                "--package",
                "dcc-mcp-core",
                "--delete-below",
                "0.19.90",
                "--keep-version",
                "0.19.44",
            ]
        )


def test_main_refuses_to_select_latest(monkeypatch: pytest.MonkeyPatch) -> None:
    state = parse_project_state("dcc-mcp-core", project_payload())
    monkeypatch.setattr(pypi_cleanup, "fetch_project_state", lambda _package: state)
    with pytest.raises(CleanupError, match="current latest release"):
        pypi_cleanup.main(
            [
                "--package",
                "dcc-mcp-core",
                "--delete-below",
                "1.0.0",
            ]
        )


def test_cli_has_no_execute_or_credential_options(monkeypatch: pytest.MonkeyPatch) -> None:
    state = parse_project_state("dcc-mcp-core", project_payload())
    monkeypatch.setattr(pypi_cleanup, "fetch_project_state", lambda _package: state)
    with pytest.raises(SystemExit) as exc_info:
        pypi_cleanup.main(
            [
                "--package",
                "dcc-mcp-core",
                "--delete-below",
                "0.19.90",
                "--execute",
            ]
        )
    assert exc_info.value.code == 2
