"""Tests for the agent memory layers (issue #1334)."""

from __future__ import annotations

import time

import pytest

from dcc_mcp_core import HookContext
from dcc_mcp_core import HookEvent
from dcc_mcp_core import InMemoryMemoryStore
from dcc_mcp_core import LifecycleHooks
from dcc_mcp_core import MemoryEntry
from dcc_mcp_core import MemoryLayer
from dcc_mcp_core import MemoryQuery
from dcc_mcp_core import MemoryRecorder


def _entry(
    layer: MemoryLayer,
    key: str,
    sid: str = "s1",
    dcc: str = "maya",
    t: float | None = None,
):
    return MemoryEntry(
        layer=layer,
        key=key,
        session_id=sid,
        dcc_name=dcc,
        created_unix_secs=time.time() if t is None else t,
        payload={},
    )


class TestMemoryLayer:
    def test_parse_accepts_string_and_enum(self) -> None:
        assert MemoryLayer.parse("ephemeral") is MemoryLayer.EPHEMERAL
        assert MemoryLayer.parse("WORKING") is MemoryLayer.WORKING
        assert MemoryLayer.parse(MemoryLayer.LONGTERM) is MemoryLayer.LONGTERM

    def test_parse_rejects_unknown(self) -> None:
        with pytest.raises(ValueError):
            MemoryLayer.parse("ram")


class TestInMemoryStorePerLayerCaps:
    def test_ephemeral_per_session_cap_enforced(self) -> None:
        s = InMemoryMemoryStore(ephemeral_cap_per_session=3)
        for i in range(5):
            s.put(_entry(MemoryLayer.EPHEMERAL, f"k{i}"))
        rows = s.query(MemoryQuery(layer=MemoryLayer.EPHEMERAL))
        # Only the last 3 survive
        assert {r.key for r in rows} == {"k2", "k3", "k4"}

    def test_working_per_session_cap_enforced(self) -> None:
        s = InMemoryMemoryStore(working_cap_per_session=2)
        for i in range(4):
            s.put(_entry(MemoryLayer.WORKING, f"k{i}"))
        rows = s.query(MemoryQuery(layer=MemoryLayer.WORKING))
        assert {r.key for r in rows} == {"k2", "k3"}

    def test_longterm_total_cap_enforced(self) -> None:
        s = InMemoryMemoryStore(longterm_cap_total=2)
        for i in range(4):
            s.put(_entry(MemoryLayer.LONGTERM, f"k{i}"))
        rows = s.query(MemoryQuery(layer=MemoryLayer.LONGTERM))
        assert {r.key for r in rows} == {"k2", "k3"}


class TestInMemoryStoreFilters:
    def test_session_filter(self) -> None:
        s = InMemoryMemoryStore()
        s.put(_entry(MemoryLayer.EPHEMERAL, "k1", sid="A"))
        s.put(_entry(MemoryLayer.EPHEMERAL, "k2", sid="B"))
        rows = s.query(MemoryQuery(session_id="A"))
        assert [r.key for r in rows] == ["k1"]

    def test_dcc_filter(self) -> None:
        s = InMemoryMemoryStore()
        s.put(_entry(MemoryLayer.EPHEMERAL, "k1", dcc="maya"))
        s.put(_entry(MemoryLayer.EPHEMERAL, "k2", dcc="blender"))
        rows = s.query(MemoryQuery(dcc_name="blender"))
        assert [r.key for r in rows] == ["k2"]

    def test_key_prefix_filter(self) -> None:
        s = InMemoryMemoryStore()
        s.put(_entry(MemoryLayer.EPHEMERAL, "tool:foo"))
        s.put(_entry(MemoryLayer.EPHEMERAL, "skill:bar"))
        rows = s.query(MemoryQuery(key_prefix="tool:"))
        assert [r.key for r in rows] == ["tool:foo"]

    def test_working_ttl_filter(self) -> None:
        # ttl=10s; entries 200s in the past are expired
        s = InMemoryMemoryStore(working_ttl_secs=10)
        s.put(_entry(MemoryLayer.WORKING, "old", t=0.0))
        s.put(_entry(MemoryLayer.WORKING, "new", t=10_000_000_000.0))
        rows = s.query(MemoryQuery(layer=MemoryLayer.WORKING))
        assert [r.key for r in rows] == ["new"]

    def test_query_limit(self) -> None:
        s = InMemoryMemoryStore()
        for i in range(5):
            s.put(_entry(MemoryLayer.EPHEMERAL, f"k{i}", t=float(i)))
        rows = s.query(MemoryQuery(limit=2))
        assert len(rows) == 2  # most recent first
        assert rows[0].key == "k4"

    def test_query_layer_none_returns_all_layers(self) -> None:
        s = InMemoryMemoryStore()
        s.put(_entry(MemoryLayer.EPHEMERAL, "e"))
        s.put(_entry(MemoryLayer.WORKING, "w"))
        s.put(_entry(MemoryLayer.LONGTERM, "l"))
        keys = {r.key for r in s.query(MemoryQuery())}
        assert keys == {"e", "w", "l"}


class TestForget:
    def test_forget_specific_session_and_layer(self) -> None:
        s = InMemoryMemoryStore()
        s.put(_entry(MemoryLayer.EPHEMERAL, "k1", sid="A"))
        s.put(_entry(MemoryLayer.EPHEMERAL, "k2", sid="B"))
        s.put(_entry(MemoryLayer.WORKING, "w1", sid="A"))
        assert s.forget(session_id="A", layer=MemoryLayer.EPHEMERAL) == 1
        assert {r.key for r in s.query(MemoryQuery())} == {"k2", "w1"}

    def test_forget_all_longterm(self) -> None:
        s = InMemoryMemoryStore()
        s.put(_entry(MemoryLayer.LONGTERM, "l1"))
        s.put(_entry(MemoryLayer.LONGTERM, "l2"))
        assert s.forget(layer=MemoryLayer.LONGTERM) == 2
        assert s.query(MemoryQuery(layer=MemoryLayer.LONGTERM)) == ()


class TestMemoryRecorder:
    def test_after_skill_load_records_ephemeral_entry(self) -> None:
        store = InMemoryMemoryStore()
        hooks = LifecycleHooks()
        MemoryRecorder(store).install(hooks)
        hooks.dispatch(
            HookContext(
                event=HookEvent.AFTER_SKILL_LOAD,
                dcc_name="maya",
                session_id="s1",
                payload={"skill_name": "usd-import"},
            )
        )
        rows = store.query(MemoryQuery(layer=MemoryLayer.EPHEMERAL))
        assert len(rows) == 1
        assert rows[0].key == "skill_loaded:usd-import"

    def test_after_tool_call_records_working_entry(self) -> None:
        store = InMemoryMemoryStore()
        hooks = LifecycleHooks()
        MemoryRecorder(store).install(hooks)
        hooks.dispatch(
            HookContext(
                event=HookEvent.AFTER_TOOL_CALL,
                dcc_name="blender",
                session_id="s1",
                payload={"tool_name": "create_cube", "ok": False},
            )
        )
        rows = store.query(MemoryQuery(layer=MemoryLayer.WORKING))
        assert [r.key for r in rows] == ["tool_call:create_cube:fail"]

    def test_session_end_clears_session_scoped_layers(self) -> None:
        store = InMemoryMemoryStore()
        hooks = LifecycleHooks()
        MemoryRecorder(store).install(hooks)
        # Seed both layers for session s1, and longterm
        store.put(_entry(MemoryLayer.EPHEMERAL, "e", sid="s1"))
        store.put(_entry(MemoryLayer.WORKING, "w", sid="s1"))
        store.put(_entry(MemoryLayer.LONGTERM, "l"))
        hooks.dispatch(HookContext(event=HookEvent.SESSION_END, dcc_name="maya", session_id="s1"))
        kept = {r.key for r in store.query(MemoryQuery())}
        assert kept == {"l"}

    def test_session_end_without_session_id_is_noop(self) -> None:
        store = InMemoryMemoryStore()
        hooks = LifecycleHooks()
        MemoryRecorder(store).install(hooks)
        store.put(_entry(MemoryLayer.EPHEMERAL, "e", sid="s1"))
        hooks.dispatch(HookContext(event=HookEvent.SESSION_END, dcc_name="maya"))
        assert len(store) == 1
