from __future__ import annotations

import json
import os
from pathlib import Path
from types import SimpleNamespace

import pytest

from dcc_mcp_core import ui_control_server


def _required_args(tmp_path: Path) -> list[str]:
    skill_root = tmp_path / "skills"
    skill_root.mkdir()
    return [
        "--process-id",
        "42",
        "--window-handle",
        "84",
        "--skill-root",
        str(skill_root),
        "--registry-dir",
        str(tmp_path / "registry"),
        "--ready-file",
        str(tmp_path / "ready.json"),
        "--gateway-port",
        "18123",
    ]


@pytest.mark.parametrize(("process_id", "window_handle"), [(0, 84), (42, 0), (-1, 84)])
def test_validate_target_scope_requires_positive_exact_ids(process_id: int, window_handle: int) -> None:
    with pytest.raises(ValueError, match="must be positive"):
        ui_control_server._validate_target_scope(process_id, window_handle)


def test_validate_target_scope_is_platform_neutral() -> None:
    ui_control_server._validate_target_scope(42, 84)


def test_only_ui_control_transform_rejects_other_skills() -> None:
    metadata = SimpleNamespace(name="ui-control")
    assert ui_control_server._only_ui_control(metadata) is metadata
    with pytest.raises(RuntimeError, match="only the ui-control skill"):
        ui_control_server._only_ui_control(SimpleNamespace(name="other"))


def test_shutdown_always_clears_owned_host_state() -> None:
    events: list[str] = []

    class Bridge:
        @staticmethod
        def close_script_admission():
            events.append("close-admission")

        @staticmethod
        def clear_script_packages():
            events.append("clear-packages")

    class Handle:
        @staticmethod
        def shutdown():
            events.append("server-shutdown")
            raise RuntimeError("shutdown failed")

    with pytest.raises(RuntimeError, match="shutdown failed"):
        ui_control_server._shutdown(Bridge(), Handle())
    assert events == ["close-admission", "server-shutdown", "clear-packages"]


def test_shutdown_stops_server_when_closing_admission_fails() -> None:
    events: list[str] = []

    class Bridge:
        @staticmethod
        def close_script_admission():
            events.append("close-admission")
            raise RuntimeError("admission close failed")

        @staticmethod
        def clear_script_packages():
            events.append("clear-packages")

    class Handle:
        @staticmethod
        def shutdown():
            events.append("server-shutdown")

    with pytest.raises(RuntimeError, match="admission close failed"):
        ui_control_server._shutdown(Bridge(), Handle())
    assert events == ["close-admission", "server-shutdown", "clear-packages"]


def test_main_registers_executor_before_load_and_shuts_down_in_order(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    events: list[str] = []

    class Config:
        def __init__(self, **kwargs):
            self.__dict__.update(kwargs)

    class Bridge:
        def __init__(self, dispatcher):
            assert dispatcher is None

        def as_inprocess_executor(self):
            events.append("executor-created")
            return "executor"

        def close_script_admission(self):
            events.append("close-admission")

        def clear_script_packages(self):
            events.append("clear-packages")

    class Handle:
        port = 19123
        is_gateway = False

        @staticmethod
        def mcp_url():
            return "http://127.0.0.1:19123/mcp"

        @staticmethod
        def shutdown():
            events.append("server-shutdown")

    class Server:
        @staticmethod
        def set_skill_load_transform(transform):
            assert transform(SimpleNamespace(name="ui-control")).name == "ui-control"
            events.append("transform")

        @staticmethod
        def set_in_process_executor(executor):
            assert executor == "executor"
            events.append("executor-registered")

        @staticmethod
        def load_skill(name):
            assert name == "ui-control"
            events.append("load-skill")
            return ["ui_control__snapshot", "ui_control__act"]

        @staticmethod
        def start():
            events.append("start")
            return Handle()

    class StoppedEvent:
        @staticmethod
        def wait(_timeout):
            return True

        @staticmethod
        def set():
            pass

    def create_server(dcc_name, config, extra_paths, accumulated):
        assert dcc_name == "python"
        assert config.allowed_skill_names == ["ui-control"]
        assert extra_paths == [str((tmp_path / "skills").resolve())]
        assert accumulated is False
        events.append("create-server")
        return Server()

    monkeypatch.setattr(ui_control_server, "McpHttpConfig", Config)
    monkeypatch.setattr(ui_control_server, "HostExecutionBridge", Bridge)
    monkeypatch.setattr(ui_control_server, "create_skill_server", create_server)
    monkeypatch.setattr(ui_control_server, "resolve_cua_command", lambda configured=None: ["C:/tools/dcc-cua.exe"])
    monkeypatch.setattr(
        ui_control_server,
        "inspect_cua_contract",
        lambda _command: SimpleNamespace(
            version="0.0.0-test",
            capabilities=("trusted_confirmation_grants", "semantic_profile_extensions"),
        ),
    )
    monkeypatch.setattr(ui_control_server.threading, "Event", StoppedEvent)
    monkeypatch.setattr(ui_control_server.signal, "signal", lambda *_args: None)
    for name in (
        "DCC_MCP_UI_CONTROL_BACKEND",
        "DCC_MCP_CUA_BINARY",
        "DCC_MCP_UI_CONTROL_WINDOW_HANDLE",
        "DCC_MCP_UI_CONTROL_PROCESS_ID",
        "DCC_MCP_DISABLE_DEFAULT_SKILL_PATHS",
        "DCC_MCP_DISABLE_ACCUMULATED_SKILLS",
    ):
        monkeypatch.delenv(name, raising=False)
    monkeypatch.setenv("DCC_MCP_CUA_ALLOW_RAW_INPUT", "true")

    args = _required_args(tmp_path)
    assert ui_control_server.main(args) == 0

    assert events == [
        "create-server",
        "transform",
        "executor-created",
        "executor-registered",
        "load-skill",
        "start",
        "close-admission",
        "server-shutdown",
        "clear-packages",
    ]
    assert os.environ["DCC_MCP_CUA_ALLOW_RAW_INPUT"] == "true"
    ready = json.loads((tmp_path / "ready.json").read_text(encoding="utf-8"))
    assert ready["status"] == "ready"
    assert ready["target_process_id"] == 42
    assert ready["target_window_handle"] == 84
    assert ready["cua_binary"] == "C:/tools/dcc-cua.exe"
    assert ready["cua_version"] == "0.0.0-test"
    assert ready["cua_capabilities"] == ["trusted_confirmation_grants", "semantic_profile_extensions"]
    assert ready["raw_input_enabled"] is True


def test_raw_input_is_enabled_by_default_and_can_be_disabled(tmp_path: Path) -> None:
    required = _required_args(tmp_path)
    enabled = ui_control_server._parse_args(required)
    disabled = ui_control_server._parse_args([*required, "--no-raw-input"])
    assert enabled.allow_raw_input is True
    assert disabled.allow_raw_input is False
