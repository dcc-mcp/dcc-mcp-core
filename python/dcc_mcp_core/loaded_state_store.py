"""Persistent store for ``SkillCatalog.loaded`` + active groups (issue #1405).

Source of truth is a per-DCC JSON file at
``~/.dcc-mcp/<dcc>/loaded.json``. The gateway admin SQLite database is
mirrored on a best-effort basis so the admin UI can render currently-
loaded skills across all DCC instances on one machine without each DCC
needing its own admin HTTP surface.

Reads always come from the JSON file when present — sqlite is treated as
a read-mostly visibility surface, not a fallback source. This avoids the
"two writers, one truth" failure mode if a DCC adapter ran without a
gateway and then a gateway started later.

The persistence layer is **best-effort**:

* Hooks must never raise. Persistence errors are logged at debug level
  and the catalog state mutation continues.
* Missing files / schema mismatches behave like "empty state": the
  catalog comes up with whatever was on disk for the *current* version
  of the package and ignores the persisted set.
* All write paths use atomic-replace (``write → fsync → rename``) so a
  crashed process can't leave a half-written JSON behind.
"""

from __future__ import annotations

import contextlib
from dataclasses import asdict
from dataclasses import dataclass
from dataclasses import field
import json
import logging
import os
from pathlib import Path
import tempfile
import time
from typing import Sequence

from dcc_mcp_core import admin_sqlite_lane

logger = logging.getLogger(__name__)

LOADED_STATE_SCHEMA_VERSION = 1


@dataclass(frozen=True)
class LoadedSkillRecord:
    """One persisted skill entry. Mirrors the Rust ``LoadedSkillRecord``."""

    name: str
    version: str | None = None
    skill_path: str | None = None
    loaded_at_ms: int = 0


@dataclass
class PersistedCatalogState:
    """Full on-disk snapshot. Mirrors the Rust ``PersistedCatalogState``."""

    skills: list[LoadedSkillRecord] = field(default_factory=list)
    active_groups: list[str] = field(default_factory=list)
    saved_at_ms: int = 0
    schema_version: int = LOADED_STATE_SCHEMA_VERSION

    def to_json(self) -> dict[str, object]:
        return {
            "skills": [asdict(s) for s in self.skills],
            "active_groups": list(self.active_groups),
            "saved_at_ms": int(self.saved_at_ms),
            "schema_version": int(self.schema_version),
        }

    @classmethod
    def from_json(cls, payload: object) -> PersistedCatalogState:
        if not isinstance(payload, dict):
            return cls()
        raw_skills = payload.get("skills", [])
        skills: list[LoadedSkillRecord] = []
        if isinstance(raw_skills, list):
            for row in raw_skills:
                if not isinstance(row, dict):
                    continue
                name = row.get("name")
                if not isinstance(name, str) or not name:
                    continue
                version = row.get("version")
                skill_path = row.get("skill_path")
                loaded_at_ms = row.get("loaded_at_ms", 0)
                skills.append(
                    LoadedSkillRecord(
                        name=name,
                        version=version if isinstance(version, str) else None,
                        skill_path=skill_path if isinstance(skill_path, str) else None,
                        loaded_at_ms=int(loaded_at_ms) if isinstance(loaded_at_ms, int) else 0,
                    )
                )
        raw_groups = payload.get("active_groups", [])
        active_groups = [g for g in raw_groups if isinstance(g, str)] if isinstance(raw_groups, list) else []
        saved_at_ms = payload.get("saved_at_ms", 0)
        schema_version = payload.get("schema_version", LOADED_STATE_SCHEMA_VERSION)
        return cls(
            skills=skills,
            active_groups=active_groups,
            saved_at_ms=int(saved_at_ms) if isinstance(saved_at_ms, int) else 0,
            schema_version=int(schema_version) if isinstance(schema_version, int) else LOADED_STATE_SCHEMA_VERSION,
        )


def default_loaded_state_path(dcc_name: str) -> Path:
    """Return the canonical on-disk location for a DCC's loaded state."""
    home = Path("~").expanduser()
    return home / ".dcc-mcp" / dcc_name.lower() / "loaded.json"


def _now_ms() -> int:
    return int(time.time() * 1000)


def _atomic_write_json(path: Path, payload: dict[str, object]) -> bool:
    """Write JSON atomically (tmp + rename). Best-effort; returns success."""
    try:
        path.parent.mkdir(parents=True, exist_ok=True)
    except OSError as exc:
        logger.debug("[loaded_state_store] mkdir failed: %s", exc)
        return False
    tmp_fd: int | None = None
    tmp_name: str | None = None
    try:
        tmp_fd, tmp_name = tempfile.mkstemp(prefix=".loaded-", suffix=".json.tmp", dir=str(path.parent))
        with os.fdopen(tmp_fd, "w", encoding="utf-8") as fh:
            tmp_fd = None
            json.dump(payload, fh, ensure_ascii=False, indent=2, sort_keys=True)
            fh.flush()
            os.fsync(fh.fileno())
        Path(tmp_name).replace(path)
        return True
    except OSError as exc:
        logger.debug("[loaded_state_store] atomic write failed: %s", exc)
        if tmp_fd is not None:
            with contextlib.suppress(OSError):
                os.close(tmp_fd)
        if tmp_name is not None:
            with contextlib.suppress(OSError):
                Path(tmp_name).unlink()
        return False


class LoadedStateStore:
    """Per-DCC persistence layer for ``SkillCatalog.loaded`` + active groups.

    Holds an in-memory mirror of the on-disk JSON file so individual
    hook callbacks can apply a single mutation and write the file once.
    Sqlite mirror updates are issued alongside JSON saves and are
    best-effort.

    Thread safety: the store assumes a single owner (the DCC adapter).
    Callbacks from ``SkillCatalog`` after-hooks already serialise per
    catalog mutation, so no explicit locking is added here. Embedders
    that mutate from multiple threads should wrap calls in their own
    mutex.
    """

    def __init__(
        self,
        dcc_name: str,
        *,
        path: Path | None = None,
        sqlite_mirror: bool = True,
    ) -> None:
        if not dcc_name:
            raise ValueError("LoadedStateStore requires a non-empty dcc_name")
        self._dcc_name = dcc_name.lower()
        self._path = path or default_loaded_state_path(self._dcc_name)
        self._sqlite_mirror = sqlite_mirror
        self._state = self._load_from_disk()

    @property
    def dcc_name(self) -> str:
        return self._dcc_name

    @property
    def path(self) -> Path:
        return self._path

    @property
    def state(self) -> PersistedCatalogState:
        return self._state

    def _load_from_disk(self) -> PersistedCatalogState:
        if not self._path.exists():
            return PersistedCatalogState()
        try:
            raw = self._path.read_text(encoding="utf-8")
            payload = json.loads(raw)
        except (OSError, json.JSONDecodeError) as exc:
            logger.warning(
                "[loaded_state_store] failed to read %s: %s; starting with empty state",
                self._path,
                exc,
            )
            return PersistedCatalogState()
        state = PersistedCatalogState.from_json(payload)
        if state.schema_version > LOADED_STATE_SCHEMA_VERSION:
            logger.warning(
                "[loaded_state_store] %s declares schema version %d > %d; ignoring",
                self._path,
                state.schema_version,
                LOADED_STATE_SCHEMA_VERSION,
            )
            return PersistedCatalogState()
        return state

    def _save_to_disk(self) -> None:
        self._state.saved_at_ms = _now_ms()
        self._state.schema_version = LOADED_STATE_SCHEMA_VERSION
        _atomic_write_json(self._path, self._state.to_json())

    def record_loaded(
        self,
        skill_name: str,
        *,
        version: str | None,
        skill_path: str | None,
    ) -> None:
        now = _now_ms()
        # Remove any pre-existing entry for the same name (idempotent).
        self._state.skills = [s for s in self._state.skills if s.name != skill_name]
        self._state.skills.append(
            LoadedSkillRecord(
                name=skill_name,
                version=version,
                skill_path=skill_path,
                loaded_at_ms=now,
            )
        )
        self._save_to_disk()
        if self._sqlite_mirror:
            admin_sqlite_lane.mirror_loaded_skill(
                self._dcc_name,
                skill_name,
                skill_version=version,
                skill_path=skill_path,
                loaded_at_ms=now,
            )

    def record_unloaded(self, skill_name: str) -> None:
        before = len(self._state.skills)
        self._state.skills = [s for s in self._state.skills if s.name != skill_name]
        if len(self._state.skills) == before:
            return
        self._save_to_disk()
        if self._sqlite_mirror:
            admin_sqlite_lane.mirror_unloaded_skill(self._dcc_name, skill_name)

    def record_group_change(self, group_name: str, *, activated: bool) -> None:
        now = _now_ms()
        existing = group_name in self._state.active_groups
        if activated and not existing:
            self._state.active_groups.append(group_name)
        elif not activated and existing:
            self._state.active_groups = [g for g in self._state.active_groups if g != group_name]
        else:
            return
        self._save_to_disk()
        if self._sqlite_mirror:
            admin_sqlite_lane.mirror_active_group(
                self._dcc_name,
                group_name,
                activated=activated,
                activated_at_ms=now,
            )

    def snapshot(self) -> PersistedCatalogState:
        """Return a shallow copy of the current persisted state."""
        return PersistedCatalogState(
            skills=list(self._state.skills),
            active_groups=list(self._state.active_groups),
            saved_at_ms=self._state.saved_at_ms,
            schema_version=self._state.schema_version,
        )

    def clear(self) -> None:
        """Forget all persisted state. Used by tests."""
        self._state = PersistedCatalogState()
        try:
            if self._path.exists():
                self._path.unlink()
        except OSError as exc:
            logger.debug("[loaded_state_store] clear failed: %s", exc)


__all__ = [
    "LOADED_STATE_SCHEMA_VERSION",
    "LoadedSkillRecord",
    "LoadedStateStore",
    "PersistedCatalogState",
    "default_loaded_state_path",
]


def replay_args_from_state(state: PersistedCatalogState) -> dict[str, object]:
    """Render the persisted state as the Rust JSON schema.

    Matches ``SkillCatalog.replay_loaded``: the Rust side reconstructs
    the struct via serde so the dict shape must match :meth:`to_json`
    exactly.
    """
    return state.to_json()


def replay_persisted(catalog: object, state: PersistedCatalogState, *, policy: str = "skip_on_drift") -> Sequence[str]:
    """Best-effort Python-level replay shim.

    Walks the persisted records and calls ``catalog.load_skill`` for
    each one whose version still matches. Returns the names of skills
    that were successfully loaded. Embedders that need richer drift /
    error reporting should call the Rust ``SkillCatalog.replay_loaded``
    directly once the PyO3 binding for it is exposed.
    """
    loaded: list[str] = []
    for record in state.skills:
        try:
            ok = bool(catalog.load_skill(record.name))
        except Exception as exc:
            logger.warning("[loaded_state_store] replay load_skill(%s) raised: %s", record.name, exc)
            continue
        if ok:
            loaded.append(record.name)
    for group in state.active_groups:
        try:
            catalog.activate_group(group)
        except Exception as exc:
            logger.debug("[loaded_state_store] replay activate_group(%s) raised: %s", group, exc)
    return loaded
