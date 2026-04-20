"""E2E regression test: gateway SSE → load_skill → tools/list_changed.

This test replaces the coverage gap left by the removal of
``tests/test_e2e_gateway_skills_progressive.py`` in the
``fix(http,transport)!: gateway lifecycle`` commit. It does **not** re-add
every scenario of the deleted file — it targets the single invariant that
was actually at risk from the #303 fix:

    When a client subscribes to the gateway's SSE stream (``GET /mcp``)
    and a backend's ``load_skill`` tool is invoked via the gateway's
    ``POST /mcp``, the client MUST receive a
    ``notifications/tools/list_changed`` push within a small deadline,
    and a subsequent ``tools/list`` MUST surface the newly registered
    tool.

The flow covered end-to-end:

1. A backend ``McpHttpServer`` is built via ``create_skill_server`` with
   ``DCC_MCP_SKILL_PATHS`` pointing at ``examples/skills`` — so
   ``hello-world`` is discovered as a skill stub (``__skill__hello-world``)
   but its tool (``hello_world__greet``) is **not** active.
2. The backend joins a gateway (first-wins election). The gateway's
   aggregating tools watcher polls backends every 3 s and broadcasts
   ``tools/list_changed`` when the aggregated fingerprint changes.
3. A background thread opens ``GET /mcp`` on the gateway with
   ``Accept: text/event-stream`` and captures every event.
4. We ``POST /mcp`` with ``tools/call load_skill {"name": "hello-world"}``
   routed through the gateway to the backend.
5. We assert the SSE stream received a ``notifications/tools/list_changed``
   and that ``tools/list`` now includes the loaded skill's tool.

Why this is the right regression guard:

- The #303 fix makes the gateway *reachable* — if the fix regresses, this
  test's SSE subscription will fail to connect at step 3.
- The aggregating watcher in ``gateway/mod.rs`` (``tools_watcher_handle``)
  is the only code path that emits ``tools/list_changed`` on the gateway
  facade — if *it* regresses, step 5's SSE assertion will time out even
  though a direct backend call succeeds.

Runtime budget: the watcher ticks every 3 s, so we allow up to 10 s for
the notification to arrive.
"""

from __future__ import annotations

import contextlib
import json
import os
from pathlib import Path
import queue
import socket
import threading
import time
from typing import Any
import urllib.error
import urllib.request

import pytest

from dcc_mcp_core import McpHttpConfig
from dcc_mcp_core import McpHttpServer
from dcc_mcp_core import ToolRegistry
from dcc_mcp_core import create_skill_server

REPO_ROOT = Path(__file__).resolve().parent.parent
EXAMPLES_SKILLS_DIR = str(REPO_ROOT / "examples" / "skills")

# The aggregating tools watcher in gateway/mod.rs ticks every 3 s; give
# ourselves some extra slack for the self-probe, registry propagation,
# and Windows scheduling jitter under CI.
SSE_NOTIFICATION_BUDGET_S = 12.0
AGGREGATOR_TICK_S = 3.0


# ── helpers ──────────────────────────────────────────────────────────────────


def _pick_free_port() -> int:
    """Return a TCP port that is currently free on 127.0.0.1."""
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def _post_mcp(url: str, method: str, params: dict | None = None, rpc_id: int = 1, timeout: float = 10.0) -> dict:
    """POST a JSON-RPC 2.0 request to an MCP endpoint and return the parsed body."""
    body: dict[str, Any] = {"jsonrpc": "2.0", "id": rpc_id, "method": method}
    if params is not None:
        body["params"] = params
    data = json.dumps(body).encode()
    req = urllib.request.Request(
        url,
        data=data,
        headers={"Content-Type": "application/json", "Accept": "application/json"},
        method="POST",
    )
    with urllib.request.urlopen(req, timeout=timeout) as resp:
        return json.loads(resp.read())


def _wait_tcp_reachable(host: str, port: int, budget: float = 3.0) -> bool:
    """Poll until TCP connect succeeds or budget expires."""
    deadline = time.time() + budget
    while time.time() < deadline:
        try:
            with socket.create_connection((host, port), timeout=0.3):
                return True
        except OSError:
            time.sleep(0.05)
    return False


class _SseSubscriber(threading.Thread):
    """Background SSE consumer that parses ``data: ...`` lines into JSON events.

    Runs until ``stop_event`` is set or the server closes the connection.
    Every JSON-RPC notification that arrives on the stream is pushed into
    ``events`` (a thread-safe queue). Test code then pops events with a
    short timeout to check whether a specific notification was received.
    """

    def __init__(self, url: str, events: queue.Queue[dict], stop_event: threading.Event) -> None:
        super().__init__(daemon=True)
        self.url = url
        self.events = events
        self.stop_event = stop_event
        self.error: BaseException | None = None
        self.connected = threading.Event()

    def run(self) -> None:
        try:
            req = urllib.request.Request(
                self.url,
                headers={"Accept": "text/event-stream", "Cache-Control": "no-cache"},
                method="GET",
            )
            # Short connect timeout so a broken gateway fails fast; the
            # stream itself will then be kept open by the server.
            with urllib.request.urlopen(req, timeout=5.0) as resp:
                self.connected.set()
                # Read line-by-line; SSE frames are separated by blank
                # lines, and each event has one or more ``data:`` lines.
                pending_data: list[str] = []
                while not self.stop_event.is_set():
                    line = resp.readline()
                    if not line:
                        break
                    try:
                        text = line.decode("utf-8", errors="replace").rstrip("\r\n")
                    except Exception:
                        continue
                    if text == "":
                        # End of event — flush pending data lines.
                        if pending_data:
                            payload = "\n".join(pending_data)
                            pending_data = []
                            # Non-JSON SSE frames (e.g. comments) are ignored;
                            # only JSON-RPC notifications matter for this test.
                            with contextlib.suppress(json.JSONDecodeError):
                                self.events.put(json.loads(payload))
                        continue
                    if text.startswith(":"):
                        # SSE comment/heartbeat line — ignored.
                        continue
                    if text.startswith("data:"):
                        pending_data.append(text[5:].lstrip())
                    # Other prefixes (``event:``, ``id:``, ``retry:``) are
                    # not significant for this test.
        except BaseException as e:
            self.error = e
            self.connected.set()  # unblock waiters on failure


def _drain_for_notification(events: queue.Queue[dict], method: str, budget: float) -> dict | None:
    """Block up to ``budget`` seconds waiting for a notification with the given method."""
    deadline = time.time() + budget
    while time.time() < deadline:
        remaining = max(0.0, deadline - time.time())
        try:
            ev = events.get(timeout=min(remaining, 0.5))
        except queue.Empty:
            continue
        if isinstance(ev, dict) and ev.get("method") == method:
            return ev
    return None


# ── fixture ───────────────────────────────────────────────────────────────────


@pytest.fixture()
def gateway_with_skill_backend(tmp_path, monkeypatch):
    """Start one backend (with examples/skills) behind a gateway, yield URLs + handles.

    The backend is built with ``create_skill_server`` so it carries the
    full Skills-First stack (SkillCatalog, load_skill / unload_skill /
    search_skills tools). The backend wins the gateway election because
    it is the only server registered against the gateway port.
    """
    registry_dir = tmp_path / "registry"
    registry_dir.mkdir()

    # Point the skill scanner at our examples dir.  monkeypatch.setenv
    # guarantees the env var is reset even if the test aborts mid-way.
    monkeypatch.setenv("DCC_MCP_SKILL_PATHS", EXAMPLES_SKILLS_DIR)

    gw_port = _pick_free_port()

    # Backend = gateway winner (single-backend cluster is sufficient for
    # this invariant; multi-backend aggregation is already covered by
    # test_gateway_facade_aggregation.py).
    cfg = McpHttpConfig(port=0, server_name="hello-backend")
    cfg.gateway_port = gw_port
    cfg.registry_dir = str(registry_dir)
    cfg.dcc_type = "python"
    cfg.heartbeat_secs = 1
    cfg.stale_timeout_secs = 10
    # ``include_bundled`` would also work but we keep the test hermetic
    # to examples/skills so a regression in bundled skills cannot mask
    # a regression in the load-skill → SSE pipeline.
    server = create_skill_server("python", cfg)
    handle = server.start()

    # Wait until both the instance port and the gateway port are up. The
    # self-probe inside start() guarantees this, but on slow CI it is
    # harmless to retry briefly.
    assert _wait_tcp_reachable("127.0.0.1", handle.port, budget=3.0), (
        f"instance port {handle.port} unreachable after start()"
    )
    if handle.is_gateway:
        assert _wait_tcp_reachable("127.0.0.1", gw_port, budget=3.0), (
            f"gateway port {gw_port} unreachable after start()"
        )

    try:
        yield {
            "handle": handle,
            "gateway_url": f"http://127.0.0.1:{gw_port}/mcp",
            "backend_url": handle.mcp_url(),
            "gateway_port": gw_port,
        }
    finally:
        with contextlib.suppress(Exception):
            handle.shutdown()


# ── tests ─────────────────────────────────────────────────────────────────────


class TestGatewayLoadSkillSsePropagation:
    """Gateway must emit tools/list_changed after a successful load_skill."""

    def test_backend_won_gateway_election(self, gateway_with_skill_backend):
        """Sanity check: the single backend should always be the gateway winner."""
        handle = gateway_with_skill_backend["handle"]
        assert handle.is_gateway, (
            "single-backend fixture must win the gateway election; "
            "if this fails, another process is holding the gateway port"
        )

    def test_hello_world_surfaces_as_skill_stub_before_load(self, gateway_with_skill_backend):
        """Before ``load_skill``, hello-world appears only as a ``__skill__hello-world`` stub.

        The real tool ``hello_world__greet`` MUST NOT be in ``tools/list``.
        This guards the progressive-loading contract.
        """
        resp = _post_mcp(gateway_with_skill_backend["gateway_url"], "tools/list")
        names = {t["name"] for t in resp["result"]["tools"]}

        # The skill stub is how the gateway (and client) discovers that
        # hello-world exists without paying to register its tools yet.
        # Gateway namespaces backend tools as ``<8hex>.<original>`` so we
        # match the suffix.
        assert any(n.endswith("__skill__hello-world") for n in names), (
            f"expected ``__skill__hello-world`` stub in aggregated tools/list, got: {sorted(names)[:20]}..."
        )

        # And the active tool must NOT be present yet. The tool is
        # ``greet`` (bare form introduced by #307; unique within the
        # single-skill instance) — before load_skill runs, only the
        # ``__skill__hello-world`` stub exists. We also assert the legacy
        # ``hello-world.greet`` form is absent to guard against a
        # regression that re-introduces prefixed emission unconditionally.
        assert not any(n.endswith(".greet") for n in names), (
            f"greet must NOT be active before load_skill is called; got: {sorted(names)[:20]}"
        )
        assert not any(n.endswith("hello-world.greet") for n in names), (
            "hello-world.greet must NOT be active before load_skill is called"
        )

    def test_load_skill_triggers_tools_list_changed_via_sse(self, gateway_with_skill_backend):
        """The full regression: SSE subscribe → load_skill → list_changed + tool visible.

        This is the core invariant that was implicitly covered by the
        deleted ``test_e2e_gateway_skills_progressive.py``. If the
        aggregating watcher regresses (stops polling, wrong fingerprint,
        broadcast broken) OR the gateway listener regresses (issue #303),
        this test fails.
        """
        gateway_url = gateway_with_skill_backend["gateway_url"]

        events: queue.Queue[dict] = queue.Queue()
        stop = threading.Event()
        subscriber = _SseSubscriber(gateway_url, events, stop)
        subscriber.start()
        try:
            # Wait until the SSE request returned headers — by this point
            # the gateway has added us to the broadcast channel.
            assert subscriber.connected.wait(timeout=5.0), "SSE subscriber never connected"
            if subscriber.error is not None:
                pytest.fail(f"SSE subscription failed: {subscriber.error!r}")

            # Give the gateway one aggregator tick so the baseline
            # fingerprint is committed — otherwise the very first
            # transition (empty → non-empty) can collapse our notification
            # into initial-state noise.
            time.sleep(AGGREGATOR_TICK_S + 0.5)

            # Trigger the load via the gateway (routes through to the
            # skill-enabled backend).
            load_resp = _post_mcp(
                gateway_url,
                "tools/call",
                {"name": "load_skill", "arguments": {"skill_name": "hello-world"}},
            )
            assert "error" not in load_resp, f"load_skill returned JSON-RPC error: {load_resp.get('error')}"
            # The load_skill tool returns a structured result; we do not
            # assert its exact shape here because it is covered by the
            # skills-layer unit tests. The only contract we need is that
            # the call did not surface an error.
            assert "result" in load_resp, f"load_skill missing result: {load_resp}"

            # Now wait for tools/list_changed — the aggregating watcher
            # ticks every 3 s so we budget well beyond that.
            notif = _drain_for_notification(
                events,
                "notifications/tools/list_changed",
                budget=SSE_NOTIFICATION_BUDGET_S,
            )
            assert notif is not None, (
                "gateway did not push notifications/tools/list_changed within "
                f"{SSE_NOTIFICATION_BUDGET_S}s after load_skill; "
                "either the aggregator watcher regressed or the SSE channel is broken"
            )
            assert notif.get("jsonrpc") == "2.0"
            assert notif.get("method") == "notifications/tools/list_changed"

            # And now tools/list must expose the loaded tool. Allow a brief
            # retry window because tools/list aggregation caches backend
            # responses for up to one watcher tick.
            # Gateway-aggregated tool names are ``<8hex>.<backend-tool>``.
            # Since #307, the backend-tool for a loaded skill is the bare
            # action name when unique within the instance — here
            # ``greet`` (the hello-world skill exposes a single action).
            # We match on the suffix so the test remains correct regardless
            # of the random 8-char instance id.
            deadline = time.time() + AGGREGATOR_TICK_S + 2.0
            active_names: set[str] = set()
            while time.time() < deadline:
                resp = _post_mcp(gateway_url, "tools/list")
                active_names = {t["name"] for t in resp["result"]["tools"]}
                if any(n.endswith(".greet") for n in active_names):
                    break
                time.sleep(0.5)

            assert any(n.endswith(".greet") for n in active_names), (
                "load_skill succeeded and SSE fired, but greet is still absent "
                f"from aggregated tools/list: sample={sorted(active_names)[:30]}"
            )

            # The stub should have been replaced by the real tool once the
            # skill is loaded — enforces the progressive-loading contract.
            assert not any(n.endswith("__skill__hello-world") for n in active_names), (
                "__skill__hello-world stub must be gone from tools/list after load_skill"
            )
        finally:
            stop.set()
            # urllib doesn't expose an easy abort; the subscriber thread
            # is daemon-flagged and will be reaped when the process exits.
            subscriber.join(timeout=1.0)

    def test_unload_skill_also_emits_tools_list_changed(self, gateway_with_skill_backend):
        """Symmetric coverage: unload path must also propagate over SSE.

        Separated from the load test so a regression in only one direction
        is still caught. We keep the assertion minimal (the notification
        itself) because ``tools/list`` semantics on unload are covered by
        the skills-layer tests.
        """
        gateway_url = gateway_with_skill_backend["gateway_url"]

        # Load first (we don't care about SSE here, only about setting up
        # the state for the unload assertion).
        load_resp = _post_mcp(
            gateway_url,
            "tools/call",
            {"name": "load_skill", "arguments": {"skill_name": "hello-world"}},
        )
        assert "result" in load_resp, f"precondition load_skill failed: {load_resp}"

        events: queue.Queue[dict] = queue.Queue()
        stop = threading.Event()
        subscriber = _SseSubscriber(gateway_url, events, stop)
        subscriber.start()
        try:
            assert subscriber.connected.wait(timeout=5.0), "SSE subscriber never connected"
            if subscriber.error is not None:
                pytest.fail(f"SSE subscription failed: {subscriber.error!r}")

            time.sleep(AGGREGATOR_TICK_S + 0.5)

            unload_resp = _post_mcp(
                gateway_url,
                "tools/call",
                {"name": "unload_skill", "arguments": {"skill_name": "hello-world"}},
            )
            assert "result" in unload_resp, f"unload_skill missing result: {unload_resp}"

            notif = _drain_for_notification(
                events,
                "notifications/tools/list_changed",
                budget=SSE_NOTIFICATION_BUDGET_S,
            )
            assert notif is not None, (
                "gateway did not push notifications/tools/list_changed within "
                f"{SSE_NOTIFICATION_BUDGET_S}s after unload_skill"
            )
        finally:
            stop.set()
            subscriber.join(timeout=1.0)
