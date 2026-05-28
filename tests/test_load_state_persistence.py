"""End-to-end regression for SkillCatalog load-state persistence (#1405).

Simulates a full DCC adapter lifecycle:

1. Start a SkillCatalog, load skills + activate a group, then dump the
   in-memory state through :class:`LoadedStateStore` to a per-DCC JSON
   file (and the gateway admin sqlite mirror).
2. Throw the catalog away (= "restart").
3. Re-open the store on a fresh catalog and call ``replay_loaded``;
   verify the previous loaded set + active groups are restored.
4. Exercise the drift policy: bump a skill's version on disk and confirm
   ``skip_on_drift`` (the default) drops it from the replay while
   ``ignore_version`` re-loads it.
"""

from __future__ import annotations

import json
import os
from pathlib import Path

import pytest

from dcc_mcp_core import SkillCatalog
from dcc_mcp_core import ToolRegistry
from dcc_mcp_core.admin_sqlite_lane import resolve_admin_db_path
from dcc_mcp_core.loaded_state_store import LoadedStateStore
from dcc_mcp_core.loaded_state_store import PersistedCatalogState


def _write_skill(
    skills_root: Path,
    name: str,
    *,
    description: str,
    version: str = "1.0.0",
    dcc: str = "maya",
    groups: list[str] | None = None,
) -> None:
    skill_dir = skills_root / name
    skill_dir.mkdir(parents=True)
    (skill_dir / "scripts").mkdir()
    lines = [
        "---",
        f"name: {name}",
        f"description: {description}",
        "metadata:",
        "  dcc-mcp:",
        f"    dcc: {dcc}",
        f"    version: {version}",
    ]
    if groups:
        lines.append("groups:")
        for g in groups:
            lines.extend([f"  - name: {g}", "    default-active: false"])
    lines += ["---", f"# {name}", ""]
    (skill_dir / "SKILL.md").write_text("\n".join(lines), encoding="utf-8")


@pytest.fixture
def isolated_admin_env(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> Path:
    """Point HOME + admin sqlite at tmp_path so tests never touch real files."""
    monkeypatch.setenv("HOME", str(tmp_path))
    # Windows respects USERPROFILE; set both for cross-platform safety.
    monkeypatch.setenv("USERPROFILE", str(tmp_path))
    monkeypatch.setenv("DCC_MCP_GATEWAY_ADMIN_DB", str(tmp_path / "admin.sqlite"))
    return tmp_path


def test_store_roundtrip_to_json(isolated_admin_env: Path) -> None:
    store = LoadedStateStore("maya")
    store.record_loaded("maya-render", version="1.0.0", skill_path="/skills/maya-render")
    store.record_loaded("maya-export", version="2.0.0", skill_path="/skills/maya-export")
    store.record_group_change("rigging", activated=True)
    store.record_group_change("animation", activated=True)

    # Re-open: the on-disk file is the source of truth.
    reopened = LoadedStateStore("maya")
    names = sorted(s.name for s in reopened.state.skills)
    assert names == ["maya-export", "maya-render"]
    assert sorted(reopened.state.active_groups) == ["animation", "rigging"]
    versions = {s.name: s.version for s in reopened.state.skills}
    assert versions == {"maya-render": "1.0.0", "maya-export": "2.0.0"}


def test_store_evicts_on_unload(isolated_admin_env: Path) -> None:
    store = LoadedStateStore("maya")
    store.record_loaded("maya-render", version="1.0.0", skill_path=None)
    store.record_loaded("maya-export", version="1.0.0", skill_path=None)
    store.record_unloaded("maya-render")

    reopened = LoadedStateStore("maya")
    assert [s.name for s in reopened.state.skills] == ["maya-export"]


def test_admin_sqlite_mirror_writes_rows(isolated_admin_env: Path) -> None:
    store = LoadedStateStore("maya")
    store.record_loaded("maya-render", version="1.0.0", skill_path=None)
    store.record_group_change("rigging", activated=True)

    db = resolve_admin_db_path()
    assert db.exists(), f"admin sqlite mirror file missing: {db}"
    import sqlite3

    with sqlite3.connect(str(db)) as conn:
        rows = conn.execute("SELECT dcc_type, skill_name FROM skill_loaded_state ORDER BY skill_name").fetchall()
        groups = conn.execute("SELECT dcc_type, group_name FROM skill_active_groups ORDER BY group_name").fetchall()
    assert rows == [("maya", "maya-render")]
    assert groups == [("maya", "rigging")]


def test_catalog_replay_restores_loaded_set(tmp_path: Path, isolated_admin_env: Path) -> None:
    """Full restart simulation against a real SkillCatalog.

    Discovers two skills, loads them, persists via the store, then opens
    a fresh catalog and replays — every loaded skill must come back.
    """
    skills_root = tmp_path / "skills"
    _write_skill(skills_root, "maya-render", description="render bake helpers", version="1.0.0")
    _write_skill(skills_root, "maya-export", description="usd export", version="2.1.0")

    # Session 1: load both, activate a group, persist.
    catalog_a = SkillCatalog(ToolRegistry())
    catalog_a.discover(extra_paths=[str(skills_root)], dcc_name="maya")
    catalog_a.load_skill("maya-render")
    catalog_a.load_skill("maya-export")
    catalog_a.activate_group("rigging")

    def _attr(meta: object, name: str) -> str | None:
        if meta is None:
            return None
        if hasattr(meta, name):
            return getattr(meta, name) or None
        if isinstance(meta, dict):
            return meta.get(name) or None
        return None

    store = LoadedStateStore("maya")
    for name in ("maya-render", "maya-export"):
        meta = catalog_a.get_skill_info(name)
        store.record_loaded(name, version=_attr(meta, "version"), skill_path=_attr(meta, "skill_path"))
    store.record_group_change("rigging", activated=True)

    # Session 2: brand-new catalog, same on-disk skills, replay from store.
    catalog_b = SkillCatalog(ToolRegistry())
    catalog_b.discover(extra_paths=[str(skills_root)], dcc_name="maya")
    snapshot = LoadedStateStore("maya").snapshot()
    report_json = catalog_b.replay_loaded(json.dumps(snapshot.to_json()), "skip_on_drift")
    report = json.loads(report_json)

    assert sorted(report["loaded"]) == ["maya-export", "maya-render"]
    assert report["missing"] == []
    assert report["skipped_drift"] == []
    assert report["failed"] == []
    assert "rigging" in report["activated_groups"]
    assert catalog_b.is_loaded("maya-render")
    assert catalog_b.is_loaded("maya-export")
    assert "rigging" in catalog_b.active_groups()


def test_catalog_replay_skips_on_version_drift(tmp_path: Path, isolated_admin_env: Path) -> None:
    skills_root = tmp_path / "skills"
    _write_skill(skills_root, "maya-render", description="render bake helpers", version="2.0.0")

    # Persist a state that says we previously loaded v1.0.0.
    state = PersistedCatalogState()
    from dcc_mcp_core.loaded_state_store import LoadedSkillRecord

    state.skills.append(LoadedSkillRecord(name="maya-render", version="1.0.0", skill_path=None, loaded_at_ms=0))

    catalog = SkillCatalog(ToolRegistry())
    catalog.discover(extra_paths=[str(skills_root)], dcc_name="maya")
    report = json.loads(catalog.replay_loaded(json.dumps(state.to_json()), "skip_on_drift"))
    assert report["loaded"] == []
    assert len(report["skipped_drift"]) == 1
    assert report["skipped_drift"][0]["name"] == "maya-render"
    assert report["skipped_drift"][0]["current_version"] == "2.0.0"
    assert not catalog.is_loaded("maya-render")

    # ignore_version policy on the same drift loads anyway.
    report2 = json.loads(catalog.replay_loaded(json.dumps(state.to_json()), "ignore_version"))
    assert report2["loaded"] == ["maya-render"]
    assert catalog.is_loaded("maya-render")


def test_catalog_replay_records_missing_skill(tmp_path: Path, isolated_admin_env: Path) -> None:
    skills_root = tmp_path / "skills"
    _write_skill(skills_root, "maya-render", description="render bake helpers", version="1.0.0")

    state = PersistedCatalogState()
    from dcc_mcp_core.loaded_state_store import LoadedSkillRecord

    state.skills.append(LoadedSkillRecord(name="maya-render", version="1.0.0"))
    state.skills.append(LoadedSkillRecord(name="vanished-skill", version="1.0.0"))

    catalog = SkillCatalog(ToolRegistry())
    catalog.discover(extra_paths=[str(skills_root)], dcc_name="maya")
    report = json.loads(catalog.replay_loaded(json.dumps(state.to_json()), "skip_on_drift"))
    assert report["loaded"] == ["maya-render"]
    assert report["missing"] == ["vanished-skill"]
