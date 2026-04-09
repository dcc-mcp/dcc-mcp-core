"""Deep tests for PyDccLauncher and encode/decode framing functions.

Covers:
- PyDccLauncher: launch/terminate/kill/pid_of/running_count/restart_count
- encode_request / encode_response / encode_notify / decode_envelope roundtrip
"""

from __future__ import annotations

import contextlib
import struct
import sys
import time

import pytest

import dcc_mcp_core

# ---------------------------------------------------------------------------
# Helper
# ---------------------------------------------------------------------------


def _strip_frame(framed: bytes) -> bytes:
    """Strip the 4-byte big-endian length prefix from a framed message."""
    return framed[4:]


def decode(framed: bytes) -> dict:
    """Decode a framed message, stripping the length prefix first."""
    return dcc_mcp_core.decode_envelope(_strip_frame(framed))


# ---------------------------------------------------------------------------
# PyDccLauncher
# ---------------------------------------------------------------------------


class TestPyDccLauncherInitial:
    def test_default_construction(self) -> None:
        launcher = dcc_mcp_core.PyDccLauncher()
        assert launcher is not None

    def test_running_count_empty(self) -> None:
        launcher = dcc_mcp_core.PyDccLauncher()
        assert launcher.running_count() == 0

    def test_pid_of_unknown_returns_none(self) -> None:
        launcher = dcc_mcp_core.PyDccLauncher()
        assert launcher.pid_of("no_such_app") is None

    def test_restart_count_unknown_returns_zero(self) -> None:
        launcher = dcc_mcp_core.PyDccLauncher()
        assert launcher.restart_count("no_such_app") == 0

    def test_kill_unknown_raises(self) -> None:
        launcher = dcc_mcp_core.PyDccLauncher()
        with pytest.raises(RuntimeError, match="not running"):
            launcher.kill("no_such_app")

    def test_terminate_unknown_raises(self) -> None:
        launcher = dcc_mcp_core.PyDccLauncher()
        with pytest.raises(RuntimeError, match="not running"):
            launcher.terminate("no_such_app")


@pytest.mark.skipif(sys.platform != "win32", reason="notepad only on Windows")
class TestPyDccLauncherLifecycle:
    """Tests that actually spawn a real process (notepad on Windows)."""

    def test_launch_returns_dict(self) -> None:
        launcher = dcc_mcp_core.PyDccLauncher()
        info = launcher.launch("test_np", "notepad")
        try:
            assert isinstance(info, dict)
            assert "pid" in info
            assert "name" in info
            assert "status" in info
            assert info["name"] == "test_np"
            assert isinstance(info["pid"], int)
            assert info["pid"] > 0
        finally:
            with contextlib.suppress(Exception):
                launcher.kill("test_np")

    def test_running_count_after_launch(self) -> None:
        launcher = dcc_mcp_core.PyDccLauncher()
        launcher.launch("test_np2", "notepad")
        try:
            assert launcher.running_count() == 1
        finally:
            with contextlib.suppress(Exception):
                launcher.kill("test_np2")

    def test_pid_of_after_launch(self) -> None:
        launcher = dcc_mcp_core.PyDccLauncher()
        info = launcher.launch("test_np3", "notepad")
        try:
            pid = launcher.pid_of("test_np3")
            assert pid is not None
            assert pid == info["pid"]
        finally:
            with contextlib.suppress(Exception):
                launcher.kill("test_np3")

    def test_terminate_clears_running(self) -> None:
        launcher = dcc_mcp_core.PyDccLauncher()
        launcher.launch("test_np4", "notepad")
        assert launcher.running_count() == 1
        launcher.terminate("test_np4", timeout_ms=3000)
        time.sleep(0.4)
        assert launcher.running_count() == 0

    def test_kill_clears_running(self) -> None:
        launcher = dcc_mcp_core.PyDccLauncher()
        launcher.launch("test_np5", "notepad")
        assert launcher.running_count() == 1
        launcher.kill("test_np5")
        time.sleep(0.2)
        assert launcher.running_count() == 0

    def test_pid_of_after_kill_is_none(self) -> None:
        launcher = dcc_mcp_core.PyDccLauncher()
        launcher.launch("test_np6", "notepad")
        launcher.kill("test_np6")
        time.sleep(0.2)
        assert launcher.pid_of("test_np6") is None

    def test_restart_count_initial_zero(self) -> None:
        launcher = dcc_mcp_core.PyDccLauncher()
        launcher.launch("test_np7", "notepad")
        try:
            rc = launcher.restart_count("test_np7")
            assert rc == 0
        finally:
            with contextlib.suppress(Exception):
                launcher.kill("test_np7")

    def test_multiple_named_processes(self) -> None:
        launcher = dcc_mcp_core.PyDccLauncher()
        launcher.launch("proc_a", "notepad")
        launcher.launch("proc_b", "notepad")
        try:
            assert launcher.running_count() == 2
            pid_a = launcher.pid_of("proc_a")
            pid_b = launcher.pid_of("proc_b")
            assert pid_a is not None
            assert pid_b is not None
            assert pid_a != pid_b
        finally:
            for name in ("proc_a", "proc_b"):
                with contextlib.suppress(Exception):
                    launcher.kill(name)


class TestPyDccLauncherErrors:
    def test_launch_nonexistent_raises(self) -> None:
        launcher = dcc_mcp_core.PyDccLauncher()
        with pytest.raises(RuntimeError):
            launcher.launch("bad_app", "nonexistent_executable_xyz_abc")

    def test_launch_nonexistent_does_not_increment_count(self) -> None:
        launcher = dcc_mcp_core.PyDccLauncher()
        with contextlib.suppress(RuntimeError):
            launcher.launch("bad_app2", "nonexistent_executable_xyz_abc")
        assert launcher.running_count() == 0


# ---------------------------------------------------------------------------
# encode_request / decode_envelope
# ---------------------------------------------------------------------------


class TestEncodeRequest:
    def test_returns_bytes(self) -> None:
        frame = dcc_mcp_core.encode_request("my_method")
        assert isinstance(frame, bytes)

    def test_has_length_prefix(self) -> None:
        frame = dcc_mcp_core.encode_request("my_method")
        assert len(frame) >= 4
        declared_len = struct.unpack(">I", frame[:4])[0]
        assert declared_len == len(frame) - 4

    def test_decode_type_is_request(self) -> None:
        frame = dcc_mcp_core.encode_request("do_thing")
        env = decode(frame)
        assert env["type"] == "request"

    def test_decode_method(self) -> None:
        frame = dcc_mcp_core.encode_request("create_sphere")
        env = decode(frame)
        assert env["method"] == "create_sphere"

    def test_decode_id_is_uuid_string(self) -> None:
        frame = dcc_mcp_core.encode_request("do_thing")
        env = decode(frame)
        req_id = env["id"]
        assert isinstance(req_id, str)
        assert len(req_id) == 36  # UUID format
        assert req_id.count("-") == 4

    def test_decode_params_empty_by_default(self) -> None:
        frame = dcc_mcp_core.encode_request("list_objects")
        env = decode(frame)
        assert env["params"] == b""

    def test_decode_params_with_payload(self) -> None:
        payload = b'{"radius": 1.5}'
        frame = dcc_mcp_core.encode_request("create_sphere", payload)
        env = decode(frame)
        assert env["params"] == payload

    def test_unique_ids_per_call(self) -> None:
        frame1 = dcc_mcp_core.encode_request("method_a")
        frame2 = dcc_mcp_core.encode_request("method_a")
        id1 = decode(frame1)["id"]
        id2 = decode(frame2)["id"]
        assert id1 != id2

    def test_binary_params(self) -> None:
        payload = bytes(range(256))
        frame = dcc_mcp_core.encode_request("send_data", payload)
        env = decode(frame)
        assert env["params"] == payload

    def test_empty_method_accepted(self) -> None:
        frame = dcc_mcp_core.encode_request("")
        env = decode(frame)
        assert env["method"] == ""


# ---------------------------------------------------------------------------
# encode_response / decode_envelope
# ---------------------------------------------------------------------------


class TestEncodeResponse:
    def _make_request_id(self) -> str:
        frame = dcc_mcp_core.encode_request("ping")
        return decode(frame)["id"]

    def test_returns_bytes(self) -> None:
        req_id = self._make_request_id()
        frame = dcc_mcp_core.encode_response(req_id, True)
        assert isinstance(frame, bytes)

    def test_decode_type_is_response(self) -> None:
        req_id = self._make_request_id()
        frame = dcc_mcp_core.encode_response(req_id, True)
        env = decode(frame)
        assert env["type"] == "response"

    def test_decode_success_true(self) -> None:
        req_id = self._make_request_id()
        frame = dcc_mcp_core.encode_response(req_id, True, b"ok")
        env = decode(frame)
        assert env["success"] is True

    def test_decode_success_false(self) -> None:
        req_id = self._make_request_id()
        frame = dcc_mcp_core.encode_response(req_id, False, error="not found")
        env = decode(frame)
        assert env["success"] is False

    def test_decode_id_matches_request(self) -> None:
        req_id = self._make_request_id()
        frame = dcc_mcp_core.encode_response(req_id, True)
        env = decode(frame)
        assert env["id"] == req_id

    def test_decode_payload(self) -> None:
        req_id = self._make_request_id()
        payload = b"result_data"
        frame = dcc_mcp_core.encode_response(req_id, True, payload)
        env = decode(frame)
        assert env["payload"] == payload

    def test_decode_payload_empty_by_default(self) -> None:
        req_id = self._make_request_id()
        frame = dcc_mcp_core.encode_response(req_id, True)
        env = decode(frame)
        assert env["payload"] == b""

    def test_decode_error_message(self) -> None:
        req_id = self._make_request_id()
        frame = dcc_mcp_core.encode_response(req_id, False, error="permission denied")
        env = decode(frame)
        assert env["error"] == "permission denied"

    def test_decode_error_none_on_success(self) -> None:
        req_id = self._make_request_id()
        frame = dcc_mcp_core.encode_response(req_id, True, b"data")
        env = decode(frame)
        assert env["error"] is None

    def test_binary_payload_roundtrip(self) -> None:
        req_id = self._make_request_id()
        payload = bytes(range(200))
        frame = dcc_mcp_core.encode_response(req_id, True, payload)
        env = decode(frame)
        assert env["payload"] == payload


# ---------------------------------------------------------------------------
# encode_notify / decode_envelope
# ---------------------------------------------------------------------------


class TestEncodeNotify:
    def test_returns_bytes(self) -> None:
        frame = dcc_mcp_core.encode_notify("heartbeat")
        assert isinstance(frame, bytes)

    def test_decode_type_is_notify(self) -> None:
        frame = dcc_mcp_core.encode_notify("heartbeat")
        env = decode(frame)
        assert env["type"] == "notify"

    def test_decode_topic(self) -> None:
        frame = dcc_mcp_core.encode_notify("scene_changed")
        env = decode(frame)
        assert env["topic"] == "scene_changed"

    def test_decode_id_is_none(self) -> None:
        frame = dcc_mcp_core.encode_notify("ping")
        env = decode(frame)
        assert env["id"] is None

    def test_decode_data_empty_by_default(self) -> None:
        frame = dcc_mcp_core.encode_notify("heartbeat")
        env = decode(frame)
        assert env["data"] == b""

    def test_decode_data_with_payload(self) -> None:
        payload = b"scene_name"
        frame = dcc_mcp_core.encode_notify("scene_changed", payload)
        env = decode(frame)
        assert env["data"] == payload

    def test_binary_data_roundtrip(self) -> None:
        payload = bytes(range(128))
        frame = dcc_mcp_core.encode_notify("bulk_data", payload)
        env = decode(frame)
        assert env["data"] == payload

    def test_multiple_topics_distinct(self) -> None:
        topics = ["scene_saved", "render_started", "export_done"]
        for topic in topics:
            frame = dcc_mcp_core.encode_notify(topic, b"payload")
            env = decode(frame)
            assert env["topic"] == topic


# ---------------------------------------------------------------------------
# decode_envelope error handling
# ---------------------------------------------------------------------------


class TestDecodeEnvelopeErrors:
    def test_empty_bytes_raises(self) -> None:
        with pytest.raises(RuntimeError):
            dcc_mcp_core.decode_envelope(b"")

    def test_garbage_bytes_raises(self) -> None:
        with pytest.raises(RuntimeError):
            dcc_mcp_core.decode_envelope(b"\xff\xfe\xfd\xfc" * 10)

    def test_truncated_frame_raises(self) -> None:
        frame = dcc_mcp_core.encode_request("method")
        # pass only half the payload (without 4-byte prefix)
        payload = _strip_frame(frame)
        with pytest.raises(RuntimeError):
            dcc_mcp_core.decode_envelope(payload[: len(payload) // 2])
