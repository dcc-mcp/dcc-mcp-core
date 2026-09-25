"""Regression tests for the PyPI project size budget preflight."""

from __future__ import annotations

import argparse
import io
import json
from pathlib import Path
import urllib.error

import pytest
from scripts.ci.check_pypi_project_size import GIB
from scripts.ci.check_pypi_project_size import MANAGE_RELEASES_URL
from scripts.ci.check_pypi_project_size import STORAGE_LIMITS_DOC
from scripts.ci.check_pypi_project_size import Artifact
from scripts.ci.check_pypi_project_size import build_report
from scripts.ci.check_pypi_project_size import collect_dist_artifacts
from scripts.ci.check_pypi_project_size import fetch_pypi_files
from scripts.ci.check_pypi_project_size import parse_gb
from scripts.ci.check_pypi_project_size import render_failure
from scripts.ci.check_pypi_project_size import render_summary
from scripts.ci.check_pypi_project_size import render_warning


def _report(pypi_files, artifacts, limit_bytes=10 * GIB, warn_headroom_bytes=GIB):
    return build_report(
        package="dcc-mcp-core",
        pypi_files=pypi_files,
        artifacts=artifacts,
        limit_bytes=limit_bytes,
        warn_headroom_bytes=warn_headroom_bytes,
    )


def test_collect_dist_artifacts_ignores_non_distribution_files(tmp_path: Path) -> None:
    wheel = tmp_path / "pkg-1.0.0-cp38-abi3-win_amd64.whl"
    wheel.write_bytes(b"x" * 10)
    sdist = tmp_path / "pkg-1.0.0.tar.gz"
    sdist.write_bytes(b"y" * 20)
    (tmp_path / "pkg-1.0.0.tar.gz.publish.attestation").write_bytes(b"z" * 30)
    (tmp_path / "notes.txt").write_bytes(b"w" * 40)
    (tmp_path / "nested").mkdir()
    (tmp_path / "nested" / "pkg-2.0.0-py3-none-any.whl").write_bytes(b"v" * 50)

    artifacts = collect_dist_artifacts(tmp_path)

    assert [(a.filename, a.size_bytes) for a in artifacts] == [
        ("pkg-1.0.0-cp38-abi3-win_amd64.whl", 10),
        ("pkg-1.0.0.tar.gz", 20),
    ]


def test_collect_dist_artifacts_missing_dir_raises(tmp_path: Path) -> None:
    with pytest.raises(FileNotFoundError):
        collect_dist_artifacts(tmp_path / "missing")


def test_build_report_skips_files_already_on_pypi() -> None:
    report = _report(
        pypi_files={"a.whl": 100},
        artifacts=[Artifact("a.whl", 999), Artifact("b.whl", 100)],
    )

    assert report.current_bytes == 100
    assert report.already_present == ["a.whl"]
    assert [a.filename for a in report.to_add] == ["b.whl"]
    assert report.projected_bytes == 200
    assert not report.over_budget


def test_build_report_over_budget_when_projection_exceeds_limit() -> None:
    report = _report(
        pypi_files={"existing.whl": 9 * GIB},
        artifacts=[Artifact("new.whl", 2 * GIB)],
    )

    assert report.over_budget
    assert not report.near_budget


def test_build_report_warns_when_headroom_within_threshold() -> None:
    report = _report(
        pypi_files={"existing.whl": int(9.5 * GIB)},
        artifacts=[Artifact("new.whl", GIB // 4)],
        warn_headroom_bytes=GIB,
    )

    assert not report.over_budget
    assert report.near_budget


def test_build_report_ok_with_ample_headroom() -> None:
    report = _report(
        pypi_files={"existing.whl": GIB},
        artifacts=[Artifact("new.whl", GIB)],
        warn_headroom_bytes=GIB,
    )

    assert not report.over_budget
    assert not report.near_budget


def test_v0206_incident_shape_is_over_budget() -> None:
    """The v0.20.6 publish left PyPI at ~9.98 GiB; the remaining five files
    (macos abi3, linux abi3, windows abi3, py3-none-any, sdist) could not fit.
    """
    pypi_files = {"incumbent.whl": 10_714_226_066}
    missing = [
        Artifact(
            "dcc_mcp_core-0.20.6-cp38-abi3-macosx_10_12_x86_64.macosx_11_0_arm64.macosx_10_12_universal2.whl",
            36 * GIB // 1000,
        ),
        Artifact("dcc_mcp_core-0.20.6-cp38-abi3-manylinux_2_17_x86_64.manylinux2014_x86_64.whl", 19 * GIB // 1000),
        Artifact("dcc_mcp_core-0.20.6-cp38-abi3-win_amd64.whl", 19 * GIB // 1000),
        Artifact("dcc_mcp_core-0.20.6-py3-none-any.whl", GIB // 2000),
        Artifact("dcc_mcp_core-0.20.6.tar.gz", 8 * GIB // 1000),
    ]

    report = _report(pypi_files, missing)

    assert report.over_budget
    assert len(report.to_add) == 5


def test_render_failure_points_at_remediation_paths() -> None:
    report = _report(
        pypi_files={"existing.whl": 10 * GIB},
        artifacts=[Artifact("new.whl", GIB)],
    )

    message = render_failure(report)

    assert message.startswith("::error::")
    assert "dcc-mcp-core" in message
    assert STORAGE_LIMITS_DOC in message
    assert MANAGE_RELEASES_URL.format(package="dcc-mcp-core") in message
    assert "skip-existing" in message


def test_render_warning_reports_headroom() -> None:
    report = _report(
        pypi_files={"existing.whl": int(9.5 * GIB)},
        artifacts=[Artifact("new.whl", GIB // 4)],
        warn_headroom_bytes=GIB,
    )

    message = render_warning(report)

    assert message.startswith("::warning::")
    assert "dcc-mcp-core" in message
    assert STORAGE_LIMITS_DOC in message


def test_render_summary_lists_skipped_and_added_files() -> None:
    report = _report(
        pypi_files={"a.whl": 100},
        artifacts=[Artifact("a.whl", 999), Artifact("b.whl", 100)],
    )

    lines = render_summary(report)

    joined = "\n".join(lines)
    assert "already on PyPI (skipped): 1 file(s)" in joined
    assert "a.whl" in joined
    assert "b.whl" in joined
    assert "projected:" in joined


def test_parse_gb_rejects_invalid_values() -> None:
    with pytest.raises(argparse.ArgumentTypeError):
        parse_gb("abc")
    with pytest.raises(argparse.ArgumentTypeError):
        parse_gb("0")
    with pytest.raises(argparse.ArgumentTypeError):
        parse_gb("-1")

    assert parse_gb("10") == 10.0
    assert parse_gb("0.5") == 0.5


def _http_error(code: int) -> urllib.error.HTTPError:
    return urllib.error.HTTPError("https://pypi.org/pypi/pkg/json", code, "boom", {}, None)


def test_fetch_pypi_files_reads_every_release(monkeypatch) -> None:
    """Sizes are collected across all releases of a project."""
    payload = json.dumps(
        {"releases": {"1.0": [{"filename": "a.whl", "size": 10}], "2.0": [{"filename": "b.whl", "size": 20}]}}
    ).encode("utf-8")

    monkeypatch.setattr("urllib.request.urlopen", lambda url, timeout=None: io.BytesIO(payload))

    assert fetch_pypi_files("dcc-mcp-core", "https://pypi.org/pypi/{package}/json") == {
        "a.whl": 10,
        "b.whl": 20,
    }


def test_fetch_pypi_files_treats_404_as_unpublished(monkeypatch, capsys) -> None:
    """A project that has never been published uses 0 bytes.

    PyPI answers 404 for a project with no releases. Failing that request
    would block the very first publish of a new distribution - and the
    first publish is the only one that can ever see the 404.
    """
    monkeypatch.setattr("urllib.request.urlopen", lambda url, timeout=None: (_ for _ in ()).throw(_http_error(404)))

    assert fetch_pypi_files("dcc-mcp-cli", "https://pypi.org/pypi/{package}/json") == {}
    assert "404" in capsys.readouterr().err


def test_fetch_pypi_files_still_fails_on_other_statuses(monkeypatch) -> None:
    """Any status other than 404 is a real failure, not an empty project."""
    monkeypatch.setattr("urllib.request.urlopen", lambda url, timeout=None: (_ for _ in ()).throw(_http_error(503)))

    with pytest.raises(RuntimeError, match="dcc-mcp-cli"):
        fetch_pypi_files("dcc-mcp-cli", "https://pypi.org/pypi/{package}/json")


def test_fetch_pypi_files_still_fails_on_transport_errors(monkeypatch) -> None:
    """A DNS or TLS failure must not be mistaken for an empty project."""
    monkeypatch.setattr(
        "urllib.request.urlopen", lambda url, timeout=None: (_ for _ in ()).throw(OSError("no route to host"))
    )

    with pytest.raises(RuntimeError, match="no route to host"):
        fetch_pypi_files("dcc-mcp-cli", "https://pypi.org/pypi/{package}/json")


def test_unpublished_project_fits_the_budget(tmp_path: Path) -> None:
    """The gate a first release has to pass: 0 current bytes, all files new."""
    wheel = tmp_path / "dcc_mcp_cli-0.1.0-py3-none-manylinux_2_17_x86_64.whl"
    wheel.write_bytes(b"x" * 100)

    report = build_report(
        package="dcc-mcp-cli",
        pypi_files={},
        artifacts=collect_dist_artifacts(tmp_path),
        limit_bytes=10 * GIB,
        warn_headroom_bytes=GIB,
    )

    assert not report.over_budget
    assert report.current_bytes == 0
    assert [a.filename for a in report.to_add] == [wheel.name]
