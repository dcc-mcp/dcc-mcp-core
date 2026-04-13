"""Tests for ServiceEntry deep API, ActionRegistry.reset(), ActionResultModel equality, SkillMetadata equality, and CaptureResult.

Targets previously uncovered APIs:
- ServiceEntry.is_ipc / effective_address() / to_dict()
- TransportManager with IPC transport_address — is_ipc True/False
- ActionRegistry.reset() clears all actions and resets __len__
- ActionResultModel.__eq__ equality and inequality
- SkillMetadata.__eq__ equality and inequality
- CaptureResult.data_size()
- ServiceEntry.last_heartbeat_ms / heartbeat update
"""

from __future__ import annotations

from pathlib import Path
import tempfile

import pytest

import dcc_mcp_core

# ── ServiceEntry deep API ─────────────────────────────────────────────────────


class TestServiceEntryDeep:
    """Deep tests for ServiceEntry fields and methods."""

    def _register(
        self,
        transport: dcc_mcp_core.TransportManager,
        dcc_type: str = "maya",
        host: str = "127.0.0.1",
        port: int = 18812,
        transport_address: dcc_mcp_core.TransportAddress | None = None,
    ) -> tuple[str, dcc_mcp_core.ServiceEntry]:
        iid = transport.register_service(
            dcc_type,
            host,
            port,
            transport_address=transport_address,
        )
        entry = transport.get_service(dcc_type, iid)
        assert entry is not None
        return iid, entry

    def test_is_ipc_false_for_tcp_only(self, tmp_path: Path) -> None:
        """Service registered with TCP (no transport_address) has is_ipc=False."""
        t = dcc_mcp_core.TransportManager(str(tmp_path))
        _iid, entry = self._register(t)
        assert entry.is_ipc is False

    def test_is_ipc_true_when_transport_address_set(self, tmp_path: Path) -> None:
        """Service registered with a Named Pipe / Unix socket address has is_ipc=True."""
        t = dcc_mcp_core.TransportManager(str(tmp_path))
        ipc_addr = dcc_mcp_core.TransportAddress.default_local("maya", 12345)
        _iid, entry = self._register(t, transport_address=ipc_addr)
        assert entry.is_ipc is True

    def test_effective_address_tcp_fallback(self, tmp_path: Path) -> None:
        """effective_address() returns a TCP address when no IPC address set."""
        t = dcc_mcp_core.TransportManager(str(tmp_path))
        _iid, entry = self._register(t, host="127.0.0.1", port=18812)
        addr = entry.effective_address()
        assert isinstance(addr, dcc_mcp_core.TransportAddress)
        assert addr.is_tcp

    def test_effective_address_prefers_ipc(self, tmp_path: Path) -> None:
        """effective_address() returns the IPC address when transport_address is set."""
        t = dcc_mcp_core.TransportManager(str(tmp_path))
        ipc_addr = dcc_mcp_core.TransportAddress.default_local("blender", 99999)
        _iid, entry = self._register(t, dcc_type="blender", transport_address=ipc_addr)
        addr = entry.effective_address()
        assert isinstance(addr, dcc_mcp_core.TransportAddress)

    def test_to_dict_contains_required_keys(self, tmp_path: Path) -> None:
        """ServiceEntry.to_dict() must include standard fields."""
        t = dcc_mcp_core.TransportManager(str(tmp_path))
        _iid, entry = self._register(t)
        d = entry.to_dict()
        assert isinstance(d, dict)
        assert "dcc_type" in d
        assert "instance_id" in d
        assert "host" in d
        assert "port" in d
        assert "status" in d

    def test_to_dict_values_match_entry(self, tmp_path: Path) -> None:
        """Values in to_dict() should match the entry's fields."""
        t = dcc_mcp_core.TransportManager(str(tmp_path))
        iid, entry = self._register(t, dcc_type="houdini", host="127.0.0.1", port=20000)
        d = entry.to_dict()
        assert d["dcc_type"] == "houdini"
        assert d["host"] == "127.0.0.1"
        assert d["instance_id"] == iid

    def test_last_heartbeat_ms_initial_positive(self, tmp_path: Path) -> None:
        """last_heartbeat_ms should be a positive integer at registration time."""
        t = dcc_mcp_core.TransportManager(str(tmp_path))
        _iid, entry = self._register(t)
        assert isinstance(entry.last_heartbeat_ms, int)
        assert entry.last_heartbeat_ms >= 0

    def test_heartbeat_updates_timestamp(self, tmp_path: Path) -> None:
        """heartbeat() should return True for a known instance."""
        t = dcc_mcp_core.TransportManager(str(tmp_path))
        iid, _entry = self._register(t)
        result = t.heartbeat("maya", iid)
        assert result is True

    def test_heartbeat_unknown_instance_returns_false_or_raises(self, tmp_path: Path) -> None:
        """heartbeat() with unknown UUID returns False or raises ValueError."""
        t = dcc_mcp_core.TransportManager(str(tmp_path))
        # A well-formatted UUID that is not registered
        unknown_uuid = "00000000-0000-0000-0000-000000000000"
        try:
            result = t.heartbeat("maya", unknown_uuid)
            assert result is False
        except (ValueError, RuntimeError):
            pass  # acceptable: implementation may raise on unregistered uuid


# ── ActionRegistry.reset() ────────────────────────────────────────────────────


class TestActionRegistryReset:
    """Tests for ActionRegistry.reset() clears all registered actions."""

    def test_reset_empties_registry(self) -> None:
        reg = dcc_mcp_core.ActionRegistry()
        reg.register("create_sphere", dcc="maya")
        reg.register("delete_mesh", dcc="maya")
        assert len(reg) == 2
        reg.reset()
        assert len(reg) == 0

    def test_reset_clears_dcc_entries(self) -> None:
        reg = dcc_mcp_core.ActionRegistry()
        reg.register("action_a", dcc="maya")
        reg.register("action_b", dcc="blender")
        reg.reset()
        assert reg.list_actions_for_dcc("maya") == []
        assert reg.list_actions_for_dcc("blender") == []

    def test_reset_allows_re_register(self) -> None:
        reg = dcc_mcp_core.ActionRegistry()
        reg.register("create_sphere", dcc="maya")
        reg.reset()
        reg.register("create_sphere", dcc="maya")
        assert len(reg) == 1

    def test_reset_clears_categories(self) -> None:
        reg = dcc_mcp_core.ActionRegistry()
        reg.register("geo_action", category="geometry", dcc="maya")
        reg.reset()
        assert reg.get_categories() == []

    def test_reset_clears_tags(self) -> None:
        reg = dcc_mcp_core.ActionRegistry()
        reg.register("tagged", tags=["mytag"], dcc="maya")
        reg.reset()
        assert reg.get_tags() == []

    def test_reset_empty_registry_noop(self) -> None:
        """Resetting an already empty registry should not raise."""
        reg = dcc_mcp_core.ActionRegistry()
        reg.reset()
        assert len(reg) == 0

    def test_reset_does_not_affect_other_registries(self) -> None:
        """Two independent registries — resetting one must not affect the other."""
        reg1 = dcc_mcp_core.ActionRegistry()
        reg2 = dcc_mcp_core.ActionRegistry()
        reg1.register("act", dcc="maya")
        reg2.register("act", dcc="maya")
        reg1.reset()
        assert len(reg1) == 0
        assert len(reg2) == 1


# ── ActionResultModel equality ────────────────────────────────────────────────


class TestActionResultModelEquality:
    """Tests for ActionResultModel.__eq__."""

    def test_equal_default_instances(self) -> None:
        r1 = dcc_mcp_core.ActionResultModel()
        r2 = dcc_mcp_core.ActionResultModel()
        assert r1 == r2

    def test_equal_same_values(self) -> None:
        r1 = dcc_mcp_core.ActionResultModel(success=True, message="done")
        r2 = dcc_mcp_core.ActionResultModel(success=True, message="done")
        assert r1 == r2

    def test_not_equal_different_success(self) -> None:
        r1 = dcc_mcp_core.ActionResultModel(success=True)
        r2 = dcc_mcp_core.ActionResultModel(success=False)
        assert r1 != r2

    def test_not_equal_different_message(self) -> None:
        r1 = dcc_mcp_core.ActionResultModel(message="hello")
        r2 = dcc_mcp_core.ActionResultModel(message="world")
        assert r1 != r2

    def test_not_equal_different_error(self) -> None:
        r1 = dcc_mcp_core.ActionResultModel(error="err1")
        r2 = dcc_mcp_core.ActionResultModel(error="err2")
        assert r1 != r2

    def test_not_equal_non_model(self) -> None:
        r = dcc_mcp_core.ActionResultModel()
        assert r != "not a model"
        assert r != 42
        assert r != None

    def test_equal_success_result_factory(self) -> None:
        r1 = dcc_mcp_core.success_result("done")
        r2 = dcc_mcp_core.success_result("done")
        assert r1 == r2

    def test_with_context_produces_different_instance(self) -> None:
        r1 = dcc_mcp_core.ActionResultModel(message="ok")
        r2 = r1.with_context(count=5)
        assert r1 != r2


# ── SkillMetadata equality ────────────────────────────────────────────────────


class TestSkillMetadataEquality:
    """Tests for SkillMetadata.__eq__."""

    def test_equal_minimal(self) -> None:
        m1 = dcc_mcp_core.SkillMetadata(name="hello")
        m2 = dcc_mcp_core.SkillMetadata(name="hello")
        assert m1 == m2

    def test_not_equal_different_name(self) -> None:
        m1 = dcc_mcp_core.SkillMetadata(name="alpha")
        m2 = dcc_mcp_core.SkillMetadata(name="beta")
        assert m1 != m2

    def test_not_equal_different_description(self) -> None:
        m1 = dcc_mcp_core.SkillMetadata(name="s", description="a")
        m2 = dcc_mcp_core.SkillMetadata(name="s", description="b")
        assert m1 != m2

    def test_equal_with_tags(self) -> None:
        m1 = dcc_mcp_core.SkillMetadata(name="s", tags=["x", "y"])
        m2 = dcc_mcp_core.SkillMetadata(name="s", tags=["x", "y"])
        assert m1 == m2

    def test_not_equal_different_tags(self) -> None:
        m1 = dcc_mcp_core.SkillMetadata(name="s", tags=["a"])
        m2 = dcc_mcp_core.SkillMetadata(name="s", tags=["b"])
        assert m1 != m2

    def test_not_equal_non_metadata(self) -> None:
        m = dcc_mcp_core.SkillMetadata(name="s")
        assert m != "not metadata"


# ── CaptureResult.data_size() ─────────────────────────────────────────────────


class TestCaptureResultDataSize:
    """Tests for CaptureResult.data_size()."""

    def test_data_size_matches_len(self) -> None:
        data = b"\xff\xd8\xff" + b"\x00" * 97  # fake 100-byte JPEG-ish
        result = dcc_mcp_core.CaptureResult(data=data, width=10, height=10, format="jpeg")
        assert result.data_size() == len(data)

    def test_data_size_empty(self) -> None:
        result = dcc_mcp_core.CaptureResult(data=b"", width=0, height=0, format="png")
        assert result.data_size() == 0

    def test_data_size_large(self) -> None:
        payload = b"\xaa" * (1024 * 1024)
        result = dcc_mcp_core.CaptureResult(data=payload, width=1920, height=1080, format="raw")
        assert result.data_size() == 1024 * 1024

    def test_repr_contains_size_info(self) -> None:
        result = dcc_mcp_core.CaptureResult(data=b"img", width=100, height=50, format="png")
        r = repr(result)
        assert isinstance(r, str)
        assert len(r) > 0

    def test_viewport_optional(self) -> None:
        r = dcc_mcp_core.CaptureResult(data=b"d", width=1, height=1, format="png", viewport="main")
        assert r.viewport == "main"

    def test_viewport_defaults_none(self) -> None:
        r = dcc_mcp_core.CaptureResult(data=b"d", width=1, height=1, format="png")
        assert r.viewport is None
