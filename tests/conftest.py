"""Shared fixtures for dcc-mcp-core tests.

Auto-use fixtures in this file provide:
- Session-scoped registry directory isolation (``DCC_MCP_REGISTRY_DIR``)
- Env-var restore guard that snapshots env before the session and restores
  on teardown, preventing env-var-based test-order dependency.
- Global server/shutdown tracking (``register_shutdown_handle`` +
  ``pytest_runtest_teardown`` cleanup).
"""

# Import future modules
from __future__ import annotations

import contextlib
import json
import os
from pathlib import Path
import socket as _socket
import time
import typing
from typing import Any
import urllib.error
import urllib.request

# Import third-party modules
import pytest

# Import local modules
import dcc_mcp_core

# Resolve examples/skills relative to repo root
REPO_ROOT = Path(__file__).resolve().parent.parent
EXAMPLES_SKILLS_DIR = str(REPO_ROOT / "examples" / "skills")
SKILLS_DIR = str(REPO_ROOT / "skills")

#: Environment variable read by the Rust GatewayRunner / McpHttpConfig to
#: override the default shared registry directory (issue #793).
_DCC_MCP_REGISTRY_ENV = "DCC_MCP_REGISTRY_DIR"


@pytest.fixture(scope="session", autouse=True)
def _isolated_registry_dir(tmp_path_factory: pytest.TempPathFactory):
    """Redirect the default registry directory to a session-scoped temp dir.

    Many test modules start ``McpHttpServer`` / ``create_skill_server``
    instances without an explicit ``registry_dir``.  When they do, the Rust
    runtime falls back to ``<os-tmp>/dcc-mcp-registry`` — a **shared**
    directory that persists between test runs and accumulates stale
    ``services.json`` entries (issue #793).

    Setting ``DCC_MCP_REGISTRY_DIR`` to a fresh temporary directory ensures
    every server created during the session writes its registry entries in an
    isolated location that pytest cleans up automatically at session teardown.

    Tests that need a *distinct* registry (e.g. multi-service gateway tests)
    should create their own sub-directory via ``tmp_path_factory.mktemp(...)``
    and assign it to ``cfg.registry_dir`` explicitly — that explicit
    assignment takes precedence and is unaffected by this fixture.
    """
    registry_dir = tmp_path_factory.mktemp("session-registry", numbered=True)
    previous = os.environ.get(_DCC_MCP_REGISTRY_ENV)
    os.environ[_DCC_MCP_REGISTRY_ENV] = str(registry_dir)
    yield
    # Restore (or remove) the env-var so we don't leak state into other
    # processes that might be forked after the session ends.
    if previous is None:
        os.environ.pop(_DCC_MCP_REGISTRY_ENV, None)
    else:
        os.environ[_DCC_MCP_REGISTRY_ENV] = previous


def create_skill_dir(
    base_dir: str,
    name: str,
    frontmatter: str = "",
    *,
    dcc: str = "",
    body: str = "",
) -> str:
    """Create a temporary skill directory with a SKILL.md file.

    Auto-generated frontmatter emits the agentskills.io 1.0-compliant
    nested ``metadata.dcc-mcp.*`` form (issue #356). Top-level keys
    outside the spec allowlist are rejected by ``parse_skill_md``.

    Args:
        base_dir: Parent directory to create the skill under.
        name: Skill directory name.
        frontmatter: Raw YAML frontmatter (excluding delimiters). If empty,
            a minimal ``name: <name>`` block is generated.
        dcc: Optional DCC field placed under ``metadata.dcc-mcp.dcc``.
        body: Optional body text after the frontmatter.

    Returns:
        Path to the created skill directory.

    """
    skill_path = Path(base_dir) / name
    skill_path.mkdir(parents=True, exist_ok=True)
    if not frontmatter:
        lines = [f"name: {name}"]
        if dcc:
            lines.extend(["metadata:", "  dcc-mcp:", f"    dcc: {dcc}"])
        frontmatter = "\n".join(lines)
    content = f"---\n{frontmatter}\n---\n{body}"
    (skill_path / "SKILL.md").write_text(content, encoding="utf-8")
    return str(skill_path)


def scan_and_find(
    examples_dir: str,
    skill_name: str,
) -> dcc_mcp_core.SkillMetadata:
    """Scan examples_dir and return parsed SkillMetadata for *skill_name*.

    Raises:
        StopIteration: If the skill is not found.
        AssertionError: If parsing returns None.

    """
    scanner = dcc_mcp_core.SkillScanner()
    dirs = scanner.scan(extra_paths=[examples_dir])
    skill_dir = next(d for d in dirs if Path(d).name == skill_name)
    meta = dcc_mcp_core.parse_skill_md(skill_dir)
    assert meta is not None, f"parse_skill_md returned None for {skill_name}"
    return meta


@pytest.fixture()
def examples_dir() -> str:
    """Return the path to the examples/skills directory, skipping if absent."""
    if not Path(EXAMPLES_SKILLS_DIR).is_dir():
        pytest.skip("examples/skills directory not found")
    return EXAMPLES_SKILLS_DIR


@pytest.fixture()
def skills_dir() -> str:
    """Return the path to the top-level skills/ directory, skipping if absent."""
    if not Path(SKILLS_DIR).is_dir():
        pytest.skip("skills directory not found")
    return SKILLS_DIR


@pytest.fixture()
def scanned_metas(examples_dir: str) -> list[dcc_mcp_core.SkillMetadata]:
    """Scan all example skills and return a list of parsed SkillMetadata objects.

    Useful for tests in TestScanAndParseRoundTrip that iterate over all skills.
    """
    scanner = dcc_mcp_core.SkillScanner()
    dirs = scanner.scan(extra_paths=[examples_dir])
    metas = []
    for d in dirs:
        meta = dcc_mcp_core.parse_skill_md(d)
        assert meta is not None, f"Failed to parse {d}"
        metas.append(meta)
    return metas


# ── MCP Streamable HTTP client helper ────────────────────────────────────


class McpClient:
    """Minimal MCP Streamable HTTP client for test use.

    Handles the initialize handshake and session management automatically.
    All requests after initialization carry the Mcp-Session-Id header.
    """

    _HEADERS: typing.ClassVar[dict[str, str]] = {
        "Content-Type": "application/json",
        "Accept": "application/json, text/event-stream",
    }

    def __init__(self, url: str, *, auto_init: bool = True):
        self.url = url
        self.session_id: str | None = None
        self.protocol_version: str = "2025-11-25"
        if auto_init:
            self.initialize()

    def initialize(
        self,
        protocol_version: str = "2025-11-25",
        client_name: str = "pytest",
    ) -> dict[str, Any]:
        """Perform the MCP initialize handshake and store the session ID."""
        self.protocol_version = protocol_version
        body = {
            "jsonrpc": "2.0",
            "id": "__init__",
            "method": "initialize",
            "params": {
                "protocolVersion": protocol_version,
                "capabilities": {},
                "clientInfo": {"name": client_name, "version": "1.0"},
            },
        }
        code, resp, headers = self._raw_post(body)
        if code != 200:
            raise RuntimeError(f"initialize failed: HTTP {code}")
        # Extract session ID from response header (stateful mode)
        if headers and headers.get("Mcp-Session-Id"):
            self.session_id = headers["Mcp-Session-Id"]
        # Unwrap JSON-RPC response envelope
        if isinstance(resp, dict) and "result" in resp:
            return resp["result"]
        return resp

    def post(
        self,
        body: dict[str, Any],
        *,
        extra_headers: dict[str, str] | None = None,
    ) -> tuple[int, dict[str, Any]]:
        """Send a JSON-RPC request with session management."""
        code, resp, _ = self._raw_post(body, extra_headers=extra_headers)
        return code, resp

    def post_raw(
        self,
        data: bytes,
        *,
        extra_headers: dict[str, str] | None = None,
    ) -> tuple[int, str]:
        """Send raw bytes and return (status_code, response_text)."""
        headers = dict(self._HEADERS)
        if self.session_id:
            headers["Mcp-Session-Id"] = self.session_id
        if self.protocol_version:
            headers["MCP-Protocol-Version"] = self.protocol_version
        if extra_headers:
            headers.update(extra_headers)
        req = urllib.request.Request(self.url, data=data, headers=headers, method="POST")
        try:
            with urllib.request.urlopen(req, timeout=10) as resp:
                return resp.status, resp.read().decode()
        except urllib.error.HTTPError as e:
            return e.code, e.read().decode()

    def _raw_post(
        self,
        body: dict[str, Any],
        *,
        extra_headers: dict[str, str] | None = None,
    ) -> tuple[int, dict[str, Any], dict[str, str] | None]:
        data = json.dumps(body).encode()
        headers = dict(self._HEADERS)
        if self.session_id:
            headers["Mcp-Session-Id"] = self.session_id
        if self.protocol_version:
            headers["MCP-Protocol-Version"] = self.protocol_version
        if extra_headers:
            headers.update(extra_headers)
        req = urllib.request.Request(self.url, data=data, headers=headers, method="POST")
        try:
            with urllib.request.urlopen(req, timeout=10) as resp:
                resp_headers = {k: v for k, v in resp.getheaders()}
                return resp.status, json.loads(resp.read()), resp_headers
        except urllib.error.HTTPError as e:
            return e.code, {}, None


# ── Shared port allocation & TCP reachability helpers ──────────────────
# Every gateway/e2e test module needs these; putting them in conftest.py
# avoids per-file redefinition and allows a single retry fix to benefit
# all callers.
#
# The retry-based ``allocate_gateway_port`` mitigates the TIME_WAIT race
# inherent in the ``bind→close→rebind`` pattern used by earlier per-file
# ``_pick_free_port()`` helpers.


def allocate_gateway_port() -> int:
    """Return a free TCP port on 127.0.0.1 with retry.

    Retries mitigate the case where a concurrently-released port is
    still in ``TIME_WAIT`` (common on macOS). However, the definitive
    fix for inter-module port conflicts is file-level process isolation
    via ``pytest-xdist --dist loadfile`` (see ``just test-suite``).
    """
    last_err: OSError | None = None
    for _ in range(10):
        try:
            with _socket.socket(_socket.AF_INET, _socket.SOCK_STREAM) as s:
                s.bind(("127.0.0.1", 0))
                return s.getsockname()[1]
        except OSError as exc:
            last_err = exc
            with contextlib.suppress(Exception):
                time.sleep(0.1)
    raise RuntimeError("could not allocate a free TCP port after 10 retries") from last_err


def wait_tcp_reachable(host: str, port: int, budget: float = 3.0) -> bool:
    """Poll until a TCP connect to ``(host, port)`` succeeds or budget expires."""
    deadline = time.time() + budget
    while time.time() < deadline:
        try:
            with _socket.create_connection((host, port), timeout=0.3):
                return True
        except OSError:
            time.sleep(0.05)
    return False


def wait_tcp_unreachable(host: str, port: int, budget: float = 2.0) -> None:
    """Poll until the endpoint becomes unreachable or budget expires.

    Raises ``AssertionError`` if the endpoint is still reachable after
    the budget.
    """
    deadline = time.time() + budget
    while time.time() < deadline:
        try:
            with _socket.create_connection((host, port), timeout=0.2):
                time.sleep(0.05)
        except OSError:
            return
    raise AssertionError(f"endpoint {host}:{port} still reachable after {budget}s")


# ── Session-scoped env-var snapshot & restore ─────────────────────────
# Prevents env-var-based test-order dependency: every test module sees
# the same baseline, and any env-var mutations during a test module are
# reverted at session teardown (belt-and-suspenders on top of the lower-
# level monkeypatch each fixture ought to use).


@pytest.fixture(scope="session", autouse=True)
def _env_snapshot():
    """Snapshot ``os.environ`` before the first test and restore on teardown.

    Every module-scoped fixture that sets ``DCC_MCP_*`` env vars
    (e.g. ``_isolated_registry_dir``, ``gateway_with_skill_backend``)
    mutates the process-global environment.  Without this guard, a
    module that sets env var X and then fails to tear down cleanly
    poisons the environment for every subsequent module.
    """
    snapshot = os.environ.copy()
    yield
    # Restore any env var that was added, changed, or removed.
    for key in set(os.environ) | set(snapshot):
        old = snapshot.get(key)
        cur = os.environ.get(key)
        if cur != old:
            if old is None:
                os.environ.pop(key, None)
            else:
                os.environ[key] = old


# ── Global server handle registry & teardown cleanup ─────────────────
# If a test crashes before its ``finally`` block, the server process
# stays bound to its port. ``allocate_gateway_port`` handles the retry,
# but the orphaned process leaks resources and keeps the registry dirty.
# This registry provides a safety net: any handle registered here is
# force-shutdown by ``pytest_runtest_teardown`` after every test.
# Over time, migrations should push registration into automatic hooks.

_SHUTDOWN_HANDLES: list[Any] = []


def register_shutdown_handle(handle: Any) -> None:
    """Register a server handle for automatic teardown cleanup.

    Call in a test's setup (or the first line after ``server.start()``)
    to ensure the server is shut down even if the test body raises an
    unhandled exception before the ``finally`` block.
    """
    _SHUTDOWN_HANDLES.append(handle)


def _force_shutdown_handles() -> None:
    """Shut down every handle in the registry and clear the list."""
    while _SHUTDOWN_HANDLES:
        handle = _SHUTDOWN_HANDLES.pop()
        if hasattr(handle, "shutdown"):
            with contextlib.suppress(Exception):
                handle.shutdown()


# NOTE: pytest_runtest_teardown is intentionally omitted.
# With xdist --dist loadfile, each test file runs in its own worker process,
# so any leaked server handles are cleaned up by process exit.
# A per-test teardown hook that calls _force_shutdown_handles() would
# prematurely kill module-scoped fixture servers after the first test
# in the module completes, causing Connection refused errors in subsequent
# tests.
