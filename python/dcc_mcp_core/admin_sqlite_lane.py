"""Read-only helpers for the gateway admin SQLite lane (issue #1400).

Mirrors the Rust path-resolution and `skill_paths_custom` query so that
Python-side adapters (Maya, 3ds Max, Blender, Houdini, …) can pick up
admin-UI-added skill discovery roots **without** spawning a gateway
process or going through any HTTP API.

Why a thin Python module instead of a PyO3 binding?

* The SQLite file is plain WAL-mode `sqlite3` — Python's stdlib speaks
  it natively. Adding a PyO3 wrapper just to read one column is more
  build/cargo lock churn than it's worth for a read path.
* Keeping the resolver here in Python lets adapter code call it from
  pre-startup hooks (e.g. bootstrap scripts) that don't yet hold a
  `DccServerBase` reference.

Resolution order (matches `crates/dcc-mcp-db/src/application/gateway_admin.rs`):

  1. ``explicit`` argument
  2. ``DCC_MCP_GATEWAY_ADMIN_DB`` env var
  3. ``<registry_dir or DCC_MCP_REGISTRY_DIR or temp>/gateway_admin.sqlite``
"""

from __future__ import annotations

import logging
import os
from pathlib import Path
import sqlite3
import tempfile
from typing import Sequence

logger = logging.getLogger(__name__)

# Mirror of the Rust constants in `crates/dcc-mcp-db/src/domain/env.rs`.
# Keep both lists in sync if either side changes.
ENV_GATEWAY_ADMIN_DB = "DCC_MCP_GATEWAY_ADMIN_DB"
ENV_REGISTRY_DIR = "DCC_MCP_REGISTRY_DIR"
GATEWAY_ADMIN_SQLITE_FILENAME = "gateway_admin.sqlite"


def resolve_admin_db_path(
    explicit: str | os.PathLike[str] | None = None,
    registry_dir: str | os.PathLike[str] | None = None,
) -> Path:
    """Resolve the admin SQLite path using the same rules as the Rust gateway.

    Always returns a :class:`~pathlib.Path` — the file may or may not exist
    on disk. Callers should test ``.exists()`` before opening when the gateway
    might not have run on this machine yet.
    """
    if explicit is not None:
        return Path(explicit)
    env_db = os.environ.get(ENV_GATEWAY_ADMIN_DB)
    if env_db:
        return Path(env_db)
    base: Path
    if registry_dir is not None:
        base = Path(registry_dir)
    else:
        env_reg = os.environ.get(ENV_REGISTRY_DIR)
        base = Path(env_reg) if env_reg else Path(tempfile.gettempdir()) / "dcc-mcp-registry"
    return base / GATEWAY_ADMIN_SQLITE_FILENAME


def read_custom_skill_paths(
    db_path: str | os.PathLike[str] | None = None,
    *,
    registry_dir: str | os.PathLike[str] | None = None,
    require_exists: bool = True,
) -> list[str]:
    """Return all admin-UI-added skill discovery roots, in insertion order.

    Args:
        db_path: Explicit SQLite path. ``None`` resolves via
            :func:`resolve_admin_db_path`.
        registry_dir: Forwarded to :func:`resolve_admin_db_path` when
            ``db_path`` is ``None``.
        require_exists: When ``True`` (default), each returned path is
            filtered through :py:meth:`Path.is_dir` so callers don't try
            to scan a directory that was removed off-disk after being
            persisted. Set to ``False`` to get the raw rows for
            diagnostics.

    Returns:
        List of absolute path strings. Empty when the SQLite file does
        not exist, lacks the table, or yields no rows — callers can
        treat the return value as a best-effort additive set.

    """
    resolved = resolve_admin_db_path(db_path, registry_dir)
    if not resolved.exists():
        return []
    # Open read-only (URI form) so we never race the gateway writer.
    uri = f"file:{resolved.as_posix()}?mode=ro"
    rows: list[str] = []
    try:
        with sqlite3.connect(uri, uri=True, timeout=0.5) as conn:
            cur = conn.execute("SELECT path FROM skill_paths_custom ORDER BY id ASC")
            rows = [str(row[0]) for row in cur.fetchall()]
    except sqlite3.Error as exc:
        # Common during a fresh install: the gateway has never run, so
        # the file exists (empty) but the table doesn't. Treat as empty.
        logger.debug("[admin_sqlite_lane] read failed: %s", exc)
        return []
    if not require_exists:
        return rows
    return [p for p in rows if Path(p).is_dir()]


def filter_new_paths(known: Sequence[str], rows: Sequence[str]) -> list[str]:
    """Return ``rows`` minus anything already in ``known`` (preserving order).

    Convenience helper for callers that merge admin SQLite paths into a
    larger discovery path list and want to dedupe cheaply.
    """
    seen = set(known)
    out: list[str] = []
    for p in rows:
        if p not in seen:
            seen.add(p)
            out.append(p)
    return out


# ── skill_loaded_state mirror (#1405) ─────────────────────────────────────

_LOADED_STATE_DDL = """
CREATE TABLE IF NOT EXISTS skill_loaded_state (
  dcc_type TEXT NOT NULL,
  skill_name TEXT NOT NULL,
  skill_version TEXT,
  skill_path TEXT,
  loaded_at_ms INTEGER NOT NULL,
  PRIMARY KEY (dcc_type, skill_name)
);
CREATE TABLE IF NOT EXISTS skill_active_groups (
  dcc_type TEXT NOT NULL,
  group_name TEXT NOT NULL,
  activated_at_ms INTEGER NOT NULL,
  PRIMARY KEY (dcc_type, group_name)
);
CREATE INDEX IF NOT EXISTS idx_skill_loaded_state_dcc ON skill_loaded_state(dcc_type);
CREATE INDEX IF NOT EXISTS idx_skill_active_groups_dcc ON skill_active_groups(dcc_type);
"""


def _open_rw(db_path: Path) -> sqlite3.Connection | None:
    """Open the admin sqlite read/write; create the directory if needed.

    Returns ``None`` on failure (e.g. read-only filesystem). The mirror
    layer is best-effort; failures here must not break the catalog hooks.
    """
    try:
        db_path.parent.mkdir(parents=True, exist_ok=True)
    except OSError as exc:
        logger.debug("[admin_sqlite_lane] mkdir failed: %s", exc)
        return None
    try:
        conn = sqlite3.connect(str(db_path), timeout=0.5)
        conn.executescript(_LOADED_STATE_DDL)
        return conn
    except sqlite3.Error as exc:
        logger.debug("[admin_sqlite_lane] open rw failed: %s", exc)
        return None


def mirror_loaded_skill(
    dcc_type: str,
    skill_name: str,
    *,
    skill_version: str | None,
    skill_path: str | None,
    loaded_at_ms: int,
    db_path: str | os.PathLike[str] | None = None,
    registry_dir: str | os.PathLike[str] | None = None,
) -> bool:
    """Mirror a `load_skill` into ``skill_loaded_state``. Best-effort."""
    resolved = resolve_admin_db_path(db_path, registry_dir)
    conn = _open_rw(resolved)
    if conn is None:
        return False
    try:
        with conn:
            conn.execute(
                "INSERT OR REPLACE INTO skill_loaded_state "
                "(dcc_type, skill_name, skill_version, skill_path, loaded_at_ms) "
                "VALUES (?, ?, ?, ?, ?)",
                (dcc_type, skill_name, skill_version, skill_path, int(loaded_at_ms)),
            )
        return True
    except sqlite3.Error as exc:
        logger.debug("[admin_sqlite_lane] mirror_loaded_skill failed: %s", exc)
        return False
    finally:
        conn.close()


def mirror_unloaded_skill(
    dcc_type: str,
    skill_name: str,
    *,
    db_path: str | os.PathLike[str] | None = None,
    registry_dir: str | os.PathLike[str] | None = None,
) -> bool:
    """Mirror an ``unload_skill`` by deleting the row. Best-effort."""
    resolved = resolve_admin_db_path(db_path, registry_dir)
    conn = _open_rw(resolved)
    if conn is None:
        return False
    try:
        with conn:
            conn.execute(
                "DELETE FROM skill_loaded_state WHERE dcc_type = ? AND skill_name = ?",
                (dcc_type, skill_name),
            )
        return True
    except sqlite3.Error as exc:
        logger.debug("[admin_sqlite_lane] mirror_unloaded_skill failed: %s", exc)
        return False
    finally:
        conn.close()


def mirror_active_group(
    dcc_type: str,
    group_name: str,
    *,
    activated: bool,
    activated_at_ms: int,
    db_path: str | os.PathLike[str] | None = None,
    registry_dir: str | os.PathLike[str] | None = None,
) -> bool:
    """Mirror an active-group change into ``skill_active_groups``."""
    resolved = resolve_admin_db_path(db_path, registry_dir)
    conn = _open_rw(resolved)
    if conn is None:
        return False
    try:
        with conn:
            if activated:
                conn.execute(
                    "INSERT OR REPLACE INTO skill_active_groups "
                    "(dcc_type, group_name, activated_at_ms) VALUES (?, ?, ?)",
                    (dcc_type, group_name, int(activated_at_ms)),
                )
            else:
                conn.execute(
                    "DELETE FROM skill_active_groups WHERE dcc_type = ? AND group_name = ?",
                    (dcc_type, group_name),
                )
        return True
    except sqlite3.Error as exc:
        logger.debug("[admin_sqlite_lane] mirror_active_group failed: %s", exc)
        return False
    finally:
        conn.close()


__all__ = [
    "ENV_GATEWAY_ADMIN_DB",
    "ENV_REGISTRY_DIR",
    "GATEWAY_ADMIN_SQLITE_FILENAME",
    "filter_new_paths",
    "mirror_active_group",
    "mirror_loaded_skill",
    "mirror_unloaded_skill",
    "read_custom_skill_paths",
    "resolve_admin_db_path",
]
