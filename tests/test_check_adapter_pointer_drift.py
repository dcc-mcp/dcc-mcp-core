"""Cross-repository pointer drift scanner.

The scanner clones adapter READMEs and replays the generator against them, so
these tests stand up throwaway git repositories on disk instead of reaching the
network. Every test that needs a repository writes one into ``tmp_path`` and
points the scanner at it through a local ``file://`` URL, which git clone
accepts and which keeps the suite hermetic.
"""

from __future__ import annotations

import json
from pathlib import Path
import subprocess
import sys

import pytest
from scripts import check_adapter_pointer_drift as drift
from scripts import generate_adapter_pointer as generator

REPO_ROOT = Path(__file__).resolve().parent.parent
CATALOG = REPO_ROOT / "dcc-mcp-catalog.yml"


@pytest.fixture(scope="module")
def catalog():
    return generator.load_catalog(CATALOG)


@pytest.fixture(scope="module")
def adapters(catalog):
    return generator.adapter_entries(catalog)


def _make_origin(tmp_path: Path, name: str, readme: str, *, branch: str = "main") -> str:
    """Create a bare-ish git repository whose HEAD is ``branch`` and return its path.

    A plain (non-bare) repository is used because ``git clone`` of a working
    tree works unconfigured, while cloning a bare one needs no extra flags
    either; the working form keeps ``rev-parse --abbrev-ref HEAD`` meaningful,
    which is what the scanner reads to report the branch it checked.
    """
    origin = tmp_path / f"origin-{name}"
    origin.mkdir()
    (origin / "README.md").write_text(readme, encoding="utf-8", newline="")
    subprocess.run(["git", "init", "-q", "-b", branch, str(origin)], check=True)
    subprocess.run(["git", "-C", str(origin), "add", "README.md"], check=True)
    subprocess.run(
        [
            "git",
            "-C",
            str(origin),
            "-c",
            "user.email=tests@example.com",
            "-c",
            "user.name=Tests",
            "commit",
            "-q",
            "-m",
            "readme",
        ],
        check=True,
    )
    return str(origin)


def _entry(adapters, name):
    return next(e for e in adapters if e["name"] == name)


def _local_entry(adapters, name, origin_path: str) -> dict:
    """Build a catalog entry whose URL is a local path, so cloning stays offline."""
    entry = dict(_entry(adapters, name))
    entry["url"] = origin_path
    return entry


def _fake_scan(*rows):
    """Replace ``drift.scan`` with a callable that yields fixed rows.

    ``main`` calls ``scan(entries, workdir=..., only=..., timeout=...)``; only
    the timeout is forwarded to ``check_repository``, so the stub filters the
    keywords the real function accepts instead of forwarding them blindly.
    """
    pending = list(rows)

    def _scan(entries, *, workdir, only=None, timeout=60):
        return [drift.check_repository(entry, 0, host_count=47, workdir=workdir, timeout=timeout) for entry in pending]

    return _scan


# --- verdicts --------------------------------------------------------------


def test_a_repository_with_the_current_block_is_reported_current(adapters, tmp_path):
    """The happy path: replaying the generator changes nothing, so exit 0."""
    name = "dcc-mcp-krita"
    entry = _entry(adapters, name)
    readme = generator.upsert_pointer(
        "# krita\n\n## Install\n", generator.render_pointer(entry, host_count=len(adapters))
    )
    origin = _make_origin(tmp_path, "current", readme)

    row = drift.check_repository(
        _local_entry(adapters, name, origin),
        0,
        host_count=len(adapters),
        workdir=tmp_path / "work",
        timeout=60,
    )
    assert row["status"] == drift.CURRENT
    assert row["name"] == name
    assert str(len(adapters)) in row["detail"]


def test_a_stale_host_count_is_reported_as_drift(adapters, tmp_path):
    """The real-world failure: a block rendered when the catalog had fewer adapters."""
    name = "dcc-mcp-maya"
    entry = _entry(adapters, name)
    # Render against a catalog that had 38 adapters, then verify with today's
    # count -- the exact drift the ecosystem shipped.
    stale = generator.upsert_pointer("# maya\n\n## Install\n", generator.render_pointer(entry, host_count=38))
    assert f"**38 host adapters**" in stale  # noqa: F541 - literal is the point
    origin = _make_origin(tmp_path, "stale", stale)

    row = drift.check_repository(
        _local_entry(adapters, name, origin),
        0,
        host_count=len(adapters),
        workdir=tmp_path / "work",
        timeout=60,
    )
    assert row["status"] == drift.DRIFTED
    assert "does not match" in row["detail"]


def test_a_readme_without_a_pointer_block_is_missing(adapters, tmp_path):
    name = "dcc-mcp-maya"
    origin = _make_origin(tmp_path, "noblock", "# maya\n\n## Install\n")

    row = drift.check_repository(
        _local_entry(adapters, name, origin),
        0,
        host_count=len(adapters),
        workdir=tmp_path / "work",
        timeout=60,
    )
    assert row["status"] == drift.MISSING
    assert "no generated pointer block" in row["detail"]


def test_a_repository_with_no_readme_is_missing(adapters, tmp_path):
    """Sixteen catalog adapters are in this state today, so it must not crash."""
    origin = tmp_path / "origin-noreadme"
    origin.mkdir()
    (origin / "pyproject.toml").write_text("[project]\nname = 'x'\n", encoding="utf-8")
    subprocess.run(["git", "init", "-q", "-b", "main", str(origin)], check=True)
    subprocess.run(["git", "-C", str(origin), "add", "pyproject.toml"], check=True)
    subprocess.run(
        [
            "git",
            "-C",
            str(origin),
            "-c",
            "user.email=tests@example.com",
            "-c",
            "user.name=Tests",
            "commit",
            "-q",
            "-m",
            "no readme",
        ],
        check=True,
    )

    row = drift.check_repository(
        _local_entry(adapters, "dcc-mcp-maya", str(origin)),
        0,
        host_count=len(adapters),
        workdir=tmp_path / "work",
        timeout=60,
    )
    assert row["status"] == drift.MISSING
    assert "no README.md" in row["detail"]


def test_an_unclonable_url_is_an_error_not_a_crash(adapters, tmp_path):
    """A deleted or renamed repository must be reported, not raise out of the scan."""
    row = drift.check_repository(
        _local_entry(adapters, "dcc-mcp-maya", str(tmp_path / "does-not-exist")),
        0,
        host_count=len(adapters),
        workdir=tmp_path / "work",
        timeout=60,
    )
    assert row["status"] == drift.ERROR
    assert "clone failed" in row["detail"]


def test_an_entry_without_a_url_is_an_error(adapters, tmp_path):
    row = drift.check_repository(
        {"name": "dcc-mcp-nourl", "url": ""},
        0,
        host_count=len(adapters),
        workdir=tmp_path / "work",
        timeout=60,
    )
    assert row["status"] == drift.ERROR
    assert "no `url`" in row["detail"]


def test_crlf_readme_is_not_reported_as_drift(adapters, tmp_path):
    """Adapter READMEs are CRLF on Windows; the line endings must not count as drift."""
    name = "dcc-mcp-krita"
    entry = _entry(adapters, name)
    block = generator.upsert_pointer(
        "# krita\n\n## Install\n", generator.render_pointer(entry, host_count=len(adapters))
    )
    origin = _make_origin(tmp_path, "crlf", block.replace("\n", "\r\n"))

    row = drift.check_repository(
        _local_entry(adapters, name, origin),
        0,
        host_count=len(adapters),
        workdir=tmp_path / "work",
        timeout=60,
    )
    assert row["status"] == drift.CURRENT


def test_check_repository_leaves_no_clone_behind(adapters, tmp_path):
    """Forty-seven clones must not accumulate on a runner's disk."""
    name = "dcc-mcp-krita"
    entry = _entry(adapters, name)
    readme = generator.upsert_pointer(
        "# krita\n\n## Install\n", generator.render_pointer(entry, host_count=len(adapters))
    )
    origin = _make_origin(tmp_path, "cleanup", readme)
    workdir = tmp_path / "work"
    workdir.mkdir()

    drift.check_repository(
        _local_entry(adapters, name, origin),
        0,
        host_count=len(adapters),
        workdir=workdir,
        timeout=60,
    )
    assert list(workdir.iterdir()) == []


# --- selection and reporting ----------------------------------------------


def test_scan_honours_the_only_filter(adapters, tmp_path):
    """`--only` exists so a maintainer can check one repository without a full walk."""
    entry = _entry(adapters, "dcc-mcp-krita")
    readme = generator.upsert_pointer(
        "# krita\n\n## Install\n", generator.render_pointer(entry, host_count=len(adapters))
    )
    origin = _make_origin(tmp_path, "filtered", readme)
    entries = [_local_entry(adapters, "dcc-mcp-krita", origin)]

    assert len(drift.scan(entries, workdir=tmp_path / "w", timeout=60)) == 1
    assert drift.scan(entries, workdir=tmp_path / "w", only={"dcc-mcp-maya"}, timeout=60) == []
    assert len(drift.scan(entries, workdir=tmp_path / "w", only={"dcc-mcp-krita"}, timeout=60)) == 1


def test_render_summary_reports_the_catalog_count(adapters):
    rows = [
        {"name": "a", "url": "", "status": drift.CURRENT, "branch": "main", "detail": "ok"},
        {
            "name": "b",
            "url": "",
            "status": drift.DRIFTED,
            "branch": "main",
            "detail": "stale count",
        },
    ]
    summary = drift.render_summary(rows, host_count=len(adapters))
    assert f"**{len(adapters)}**" in summary
    assert "1 of 2 repositories drift" in summary


def test_render_summary_reports_a_clean_scan():
    rows = [{"name": "a", "url": "", "status": drift.CURRENT, "branch": "main", "detail": "ok"}]
    summary = drift.render_summary(rows, host_count=47)
    assert "All 1 scanned repositories match" in summary


# --- cli -------------------------------------------------------------------


def test_cli_fails_on_drift_and_passes_when_current(adapters, tmp_path, monkeypatch):
    """The CI gate: exit 1 with an annotation on drift, exit 0 when everything matches."""
    name = "dcc-mcp-maya"
    entry = _entry(adapters, name)
    stale = generator.upsert_pointer("# maya\n\n## Install\n", generator.render_pointer(entry, host_count=38))
    origin = _make_origin(tmp_path, "cli-stale", stale)
    local = _local_entry(adapters, name, origin)

    monkeypatch.setattr(drift, "scan", _fake_scan(local))
    monkeypatch.delenv("GITHUB_STEP_SUMMARY", raising=False)

    assert drift.main(["--catalog", str(CATALOG), "--only", name]) == 1

    fresh = generator.upsert_pointer(
        "# maya\n\n## Install\n", generator.render_pointer(entry, host_count=len(adapters))
    )
    fresh_origin = _make_origin(tmp_path, "cli-fresh", fresh)
    fresh_entry = _local_entry(adapters, name, fresh_origin)
    monkeypatch.setattr(drift, "scan", _fake_scan(fresh_entry))
    assert drift.main(["--catalog", str(CATALOG), "--only", name]) == 0


def test_cli_allow_missing_treats_a_missing_block_as_acceptable(adapters, tmp_path, monkeypatch):
    """Adapters that never received a block must not block the gate that catches drift."""
    origin = _make_origin(tmp_path, "cli-missing", "# maya\n\n## Install\n")
    local = _local_entry(adapters, "dcc-mcp-maya", origin)
    monkeypatch.setattr(drift, "scan", _fake_scan(local))
    monkeypatch.delenv("GITHUB_STEP_SUMMARY", raising=False)

    assert drift.main(["--catalog", str(CATALOG), "--only", "dcc-mcp-maya"]) == 1
    assert drift.main(["--catalog", str(CATALOG), "--only", "dcc-mcp-maya", "--allow-missing"]) == 0


def test_cli_rejects_an_unknown_adapter_name(tmp_path):
    assert drift.main(["--catalog", str(CATALOG), "--only", "dcc-mcp-nope"]) == 2


def test_cli_writes_json_results(adapters, tmp_path, monkeypatch):
    entry = _entry(adapters, "dcc-mcp-krita")
    readme = generator.upsert_pointer(
        "# krita\n\n## Install\n", generator.render_pointer(entry, host_count=len(adapters))
    )
    origin = _make_origin(tmp_path, "cli-json", readme)
    local = _local_entry(adapters, "dcc-mcp-krita", origin)
    monkeypatch.setattr(drift, "scan", _fake_scan(local))
    monkeypatch.delenv("GITHUB_STEP_SUMMARY", raising=False)

    out = tmp_path / "drift.json"
    assert (
        drift.main(
            [
                "--catalog",
                str(CATALOG),
                "--only",
                "dcc-mcp-krita",
                "--json-out",
                str(out),
            ]
        )
        == 0
    )
    payload = json.loads(out.read_text(encoding="utf-8"))
    assert payload["adapter_count"] == len(adapters)
    assert payload["results"][0]["status"] == drift.CURRENT


def test_cli_appends_the_summary_to_the_github_step_file(adapters, tmp_path, monkeypatch):
    entry = _entry(adapters, "dcc-mcp-krita")
    readme = generator.upsert_pointer(
        "# krita\n\n## Install\n", generator.render_pointer(entry, host_count=len(adapters))
    )
    origin = _make_origin(tmp_path, "cli-summary", readme)
    local = _local_entry(adapters, "dcc-mcp-krita", origin)
    monkeypatch.setattr(drift, "scan", _fake_scan(local))

    step_summary = tmp_path / "step-summary.md"
    step_summary.write_text("## earlier step\n", encoding="utf-8")
    monkeypatch.setenv("GITHUB_STEP_SUMMARY", str(step_summary))

    assert drift.main(["--catalog", str(CATALOG), "--only", "dcc-mcp-krita", "--summary"]) == 0
    written = step_summary.read_text(encoding="utf-8")
    assert written.startswith("## earlier step\n")
    assert "Adapter pointer drift" in written


def test_safe_dirname_avoids_collisions_and_unsafe_characters():
    assert drift._safe_dirname("dcc-mcp-maya", 3) == "003-dcc-mcp-maya"
    assert drift._safe_dirname("../../etc/passwd", 4) == "004-.._.._etc_passwd"
    # The index prefix keeps two entries with names that sanitise identically apart.
    assert drift._safe_dirname("a/b", 1) != drift._safe_dirname("a/b", 2)
