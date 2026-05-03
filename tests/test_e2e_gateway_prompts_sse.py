"""E2E regression test: gateway SSE → load_skill → prompts/list_changed (#731).

Complement to ``test_e2e_gateway_skill_load_sse.py`` which guards the
``notifications/tools/list_changed`` invariant. This module guards the
prompts-primitive counterpart introduced by PR #740: the gateway's
prompts fingerprint watcher must emit ``notifications/prompts/list_changed``
over SSE whenever a backend's set of MCP prompts changes — which happens
when a skill that carries a ``metadata.dcc-mcp.prompts`` sibling file is
loaded or unloaded.

The flow covered end-to-end:

1. A backend ``McpHttpServer`` is built via ``create_skill_server`` with
   skill discovery pointed at ``tests/fixtures/prompts_skills/maya-only``
   so the ``maya-prompts-demo`` fixture skill is visible as a skill stub
   but **not** loaded — meaning its prompts have not been published to
   the gateway-facing ``prompts/list`` yet.
2. A background SSE subscriber opens ``GET /mcp`` on the gateway and
   records every JSON-RPC notification.
3. ``load_skill`` is invoked through the gateway. After the prompts
   watcher's tick (~3 s), the client MUST see a
   ``notifications/prompts/list_changed`` and a subsequent
   ``prompts/list`` MUST expose the fixture's prompts.
4. ``unload_skill`` is then invoked — we assert a *second*
   ``notifications/prompts/list_changed`` is delivered, guarding the
   symmetric unload path.

Runtime budget: the prompts watcher inherits the same 3-second tick as
the tools/resources aggregators, so we allow up to 12 s end-to-end for
each wait.
"""

from __future__ import annotations

import contextlib
import json
from pathlib import Path
import queue
import socket
import threading
import time
from typing import Any
import urllib.request

import pytest

from dcc_mcp_core import McpHttpConfig
from dcc_mcp_core import create_skill_server

REPO_ROOT = Path(__file__).resolve().parent.parent
FIXTURE_SKILL_PARENT = str(REPO_ROOT / "tests" / "fixtures" / "prompts_skills" / "maya-only")

# The aggregating prompts watcher ticks every 3 s (same cadence as the
# tools and resources aggregators); add slack for scheduling jitter.
SSE_NOTIFICATION_BUDGET_S = 12.0
AGGREGATOR_TICK_S = 3.0


# ── helpers ──────────────────────────────────────────────────────────────────


def _pick_free_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def _post_mcp(url: str, method: str, params: dict | None = None, rpc_id: int = 1, timeout: float = 10.0) -> dict:
    body: dict[str, Any] = {"jsonrpc": "2.0", "id": rpc_id, "method": method}
    if params is not None:
        body["params"] = params
    req = urllib.request.Request(
        url,
        data=json.dumps(body).encode(),
        headers={"Content-Type": "application/json", "Accept": "application/json"},
        method="POST",
    )
    with urllib.request.urlopen(req, timeout=timeout) as resp:
        return json.loads(resp.read())


def _wait_tcp_reachable(host: str, port: int, budget: float = 3.0) -> bool:
    deadline = time.time() + budget
    while time.time() < deadline:
        try:
            with socket.create_connection((host, port), timeout=0.3):
                return True
        except OSError:
            time.sleep(0.05)
    return False


class _SseSubscriber(threading.Thread):
    """Background SSE consumer — same shape as the one in ``test_e2e_gateway_skill_load_sse.py``."""

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
            with urllib.request.urlopen(req, timeout=20.0) as resp:
                self.connected.set()
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
                        if pending_data:
                            payload = "\n".join(pending_data)
                            pending_data = []
                            with contextlib.suppress(json.JSONDecodeError):
                                self.events.put(json.loads(payload))
                        continue
                    if text.startswith(":"):
                        continue
                    if text.startswith("data:"):
                        pending_data.append(text[5:].lstrip())
        except BaseException as e:
            self.error = e
            self.connected.set()


def _drain_for_notification(events: queue.Queue[dict], method: str, budget: float) -> dict | None:
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
def gateway_with_prompts_skill(tmp_path):
    """Start a single backend + gateway. The fixture skill is discovered but not loaded."""
    registry_dir = tmp_path / "registry"
    registry_dir.mkdir()

    if not (Path(FIXTURE_SKILL_PARENT) / "maya-prompts-demo" / "prompts.yaml").exists():
        pytest.skip(f"fixture skill not present at {FIXTURE_SKILL_PARENT}")

    gw_port = _pick_free_port()

    cfg = McpHttpConfig(port=0, server_name="prompts-backend")
    cfg.gateway_port = gw_port
    cfg.registry_dir = str(registry_dir)
    cfg.dcc_type = "maya"
    cfg.heartbeat_secs = 1
    cfg.stale_timeout_secs = 10

    server = create_skill_server(
        "maya",
        cfg,
        extra_paths=[FIXTURE_SKILL_PARENT],
        accumulated=False,
    )
    handle = server.start()

    assert _wait_tcp_reachable("127.0.0.1", handle.port, budget=3.0), (
        f"backend port {handle.port} unreachable after start()"
    )
    if handle.is_gateway:
        assert _wait_tcp_reachable("127.0.0.1", gw_port, budget=3.0), (
            f"gateway port {gw_port} unreachable after start()"
        )

    try:
        yield {
            "handle": handle,
            "gateway_url": f"http://127.0.0.1:{gw_port}/mcp",
        }
    finally:
        with contextlib.suppress(Exception):
            handle.shutdown()


# ── tests ─────────────────────────────────────────────────────────────────────


class TestGatewayPromptsListChangedSse:
    """The gateway must push ``notifications/prompts/list_changed`` on load/unload."""

    def test_backend_won_gateway_election(self, gateway_with_prompts_skill):
        """The single backend must win the election so the test URL has a gateway."""
        assert gateway_with_prompts_skill["handle"].is_gateway, "single-backend fixture must win the gateway election"

    def test_prompts_list_starts_empty_before_load_skill(self, gateway_with_prompts_skill):
        """The fixture prompts must not appear in prompts/list until load_skill runs."""
        resp = _post_mcp(gateway_with_prompts_skill["gateway_url"], "prompts/list")
        assert "error" not in resp, f"prompts/list error: {resp.get('error')}"
        assert resp["result"]["prompts"] == [], f"prompts/list should be empty pre-load; got: {resp['result']}"

    def test_load_skill_emits_prompts_list_changed(self, gateway_with_prompts_skill):
        """Full regression: SSE subscribe → load_skill → notifications/prompts/list_changed."""
        gateway_url = gateway_with_prompts_skill["gateway_url"]
        events: queue.Queue[dict] = queue.Queue()
        stop = threading.Event()
        subscriber = _SseSubscriber(gateway_url, events, stop)
        subscriber.start()
        try:
            assert subscriber.connected.wait(timeout=5.0), "SSE subscriber never connected"
            if subscriber.error is not None:
                pytest.fail(f"SSE subscription failed: {subscriber.error!r}")

            # Let the baseline prompts fingerprint settle (empty set) so
            # the load-triggered transition isn't collapsed into any
            # initial-state broadcast.
            time.sleep(AGGREGATOR_TICK_S + 0.5)

            load_resp = _post_mcp(
                gateway_url,
                "tools/call",
                {"name": "load_skill", "arguments": {"skill_name": "maya-prompts-demo"}},
            )
            assert "error" not in load_resp, f"load_skill JSON-RPC error: {load_resp.get('error')}"
            assert "result" in load_resp
            # Surface the structured result so failures show *why* load_skill
            # was a no-op (e.g. ambiguous target, unknown skill, missing DCC).
            lr_text = load_resp["result"]["content"][0]["text"]
            lr_body = json.loads(lr_text)
            assert lr_body.get("loaded") or lr_body.get("newly_loaded"), (
                f"load_skill did not load maya-prompts-demo: {lr_text}"
            )

            notif = _drain_for_notification(
                events,
                "notifications/prompts/list_changed",
                budget=SSE_NOTIFICATION_BUDGET_S,
            )
            assert notif is not None, (
                "gateway did not push notifications/prompts/list_changed within "
                f"{SSE_NOTIFICATION_BUDGET_S}s after load_skill"
            )
            assert notif.get("jsonrpc") == "2.0"
            assert notif.get("method") == "notifications/prompts/list_changed"

            # Sanity: the new prompts must now be visible on the aggregated
            # prompts/list. Allow one watcher tick of cache slack.
            deadline = time.time() + AGGREGATOR_TICK_S + 2.0
            names: list[str] = []
            while time.time() < deadline:
                resp = _post_mcp(gateway_url, "prompts/list")
                names = [p["name"] for p in resp["result"]["prompts"]]
                if names:
                    break
                time.sleep(0.3)
            assert names, "prompts/list is still empty after load_skill + list_changed fired"
        finally:
            stop.set()
            subscriber.join(timeout=1.0)

    def test_unload_skill_also_emits_prompts_list_changed(self, gateway_with_prompts_skill):
        """Symmetric unload path — a regression in only one direction must still be caught."""
        gateway_url = gateway_with_prompts_skill["gateway_url"]

        # Precondition: load the skill so the fingerprint is non-empty.
        pre = _post_mcp(
            gateway_url,
            "tools/call",
            {"name": "load_skill", "arguments": {"skill_name": "maya-prompts-demo"}},
        )
        assert "result" in pre, f"precondition load_skill failed: {pre}"

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
                {"name": "unload_skill", "arguments": {"skill_name": "maya-prompts-demo"}},
            )
            assert "result" in unload_resp, f"unload_skill missing result: {unload_resp}"

            notif = _drain_for_notification(
                events,
                "notifications/prompts/list_changed",
                budget=SSE_NOTIFICATION_BUDGET_S,
            )
            assert notif is not None, (
                "gateway did not push notifications/prompts/list_changed within "
                f"{SSE_NOTIFICATION_BUDGET_S}s after unload_skill"
            )
        finally:
            stop.set()
            subscriber.join(timeout=1.0)
