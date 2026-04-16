"""Deep tests for HTTP server, transport, and DCC protocol types.

Covers McpHttpServer, McpHttpConfig, TransportAddress, TransportScheme,
IpcListener, ListenerHandle, TransportManager, ServiceEntry, SkillWatcher,
ScriptLanguage, DccErrorCode, DccInfo, DccCapabilities, DccError,
RoutingStrategy, and ServiceStatus.

All tests are pure unit tests; no real DCC process is required.
"""

from __future__ import annotations

import json
import os
import tempfile
import time
from typing import Any
import urllib.request

import pytest

from dcc_mcp_core import ActionRegistry
from dcc_mcp_core import DccCapabilities
from dcc_mcp_core import DccError
from dcc_mcp_core import DccErrorCode
from dcc_mcp_core import DccInfo
from dcc_mcp_core import IpcListener
from dcc_mcp_core import McpHttpConfig
from dcc_mcp_core import McpHttpServer
from dcc_mcp_core import McpServerHandle
from dcc_mcp_core import RoutingStrategy
from dcc_mcp_core import ScriptLanguage
from dcc_mcp_core import ServiceStatus
from dcc_mcp_core import SkillWatcher
from dcc_mcp_core import TransportAddress
from dcc_mcp_core import TransportManager
from dcc_mcp_core import TransportScheme

# ── Helpers ──────────────────────────────────────────────────────────────────


def _make_registry(*names: str) -> ActionRegistry:
    reg = ActionRegistry()
    for name in names:
        reg.register(name, description=f"desc {name}", category="test", tags=[], dcc="test", version="1.0.0")
    return reg


def _post_json(url: str, body: dict[str, Any]) -> tuple[int, dict[str, Any]]:
    data = json.dumps(body).encode()
    req = urllib.request.Request(
        url,
        data=data,
        headers={"Content-Type": "application/json", "Accept": "application/json"},
        method="POST",
    )
    with urllib.request.urlopen(req, timeout=5) as resp:
        return resp.status, json.loads(resp.read())


# ══════════════════════════════════════════════════════════════════════════════
# McpHttpConfig
# ══════════════════════════════════════════════════════════════════════════════


class TestMcpHttpConfigCreate:
    def test_default_port(self) -> None:
        cfg = McpHttpConfig()
        assert cfg.port == 8765

    def test_custom_port(self) -> None:
        cfg = McpHttpConfig(port=9999)
        assert cfg.port == 9999

    def test_default_server_name_is_str(self) -> None:
        cfg = McpHttpConfig()
        assert isinstance(cfg.server_name, str)

    def test_default_server_name_nonempty(self) -> None:
        cfg = McpHttpConfig()
        assert len(cfg.server_name) > 0

    def test_custom_server_name(self) -> None:
        cfg = McpHttpConfig(server_name="maya-mcp")
        assert cfg.server_name == "maya-mcp"

    def test_default_server_version_is_str(self) -> None:
        cfg = McpHttpConfig()
        assert isinstance(cfg.server_version, str)

    def test_default_server_version_nonempty(self) -> None:
        cfg = McpHttpConfig()
        assert len(cfg.server_version) > 0

    def test_custom_server_version(self) -> None:
        cfg = McpHttpConfig(server_version="2.0.0")
        assert cfg.server_version == "2.0.0"

    def test_both_name_and_version(self) -> None:
        cfg = McpHttpConfig(server_name="blender-mcp", server_version="0.5.0")
        assert cfg.server_name == "blender-mcp"
        assert cfg.server_version == "0.5.0"

    def test_repr_is_str(self) -> None:
        cfg = McpHttpConfig(port=1234)
        assert isinstance(repr(cfg), str)

    def test_repr_contains_port(self) -> None:
        cfg = McpHttpConfig(port=5678)
        assert "5678" in repr(cfg)

    def test_port_zero_allowed(self) -> None:
        cfg = McpHttpConfig(port=0)
        assert cfg.port == 0

    def test_enable_cors_false_default(self) -> None:
        # Default no CORS; constructor accepts kwarg without error
        cfg = McpHttpConfig(enable_cors=False)
        assert cfg.enable_cors is False

    def test_request_timeout_ms_default(self) -> None:
        cfg = McpHttpConfig(request_timeout_ms=5000)
        assert cfg.request_timeout_ms == 5000

    def test_default_host_is_localhost(self) -> None:
        cfg = McpHttpConfig()
        assert cfg.host == "127.0.0.1"

    def test_default_endpoint_path(self) -> None:
        cfg = McpHttpConfig()
        assert cfg.endpoint_path == "/mcp"

    def test_default_max_sessions(self) -> None:
        cfg = McpHttpConfig()
        assert cfg.max_sessions == 100


# ══════════════════════════════════════════════════════════════════════════════
# McpHttpServer
# ══════════════════════════════════════════════════════════════════════════════


class TestMcpHttpServerCreate:
    def test_create_with_registry(self) -> None:
        server = McpHttpServer(_make_registry())
        assert server is not None

    def test_create_with_config(self) -> None:
        server = McpHttpServer(_make_registry(), McpHttpConfig(port=0))
        assert server is not None

    def test_create_config_none(self) -> None:
        server = McpHttpServer(_make_registry(), None)
        assert server is not None

    def test_catalog_property_is_str(self) -> None:
        server = McpHttpServer(_make_registry())
        assert isinstance(server.catalog, str)

    def test_catalog_contains_total(self) -> None:
        server = McpHttpServer(_make_registry())
        assert "total" in server.catalog.lower() or "0" in server.catalog

    def test_has_handler_false_initially(self) -> None:
        server = McpHttpServer(_make_registry("action_a"))
        assert server.has_handler("action_a") is False

    def test_has_handler_unknown_false(self) -> None:
        server = McpHttpServer(_make_registry())
        assert server.has_handler("nonexistent") is False

    def test_register_handler_makes_has_handler_true(self) -> None:
        server = McpHttpServer(_make_registry("my_action"))
        server.register_handler("my_action", lambda params: {"ok": True})
        assert server.has_handler("my_action") is True

    def test_register_handler_non_callable_raises_type_error(self) -> None:
        server = McpHttpServer(_make_registry("x"))
        with pytest.raises(TypeError):
            server.register_handler("x", "not_a_callable")

    def test_register_handler_none_raises_type_error(self) -> None:
        server = McpHttpServer(_make_registry("x"))
        with pytest.raises(TypeError):
            server.register_handler("x", None)

    def test_register_multiple_handlers(self) -> None:
        reg = _make_registry("a", "b", "c")
        server = McpHttpServer(reg)
        server.register_handler("a", lambda p: 1)
        server.register_handler("b", lambda p: 2)
        assert server.has_handler("a") is True
        assert server.has_handler("b") is True
        assert server.has_handler("c") is False

    def test_register_handler_receives_dict_params(self) -> None:
        server = McpHttpServer(_make_registry("echo"), McpHttpConfig(port=0))
        received = []

        server.register_handler("echo", lambda params: received.append(params) or params)

        # Route through the HTTP surface in dedicated tests; here we only assert
        # the callable contract by starting the server and invoking the MCP tool.
        handle = server.start()
        try:
            code, body = _post_json(
                handle.mcp_url(),
                {
                    "jsonrpc": "2.0",
                    "id": 3,
                    "method": "tools/call",
                    "params": {"name": "echo", "arguments": {"count": 2}},
                },
            )
            assert code == 200
            assert received == [{"count": 2}]
            assert body["result"]["isError"] is False
        finally:
            handle.shutdown()

    def test_discover_returns_int(self) -> None:
        server = McpHttpServer(_make_registry())
        result = server.discover()
        assert isinstance(result, int)

    def test_discover_returns_zero_no_paths(self) -> None:
        server = McpHttpServer(_make_registry())
        result = server.discover()
        assert result == 0

    def test_discover_with_extra_paths_returns_int(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            server = McpHttpServer(_make_registry())
            result = server.discover(extra_paths=[tmpdir])
            assert isinstance(result, int)

    def test_discover_with_dcc_name_filter(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            server = McpHttpServer(_make_registry())
            result = server.discover(extra_paths=[tmpdir], dcc_name="maya")
            assert isinstance(result, int)


# ══════════════════════════════════════════════════════════════════════════════
# McpServerHandle (ServerHandle)
# ══════════════════════════════════════════════════════════════════════════════


class TestMcpServerHandle:
    def test_start_returns_handle(self) -> None:
        server = McpHttpServer(_make_registry(), McpHttpConfig(port=0))
        handle = server.start()
        handle.shutdown()
        assert handle is not None

    def test_port_is_positive_int(self) -> None:
        server = McpHttpServer(_make_registry(), McpHttpConfig(port=0))
        handle = server.start()
        port = handle.port
        handle.shutdown()
        assert isinstance(port, int)
        assert port > 0

    def test_bind_addr_is_str(self) -> None:
        server = McpHttpServer(_make_registry(), McpHttpConfig(port=0))
        handle = server.start()
        addr = handle.bind_addr
        handle.shutdown()
        assert isinstance(addr, str)

    def test_bind_addr_contains_port(self) -> None:
        server = McpHttpServer(_make_registry(), McpHttpConfig(port=0))
        handle = server.start()
        assert str(handle.port) in handle.bind_addr
        handle.shutdown()

    def test_mcp_url_starts_with_http(self) -> None:
        server = McpHttpServer(_make_registry(), McpHttpConfig(port=0))
        handle = server.start()
        url = handle.mcp_url()
        handle.shutdown()
        assert url.startswith("http")

    def test_mcp_url_contains_mcp_path(self) -> None:
        server = McpHttpServer(_make_registry(), McpHttpConfig(port=0))
        handle = server.start()
        url = handle.mcp_url()
        handle.shutdown()
        assert "/mcp" in url

    def test_mcp_url_contains_port(self) -> None:
        server = McpHttpServer(_make_registry(), McpHttpConfig(port=0))
        handle = server.start()
        url = handle.mcp_url()
        port = handle.port
        handle.shutdown()
        assert str(port) in url

    def test_repr_is_str(self) -> None:
        server = McpHttpServer(_make_registry(), McpHttpConfig(port=0))
        handle = server.start()
        r = repr(handle)
        handle.shutdown()
        assert isinstance(r, str)

    def test_repr_contains_addr(self) -> None:
        server = McpHttpServer(_make_registry(), McpHttpConfig(port=0))
        handle = server.start()
        r = repr(handle)
        port = handle.port
        handle.shutdown()
        assert str(port) in r

    def test_signal_shutdown_does_not_block(self) -> None:
        server = McpHttpServer(_make_registry(), McpHttpConfig(port=0))
        handle = server.start()
        time.sleep(0.05)
        handle.signal_shutdown()
        time.sleep(0.15)

    def test_shutdown_blocks_until_stopped(self) -> None:
        server = McpHttpServer(_make_registry(), McpHttpConfig(port=0))
        handle = server.start()
        time.sleep(0.05)
        handle.shutdown()

    def test_multiple_starts_independent(self) -> None:
        reg = _make_registry()
        s1 = McpHttpServer(reg, McpHttpConfig(port=0))
        s2 = McpHttpServer(reg, McpHttpConfig(port=0))
        h1 = s1.start()
        h2 = s2.start()
        assert h1.port != h2.port
        h1.shutdown()
        h2.shutdown()


# ══════════════════════════════════════════════════════════════════════════════
# ScriptLanguage
# ══════════════════════════════════════════════════════════════════════════════


class TestScriptLanguage:
    def test_python_exists(self) -> None:
        assert ScriptLanguage.PYTHON is not None

    def test_mel_exists(self) -> None:
        assert ScriptLanguage.MEL is not None

    def test_maxscript_exists(self) -> None:
        assert ScriptLanguage.MAXSCRIPT is not None

    def test_hscript_exists(self) -> None:
        assert ScriptLanguage.HSCRIPT is not None

    def test_vex_exists(self) -> None:
        assert ScriptLanguage.VEX is not None

    def test_lua_exists(self) -> None:
        assert ScriptLanguage.LUA is not None

    def test_csharp_exists(self) -> None:
        assert ScriptLanguage.CSHARP is not None

    def test_blueprint_exists(self) -> None:
        assert ScriptLanguage.BLUEPRINT is not None

    def test_repr_is_str(self) -> None:
        assert isinstance(repr(ScriptLanguage.PYTHON), str)

    def test_str_is_str(self) -> None:
        assert isinstance(str(ScriptLanguage.PYTHON), str)

    def test_eq_same(self) -> None:
        assert ScriptLanguage.PYTHON == ScriptLanguage.PYTHON

    def test_ne_different(self) -> None:
        assert ScriptLanguage.PYTHON != ScriptLanguage.MEL

    def test_eq_mel_same(self) -> None:
        assert ScriptLanguage.MEL == ScriptLanguage.MEL

    def test_in_list(self) -> None:
        langs = [ScriptLanguage.PYTHON, ScriptLanguage.MEL]
        assert ScriptLanguage.PYTHON in langs

    def test_not_in_list(self) -> None:
        langs = [ScriptLanguage.MEL, ScriptLanguage.MAXSCRIPT]
        assert ScriptLanguage.PYTHON not in langs


# ══════════════════════════════════════════════════════════════════════════════
# DccErrorCode
# ══════════════════════════════════════════════════════════════════════════════


class TestDccErrorCode:
    def test_connection_failed(self) -> None:
        assert DccErrorCode.CONNECTION_FAILED is not None

    def test_timeout(self) -> None:
        assert DccErrorCode.TIMEOUT is not None

    def test_script_error(self) -> None:
        assert DccErrorCode.SCRIPT_ERROR is not None

    def test_not_responding(self) -> None:
        assert DccErrorCode.NOT_RESPONDING is not None

    def test_unsupported(self) -> None:
        assert DccErrorCode.UNSUPPORTED is not None

    def test_permission_denied(self) -> None:
        assert DccErrorCode.PERMISSION_DENIED is not None

    def test_invalid_input(self) -> None:
        assert DccErrorCode.INVALID_INPUT is not None

    def test_scene_error(self) -> None:
        assert DccErrorCode.SCENE_ERROR is not None

    def test_internal(self) -> None:
        assert DccErrorCode.INTERNAL is not None

    def test_eq_same(self) -> None:
        assert DccErrorCode.TIMEOUT == DccErrorCode.TIMEOUT

    def test_ne_different(self) -> None:
        assert DccErrorCode.TIMEOUT != DccErrorCode.CONNECTION_FAILED

    def test_repr_is_str(self) -> None:
        assert isinstance(repr(DccErrorCode.TIMEOUT), str)

    def test_str_is_str(self) -> None:
        assert isinstance(str(DccErrorCode.TIMEOUT), str)


# ══════════════════════════════════════════════════════════════════════════════
# DccError
# ══════════════════════════════════════════════════════════════════════════════


class TestDccError:
    def test_create(self) -> None:
        err = DccError(DccErrorCode.TIMEOUT, "timed out")
        assert err is not None

    def test_code(self) -> None:
        err = DccError(DccErrorCode.TIMEOUT, "timed out")
        assert err.code == DccErrorCode.TIMEOUT

    def test_message(self) -> None:
        err = DccError(DccErrorCode.SCRIPT_ERROR, "division by zero")
        assert err.message == "division by zero"

    def test_repr_is_str(self) -> None:
        err = DccError(DccErrorCode.TIMEOUT, "timed out")
        assert isinstance(repr(err), str)

    def test_repr_contains_code(self) -> None:
        err = DccError(DccErrorCode.TIMEOUT, "timed out")
        r = repr(err)
        assert "TIMEOUT" in r

    def test_repr_contains_message(self) -> None:
        err = DccError(DccErrorCode.INTERNAL, "internal error")
        assert "internal error" in repr(err)

    def test_code_eq_check(self) -> None:
        err = DccError(DccErrorCode.CONNECTION_FAILED, "no connection")
        assert err.code == DccErrorCode.CONNECTION_FAILED

    def test_different_codes(self) -> None:
        err1 = DccError(DccErrorCode.TIMEOUT, "t")
        err2 = DccError(DccErrorCode.UNSUPPORTED, "u")
        assert err1.code != err2.code


# ══════════════════════════════════════════════════════════════════════════════
# DccInfo
# ══════════════════════════════════════════════════════════════════════════════


class TestDccInfoCreate:
    def test_create_minimal(self) -> None:
        info = DccInfo("maya", "2025", "windows", 12345)
        assert info is not None

    def test_dcc_type(self) -> None:
        info = DccInfo("maya", "2025", "windows", 12345)
        assert info.dcc_type == "maya"

    def test_version(self) -> None:
        info = DccInfo("maya", "2025", "windows", 12345)
        assert info.version == "2025"

    def test_platform(self) -> None:
        info = DccInfo("maya", "2025", "windows", 12345)
        assert info.platform == "windows"

    def test_pid(self) -> None:
        info = DccInfo("maya", "2025", "windows", 12345)
        assert info.pid == 12345

    def test_python_version_default_none(self) -> None:
        info = DccInfo("maya", "2025", "windows", 1)
        assert info.python_version is None

    def test_metadata_default_empty(self) -> None:
        info = DccInfo("maya", "2025", "windows", 1)
        assert info.metadata == {}

    def test_repr_is_str(self) -> None:
        info = DccInfo("maya", "2025", "windows", 1)
        assert isinstance(repr(info), str)

    def test_repr_contains_dcc_type(self) -> None:
        info = DccInfo("maya", "2025", "windows", 1)
        assert "maya" in repr(info)

    def test_repr_contains_version(self) -> None:
        info = DccInfo("maya", "2025", "windows", 1)
        assert "2025" in repr(info)


class TestDccInfoOptional:
    def test_python_version_set(self) -> None:
        info = DccInfo("blender", "4.0", "linux", 9999, python_version="3.11")
        assert info.python_version == "3.11"

    def test_metadata_set(self) -> None:
        info = DccInfo("blender", "4.0", "linux", 9999, metadata={"scene": "test.blend"})
        assert info.metadata["scene"] == "test.blend"

    def test_metadata_multiple_keys(self) -> None:
        meta = {"scene": "a.ma", "fps": "24"}
        info = DccInfo("maya", "2025", "win", 1, metadata=meta)
        assert info.metadata["fps"] == "24"

    def test_all_fields(self) -> None:
        info = DccInfo("houdini", "20.0", "macos", 42, python_version="3.10", metadata={"hip": "scene.hip"})
        assert info.dcc_type == "houdini"
        assert info.version == "20.0"
        assert info.platform == "macos"
        assert info.pid == 42
        assert info.python_version == "3.10"
        assert info.metadata["hip"] == "scene.hip"


class TestDccInfoToDict:
    def test_to_dict_returns_dict(self) -> None:
        info = DccInfo("maya", "2025", "win", 1)
        assert isinstance(info.to_dict(), dict)

    def test_to_dict_has_dcc_type(self) -> None:
        info = DccInfo("maya", "2025", "win", 1)
        d = info.to_dict()
        assert "dcc_type" in d

    def test_to_dict_has_version(self) -> None:
        info = DccInfo("maya", "2025", "win", 1)
        assert "version" in info.to_dict()

    def test_to_dict_has_platform(self) -> None:
        info = DccInfo("maya", "2025", "win", 1)
        assert "platform" in info.to_dict()

    def test_to_dict_has_pid(self) -> None:
        info = DccInfo("maya", "2025", "win", 1)
        assert "pid" in info.to_dict()

    def test_to_dict_has_metadata(self) -> None:
        info = DccInfo("maya", "2025", "win", 1)
        assert "metadata" in info.to_dict()

    def test_to_dict_has_python_version(self) -> None:
        info = DccInfo("maya", "2025", "win", 1)
        assert "python_version" in info.to_dict()

    def test_to_dict_values_match(self) -> None:
        info = DccInfo("maya", "2025", "win", 99)
        d = info.to_dict()
        assert d["dcc_type"] == "maya"
        assert d["pid"] == 99


# ══════════════════════════════════════════════════════════════════════════════
# DccCapabilities
# ══════════════════════════════════════════════════════════════════════════════


class TestDccCapabilitiesCreate:
    def test_create_default(self) -> None:
        caps = DccCapabilities()
        assert caps is not None

    def test_default_scene_info_false(self) -> None:
        caps = DccCapabilities()
        assert caps.scene_info is False

    def test_default_snapshot_false(self) -> None:
        caps = DccCapabilities()
        assert caps.snapshot is False

    def test_default_undo_redo_false(self) -> None:
        caps = DccCapabilities()
        assert caps.undo_redo is False

    def test_default_progress_reporting_false(self) -> None:
        caps = DccCapabilities()
        assert caps.progress_reporting is False

    def test_default_file_operations_false(self) -> None:
        caps = DccCapabilities()
        assert caps.file_operations is False

    def test_default_selection_false(self) -> None:
        caps = DccCapabilities()
        assert caps.selection is False

    def test_default_script_languages_empty(self) -> None:
        caps = DccCapabilities()
        assert caps.script_languages == []

    def test_default_extensions_empty(self) -> None:
        caps = DccCapabilities()
        assert caps.extensions == {}

    def test_repr_is_str(self) -> None:
        caps = DccCapabilities()
        assert isinstance(repr(caps), str)


class TestDccCapabilitiesSet:
    def test_set_scene_info_true(self) -> None:
        caps = DccCapabilities(scene_info=True)
        assert caps.scene_info is True

    def test_set_snapshot_true(self) -> None:
        caps = DccCapabilities(snapshot=True)
        assert caps.snapshot is True

    def test_set_undo_redo_true(self) -> None:
        caps = DccCapabilities(undo_redo=True)
        assert caps.undo_redo is True

    def test_set_file_operations_true(self) -> None:
        caps = DccCapabilities(file_operations=True)
        assert caps.file_operations is True

    def test_set_selection_true(self) -> None:
        caps = DccCapabilities(selection=True)
        assert caps.selection is True

    def test_set_progress_reporting_true(self) -> None:
        caps = DccCapabilities(progress_reporting=True)
        assert caps.progress_reporting is True

    def test_set_script_languages(self) -> None:
        caps = DccCapabilities(script_languages=[ScriptLanguage.PYTHON])
        assert ScriptLanguage.PYTHON in caps.script_languages

    def test_set_multiple_script_languages(self) -> None:
        langs = [ScriptLanguage.PYTHON, ScriptLanguage.MEL]
        caps = DccCapabilities(script_languages=langs)
        assert len(caps.script_languages) == 2

    def test_repr_contains_language_count(self) -> None:
        caps = DccCapabilities(script_languages=[ScriptLanguage.PYTHON])
        r = repr(caps)
        assert "1" in r

    def test_multiple_flags(self) -> None:
        caps = DccCapabilities(scene_info=True, snapshot=True, undo_redo=True)
        assert caps.scene_info is True
        assert caps.snapshot is True
        assert caps.undo_redo is True


# ══════════════════════════════════════════════════════════════════════════════
# TransportAddress
# ══════════════════════════════════════════════════════════════════════════════


class TestTransportAddressTcp:
    def test_tcp_creates(self) -> None:
        addr = TransportAddress.tcp("127.0.0.1", 18812)
        assert addr is not None

    def test_tcp_repr_contains_scheme(self) -> None:
        addr = TransportAddress.tcp("127.0.0.1", 18812)
        assert "tcp" in repr(addr).lower()

    def test_tcp_repr_contains_host(self) -> None:
        addr = TransportAddress.tcp("127.0.0.1", 18812)
        assert "127.0.0.1" in repr(addr)

    def test_tcp_repr_contains_port(self) -> None:
        addr = TransportAddress.tcp("127.0.0.1", 18812)
        assert "18812" in repr(addr)

    def test_tcp_port_zero(self) -> None:
        addr = TransportAddress.tcp("127.0.0.1", 0)
        assert "0" in repr(addr)

    def test_tcp_different_ports(self) -> None:
        a1 = TransportAddress.tcp("127.0.0.1", 1234)
        a2 = TransportAddress.tcp("127.0.0.1", 5678)
        assert repr(a1) != repr(a2)


class TestTransportAddressNamedPipe:
    def test_named_pipe_creates(self) -> None:
        addr = TransportAddress.named_pipe("dcc-mcp-maya-1234")
        assert addr is not None

    def test_named_pipe_repr_contains_pipe(self) -> None:
        addr = TransportAddress.named_pipe("dcc-mcp-maya-1234")
        assert "pipe" in repr(addr).lower()

    def test_named_pipe_repr_contains_name(self) -> None:
        addr = TransportAddress.named_pipe("my-dcc-pipe")
        assert "my-dcc-pipe" in repr(addr)


class TestTransportAddressDefaultLocal:
    def test_default_local_creates(self) -> None:
        addr = TransportAddress.default_local("maya", os.getpid())
        assert addr is not None

    def test_default_local_repr_contains_dcc_type(self) -> None:
        addr = TransportAddress.default_local("maya", 9999)
        assert "maya" in repr(addr).lower()

    def test_default_local_repr_contains_pid(self) -> None:
        addr = TransportAddress.default_local("maya", 9999)
        assert "9999" in repr(addr)

    def test_default_local_different_dccs(self) -> None:
        addr_maya = TransportAddress.default_local("maya", 1000)
        addr_blender = TransportAddress.default_local("blender", 1000)
        assert repr(addr_maya) != repr(addr_blender)


class TestTransportAddressDefaultPipeName:
    def test_default_pipe_name_creates(self) -> None:
        addr = TransportAddress.default_pipe_name("maya", 12345)
        assert addr is not None

    def test_default_pipe_name_repr_not_empty(self) -> None:
        addr = TransportAddress.default_pipe_name("houdini", 8888)
        assert len(repr(addr)) > 0


class TestTransportAddressDefaultUnixSocket:
    def test_default_unix_socket_creates(self) -> None:
        addr = TransportAddress.default_unix_socket("maya", 12345)
        assert addr is not None

    def test_default_unix_socket_repr_not_empty(self) -> None:
        addr = TransportAddress.default_unix_socket("blender", 5555)
        assert len(repr(addr)) > 0


# ══════════════════════════════════════════════════════════════════════════════
# TransportScheme
# ══════════════════════════════════════════════════════════════════════════════


class TestTransportScheme:
    def test_auto_exists(self) -> None:
        assert TransportScheme.AUTO is not None

    def test_tcp_only_exists(self) -> None:
        assert TransportScheme.TCP_ONLY is not None

    def test_prefer_named_pipe_exists(self) -> None:
        assert TransportScheme.PREFER_NAMED_PIPE is not None

    def test_prefer_unix_socket_exists(self) -> None:
        assert TransportScheme.PREFER_UNIX_SOCKET is not None

    def test_prefer_ipc_exists(self) -> None:
        assert TransportScheme.PREFER_IPC is not None

    def test_eq_same(self) -> None:
        assert TransportScheme.AUTO == TransportScheme.AUTO

    def test_ne_different(self) -> None:
        assert TransportScheme.AUTO != TransportScheme.TCP_ONLY

    def test_eq_tcp_only(self) -> None:
        assert TransportScheme.TCP_ONLY == TransportScheme.TCP_ONLY

    def test_repr_is_str(self) -> None:
        assert isinstance(repr(TransportScheme.AUTO), str)

    def test_str_is_str(self) -> None:
        assert isinstance(str(TransportScheme.AUTO), str)


class TestTransportSchemeSelectAddress:
    def test_select_address_tcp_only(self) -> None:
        addr = TransportScheme.TCP_ONLY.select_address("maya", "127.0.0.1", 18812)
        assert addr is not None

    def test_select_address_tcp_repr_contains_tcp(self) -> None:
        addr = TransportScheme.TCP_ONLY.select_address("maya", "127.0.0.1", 18812)
        assert "tcp" in repr(addr).lower()

    def test_select_address_tcp_contains_host(self) -> None:
        addr = TransportScheme.TCP_ONLY.select_address("maya", "127.0.0.1", 18812)
        assert "127.0.0.1" in repr(addr)

    def test_select_address_tcp_contains_port(self) -> None:
        addr = TransportScheme.TCP_ONLY.select_address("maya", "127.0.0.1", 18812)
        assert "18812" in repr(addr)

    def test_select_address_auto_returns_address(self) -> None:
        addr = TransportScheme.AUTO.select_address("maya", "127.0.0.1", 18812)
        assert addr is not None

    def test_select_address_with_pid(self) -> None:
        addr = TransportScheme.AUTO.select_address("maya", "127.0.0.1", 18812, pid=os.getpid())
        assert addr is not None

    def test_select_address_prefer_ipc_returns_address(self) -> None:
        addr = TransportScheme.PREFER_IPC.select_address("maya", "127.0.0.1", 18812, pid=1234)
        assert addr is not None


# ══════════════════════════════════════════════════════════════════════════════
# RoutingStrategy
# ══════════════════════════════════════════════════════════════════════════════


class TestRoutingStrategy:
    def test_first_available(self) -> None:
        assert RoutingStrategy.FIRST_AVAILABLE is not None

    def test_round_robin(self) -> None:
        assert RoutingStrategy.ROUND_ROBIN is not None

    def test_least_busy(self) -> None:
        assert RoutingStrategy.LEAST_BUSY is not None

    def test_specific(self) -> None:
        assert RoutingStrategy.SPECIFIC is not None

    def test_scene_match(self) -> None:
        assert RoutingStrategy.SCENE_MATCH is not None

    def test_random(self) -> None:
        assert RoutingStrategy.RANDOM is not None

    def test_eq_same(self) -> None:
        assert RoutingStrategy.ROUND_ROBIN == RoutingStrategy.ROUND_ROBIN

    def test_ne_different(self) -> None:
        assert RoutingStrategy.FIRST_AVAILABLE != RoutingStrategy.ROUND_ROBIN

    def test_repr_is_str(self) -> None:
        assert isinstance(repr(RoutingStrategy.FIRST_AVAILABLE), str)

    def test_str_is_str(self) -> None:
        assert isinstance(str(RoutingStrategy.FIRST_AVAILABLE), str)


# ══════════════════════════════════════════════════════════════════════════════
# ServiceStatus
# ══════════════════════════════════════════════════════════════════════════════


class TestServiceStatus:
    def test_available(self) -> None:
        assert ServiceStatus.AVAILABLE is not None

    def test_busy(self) -> None:
        assert ServiceStatus.BUSY is not None

    def test_unreachable(self) -> None:
        assert ServiceStatus.UNREACHABLE is not None

    def test_shutting_down(self) -> None:
        assert ServiceStatus.SHUTTING_DOWN is not None

    def test_eq_same(self) -> None:
        assert ServiceStatus.AVAILABLE == ServiceStatus.AVAILABLE

    def test_ne_different(self) -> None:
        assert ServiceStatus.AVAILABLE != ServiceStatus.BUSY

    def test_repr_is_str(self) -> None:
        assert isinstance(repr(ServiceStatus.AVAILABLE), str)


# ══════════════════════════════════════════════════════════════════════════════
# IpcListener & ListenerHandle
# ══════════════════════════════════════════════════════════════════════════════


class TestIpcListenerCreate:
    def test_bind_returns_listener(self) -> None:
        addr = TransportAddress.tcp("127.0.0.1", 0)
        listener = IpcListener.bind(addr)
        listener.into_handle().shutdown()
        assert listener is not None

    def test_repr_is_str(self) -> None:
        addr = TransportAddress.tcp("127.0.0.1", 0)
        listener = IpcListener.bind(addr)
        r = repr(listener)
        listener.into_handle().shutdown()
        assert isinstance(r, str)

    def test_repr_contains_transport(self) -> None:
        addr = TransportAddress.tcp("127.0.0.1", 0)
        listener = IpcListener.bind(addr)
        r = repr(listener)
        listener.into_handle().shutdown()
        assert "tcp" in r.lower()

    def test_local_address_is_transport_address(self) -> None:
        addr = TransportAddress.tcp("127.0.0.1", 0)
        listener = IpcListener.bind(addr)
        local = listener.local_address()
        listener.into_handle().shutdown()
        from dcc_mcp_core import TransportAddress as TA

        assert isinstance(local, TA)

    def test_local_address_repr_contains_127(self) -> None:
        addr = TransportAddress.tcp("127.0.0.1", 0)
        listener = IpcListener.bind(addr)
        local = listener.local_address()
        listener.into_handle().shutdown()
        assert "127.0.0.1" in repr(local)

    def test_local_address_port_nonzero_after_bind(self) -> None:
        addr = TransportAddress.tcp("127.0.0.1", 0)
        listener = IpcListener.bind(addr)
        local = listener.local_address()
        r = repr(local)
        listener.into_handle().shutdown()
        # Port is assigned by OS, repr should not contain ":0)"
        assert ":0)" not in r


class TestListenerHandle:
    def test_into_handle_returns_handle(self) -> None:
        addr = TransportAddress.tcp("127.0.0.1", 0)
        listener = IpcListener.bind(addr)
        handle = listener.into_handle()
        handle.shutdown()
        assert handle is not None

    def test_accept_count_zero_before_accept(self) -> None:
        addr = TransportAddress.tcp("127.0.0.1", 0)
        listener = IpcListener.bind(addr)
        handle = listener.into_handle()
        count = handle.accept_count
        handle.shutdown()
        assert count == 0

    def test_is_shutdown_false_before_shutdown(self) -> None:
        addr = TransportAddress.tcp("127.0.0.1", 0)
        listener = IpcListener.bind(addr)
        handle = listener.into_handle()
        is_shut = handle.is_shutdown
        handle.shutdown()
        assert is_shut is False

    def test_is_shutdown_true_after_shutdown(self) -> None:
        addr = TransportAddress.tcp("127.0.0.1", 0)
        listener = IpcListener.bind(addr)
        handle = listener.into_handle()
        handle.shutdown()
        assert handle.is_shutdown is True

    def test_transport_name_is_str(self) -> None:
        addr = TransportAddress.tcp("127.0.0.1", 0)
        listener = IpcListener.bind(addr)
        handle = listener.into_handle()
        name = handle.transport_name
        handle.shutdown()
        assert isinstance(name, str)

    def test_transport_name_tcp(self) -> None:
        addr = TransportAddress.tcp("127.0.0.1", 0)
        listener = IpcListener.bind(addr)
        handle = listener.into_handle()
        name = handle.transport_name
        handle.shutdown()
        assert name == "tcp"

    def test_local_address_returns_transport_address(self) -> None:
        addr = TransportAddress.tcp("127.0.0.1", 0)
        listener = IpcListener.bind(addr)
        handle = listener.into_handle()
        la = handle.local_address()
        handle.shutdown()
        assert isinstance(la, TransportAddress)

    def test_local_address_contains_127(self) -> None:
        addr = TransportAddress.tcp("127.0.0.1", 0)
        listener = IpcListener.bind(addr)
        handle = listener.into_handle()
        la = handle.local_address()
        handle.shutdown()
        assert "127.0.0.1" in repr(la)

    def test_accept_count_is_int(self) -> None:
        addr = TransportAddress.tcp("127.0.0.1", 0)
        listener = IpcListener.bind(addr)
        handle = listener.into_handle()
        c = handle.accept_count
        handle.shutdown()
        assert isinstance(c, int)


# ══════════════════════════════════════════════════════════════════════════════
# TransportManager & ServiceEntry
# ══════════════════════════════════════════════════════════════════════════════


class TestTransportManagerCreate:
    def test_create(self) -> None:
        with tempfile.TemporaryDirectory() as d:
            mgr = TransportManager(d)
            assert mgr is not None

    def test_list_instances_empty_initially(self) -> None:
        with tempfile.TemporaryDirectory() as d:
            mgr = TransportManager(d)
            assert mgr.list_instances("maya") == []

    def test_list_all_services_empty_initially(self) -> None:
        with tempfile.TemporaryDirectory() as d:
            mgr = TransportManager(d)
            assert mgr.list_all_services() == []


class TestTransportManagerRegister:
    def test_register_returns_str(self) -> None:
        with tempfile.TemporaryDirectory() as d:
            mgr = TransportManager(d)
            iid = mgr.register_service("maya", "127.0.0.1", 18812)
            assert isinstance(iid, str)

    def test_register_returns_uuid_format(self) -> None:
        with tempfile.TemporaryDirectory() as d:
            mgr = TransportManager(d)
            iid = mgr.register_service("maya", "127.0.0.1", 18812)
            assert len(iid) == 36
            assert iid.count("-") == 4

    def test_list_instances_after_register(self) -> None:
        with tempfile.TemporaryDirectory() as d:
            mgr = TransportManager(d)
            mgr.register_service("maya", "127.0.0.1", 18812)
            instances = mgr.list_instances("maya")
            assert len(instances) == 1

    def test_list_all_services_after_register(self) -> None:
        with tempfile.TemporaryDirectory() as d:
            mgr = TransportManager(d)
            mgr.register_service("maya", "127.0.0.1", 18812)
            all_s = mgr.list_all_services()
            assert len(all_s) == 1

    def test_multiple_dccs_in_list_all(self) -> None:
        with tempfile.TemporaryDirectory() as d:
            mgr = TransportManager(d)
            mgr.register_service("maya", "127.0.0.1", 18812)
            mgr.register_service("blender", "127.0.0.1", 18813)
            all_s = mgr.list_all_services()
            assert len(all_s) == 2

    def test_different_dccs_isolated(self) -> None:
        with tempfile.TemporaryDirectory() as d:
            mgr = TransportManager(d)
            mgr.register_service("maya", "127.0.0.1", 18812)
            assert len(mgr.list_instances("blender")) == 0


class TestServiceEntry:
    def test_entry_dcc_type(self) -> None:
        with tempfile.TemporaryDirectory() as d:
            mgr = TransportManager(d)
            mgr.register_service("maya", "127.0.0.1", 18812)
            entry = mgr.list_instances("maya")[0]
            assert entry.dcc_type == "maya"

    def test_entry_host(self) -> None:
        with tempfile.TemporaryDirectory() as d:
            mgr = TransportManager(d)
            mgr.register_service("maya", "127.0.0.1", 18812)
            entry = mgr.list_instances("maya")[0]
            assert entry.host == "127.0.0.1"

    def test_entry_port(self) -> None:
        with tempfile.TemporaryDirectory() as d:
            mgr = TransportManager(d)
            mgr.register_service("maya", "127.0.0.1", 18812)
            entry = mgr.list_instances("maya")[0]
            assert entry.port == 18812

    def test_entry_status_available(self) -> None:
        with tempfile.TemporaryDirectory() as d:
            mgr = TransportManager(d)
            mgr.register_service("maya", "127.0.0.1", 18812)
            entry = mgr.list_instances("maya")[0]
            assert entry.status == ServiceStatus.AVAILABLE

    def test_entry_instance_id_is_str(self) -> None:
        with tempfile.TemporaryDirectory() as d:
            mgr = TransportManager(d)
            mgr.register_service("maya", "127.0.0.1", 18812)
            entry = mgr.list_instances("maya")[0]
            assert isinstance(entry.instance_id, str)

    def test_entry_instance_id_uuid_format(self) -> None:
        with tempfile.TemporaryDirectory() as d:
            mgr = TransportManager(d)
            mgr.register_service("maya", "127.0.0.1", 18812)
            entry = mgr.list_instances("maya")[0]
            assert len(entry.instance_id) == 36

    def test_entry_version_none_default(self) -> None:
        with tempfile.TemporaryDirectory() as d:
            mgr = TransportManager(d)
            mgr.register_service("maya", "127.0.0.1", 18812)
            entry = mgr.list_instances("maya")[0]
            assert entry.version is None or entry.version == ""

    def test_entry_version_set(self) -> None:
        with tempfile.TemporaryDirectory() as d:
            mgr = TransportManager(d)
            mgr.register_service("maya", "127.0.0.1", 18812, version="2025")
            entry = mgr.list_instances("maya")[0]
            assert entry.version == "2025"

    def test_entry_scene_none_default(self) -> None:
        with tempfile.TemporaryDirectory() as d:
            mgr = TransportManager(d)
            mgr.register_service("maya", "127.0.0.1", 18812)
            entry = mgr.list_instances("maya")[0]
            assert entry.scene is None or entry.scene == ""

    def test_entry_scene_set(self) -> None:
        with tempfile.TemporaryDirectory() as d:
            mgr = TransportManager(d)
            mgr.register_service("maya", "127.0.0.1", 18812, scene="test.ma")
            entry = mgr.list_instances("maya")[0]
            assert entry.scene == "test.ma"

    def test_entry_is_ipc_false_without_transport(self) -> None:
        with tempfile.TemporaryDirectory() as d:
            mgr = TransportManager(d)
            mgr.register_service("maya", "127.0.0.1", 18812)
            entry = mgr.list_instances("maya")[0]
            assert entry.is_ipc is False

    def test_entry_last_heartbeat_ms_is_int(self) -> None:
        with tempfile.TemporaryDirectory() as d:
            mgr = TransportManager(d)
            mgr.register_service("maya", "127.0.0.1", 18812)
            entry = mgr.list_instances("maya")[0]
            assert isinstance(entry.last_heartbeat_ms, int)

    def test_entry_to_dict(self) -> None:
        with tempfile.TemporaryDirectory() as d:
            mgr = TransportManager(d)
            mgr.register_service("maya", "127.0.0.1", 18812)
            entry = mgr.list_instances("maya")[0]
            d_ = entry.to_dict()
            assert isinstance(d_, dict)

    def test_entry_to_dict_has_dcc_type(self) -> None:
        with tempfile.TemporaryDirectory() as d:
            mgr = TransportManager(d)
            mgr.register_service("maya", "127.0.0.1", 18812)
            entry = mgr.list_instances("maya")[0]
            assert "dcc_type" in entry.to_dict()


class TestTransportManagerOperations:
    def test_get_service_returns_entry(self) -> None:
        with tempfile.TemporaryDirectory() as d:
            mgr = TransportManager(d)
            iid = mgr.register_service("maya", "127.0.0.1", 18812)
            entry = mgr.get_service("maya", iid)
            assert entry is not None

    def test_get_service_fields_match(self) -> None:
        with tempfile.TemporaryDirectory() as d:
            mgr = TransportManager(d)
            iid = mgr.register_service("maya", "127.0.0.1", 18812)
            entry = mgr.get_service("maya", iid)
            assert entry.dcc_type == "maya"
            assert entry.port == 18812

    def test_deregister_returns_true(self) -> None:
        with tempfile.TemporaryDirectory() as d:
            mgr = TransportManager(d)
            iid = mgr.register_service("maya", "127.0.0.1", 18812)
            assert mgr.deregister_service("maya", iid) is True

    def test_deregister_removes_from_list(self) -> None:
        with tempfile.TemporaryDirectory() as d:
            mgr = TransportManager(d)
            iid = mgr.register_service("maya", "127.0.0.1", 18812)
            mgr.deregister_service("maya", iid)
            assert len(mgr.list_instances("maya")) == 0

    def test_heartbeat_returns_bool(self) -> None:
        with tempfile.TemporaryDirectory() as d:
            mgr = TransportManager(d)
            iid = mgr.register_service("maya", "127.0.0.1", 18812)
            result = mgr.heartbeat("maya", iid)
            assert isinstance(result, bool)

    def test_heartbeat_returns_true_for_valid(self) -> None:
        with tempfile.TemporaryDirectory() as d:
            mgr = TransportManager(d)
            iid = mgr.register_service("maya", "127.0.0.1", 18812)
            assert mgr.heartbeat("maya", iid) is True

    def test_update_service_status_returns_true(self) -> None:
        with tempfile.TemporaryDirectory() as d:
            mgr = TransportManager(d)
            iid = mgr.register_service("maya", "127.0.0.1", 18812)
            assert mgr.update_service_status("maya", iid, ServiceStatus.BUSY) is True

    def test_update_service_status_persists(self) -> None:
        with tempfile.TemporaryDirectory() as d:
            mgr = TransportManager(d)
            iid = mgr.register_service("maya", "127.0.0.1", 18812)
            mgr.update_service_status("maya", iid, ServiceStatus.BUSY)
            entry = mgr.get_service("maya", iid)
            assert entry.status == ServiceStatus.BUSY

    def test_list_all_instances_alias(self) -> None:
        with tempfile.TemporaryDirectory() as d:
            mgr = TransportManager(d)
            mgr.register_service("maya", "127.0.0.1", 18812)
            all_s = mgr.list_all_instances()
            assert len(all_s) == 1

    def test_multiple_register_same_dcc(self) -> None:
        with tempfile.TemporaryDirectory() as d:
            mgr = TransportManager(d)
            mgr.register_service("maya", "127.0.0.1", 18812)
            mgr.register_service("maya", "127.0.0.1", 18813)
            assert len(mgr.list_instances("maya")) == 2


# ══════════════════════════════════════════════════════════════════════════════
# SkillWatcher
# ══════════════════════════════════════════════════════════════════════════════


class TestSkillWatcherCreate:
    def test_create_default(self) -> None:
        sw = SkillWatcher()
        assert sw is not None

    def test_create_with_debounce_ms(self) -> None:
        sw = SkillWatcher(debounce_ms=500)
        assert sw is not None

    def test_repr_is_str(self) -> None:
        sw = SkillWatcher()
        assert isinstance(repr(sw), str)

    def test_repr_contains_skills(self) -> None:
        sw = SkillWatcher()
        assert "skill" in repr(sw).lower() or "0" in repr(sw)

    def test_skills_initially_empty(self) -> None:
        sw = SkillWatcher()
        assert sw.skills() == []

    def test_skill_count_zero_initially(self) -> None:
        sw = SkillWatcher()
        assert sw.skill_count() == 0

    def test_watched_paths_empty_initially(self) -> None:
        sw = SkillWatcher()
        assert sw.watched_paths() == []

    def test_skills_returns_list(self) -> None:
        sw = SkillWatcher()
        assert isinstance(sw.skills(), list)

    def test_watched_paths_returns_list(self) -> None:
        sw = SkillWatcher()
        assert isinstance(sw.watched_paths(), list)

    def test_skill_count_is_int(self) -> None:
        sw = SkillWatcher()
        assert isinstance(sw.skill_count(), int)


class TestSkillWatcherWatch:
    def test_watch_adds_path(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            sw = SkillWatcher()
            sw.watch(tmpdir)
            paths = sw.watched_paths()
            assert len(paths) == 1

    def test_watched_paths_contains_path(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            sw = SkillWatcher()
            sw.watch(tmpdir)
            paths = sw.watched_paths()
            # Normalize separators for comparison
            assert any(tmpdir.replace("\\", "/") in p.replace("\\", "/") or tmpdir in p for p in paths)

    def test_watch_nonexistent_raises(self) -> None:
        sw = SkillWatcher()
        with pytest.raises((RuntimeError, OSError, Exception)):
            sw.watch("/nonexistent/path/that/does/not/exist/12345")

    def test_watch_multiple_paths(self) -> None:
        with tempfile.TemporaryDirectory() as d1, tempfile.TemporaryDirectory() as d2:
            sw = SkillWatcher()
            sw.watch(d1)
            sw.watch(d2)
            assert len(sw.watched_paths()) == 2

    def test_reload_does_not_raise(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            sw = SkillWatcher()
            sw.watch(tmpdir)
            sw.reload()

    def test_skills_after_watch_empty_dir(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            sw = SkillWatcher()
            sw.watch(tmpdir)
            assert sw.skills() == []

    def test_skill_count_after_watch_empty_dir(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            sw = SkillWatcher()
            sw.watch(tmpdir)
            assert sw.skill_count() == 0

    def test_reload_without_watch_ok(self) -> None:
        sw = SkillWatcher()
        sw.reload()


# ══════════════════════════════════════════════════════════════════════════════
# Integration: McpHttpServer + TransportManager
# ══════════════════════════════════════════════════════════════════════════════


class TestMcpHttpAndTransportIntegration:
    def test_server_port_matches_mcp_url(self) -> None:
        server = McpHttpServer(_make_registry(), McpHttpConfig(port=0))
        handle = server.start()
        port = handle.port
        url = handle.mcp_url()
        handle.shutdown()
        assert str(port) in url

    def test_transport_manager_with_ipc_address(self) -> None:
        with tempfile.TemporaryDirectory() as d:
            mgr = TransportManager(d)
            ipc_addr = TransportAddress.default_local("maya", 12345)
            iid = mgr.register_service("maya", "127.0.0.1", 18812, transport_address=ipc_addr)
            entry = mgr.get_service("maya", iid)
            assert entry is not None
            assert entry.is_ipc is True

    def test_bind_and_register(self) -> None:
        with tempfile.TemporaryDirectory() as d:
            mgr = TransportManager(d)
            iid, listener = mgr.bind_and_register("maya", version="2025")
            la = listener.local_address()
            handle = listener.into_handle()
            handle.shutdown()
            assert isinstance(iid, str)
            assert len(iid) == 36
            assert la is not None

    def test_schema_and_address_combination(self) -> None:
        scheme = TransportScheme.TCP_ONLY
        addr = scheme.select_address("maya", "127.0.0.1", 18812)
        assert "127.0.0.1" in repr(addr)
        assert "18812" in repr(addr)
