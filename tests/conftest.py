"""Shared fixtures for dcc-mcp-core tests."""

# Import future modules
from __future__ import annotations

# Import built-in modules
import contextlib
import json
import os
from pathlib import Path
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


# ── Gateway endpoint fixture ────────────────────────────────────────────


@pytest.fixture(scope="module")
def gateway_endpoint(tmp_path_factory):
    """Start a standalone gateway server and return its ``/mcp`` URL.

    This fixture launches a single ``McpHttpServer`` with ``gateway_port`` set
    so that it wins the election and serves the aggregating-facade ``/mcp``
    endpoint via axum native handlers (NOT ``rmcp::StreamableHttpService``).

    The returned URL is used by ``test_stateless_protocol.py`` to validate
    ADR-010 Phase 1a header-based routing (``MCP-Protocol-Version:
    2026-07-28`` → 501 Not Implemented).
    """
    import socket
    import time
    from dcc_mcp_core import McpHttpConfig
    from dcc_mcp_core import McpHttpServer
    from dcc_mcp_core import ToolRegistry

    registry_dir = tmp_path_factory.mktemp("gw-stateless-registry")

    # Pick a free port for the gateway
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        s.bind(("127.0.0.1", 0))
        gw_port = s.getsockname()[1]

    reg = ToolRegistry()
    reg.register(
        name="ping",
        description="Health-check tool for gateway fixture",
        dcc="gateway",
        version="1.0.0",
    )

    cfg = McpHttpConfig(port=0, server_name="gw-stateless-test")
    cfg.gateway_port = gw_port
    cfg.registry_dir = str(registry_dir)
    cfg.dcc_type = "gateway"
    cfg.heartbeat_secs = 1
    cfg.stale_timeout_secs = 10

    server = McpHttpServer(reg, cfg)
    handle = server.start()

    endpoint = f"http://127.0.0.1:{gw_port}/mcp"

    # Wait for gateway to be ready
    deadline = time.monotonic() + 10
    while True:
        try:
            req = urllib.request.Request(
                endpoint,
                data=json.dumps({
                    "jsonrpc": "2.0",
                    "id": 1,
                    "method": "initialize",
                    "params": {
                        "protocolVersion": "2025-06-18",
                        "capabilities": {},
                        "clientInfo": {"name": "pytest", "version": "1.0"},
                    },
                }).encode("utf-8"),
                headers={"Content-Type": "application/json"},
                method="POST",
            )
            with urllib.request.urlopen(req, timeout=3) as resp:
                if resp.status == 200:
                    break
        except Exception:
            pass
        if time.monotonic() > deadline:
            handle.shutdown()
            raise RuntimeError("Gateway did not become ready within 10 seconds")
        time.sleep(0.5)

    yield endpoint

    with contextlib.suppress(Exception):
        handle.shutdown()
