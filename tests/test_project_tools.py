"""Tests for register_project_tools (issue #576).

These tests use a lightweight fake MCP server (MagicMock-backed) in the same
style as tests/test_checkpoint.py, so the tool handlers can be exercised
directly without spinning up the Rust HTTP stack.  End-to-end validation
against a real MCP server-in-DCC is intentionally out of scope for the
unit-test layer; that happens via tests/test_mcp_mcporter_e2e.py or manual
invocation from inside Maya/Blender.
"""

from __future__ import annotations

import json
from pathlib import Path
from unittest.mock import MagicMock

import pytest

from dcc_mcp_core.project import PROJECT_DIR_NAME
from dcc_mcp_core.project import PROJECT_STATE_FILE
from dcc_mcp_core.project import DccProject
from dcc_mcp_core.project import register_project_tools


def _make_server() -> tuple[MagicMock, dict]:
    """Create a fake server that captures (name → handler) registrations."""
    server = MagicMock()
    server.registry = MagicMock()
    handlers: dict = {}
    server.register_handler.side_effect = lambda name, fn: handlers.__setitem__(name, fn)
    return server, handlers


class TestRegisterProjectTools:
    def test_registers_all_four_tools(self) -> None:
        server, _handlers = _make_server()

        register_project_tools(server)

        names = {c.kwargs["name"] for c in server.registry.register.call_args_list}
        assert names == {"project.save", "project.load", "project.resume", "project.status"}
        # All metadata registrations use category="project".
        for call in server.registry.register.call_args_list:
            assert call.kwargs["category"] == "project"
            assert call.kwargs["version"] == "1.0.0"

    def test_save_persists_state_file(self, tmp_path: Path) -> None:
        server, handlers = _make_server()
        register_project_tools(server)
        scene = tmp_path / "shot.ma"

        result = handlers["project.save"](json.dumps({"scene_path": str(scene)}))

        assert result["success"] is True
        assert (tmp_path / PROJECT_DIR_NAME / PROJECT_STATE_FILE).is_file()
        assert result["context"]["state"]["scene_path"] == str(scene)

    def test_save_without_scene_path_returns_success_false(self) -> None:
        server, handlers = _make_server()
        register_project_tools(server)

        result = handlers["project.save"]({})

        assert result["success"] is False
        assert "scene_path" in result["message"]

    def test_load_of_nonexistent_project_returns_success_false(self, tmp_path: Path) -> None:
        server, handlers = _make_server()
        register_project_tools(server)

        # No .dcc-mcp/ has been created under tmp_path.
        result = handlers["project.load"]({"scene_path": str(tmp_path / "never_saved.ma")})

        assert result["success"] is False
        assert "No project.json" in result["message"]
        # The project dir is still reported so the caller can create it if desired.
        assert result["context"]["project_dir"].endswith(PROJECT_DIR_NAME)

    def test_load_without_any_path_returns_success_false(self) -> None:
        server, handlers = _make_server()
        register_project_tools(server)

        result = handlers["project.load"]({})

        assert result["success"] is False
        assert "scene_path" in result["message"] or "project_dir" in result["message"]

    def test_save_then_load_round_trip(self, tmp_path: Path) -> None:
        server, handlers = _make_server()
        register_project_tools(server)
        scene = tmp_path / "shot.ma"

        # Save first so project.json exists; then mutate via a direct DccProject
        # handle to simulate adapter-side state changes between sessions.
        handlers["project.save"]({"scene_path": str(scene)})
        project = DccProject.load(scene)
        project.add_asset(tmp_path / "char.ma")
        project.activate_skill("maya-lookdev")
        project.activate_tool_group("group-a")

        loaded = handlers["project.load"]({"scene_path": str(scene)})

        assert loaded["success"] is True
        state = loaded["context"]["state"]
        assert state["loaded_assets"] == [str(tmp_path / "char.ma")]
        assert state["active_skills"] == ["maya-lookdev"]
        assert state["active_tool_groups"] == ["group-a"]

    def test_resume_returns_full_session_payload(self, tmp_path: Path) -> None:
        server, handlers = _make_server()
        register_project_tools(server)
        scene = tmp_path / "shot.ma"
        handlers["project.save"]({"scene_path": str(scene)})
        project = DccProject.load(scene)
        project.add_checkpoint_id("job-a")
        project.update_metadata(units="cm")

        result = handlers["project.resume"]({"scene_path": str(scene)})

        assert result["success"] is True
        ctx = result["context"]
        # resume_session payload keys — must include the new #576 fields.
        for key in (
            "scene_path",
            "loaded_assets",
            "active_skills",
            "active_tool_groups",
            "checkpoint_ids",
            "metadata",
            "session_id",
            "created_at",
            "updated_at",
            "project_dir",
            "state_path",
        ):
            assert key in ctx, f"resume payload missing {key!r}"
        assert ctx["checkpoint_ids"] == ["job-a"]
        assert ctx["metadata"] == {"units": "cm"}

    def test_status_on_existing_project(self, tmp_path: Path) -> None:
        server, handlers = _make_server()
        register_project_tools(server)
        scene = tmp_path / "shot.ma"
        handlers["project.save"]({"scene_path": str(scene)})

        result = handlers["project.status"]({"scene_path": str(scene)})

        assert result["success"] is True
        assert result["context"]["state"]["scene_path"] == str(scene)

    def test_status_without_any_path_returns_success_false(self) -> None:
        server, handlers = _make_server()
        register_project_tools(server)

        result = handlers["project.status"]({})

        assert result["success"] is False

    def test_default_project_binding_is_used_when_args_omit_paths(self, tmp_path: Path) -> None:
        """A caller-bound default DccProject is used when tool args omit paths."""
        default_project = DccProject.open(tmp_path / "bound.ma")
        default_project.activate_skill("skill-bound")

        server, handlers = _make_server()
        register_project_tools(server, project=default_project)

        result = handlers["project.resume"]({})
        assert result["success"] is True
        assert result["context"]["active_skills"] == ["skill-bound"]

    def test_handler_accepts_dict_params_not_just_json_string(self, tmp_path: Path) -> None:
        server, handlers = _make_server()
        register_project_tools(server)

        result = handlers["project.save"]({"scene_path": str(tmp_path / "direct.ma")})

        assert result["success"] is True

    def test_no_registry_logs_warning_does_not_raise(self) -> None:
        import logging

        class _BadServer:
            @property
            def registry(self) -> object:  # type: ignore[override]
                raise AttributeError("no registry")

        with pytest.MonkeyPatch.context() as mp:
            mock_warn = MagicMock()
            mp.setattr(logging.getLogger("dcc_mcp_core.project"), "warning", mock_warn)
            register_project_tools(_BadServer())  # must not raise
            mock_warn.assert_called_once()
