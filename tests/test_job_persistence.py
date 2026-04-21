"""End-to-end test for the optional SQLite JobStorage backend (issue #328).

Verifies the full restart-recovery contract from Python:

1. Dispatch an async tool while the server has ``job_storage_path`` set
   to a fresh SQLite file — SQLite is created with the expected schema
   and the job row is persisted.
2. Stop the server before the async job completes (by starting a second
   server against the same file before the first finishes).
3. On the second incarnation, any row left in ``pending`` / ``running``
   is visible as ``interrupted`` via ``jobs.get_status``.

The test is skipped at module level when the wheel was not built with
the ``job-persist-sqlite`` Cargo feature — detected by trying to set
``job_storage_path`` on a config and asserting the server accepts it.
"""

from __future__ import annotations

import json
from pathlib import Path
import sqlite3
import tempfile
import time
from typing import Any
import urllib.request

import pytest

from dcc_mcp_core import McpHttpConfig
from dcc_mcp_core import McpHttpServer
from dcc_mcp_core import ToolRegistry


def _feature_enabled() -> bool:
    """Return ``True`` when the wheel was built with job-persist-sqlite.

    The property is always exposed (for stable stub shape) but calling
    ``server.start()`` with a path raises when the feature is absent.
    So we use the roundtrip to tell: construct a config, start a
    minimal server and observe the outcome.
    """
    cfg = McpHttpConfig(port=0, server_name="sqlite-probe")
    # On Windows the SQLite WAL/SHM side-files keep the directory
    # locked for a brief moment after shutdown, so we use
    # ``ignore_cleanup_errors=True`` to avoid a spurious collection
    # failure here.
    with tempfile.TemporaryDirectory(ignore_cleanup_errors=True) as d:
        cfg.job_storage_path = str(Path(d) / "probe.sqlite3")
        reg = ToolRegistry()
        server = McpHttpServer(reg, cfg)
        try:
            handle = server.start()
        except Exception as exc:  # pragma: no cover - pure env detection
            msg = str(exc).lower()
            if "job-persist-sqlite" in msg or "feature" in msg:
                return False
            raise
        try:
            return True
        finally:
            handle.shutdown()


_FEATURE = _feature_enabled()
pytestmark = pytest.mark.skipif(
    not _FEATURE,
    reason="dcc-mcp-core wheel was built without the job-persist-sqlite feature",
)


def _post(url: str, body: dict[str, Any], sid: str | None = None) -> dict[str, Any]:
    headers = {"Content-Type": "application/json", "Accept": "application/json"}
    if sid is not None:
        headers["Mcp-Session-Id"] = sid
    req = urllib.request.Request(
        url,
        data=json.dumps(body).encode(),
        headers=headers,
        method="POST",
    )
    with urllib.request.urlopen(req, timeout=5) as resp:
        return json.loads(resp.read())


def _initialize_session(url: str) -> str:
    body = _post(
        url,
        {
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-03-26",
                "capabilities": {},
                "clientInfo": {"name": "pytest-328", "version": "1.0"},
            },
        },
    )
    return body["result"]["__session_id"]


def _make_server(db_path: str, handler):
    reg = ToolRegistry()
    reg.register(
        "slow_echo",
        description="Slow echo — sleeps so the async job stays 'running' long enough to be interrupted.",
        category="test",
        tags=[],
        dcc="test",
        version="1.0.0",
    )
    cfg = McpHttpConfig(port=0, server_name="jobs-persist-test")
    cfg.enable_job_notifications = True
    cfg.job_storage_path = db_path
    server = McpHttpServer(reg, cfg)
    server.register_handler("slow_echo", handler)
    handle = server.start()
    return server, handle, handle.mcp_url()


def test_sqlite_storage_persists_rows_across_restart():
    with tempfile.TemporaryDirectory(ignore_cleanup_errors=True) as d:
        db = str(Path(d) / "jobs.sqlite3")

        # First incarnation — dispatch an async tool; shut the server
        # down before the handler returns so the row stays "running".
        def slow_handler(params):
            time.sleep(2.0)
            return {"echoed": params, "ok": True}

        job_id: str
        _server, handle, url = _make_server(db, slow_handler)
        try:
            sid = _initialize_session(url)
            dispatch = _post(
                url,
                {
                    "jsonrpc": "2.0",
                    "id": 2,
                    "method": "tools/call",
                    "params": {
                        "name": "slow_echo",
                        "arguments": {"hello": "world"},
                        "_meta": {"dcc": {"async": True}},
                    },
                },
                sid=sid,
            )
            assert dispatch["result"]["isError"] is False, dispatch
            sc = dispatch["result"].get("structuredContent") or json.loads(dispatch["result"]["content"][0]["text"])
            job_id = sc["job_id"]
            assert isinstance(job_id, str) and job_id
            # Give the dispatcher a moment to flip Pending → Running
            # before we tear the server down.
            time.sleep(0.1)
        finally:
            handle.shutdown()

        # SQLite file must exist and contain the `jobs` table.
        assert Path(db).exists(), "SQLite database file should have been created"
        with sqlite3.connect(db) as conn:
            rows = conn.execute(
                "SELECT job_id, tool, status FROM jobs WHERE job_id = ?",
                (job_id,),
            ).fetchall()
        assert len(rows) == 1, f"expected one persisted row, got {rows}"
        assert rows[0][1] == "slow_echo"
        # The status will be either 'pending' or 'running' — either is
        # recoverable. Terminal values would mean the handler completed
        # before we tore down, which is a timing failure.
        assert rows[0][2] in {"pending", "running"}, rows

        # Second incarnation against the SAME database file.
        _server, handle, url = _make_server(db, slow_handler)
        try:
            sid = _initialize_session(url)
            status = _post(
                url,
                {
                    "jsonrpc": "2.0",
                    "id": 3,
                    "method": "tools/call",
                    "params": {
                        "name": "jobs.get_status",
                        "arguments": {"job_id": job_id},
                    },
                },
                sid=sid,
            )
            assert status["result"]["isError"] is False, status
            env = status["result"]["structuredContent"]
            assert env["job_id"] == job_id
            assert env["status"] == "interrupted", env
            assert env["error"] == "server restart", env
        finally:
            handle.shutdown()


def test_jobs_cleanup_tool_roundtrip_with_sqlite_storage():
    with tempfile.TemporaryDirectory(ignore_cleanup_errors=True) as d:
        db = str(Path(d) / "jobs.sqlite3")

        def fast_handler(params):
            return {"echoed": params}

        _server, handle, url = _make_server(db, fast_handler)
        try:
            sid = _initialize_session(url)
            # Complete a job so there's something terminal to prune.
            dispatch = _post(
                url,
                {
                    "jsonrpc": "2.0",
                    "id": 4,
                    "method": "tools/call",
                    "params": {
                        "name": "slow_echo",
                        "arguments": {"x": 1},
                        "_meta": {"dcc": {"async": True}},
                    },
                },
                sid=sid,
            )
            sc = dispatch["result"].get("structuredContent") or json.loads(dispatch["result"]["content"][0]["text"])
            job_id = sc["job_id"]

            # Poll until terminal (or bail).
            deadline = time.monotonic() + 3.0
            while time.monotonic() < deadline:
                status = _post(
                    url,
                    {
                        "jsonrpc": "2.0",
                        "id": 5,
                        "method": "tools/call",
                        "params": {
                            "name": "jobs.get_status",
                            "arguments": {"job_id": job_id},
                        },
                    },
                    sid=sid,
                )
                env = status["result"]["structuredContent"]
                if env["status"] in {"completed", "failed", "cancelled", "interrupted"}:
                    break
                time.sleep(0.05)

            # older_than_hours=0 means "prune everything terminal that exists".
            cleanup = _post(
                url,
                {
                    "jsonrpc": "2.0",
                    "id": 6,
                    "method": "tools/call",
                    "params": {
                        "name": "jobs.cleanup",
                        "arguments": {"older_than_hours": 0},
                    },
                },
                sid=sid,
            )
            assert cleanup["result"]["isError"] is False, cleanup
            env = cleanup["result"]["structuredContent"]
            assert env["removed"] >= 1, env

            # jobs.get_status for the pruned id now reports unknown.
            follow = _post(
                url,
                {
                    "jsonrpc": "2.0",
                    "id": 7,
                    "method": "tools/call",
                    "params": {
                        "name": "jobs.get_status",
                        "arguments": {"job_id": job_id},
                    },
                },
                sid=sid,
            )
            assert follow["result"]["isError"] is True
        finally:
            handle.shutdown()
