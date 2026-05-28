"""End-to-end regression for adapter-side admin skill-paths reload (#1400).

Reproduces the workflow:

    operator opens admin UI → POST /admin/api/skill-paths → row appears in
    gateway_admin.sqlite → adapter (Maya / Blender / …) must rediscover
    its catalog so the new path's skills become visible.

Pre-#1400 only the standalone ``dcc-mcp-server`` binary picked these up via
its ``catalog_discover_hook``; per-DCC adapter processes did not. This
suite covers the new ``DccServerBase.reload_skill_paths()`` path and the
``collect_skill_search_paths(include_admin_custom=True)`` default.

The test writes to the SQLite file directly using stdlib sqlite3 (matching
the gateway writer's schema and behaviour) so we don't need to spin up a
real gateway process.
"""

from __future__ import annotations

from pathlib import Path
import sqlite3
import time
from typing import Sequence

import pytest

from dcc_mcp_core import admin_sqlite_lane
from dcc_mcp_core.admin_sqlite_lane import GATEWAY_ADMIN_SQLITE_FILENAME
from dcc_mcp_core.admin_sqlite_lane import filter_new_paths
from dcc_mcp_core.admin_sqlite_lane import read_custom_skill_paths
from dcc_mcp_core.admin_sqlite_lane import resolve_admin_db_path

_SKILL_PATHS_DDL = """
CREATE TABLE IF NOT EXISTS skill_paths_custom (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  path TEXT NOT NULL UNIQUE,
  created_ms INTEGER NOT NULL
)
"""


def _write_admin_skill_path(db_path: Path, value: str) -> None:
    """Mimic ``POST /admin/api/skill-paths`` against the gateway SQLite lane.

    Uses the same DDL as ``crates/dcc-mcp-db/src/infra/gateway_admin_schema.rs``.
    """
    db_path.parent.mkdir(parents=True, exist_ok=True)
    with sqlite3.connect(db_path, timeout=2.0) as conn:
        conn.execute(_SKILL_PATHS_DDL)
        conn.execute(
            "INSERT OR IGNORE INTO skill_paths_custom (path, created_ms) VALUES (?, ?)",
            (str(value), int(time.time() * 1000)),
        )
        conn.commit()


def _write_skill(skill_root: Path, name: str) -> None:
    skill_dir = skill_root / name
    skill_dir.mkdir(parents=True)
    (skill_dir / "scripts").mkdir()
    (skill_dir / "SKILL.md").write_text(
        "\n".join(
            [
                "---",
                f"name: {name}",
                f"description: e2e fixture {name}",
                "metadata:",
                "  dcc-mcp:",
                "    dcc: maya",
                "---",
                f"# {name}",
                "",
            ]
        ),
        encoding="utf-8",
    )


@pytest.fixture
def registry_dir(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> Path:
    """Isolate the admin SQLite file under tmp_path via the env var."""
    monkeypatch.setenv("DCC_MCP_REGISTRY_DIR", str(tmp_path))
    return tmp_path


def test_resolve_admin_db_path_honours_env(registry_dir: Path) -> None:
    resolved = resolve_admin_db_path()
    assert resolved == registry_dir / GATEWAY_ADMIN_SQLITE_FILENAME


def test_resolve_admin_db_path_explicit_wins(registry_dir: Path, tmp_path_factory: pytest.TempPathFactory) -> None:
    explicit = tmp_path_factory.mktemp("explicit") / "custom.sqlite"
    assert resolve_admin_db_path(explicit) == explicit


def test_read_custom_skill_paths_missing_db_is_empty(registry_dir: Path) -> None:
    # No SQLite file yet → empty list, no crash.
    assert read_custom_skill_paths() == []


def test_read_custom_skill_paths_round_trip(registry_dir: Path) -> None:
    skill_root = registry_dir / "studio-skills"
    _write_skill(skill_root, "ext-render")
    db_path = registry_dir / GATEWAY_ADMIN_SQLITE_FILENAME
    _write_admin_skill_path(db_path, str(skill_root))

    rows = read_custom_skill_paths()
    assert rows == [str(skill_root)]


def test_read_custom_skill_paths_filters_missing_dirs(registry_dir: Path) -> None:
    db_path = registry_dir / GATEWAY_ADMIN_SQLITE_FILENAME
    _write_admin_skill_path(db_path, str(registry_dir / "does-not-exist"))

    assert read_custom_skill_paths() == []
    # Diagnostic mode returns the row even when the dir is gone.
    assert read_custom_skill_paths(require_exists=False) == [str(registry_dir / "does-not-exist")]


def test_filter_new_paths_dedupes(registry_dir: Path) -> None:
    known: Sequence[str] = ["a", "b"]
    rows: Sequence[str] = ["b", "c", "a", "d", "c"]
    assert filter_new_paths(known, rows) == ["c", "d"]


def test_server_base_reload_picks_up_admin_path(registry_dir: Path, tmp_path_factory: pytest.TempPathFactory) -> None:
    """The key #1400 regression: a path added to admin SQLite after the
    adapter has started must become visible after ``reload_skill_paths``.
    """
    skill_root = tmp_path_factory.mktemp("studio-skills")
    _write_skill(skill_root, "ext-rigging")

    # Simulate a running adapter: build a DccServerBase but don't start
    # the HTTP listener (we only need its catalog wiring for this test).
    from dcc_mcp_core import DccServerBase
    from dcc_mcp_core._server.options import DccServerOptions

    builtin = tmp_path_factory.mktemp("builtin-skills")
    opts = DccServerOptions.from_env(
        dcc_name="maya",
        builtin_skills_dir=builtin,
        server_name="test-maya",
        port=0,
        gateway_port=None,
        enable_gateway_failover=False,
    )
    server = DccServerBase(opts)

    def _skill_names(rows: object) -> set[str]:
        # `list_skills()` returns either `SkillSummary` objects or dicts
        # depending on the build path — handle both for portability.
        out: set[str] = set()
        for row in rows:  # type: ignore[union-attr]
            name = row["name"] if isinstance(row, dict) else getattr(row, "name", None)
            if name:
                out.add(name)
        return out

    # Before the operator adds the path, the catalog has no fixture.
    server.register_builtin_actions(include_bundled=False)
    initial = _skill_names(server.list_skills())
    assert "ext-rigging" not in initial, f"fixture leaked into initial discovery: {initial}"

    # Operator clicks "Add path" in the admin UI → row lands in SQLite.
    db_path = registry_dir / GATEWAY_ADMIN_SQLITE_FILENAME
    _write_admin_skill_path(db_path, str(skill_root))

    # The new public reload API picks it up without restarting the server.
    count = server.reload_skill_paths(include_bundled=False)
    assert count >= 1
    after = _skill_names(server.list_skills())
    assert "ext-rigging" in after, f"reload_skill_paths did not pick up admin SQLite path; got {after}"


def test_module_re_exports() -> None:
    """The public symbols are reachable from the top-level package."""
    import dcc_mcp_core

    assert dcc_mcp_core.resolve_admin_db_path is admin_sqlite_lane.resolve_admin_db_path
    assert dcc_mcp_core.read_custom_skill_paths is admin_sqlite_lane.read_custom_skill_paths
