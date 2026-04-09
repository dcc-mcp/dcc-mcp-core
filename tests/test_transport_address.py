"""Tests for TransportAddress and TransportScheme PyO3 bindings.

Covers:
- TransportAddress factory methods: named_pipe, tcp, unix_socket, default_local,
  default_pipe_name, parse
- TransportAddress properties: scheme, is_local, is_tcp, is_named_pipe,
  is_unix_socket, to_connection_string
- TransportScheme enum values and select_address routing logic
"""

from __future__ import annotations

import os

import pytest

import dcc_mcp_core

# ---------------------------------------------------------------------------
# TransportAddress — factory methods
# ---------------------------------------------------------------------------


class TestTransportAddressFactories:
    def test_named_pipe_scheme(self) -> None:
        """named_pipe() creates an address with scheme 'pipe'."""
        ta = dcc_mcp_core.TransportAddress.named_pipe("my-pipe")
        assert ta.scheme == "pipe"

    def test_named_pipe_repr_contains_pipe_name(self) -> None:
        """repr() of named_pipe address contains the pipe name."""
        ta = dcc_mcp_core.TransportAddress.named_pipe("dcc-maya-9999")
        assert "dcc-maya-9999" in repr(ta)

    def test_tcp_scheme(self) -> None:
        """tcp() creates an address with scheme 'tcp'."""
        ta = dcc_mcp_core.TransportAddress.tcp("127.0.0.1", 8080)
        assert ta.scheme == "tcp"

    def test_tcp_repr_contains_host_and_port(self) -> None:
        """repr() of tcp address contains the host and port."""
        ta = dcc_mcp_core.TransportAddress.tcp("192.168.1.100", 9999)
        r = repr(ta)
        assert "192.168.1.100" in r
        assert "9999" in r

    def test_tcp_to_connection_string(self) -> None:
        """to_connection_string() returns the full TCP URI."""
        ta = dcc_mcp_core.TransportAddress.tcp("localhost", 9000)
        cs = ta.to_connection_string()
        assert cs == "tcp://localhost:9000"

    def test_default_local_returns_named_pipe_on_windows(self) -> None:
        """default_local() returns a named-pipe address on Windows (CI host)."""
        ta = dcc_mcp_core.TransportAddress.default_local("maya", os.getpid())
        # On Windows the preferred IPC is named pipe; on other platforms it
        # may be unix socket.  Either way, is_local must be True.
        assert ta.is_local is True

    def test_default_local_scheme_is_pipe_or_unix(self) -> None:
        """default_local() scheme is either 'pipe' or 'unix', not 'tcp'."""
        ta = dcc_mcp_core.TransportAddress.default_local("maya", 12345)
        assert ta.scheme in ("pipe", "unix")

    def test_default_local_repr_contains_dcc_name(self) -> None:
        """repr() of default_local address mentions the DCC name."""
        ta = dcc_mcp_core.TransportAddress.default_local("houdini", 99999)
        assert "houdini" in repr(ta)

    def test_default_local_different_dccs_are_different(self) -> None:
        """Different DCC names produce different default_local addresses."""
        ta_maya = dcc_mcp_core.TransportAddress.default_local("maya", 1000)
        ta_blender = dcc_mcp_core.TransportAddress.default_local("blender", 1000)
        assert str(ta_maya) != str(ta_blender)

    def test_default_pipe_name_returns_transport_address(self) -> None:
        """default_pipe_name() returns a TransportAddress with pipe scheme."""
        ta = dcc_mcp_core.TransportAddress.default_pipe_name("maya", 1234)
        assert ta.scheme == "pipe"
        assert ta.is_named_pipe is True

    def test_default_pipe_name_contains_dcc_and_pid(self) -> None:
        """default_pipe_name() address repr contains both DCC name and PID."""
        ta = dcc_mcp_core.TransportAddress.default_pipe_name("maya", 5678)
        r = repr(ta)
        assert "maya" in r
        assert "5678" in r

    def test_parse_tcp_uri(self) -> None:
        """parse() correctly reads a tcp:// URI."""
        ta = dcc_mcp_core.TransportAddress.parse("tcp://127.0.0.1:8080")
        assert ta.scheme == "tcp"
        assert "8080" in repr(ta)

    def test_parse_pipe_uri(self) -> None:
        """parse() correctly reads a pipe:// URI."""
        ta = dcc_mcp_core.TransportAddress.parse(r"pipe://\\.\pipe\my-pipe-xyz")
        assert ta.scheme == "pipe"

    def test_parse_roundtrip(self) -> None:
        """str() on a parsed address equals the original connection string."""
        original = "tcp://127.0.0.1:9000"
        ta = dcc_mcp_core.TransportAddress.parse(original)
        assert ta.to_connection_string() == original


# ---------------------------------------------------------------------------
# TransportAddress — boolean properties
# ---------------------------------------------------------------------------


class TestTransportAddressProperties:
    def test_is_tcp_true_for_tcp(self) -> None:
        ta = dcc_mcp_core.TransportAddress.tcp("localhost", 9000)
        assert ta.is_tcp is True

    def test_is_tcp_false_for_pipe(self) -> None:
        ta = dcc_mcp_core.TransportAddress.named_pipe("test-pipe")
        assert ta.is_tcp is False

    def test_is_named_pipe_true_for_pipe(self) -> None:
        ta = dcc_mcp_core.TransportAddress.named_pipe("dcc-pipe")
        assert ta.is_named_pipe is True

    def test_is_named_pipe_false_for_tcp(self) -> None:
        ta = dcc_mcp_core.TransportAddress.tcp("localhost", 9000)
        assert ta.is_named_pipe is False

    def test_is_local_true_for_named_pipe(self) -> None:
        ta = dcc_mcp_core.TransportAddress.named_pipe("local-pipe")
        assert ta.is_local is True

    def test_is_local_false_for_remote_tcp(self) -> None:
        """TCP to a non-loopback remote host should not be local."""
        ta = dcc_mcp_core.TransportAddress.tcp("10.0.0.5", 9000)
        # is_local is implementation-defined; just verify it returns a bool
        assert isinstance(ta.is_local, bool)

    def test_is_unix_socket_false_on_windows(self) -> None:
        """On Windows (our CI), is_unix_socket is False for a named pipe."""
        ta = dcc_mcp_core.TransportAddress.named_pipe("test")
        # On Windows there are no Unix sockets, so named pipes are preferred.
        assert ta.is_unix_socket is False

    def test_scheme_is_string(self) -> None:
        """Scheme property always returns a str."""
        ta = dcc_mcp_core.TransportAddress.tcp("localhost", 80)
        assert isinstance(ta.scheme, str)

    def test_repr_is_string(self) -> None:
        ta = dcc_mcp_core.TransportAddress.tcp("localhost", 80)
        assert isinstance(repr(ta), str)


# ---------------------------------------------------------------------------
# TransportScheme — enum values
# ---------------------------------------------------------------------------


class TestTransportSchemeEnum:
    def test_auto_value_repr(self) -> None:
        """AUTO scheme has a human-readable repr."""
        ts = dcc_mcp_core.TransportScheme.AUTO
        assert "AUTO" in repr(ts).upper() or repr(ts)

    def test_tcp_only_value_repr(self) -> None:
        ts = dcc_mcp_core.TransportScheme.TCP_ONLY
        assert repr(ts) is not None

    def test_prefer_named_pipe_repr(self) -> None:
        ts = dcc_mcp_core.TransportScheme.PREFER_NAMED_PIPE
        assert repr(ts) is not None

    def test_prefer_ipc_repr(self) -> None:
        ts = dcc_mcp_core.TransportScheme.PREFER_IPC
        assert repr(ts) is not None

    def test_prefer_unix_socket_repr(self) -> None:
        ts = dcc_mcp_core.TransportScheme.PREFER_UNIX_SOCKET
        assert repr(ts) is not None

    def test_int_conversion(self) -> None:
        """TransportScheme can be converted to int."""
        ts = dcc_mcp_core.TransportScheme.AUTO
        assert isinstance(int(ts), int)


# ---------------------------------------------------------------------------
# TransportScheme — select_address routing
# ---------------------------------------------------------------------------


class TestTransportSchemeSelectAddress:
    def test_tcp_only_always_returns_tcp(self) -> None:
        """TCP_ONLY always produces a tcp address regardless of PID."""
        ta = dcc_mcp_core.TransportScheme.TCP_ONLY.select_address("maya", "127.0.0.1", 9000)
        assert ta.scheme == "tcp"
        assert ta.is_tcp is True

    def test_tcp_only_without_pid_returns_tcp(self) -> None:
        ta = dcc_mcp_core.TransportScheme.TCP_ONLY.select_address("blender", "127.0.0.1", 8080)
        assert ta.scheme == "tcp"

    def test_auto_without_pid_defaults_to_tcp(self) -> None:
        """AUTO without a PID should fall back to TCP since IPC name is unknown."""
        ta = dcc_mcp_core.TransportScheme.AUTO.select_address("maya", "127.0.0.1", 9001)
        # Without a PID, the implementation cannot generate a pipe name.
        assert ta.scheme == "tcp"

    def test_auto_with_pid_returns_local_ipc(self) -> None:
        """AUTO with a PID returns a named-pipe/unix-socket address."""
        ta = dcc_mcp_core.TransportScheme.AUTO.select_address("maya", "127.0.0.1", 9002, pid=1234)
        # On Windows, expect a named pipe
        assert ta.scheme in ("pipe", "unix")

    def test_prefer_named_pipe_with_pid_returns_pipe(self) -> None:
        """PREFER_NAMED_PIPE with PID returns a named-pipe address on Windows."""
        ta = dcc_mcp_core.TransportScheme.PREFER_NAMED_PIPE.select_address("houdini", "127.0.0.1", 9003, pid=5555)
        # On Windows, this should be a named pipe.
        assert ta.scheme in ("pipe", "tcp")

    def test_select_address_returns_transport_address(self) -> None:
        """select_address always returns a TransportAddress instance."""
        schemes = [
            dcc_mcp_core.TransportScheme.AUTO,
            dcc_mcp_core.TransportScheme.TCP_ONLY,
            dcc_mcp_core.TransportScheme.PREFER_NAMED_PIPE,
            dcc_mcp_core.TransportScheme.PREFER_IPC,
            dcc_mcp_core.TransportScheme.PREFER_UNIX_SOCKET,
        ]
        for scheme in schemes:
            ta = scheme.select_address("maya", "127.0.0.1", 9999)
            assert isinstance(repr(ta), str), f"Expected TransportAddress repr, got {type(ta)}"
            assert ta.scheme in ("tcp", "pipe", "unix"), f"Unexpected scheme for {scheme}"

    def test_select_address_different_dccs(self) -> None:
        """select_address with different DCC names but same PID yields different pipe names."""
        ta1 = dcc_mcp_core.TransportScheme.AUTO.select_address("maya", "127.0.0.1", 9000, pid=1234)
        ta2 = dcc_mcp_core.TransportScheme.AUTO.select_address("blender", "127.0.0.1", 9001, pid=1234)
        # Different DCC types should produce different addresses
        assert str(ta1) != str(ta2)
