"""Tests for the checkpoint/resume system (issue #436)."""

from __future__ import annotations

import json
from pathlib import Path
import time
from unittest.mock import MagicMock
from unittest.mock import patch

import pytest

from dcc_mcp_core.checkpoint import CheckpointStore
from dcc_mcp_core.checkpoint import checkpoint_every
from dcc_mcp_core.checkpoint import clear_checkpoint
from dcc_mcp_core.checkpoint import configure_checkpoint_store
from dcc_mcp_core.checkpoint import get_checkpoint
from dcc_mcp_core.checkpoint import list_checkpoints
from dcc_mcp_core.checkpoint import register_checkpoint_tools
from dcc_mcp_core.checkpoint import save_checkpoint

# ── CheckpointStore ────────────────────────────────────────────────────────


class TestCheckpointStore:
    def test_save_and_get(self) -> None:
        store = CheckpointStore()
        store.save("job-1", {"count": 10}, progress_hint="10/100")
        cp = store.get("job-1")
        assert cp is not None
        assert cp["context"] == {"count": 10}
        assert cp["progress_hint"] == "10/100"
        assert cp["job_id"] == "job-1"
        assert "saved_at" in cp

    def test_get_missing_returns_none(self) -> None:
        store = CheckpointStore()
        assert store.get("nonexistent") is None

    def test_clear_existing(self) -> None:
        store = CheckpointStore()
        store.save("job-2", {"x": 1})
        assert store.clear("job-2") is True
        assert store.get("job-2") is None

    def test_clear_missing_returns_false(self) -> None:
        store = CheckpointStore()
        assert store.clear("does-not-exist") is False

    def test_list_ids(self) -> None:
        store = CheckpointStore()
        store.save("a", {})
        store.save("b", {})
        ids = store.list_ids()
        assert "a" in ids
        assert "b" in ids

    def test_clear_all(self) -> None:
        store = CheckpointStore()
        store.save("a", {})
        store.save("b", {})
        count = store.clear_all()
        assert count == 2
        assert store.list_ids() == []

    def test_overwrite_updates_state(self) -> None:
        store = CheckpointStore()
        store.save("job", {"v": 1})
        store.save("job", {"v": 2})
        assert store.get("job")["context"]["v"] == 2  # type: ignore[index]

    def test_persistence_to_json_file(self, tmp_path: Path) -> None:
        path = tmp_path / "cp.json"
        store = CheckpointStore(path=str(path))
        store.save("p-job", {"done": 5}, progress_hint="5 done")
        assert path.exists()
        raw = json.loads(path.read_text())
        assert "p-job" in raw

    def test_load_from_existing_file(self, tmp_path: Path) -> None:
        path = tmp_path / "cp.json"
        path.write_text(json.dumps({"j1": {"job_id": "j1", "saved_at": 0, "progress_hint": "", "context": {"n": 7}}}))
        store = CheckpointStore(path=str(path))
        cp = store.get("j1")
        assert cp is not None
        assert cp["context"]["n"] == 7

    def test_corrupt_file_starts_empty(self, tmp_path: Path) -> None:
        path = tmp_path / "cp.json"
        path.write_text("not-valid-json")
        store = CheckpointStore(path=str(path))
        assert store.list_ids() == []


# ── Module-level helpers ──────────────────────────────────────────────────


class TestModuleLevelHelpers:
    def setup_method(self) -> None:
        # Ensure a clean store for each test
        from dcc_mcp_core import checkpoint as cp_mod
        cp_mod._DEFAULT_STORE = CheckpointStore()

    def test_save_and_get_checkpoint(self) -> None:
        save_checkpoint("job-x", {"i": 3}, progress_hint="3/10")
        cp = get_checkpoint("job-x")
        assert cp is not None
        assert cp["context"]["i"] == 3

    def test_clear_checkpoint(self) -> None:
        save_checkpoint("job-y", {"i": 3})
        assert clear_checkpoint("job-y") is True
        assert get_checkpoint("job-y") is None

    def test_clear_checkpoint_missing_returns_false(self) -> None:
        assert clear_checkpoint("no-such-job") is False

    def test_list_checkpoints(self) -> None:
        save_checkpoint("lc-1", {})
        save_checkpoint("lc-2", {})
        ids = list_checkpoints()
        assert "lc-1" in ids
        assert "lc-2" in ids

    def test_configure_checkpoint_store_replaces_default(self, tmp_path: Path) -> None:
        new_store = configure_checkpoint_store(path=str(tmp_path / "test.json"))
        assert isinstance(new_store, CheckpointStore)
        save_checkpoint("new-job", {"v": 99})
        assert get_checkpoint("new-job") is not None
        # Reset
        configure_checkpoint_store()

    def test_checkpoint_every_saves(self) -> None:
        checkpoint_every(
            50,
            "ce-job",
            state_fn=lambda: {"count": 50},
            progress_fn=lambda: "50 done",
        )
        cp = get_checkpoint("ce-job")
        assert cp is not None
        assert cp["context"]["count"] == 50
        assert cp["progress_hint"] == "50 done"

    def test_checkpoint_every_n_zero_is_noop(self) -> None:
        checkpoint_every(0, "noop-job", state_fn=lambda: {"x": 1})
        assert get_checkpoint("noop-job") is None


# ── register_checkpoint_tools ─────────────────────────────────────────────


class TestRegisterCheckpointTools:
    def _make_server(self) -> tuple[MagicMock, dict, CheckpointStore]:
        server = MagicMock()
        registry = MagicMock()
        server.registry = registry
        handlers: dict = {}
        server.register_handler.side_effect = lambda name, fn: handlers.__setitem__(name, fn)
        store = CheckpointStore()
        return server, handlers, store

    def test_registers_two_tools(self) -> None:
        server, _handlers, store = self._make_server()
        register_checkpoint_tools(server, store=store)
        names = {c.kwargs["name"] for c in server.registry.register.call_args_list}
        assert "jobs.checkpoint_status" in names
        assert "jobs.resume_context" in names

    def test_checkpoint_status_no_checkpoint(self) -> None:
        server, handlers, store = self._make_server()
        register_checkpoint_tools(server, store=store)
        result = handlers["jobs.checkpoint_status"](json.dumps({"job_id": "j1"}))
        assert result["success"] is True
        assert result["context"]["checkpoint"] is None

    def test_checkpoint_status_with_checkpoint(self) -> None:
        server, handlers, store = self._make_server()
        store.save("j2", {"n": 5}, progress_hint="5/10")
        register_checkpoint_tools(server, store=store)
        result = handlers["jobs.checkpoint_status"](json.dumps({"job_id": "j2"}))
        assert result["success"] is True
        assert result["context"]["context"]["n"] == 5

    def test_resume_context_no_checkpoint(self) -> None:
        server, handlers, store = self._make_server()
        register_checkpoint_tools(server, store=store)
        result = handlers["jobs.resume_context"](json.dumps({"job_id": "j3"}))
        assert result["success"] is True
        assert result["context"]["has_checkpoint"] is False
        assert result["context"]["resume_state"] is None

    def test_resume_context_with_checkpoint(self) -> None:
        server, handlers, store = self._make_server()
        store.save("j4", {"count": 80}, progress_hint="80/100")
        register_checkpoint_tools(server, store=store)
        result = handlers["jobs.resume_context"](json.dumps({"job_id": "j4"}))
        assert result["success"] is True
        assert result["context"]["has_checkpoint"] is True
        assert result["context"]["resume_state"]["count"] == 80
        assert result["context"]["progress_hint"] == "80/100"

    def test_handler_accepts_dict_params(self) -> None:
        server, handlers, store = self._make_server()
        register_checkpoint_tools(server, store=store)
        result = handlers["jobs.checkpoint_status"]({"job_id": "j5"})
        assert result["success"] is True

    def test_no_registry_logs_warning(self) -> None:
        import logging

        class _BadServer:
            @property
            def registry(self):
                raise AttributeError("no registry")

        with patch.object(logging.getLogger("dcc_mcp_core.checkpoint"), "warning") as mock_warn:
            register_checkpoint_tools(_BadServer())
        mock_warn.assert_called_once()
