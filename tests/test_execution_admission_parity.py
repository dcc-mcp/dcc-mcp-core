"""Packaged Python handlers share explicit REST/MCP job-admission contracts."""

from __future__ import annotations

import json
import time
import urllib.error
import urllib.request

import pytest

from conftest import McpClient
from dcc_mcp_core import McpHttpConfig
from dcc_mcp_core import McpHttpServer
from dcc_mcp_core import ToolRegistry


@pytest.fixture
def admission_server():
    registry = ToolRegistry()
    for name, execution, budget in (
        ("sync_budget", "sync", 5),
        ("sync_plain", "sync", None),
        ("declared_async", "async", 5),
    ):
        registry.register(name, dcc="test", execution=execution, timeout_hint_secs=budget)
    calls = []

    def handler(arguments):
        calls.append(arguments)
        return {"marker": arguments["marker"]}

    server = McpHttpServer(registry, McpHttpConfig(port=0))
    for name in ("sync_budget", "sync_plain", "declared_async"):
        server.register_handler(name, handler, thread_affinity="any")
    with server.start() as handle:
        client = McpClient(handle.mcp_url(), auto_init=False)
        client.initialize(protocol_version="2025-06-18")
        yield handle.mcp_url().rsplit("/mcp", 1)[0], client, calls


def _mcp_call(client, name, arguments, request_id, meta=None):
    params = {"name": name, "arguments": arguments}
    if meta is not None:
        # Use the actual protocol field, not the old test helper's `meta` key.
        params["_meta"] = meta
    status, body = client.post({"jsonrpc": "2.0", "id": request_id, "method": "tools/call", "params": params})
    assert status == 200, body
    assert body["id"] == request_id, body
    assert "error" not in body, body
    assert body["result"].get("isError") is not True, body
    return body["result"]["structuredContent"]


def _rest_call(base, name, arguments, meta):
    request = {"tool_slug": f"test.core.{name}", "params": arguments}
    if meta is not None:
        request["meta"] = meta
    request = urllib.request.Request(
        base + "/v1/call",
        data=json.dumps(request).encode(),
        headers={"Content-Type": "application/json", "Accept": "application/json"},
        method="POST",
    )
    try:
        response = urllib.request.urlopen(request, timeout=5)
    except urllib.error.HTTPError as error:
        response = error
    with response:
        return response.status, json.loads(response.read())


def _terminal_output(client, output, pending, marker):
    if not pending:
        assert "job_id" not in output, output
        return output
    assert output.get("status") == "pending", output
    job_id = output["job_id"]
    assert isinstance(job_id, str) and job_id, output
    deadline = time.monotonic() + 5
    while time.monotonic() < deadline:
        job = _mcp_call(
            client,
            "jobs_get_status",
            {"job_id": job_id, "include_result": True},
            "poll-" + marker,
        )
        assert job["job_id"] == job_id, job
        if job["status"] == "completed":
            return job["result"]
        assert job["status"] in {"pending", "running"}, job
        time.sleep(0.01)
    pytest.fail(f"job {job_id} did not reach its terminal state")


@pytest.mark.parametrize("route", ["rest", "mcp"])
@pytest.mark.parametrize(
    ("name", "meta", "rest_pending", "mcp_pending"),
    [
        pytest.param("sync_budget", None, False, False, id="budget-only"),
        pytest.param("declared_async", None, True, True, id="declared-async"),
        pytest.param("sync_plain", {"dcc": {"async": True}}, True, True, id="explicit-async"),
        pytest.param("sync_plain", {"dcc": {"async": False}}, False, False, id="explicit-false"),
        pytest.param("sync_plain", {"progressToken": "token"}, True, True, id="progress-string"),
        pytest.param("sync_plain", {"progressToken": 0}, True, True, id="progress-zero"),
        pytest.param("sync_plain", {"progressToken": 0.5}, True, True, id="legacy-progress-fraction"),
        pytest.param("sync_plain", {"progressToken": None}, False, False, id="progress-null"),
        pytest.param("sync_plain", {"progressToken": False}, False, False, id="progress-bool"),
        pytest.param("sync_plain", {"progressToken": []}, False, False, id="progress-array"),
        pytest.param("sync_plain", {"progressToken": {}}, False, False, id="progress-object"),
        pytest.param("sync_plain", {"progress_token": "token"}, True, False, id="rest-only-alias"),
        pytest.param(
            "sync_plain",
            {"progressToken": None, "progress_token": "masked"},
            False,
            False,
            id="canonical-null-masks-rest-alias",
        ),
    ],
)
def test_packaged_routes_preserve_job_admission(admission_server, route, name, meta, rest_pending, mcp_pending):
    base, client, calls = admission_server
    marker = name + "-" + route
    arguments = {"marker": marker}
    pending = rest_pending if route == "rest" else mcp_pending
    if route == "rest":
        status, body = _rest_call(base, name, arguments, meta)
        assert status == (202 if pending else 200), body
        output = body["output"]
    else:
        output = _mcp_call(client, name, arguments, marker, meta)
    assert _terminal_output(client, output, pending, marker) == arguments
    assert len(calls) == 1, "the original handler must execute exactly once"
    assert calls[0]["marker"] == marker
    if pending:
        # Python dispatch injects job ownership into handler arguments.
        assert calls[0]["_meta"]["dcc"]["jobId"] == output["job_id"]
