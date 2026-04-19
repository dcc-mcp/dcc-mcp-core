"""Tests for DCC-Link adapter Python bindings (DccLinkFrame, IpcChannelAdapter, etc.)."""

from __future__ import annotations

import os
import pathlib
import tempfile
import threading

import pytest

from dcc_mcp_core import DccLinkFrame
from dcc_mcp_core import GracefulIpcChannelAdapter
from dcc_mcp_core import IpcChannelAdapter
from dcc_mcp_core import SocketServerAdapter

# ── DccLinkFrame ──────────────────────────────────────────────────────────────


class TestDccLinkFrame:
    """Tests for DccLinkFrame construction, encode/decode round-trip."""

    def test_create_with_body(self) -> None:
        frame = DccLinkFrame(msg_type=1, seq=42, body=b"hello")
        assert frame.msg_type == 1
        assert frame.seq == 42
        assert frame.body == b"hello"

    def test_create_without_body(self) -> None:
        frame = DccLinkFrame(msg_type=2, seq=0)
        assert frame.body == b""

    def test_rejects_invalid_msg_type(self) -> None:
        with pytest.raises(ValueError, match="unknown DccLinkType"):
            DccLinkFrame(msg_type=255, seq=0)

    def test_encode_decode_roundtrip(self) -> None:
        frame = DccLinkFrame(msg_type=1, seq=99, body=b"\x01\x02\x03")
        encoded = frame.encode()
        assert isinstance(encoded, bytes)
        decoded = DccLinkFrame.decode(encoded)
        assert decoded.msg_type == 1
        assert decoded.seq == 99
        assert decoded.body == b"\x01\x02\x03"

    def test_repr(self) -> None:
        frame = DccLinkFrame(msg_type=1, seq=0, body=b"abc")
        assert "DccLinkFrame" in repr(frame)
        assert "3 bytes" in repr(frame)


# ── IpcChannelAdapter ─────────────────────────────────────────────────────────


class TestIpcChannelAdapter:
    """Tests for IpcChannelAdapter create/connect/send/recv."""

    def test_create(self) -> None:
        name = f"test-ipc-create-{os.getpid()}-{id(object())}"
        server = IpcChannelAdapter.create(name)
        assert server is not None

    @pytest.mark.skipif(os.name == "nt", reason="Named pipe race on Windows CI")
    def test_send_recv_roundtrip(self) -> None:
        name = f"test-ipc-send-{os.getpid()}-{id(object())}"
        server = IpcChannelAdapter.create(name)

        # Client connects in a background thread (wait_for_client blocks).
        connected = threading.Event()

        def client_connect() -> None:
            IpcChannelAdapter.connect(name)
            connected.set()

        t = threading.Thread(target=client_connect, daemon=True)
        t.start()

        server.wait_for_client()
        connected.wait(timeout=5)

        # IPC send/recv is tested at Rust level.


# ── GracefulIpcChannelAdapter ─────────────────────────────────────────────────


class TestGracefulIpcChannelAdapter:
    """Tests for GracefulIpcChannelAdapter create/connect/shutdown."""

    def test_create(self) -> None:
        name = f"test-graceful-create-{os.getpid()}-{id(object())}"
        server = GracefulIpcChannelAdapter.create(name)
        assert server is not None

    def test_shutdown(self) -> None:
        name = f"test-graceful-shutdown-{os.getpid()}-{id(object())}"
        server = GracefulIpcChannelAdapter.create(name)
        server.shutdown()

    def test_bind_affinity_thread(self) -> None:
        name = f"test-graceful-affinity-{os.getpid()}-{id(object())}"
        server = GracefulIpcChannelAdapter.create(name)
        server.bind_affinity_thread()

    def test_pump_pending(self) -> None:
        name = f"test-graceful-pump-{os.getpid()}-{id(object())}"
        server = GracefulIpcChannelAdapter.create(name)
        count = server.pump_pending(budget_ms=10)
        assert isinstance(count, int)
        assert count == 0  # Nothing queued

    @pytest.mark.skipif(os.name == "nt", reason="Named pipe race on Windows CI")
    def test_send_recv_roundtrip(self) -> None:
        name = f"test-graceful-send-{os.getpid()}-{id(object())}"
        server = GracefulIpcChannelAdapter.create(name)

        # Client connects in a background thread.
        connected = threading.Event()

        def client_connect() -> None:
            GracefulIpcChannelAdapter.connect(name)
            connected.set()

        t = threading.Thread(target=client_connect, daemon=True)
        t.start()

        server.wait_for_client()
        connected.wait(timeout=5)

        # IPC send/recv is tested at Rust level.


# ── SocketServerAdapter ───────────────────────────────────────────────────────


class TestSocketServerAdapter:
    """Tests for SocketServerAdapter construction and properties."""

    def test_create_and_properties(self) -> None:
        if os.name == "nt":
            path = f"dcc-mcp-test-{os.getpid()}-{id(object())}"
        else:
            path = str(pathlib.Path(tempfile.gettempdir()) / f"dcc-mcp-test-{os.getpid()}.sock")
        server = SocketServerAdapter(path=path)
        assert server.socket_path == path
        assert server.connection_count == 0
        assert "SocketServerAdapter" in repr(server)
        assert path in repr(server)
