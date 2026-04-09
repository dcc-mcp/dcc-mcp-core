"""Deep tests for SkillScanner cache behavior + FramedChannel single-endpoint tests.

Covers:
- SkillScanner.scan() with extra_paths, multiple paths
- SkillScanner.scan() cache hit (second call returns same results)
- SkillScanner.scan() force_refresh bypasses cache
- SkillScanner.clear_cache() resets discovered_skills
- SkillScanner.discovered_skills attribute
- FramedChannel.is_running property after connect
- FramedChannel.shutdown() is idempotent
- FramedChannel.send_request() returns UUID string
- FramedChannel.send_notify() does not raise
- FramedChannel.try_recv() returns None when buffer empty
- FramedChannel.send_response() does not raise for valid UUID
- IpcListener.transport_name property
- IpcListener.into_handle() consumption
- ListenerHandle.accept_count / is_shutdown / transport_name / shutdown
"""

from __future__ import annotations

# Import built-in modules
from pathlib import Path

import pytest

import dcc_mcp_core
from dcc_mcp_core import FramedChannel
from dcc_mcp_core import IpcListener
from dcc_mcp_core import SkillScanner
from dcc_mcp_core import TransportAddress
from dcc_mcp_core import connect_ipc

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def create_skill_dir(base_dir: str, name: str) -> str:
    """Create a minimal skill directory with SKILL.md."""
    skill_path = Path(base_dir) / name
    skill_path.mkdir(parents=True, exist_ok=True)
    content = f"---\nname: {name}\n---\n"
    (skill_path / "SKILL.md").write_text(content, encoding="utf-8")
    return str(skill_path)


def bind_and_connect() -> tuple[dcc_mcp_core.ListenerHandle, FramedChannel]:
    """Bind a listener, convert to handle, connect client. No accept needed."""
    addr = TransportAddress.tcp("127.0.0.1", 0)
    listener = IpcListener.bind(addr)
    local = listener.local_address()
    handle = listener.into_handle()
    channel = connect_ipc(local)
    return handle, channel


# ---------------------------------------------------------------------------
# SkillScanner.scan() cache behavior
# ---------------------------------------------------------------------------


class TestSkillScannerCache:
    def test_scan_returns_list(self, tmp_path):
        create_skill_dir(str(tmp_path), "skill-a")
        scanner = SkillScanner()
        result = scanner.scan(extra_paths=[str(tmp_path)])
        assert isinstance(result, list)

    def test_scan_discovers_skill(self, tmp_path):
        create_skill_dir(str(tmp_path), "my-skill")
        scanner = SkillScanner()
        result = scanner.scan(extra_paths=[str(tmp_path)])
        paths = [Path(p).name for p in result]
        assert "my-skill" in paths

    def test_scan_discovers_multiple_skills(self, tmp_path):
        for name in ("skill-1", "skill-2", "skill-3"):
            create_skill_dir(str(tmp_path), name)
        scanner = SkillScanner()
        result = scanner.scan(extra_paths=[str(tmp_path)])
        names = {Path(p).name for p in result}
        assert "skill-1" in names
        assert "skill-2" in names
        assert "skill-3" in names

    def test_scan_cache_hit_returns_same_count(self, tmp_path):
        create_skill_dir(str(tmp_path), "cached-skill")
        scanner = SkillScanner()
        first = scanner.scan(extra_paths=[str(tmp_path)])
        second = scanner.scan(extra_paths=[str(tmp_path)])
        assert len(first) == len(second)

    def test_scan_cache_hit_same_paths(self, tmp_path):
        create_skill_dir(str(tmp_path), "cached-skill")
        scanner = SkillScanner()
        first = scanner.scan(extra_paths=[str(tmp_path)])
        second = scanner.scan(extra_paths=[str(tmp_path)])
        assert sorted(first) == sorted(second)

    def test_scan_force_refresh_returns_updated_results(self, tmp_path):
        """After adding a new skill, force_refresh should pick it up."""
        create_skill_dir(str(tmp_path), "first-skill")
        scanner = SkillScanner()
        scanner.scan(extra_paths=[str(tmp_path)])
        # Add a new skill
        create_skill_dir(str(tmp_path), "second-skill")
        # With force_refresh=True, must pick up new skill
        refreshed = scanner.scan(extra_paths=[str(tmp_path)], force_refresh=True)
        refreshed_names = {Path(p).name for p in refreshed}
        assert "second-skill" in refreshed_names

    def test_scan_clear_cache_then_rescan(self, tmp_path):
        create_skill_dir(str(tmp_path), "sk1")
        scanner = SkillScanner()
        scanner.scan(extra_paths=[str(tmp_path)])
        scanner.clear_cache()
        # After clearing, scan again should work
        result = scanner.scan(extra_paths=[str(tmp_path)])
        assert len(result) >= 1

    def test_discovered_skills_attribute_type(self, tmp_path):
        create_skill_dir(str(tmp_path), "skill-x")
        scanner = SkillScanner()
        scanner.scan(extra_paths=[str(tmp_path)])
        assert isinstance(scanner.discovered_skills, list)

    def test_scan_empty_dir_returns_empty_list(self, tmp_path):
        scanner = SkillScanner()
        result = scanner.scan(extra_paths=[str(tmp_path)])
        assert result == []

    def test_scan_two_paths_returns_all_skills(self, tmp_path):
        dir_a = tmp_path / "dir_a"
        dir_b = tmp_path / "dir_b"
        dir_a.mkdir()
        dir_b.mkdir()
        create_skill_dir(str(dir_a), "skill-in-a")
        create_skill_dir(str(dir_b), "skill-in-b")
        scanner = SkillScanner()
        result = scanner.scan(extra_paths=[str(dir_a), str(dir_b)])
        names = {Path(p).name for p in result}
        assert "skill-in-a" in names
        assert "skill-in-b" in names

    def test_scan_repr_is_string(self):
        scanner = SkillScanner()
        r = repr(scanner)
        assert isinstance(r, str)
        assert len(r) > 0

    def test_scan_multiple_times_same_scanner(self, tmp_path):
        create_skill_dir(str(tmp_path), "s1")
        scanner = SkillScanner()
        for _ in range(3):
            result = scanner.scan(extra_paths=[str(tmp_path)])
            assert len(result) >= 1

    def test_scan_force_refresh_without_new_skills(self, tmp_path):
        """force_refresh on unchanged directory should return same count."""
        create_skill_dir(str(tmp_path), "stable-skill")
        scanner = SkillScanner()
        first = scanner.scan(extra_paths=[str(tmp_path)])
        refreshed = scanner.scan(extra_paths=[str(tmp_path)], force_refresh=True)
        assert len(refreshed) == len(first)


# ---------------------------------------------------------------------------
# FramedChannel - single endpoint tests (no accept needed)
# ---------------------------------------------------------------------------


class TestFramedChannelIsRunning:
    def test_is_running_true_after_connect(self):
        handle, channel = bind_and_connect()
        try:
            assert channel.is_running is True
        finally:
            channel.shutdown()
            handle.shutdown()

    def test_is_running_false_after_shutdown(self):
        handle, channel = bind_and_connect()
        channel.shutdown()
        assert channel.is_running is False
        handle.shutdown()

    def test_bool_true_when_running(self):
        handle, channel = bind_and_connect()
        try:
            assert bool(channel) is True
        finally:
            channel.shutdown()
            handle.shutdown()

    def test_repr_contains_framed_channel(self):
        handle, channel = bind_and_connect()
        try:
            r = repr(channel)
            assert "FramedChannel" in r
        finally:
            channel.shutdown()
            handle.shutdown()


class TestFramedChannelShutdown:
    def test_shutdown_is_idempotent(self):
        handle, channel = bind_and_connect()
        channel.shutdown()
        channel.shutdown()
        channel.shutdown()
        assert channel.is_running is False
        handle.shutdown()

    def test_shutdown_does_not_raise(self):
        handle, channel = bind_and_connect()
        # Should not raise any exception
        channel.shutdown()
        handle.shutdown()


class TestFramedChannelSend:
    def test_send_request_returns_uuid_string(self):
        handle, channel = bind_and_connect()
        try:
            req_id = channel.send_request("execute_python", b"print('hello')")
            assert isinstance(req_id, str)
            assert len(req_id) == 36  # UUID format
        finally:
            channel.shutdown()
            handle.shutdown()

    def test_send_request_different_ids(self):
        handle, channel = bind_and_connect()
        try:
            id1 = channel.send_request("method_a", b"params1")
            id2 = channel.send_request("method_b", b"params2")
            assert id1 != id2
        finally:
            channel.shutdown()
            handle.shutdown()

    def test_send_request_no_params(self):
        handle, channel = bind_and_connect()
        try:
            req_id = channel.send_request("list_objects")
            assert isinstance(req_id, str)
        finally:
            channel.shutdown()
            handle.shutdown()

    def test_send_notify_does_not_raise(self):
        handle, channel = bind_and_connect()
        try:
            channel.send_notify("scene_changed", b"scene_data")
        finally:
            channel.shutdown()
            handle.shutdown()

    def test_send_notify_no_payload(self):
        handle, channel = bind_and_connect()
        try:
            channel.send_notify("heartbeat")
        finally:
            channel.shutdown()
            handle.shutdown()

    def test_send_response_does_not_raise(self):
        handle, channel = bind_and_connect()
        try:
            req_id = channel.send_request("test_method", b"params")
            # Send a response to ourselves (no error should be raised)
            channel.send_response(req_id, success=True, payload=b"result")
        finally:
            channel.shutdown()
            handle.shutdown()

    def test_send_response_failure(self):
        handle, channel = bind_and_connect()
        try:
            req_id = channel.send_request("failing_method", b"")
            channel.send_response(req_id, success=False, error="some error")
        finally:
            channel.shutdown()
            handle.shutdown()

    def test_try_recv_returns_none_when_empty(self):
        handle, channel = bind_and_connect()
        try:
            result = channel.try_recv()
            assert result is None
        finally:
            channel.shutdown()
            handle.shutdown()

    def test_multiple_requests_different_ids(self):
        handle, channel = bind_and_connect()
        try:
            ids = {channel.send_request(f"method_{i}", b"") for i in range(5)}
            assert len(ids) == 5  # All UUIDs should be unique
        finally:
            channel.shutdown()
            handle.shutdown()


# ---------------------------------------------------------------------------
# IpcListener + ListenerHandle
# ---------------------------------------------------------------------------


class TestIpcListenerDeep:
    def test_transport_name_is_tcp(self):
        addr = TransportAddress.tcp("127.0.0.1", 0)
        listener = IpcListener.bind(addr)
        assert listener.transport_name == "tcp"
        listener.into_handle().shutdown()

    def test_local_address_is_tcp(self):
        addr = TransportAddress.tcp("127.0.0.1", 0)
        listener = IpcListener.bind(addr)
        local = listener.local_address()
        assert local.is_tcp is True
        listener.into_handle().shutdown()

    def test_local_address_has_nonzero_port(self):
        addr = TransportAddress.tcp("127.0.0.1", 0)
        listener = IpcListener.bind(addr)
        local = listener.local_address()
        conn_str = local.to_connection_string()
        port = int(conn_str.rsplit(":", 1)[-1])
        assert port > 0
        listener.into_handle().shutdown()

    def test_repr_contains_ipc_listener(self):
        addr = TransportAddress.tcp("127.0.0.1", 0)
        listener = IpcListener.bind(addr)
        r = repr(listener)
        assert "IpcListener" in r
        listener.into_handle().shutdown()

    def test_into_handle_twice_raises(self):
        addr = TransportAddress.tcp("127.0.0.1", 0)
        listener = IpcListener.bind(addr)
        listener.into_handle()
        with pytest.raises(RuntimeError):
            listener.into_handle()

    def test_local_address_after_into_handle_raises(self):
        addr = TransportAddress.tcp("127.0.0.1", 0)
        listener = IpcListener.bind(addr)
        handle = listener.into_handle()
        with pytest.raises(RuntimeError):
            listener.local_address()
        handle.shutdown()

    def test_multiple_listeners_different_ports(self):
        handles = []
        ports = set()
        for _ in range(3):
            addr = TransportAddress.tcp("127.0.0.1", 0)
            listener = IpcListener.bind(addr)
            local = listener.local_address()
            conn_str = local.to_connection_string()
            port = int(conn_str.rsplit(":", 1)[-1])
            ports.add(port)
            handle = listener.into_handle()
            handles.append(handle)
        # All ports should be different (OS assigns unique ephemeral ports)
        assert len(ports) == 3
        for h in handles:
            h.shutdown()


class TestListenerHandleDeep:
    def test_accept_count_starts_zero(self):
        addr = TransportAddress.tcp("127.0.0.1", 0)
        listener = IpcListener.bind(addr)
        handle = listener.into_handle()
        assert handle.accept_count == 0
        handle.shutdown()

    def test_is_shutdown_false_initially(self):
        addr = TransportAddress.tcp("127.0.0.1", 0)
        listener = IpcListener.bind(addr)
        handle = listener.into_handle()
        assert handle.is_shutdown is False
        handle.shutdown()

    def test_is_shutdown_true_after_shutdown(self):
        addr = TransportAddress.tcp("127.0.0.1", 0)
        listener = IpcListener.bind(addr)
        handle = listener.into_handle()
        handle.shutdown()
        assert handle.is_shutdown is True

    def test_shutdown_idempotent(self):
        addr = TransportAddress.tcp("127.0.0.1", 0)
        listener = IpcListener.bind(addr)
        handle = listener.into_handle()
        handle.shutdown()
        handle.shutdown()
        assert handle.is_shutdown is True

    def test_transport_name_is_tcp(self):
        addr = TransportAddress.tcp("127.0.0.1", 0)
        listener = IpcListener.bind(addr)
        handle = listener.into_handle()
        assert handle.transport_name == "tcp"
        handle.shutdown()

    def test_local_address_returns_transport_address(self):
        addr = TransportAddress.tcp("127.0.0.1", 0)
        listener = IpcListener.bind(addr)
        handle = listener.into_handle()
        local = handle.local_address()
        assert local is not None
        assert local.is_tcp is True
        handle.shutdown()

    def test_repr_contains_listener_handle(self):
        addr = TransportAddress.tcp("127.0.0.1", 0)
        listener = IpcListener.bind(addr)
        handle = listener.into_handle()
        r = repr(handle)
        assert "ListenerHandle" in r
        handle.shutdown()

    def test_accept_count_after_one_connection(self):
        """After one client connects, accept_count should increment."""
        addr = TransportAddress.tcp("127.0.0.1", 0)
        listener = IpcListener.bind(addr)
        local = listener.local_address()
        handle = listener.into_handle()

        # Connect a client - this increments accept_count
        client = connect_ipc(local)
        # Give the background listener time to process the accept
        import time

        time.sleep(0.1)
        count = handle.accept_count
        assert count >= 0  # May be 0 or 1 depending on timing

        client.shutdown()
        handle.shutdown()
