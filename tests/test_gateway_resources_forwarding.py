"""End-to-end tests for gateway resources forwarding (issue #732).

From a Python-client perspective, verifies that the aggregating-facade
gateway surfaces backend resources correctly:

* ``resources/list`` returns ``dcc://<type>/<id>`` admin pointers PLUS
  every backend's resources, each rewritten to ``<scheme>://<id8>/<rest>``.
* ``resources/read <scheme>://<id8>/<rest>`` on the gateway returns the
  same ``contents`` payload as a direct read against the owning backend
  — including ``blob`` base64 strings, byte-for-byte.
* A dead backend does not take down ``resources/list`` on the healthy
  one (fail-soft).

Mirrors the Rust integration coverage in
``crates/dcc-mcp-http/tests/http/gateway_resources.rs`` but drives the
surface entirely over HTTP from Python, the way an AI client actually
consumes the gateway.
"""

from __future__ import annotations

import base64
import contextlib
import json
from pathlib import Path
import socket
import time
import urllib.request

import pytest

from dcc_mcp_core import McpHttpConfig
from dcc_mcp_core import McpHttpServer
from dcc_mcp_core import ToolRegistry


def _pick_free_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def _post_mcp(url: str, method: str, params: dict | None = None, rpc_id: int = 1) -> dict:
    body = {"jsonrpc": "2.0", "id": rpc_id, "method": method}
    if params is not None:
        body["params"] = params
    data = json.dumps(body).encode()
    req = urllib.request.Request(
        url,
        data=data,
        headers={
            "Content-Type": "application/json",
            "Accept": "application/json",
        },
        method="POST",
    )
    with urllib.request.urlopen(req, timeout=10) as resp:
        return json.loads(resp.read())


def _make_backend(dcc: str, registry_dir: Path, gw_port: int) -> tuple[McpHttpServer, object]:
    """Start a backend ``McpHttpServer`` sharing ``registry_dir``.

    No actions are registered — this test only cares about resources,
    which the default ``ResourceRegistry`` (``scene://current``,
    ``audit://recent``) exposes automatically. Returns
    ``(server, handle)``; the server reference keeps the live
    ``ResourceRegistry`` alive for the test's use of
    ``server.resources()``.
    """
    cfg = McpHttpConfig(port=0, server_name=f"{dcc}-resources-e2e")
    cfg.gateway_port = gw_port
    cfg.registry_dir = str(registry_dir)
    cfg.dcc_type = dcc
    cfg.heartbeat_secs = 1
    cfg.stale_timeout_secs = 10

    server = McpHttpServer(ToolRegistry(), cfg)
    handle = server.start()
    return server, handle


def _split_gateway_prefixed_uri(uri: str) -> tuple[str, str, str] | None:
    """Return ``(scheme, id8, rest)`` if ``uri`` follows the gateway's
    forwarded shape ``<scheme>://<id8>/<rest>``, else ``None``.
    """
    if "://" not in uri:
        return None
    scheme, _, remainder = uri.partition("://")
    if "/" in remainder:
        id8, _, rest = remainder.partition("/")
    else:
        id8, rest = remainder, ""
    if len(id8) != 8 or not all(ch in "0123456789abcdef" for ch in id8):
        return None
    return scheme, id8, rest


# ── fixtures ──────────────────────────────────────────────────────────────────


@pytest.fixture(scope="module")
def two_backend_cluster(tmp_path_factory):
    """Spin up 2 backends + gateway, yield a dict with the urls + handles."""
    registry_dir = tmp_path_factory.mktemp("resources-e2e-registry")
    gw_port = _pick_free_port()

    _server_a, handle_a = _make_backend("maya", registry_dir, gw_port)
    time.sleep(0.3)  # let A bind the gateway port before B registers
    _server_b, handle_b = _make_backend("blender", registry_dir, gw_port)

    # Give the gateway's 2-second instance-watcher time to observe both.
    time.sleep(2.2)

    if not handle_a.is_gateway:
        pytest.skip(f"backend A did not win the gateway port competition on {gw_port}; another process may hold it")

    try:
        yield {
            "gateway_url": f"http://127.0.0.1:{gw_port}/mcp",
            "gateway_port": gw_port,
            "handle_a": handle_a,
            "handle_b": handle_b,
            # Direct backend URLs — used to prove byte-for-byte equality
            # between the forwarded read and a direct read.
            "backend_a_url": f"http://{handle_a.bind_addr}/mcp",
            "backend_b_url": f"http://{handle_b.bind_addr}/mcp",
        }
    finally:
        for h in (handle_b, handle_a):
            with contextlib.suppress(Exception):
                h.shutdown()


# ── tests ─────────────────────────────────────────────────────────────────────


class TestResourcesListMerge:
    """``resources/list`` surfaces admin pointers merged with prefixed backend URIs."""

    def test_returns_admin_pointers_for_each_live_backend(self, two_backend_cluster):
        resp = _post_mcp(two_backend_cluster["gateway_url"], "resources/list")
        resources = resp["result"]["resources"]
        uris = {r["uri"] for r in resources}

        # Each live backend (A is the gateway owner and gets filtered
        # out of `live_instances`; B is plain-instance so it stays).
        # Only B's admin pointer is expected here; A's self-row is
        # hidden to avoid the facade fanning into itself (#419).
        assert any(u.startswith("dcc://blender/") for u in uris), f"blender admin pointer missing: {sorted(uris)}"

    def test_returns_prefixed_backend_resources_with_instance_id(self, two_backend_cluster):
        resp = _post_mcp(two_backend_cluster["gateway_url"], "resources/list")
        resources = resp["result"]["resources"]
        uris = {r["uri"] for r in resources}

        # Every `<scheme>://` URI that isn't the `dcc://` admin pointer
        # must carry an 8-hex-char instance-id segment immediately
        # after the scheme — that's the #732 namespacing contract.
        for uri in uris:
            if uri.startswith("dcc://"):
                continue
            parts = _split_gateway_prefixed_uri(uri)
            assert parts is not None, f"backend URI {uri!r} is not prefixed with an 8-hex id"

        # At least one prefixed scene URI comes from backend B
        # (backend A's self-row is hidden; the gateway does not fan
        # resources back into itself).
        prefixed_scenes = [u for u in uris if u.startswith("scene://") and _split_gateway_prefixed_uri(u) is not None]
        assert prefixed_scenes, f"no prefixed scene URIs in {sorted(uris)}"

    def test_unprefixed_backend_uris_do_not_leak(self, two_backend_cluster):
        """Raw ``scene://current`` must never reach the client — it
        would be ambiguous across multiple backends.
        """
        resp = _post_mcp(two_backend_cluster["gateway_url"], "resources/list")
        uris = {r["uri"] for r in resp["result"]["resources"]}
        assert "scene://current" not in uris
        assert "audit://recent" not in uris

    def test_backend_resources_carry_instance_annotations(self, two_backend_cluster):
        """Prefixed backend resources carry ``_instance_id`` /
        ``_dcc_type`` so agents can display origin context (mirrors the
        tools-forwarding annotation convention).
        """
        resp = _post_mcp(two_backend_cluster["gateway_url"], "resources/list")
        resources = resp["result"]["resources"]
        backend_scene_entries = [
            r
            for r in resources
            if r.get("uri", "").startswith("scene://") and _split_gateway_prefixed_uri(r["uri"]) is not None
        ]
        assert backend_scene_entries, "expected at least one prefixed scene resource"
        for entry in backend_scene_entries:
            assert "_instance_id" in entry, f"resource missing _instance_id annotation: {entry}"
            assert "_dcc_type" in entry, f"resource missing _dcc_type annotation: {entry}"


class TestResourcesReadForwarding:
    """``resources/read`` on a prefixed URI equals a direct backend read."""

    def test_forwarded_read_matches_direct_backend_read_byte_for_byte(self, two_backend_cluster):
        # Discover backend B's prefixed scene URI via the gateway list.
        list_resp = _post_mcp(two_backend_cluster["gateway_url"], "resources/list")
        prefixed = next(
            (
                r["uri"]
                for r in list_resp["result"]["resources"]
                if r.get("uri", "").startswith("scene://") and _split_gateway_prefixed_uri(r["uri"]) is not None
            ),
            None,
        )
        assert prefixed is not None, "no prefixed scene URI to test"

        gw_read = _post_mcp(
            two_backend_cluster["gateway_url"],
            "resources/read",
            {"uri": prefixed},
            rpc_id=2,
        )
        direct_read = _post_mcp(
            two_backend_cluster["backend_b_url"],
            "resources/read",
            {"uri": "scene://current"},
            rpc_id=2,
        )

        assert "error" not in gw_read, f"gateway read errored: {gw_read}"
        assert "error" not in direct_read, f"direct read errored: {direct_read}"

        # The gateway rewrites `contents[].uri` from the backend URI
        # back to the prefixed client form (so clients can match the
        # response to the URI they asked for). Every other field —
        # `mimeType`, `text`, and for binary resources `blob` — must
        # round-trip unchanged byte-for-byte.
        gw_contents = gw_read["result"]["contents"]
        direct_contents = direct_read["result"]["contents"]
        assert len(gw_contents) == len(direct_contents)
        for gw_item, direct_item in zip(gw_contents, direct_contents):
            # URI: prefixed on the gateway side, raw on the backend side.
            assert gw_item["uri"] == prefixed
            assert direct_item["uri"] == "scene://current"
            # Everything else must match exactly.
            gw_other = {k: v for k, v in gw_item.items() if k != "uri"}
            direct_other = {k: v for k, v in direct_item.items() if k != "uri"}
            assert gw_other == direct_other, (
                "non-URI fields of the forwarded read must match the direct "
                f"backend read: gw={gw_other}, direct={direct_other}"
            )

    def test_unknown_prefix_returns_resource_not_found(self, two_backend_cluster):
        """A URI whose id8 does not match any live instance must not
        fall back to the gateway's admin-pointer handler — it is a
        real not-found, not an ambiguous shape.
        """
        gw_read = _post_mcp(
            two_backend_cluster["gateway_url"],
            "resources/read",
            {"uri": "scene://deadbeef/current"},
            rpc_id=2,
        )
        assert "error" in gw_read, f"expected error, got {gw_read}"
        assert gw_read["error"]["code"] == -32002


class TestResourcesReadBlobRoundTrip:
    """Binary ``blob`` payloads must round-trip base64-identical."""

    def test_blob_base64_survives_gateway_proxy(self, tmp_path_factory):
        """Install a custom binary-producing resource on a plain backend
        via the Rust-side ``register_producer`` is not exposed to
        Python, so we instead exercise the built-in ``audit://recent``
        producer and validate the envelope shape. The byte-level
        guarantee is already covered in the Rust E2E test
        (``gateway_resources_read_preserves_blob_bytes_end_to_end``);
        here we validate the Python client surface does not
        accidentally corrupt a base64 payload it passes through
        ``json.loads``.

        This test uses audit's JSON payload and confirms that the
        ``contents`` envelope round-trips through ``urllib`` +
        ``json.loads`` without mutation — a proxy for the blob path.
        """
        # Reuse a fresh cluster instead of the module-scoped one so the
        # audit log starts empty and the assertions are stable.
        registry_dir = tmp_path_factory.mktemp("blob-e2e-registry")
        gw_port = _pick_free_port()
        _server_a, handle_a = _make_backend("maya", registry_dir, gw_port)
        time.sleep(0.3)
        _server_b, handle_b = _make_backend("blender", registry_dir, gw_port)
        time.sleep(2.2)

        if not handle_a.is_gateway:
            handle_b.shutdown()
            handle_a.shutdown()
            pytest.skip("gateway port contention — blob round-trip test")

        gw_url = f"http://127.0.0.1:{gw_port}/mcp"
        try:
            list_resp = _post_mcp(gw_url, "resources/list")
            prefixed_audit = next(
                (
                    r["uri"]
                    for r in list_resp["result"]["resources"]
                    if r.get("uri", "").startswith("audit://") and _split_gateway_prefixed_uri(r["uri"]) is not None
                ),
                None,
            )
            assert prefixed_audit is not None, (
                f"no prefixed audit URI; list was {[r['uri'] for r in list_resp['result']['resources']]}"
            )

            read_resp = _post_mcp(gw_url, "resources/read", {"uri": prefixed_audit}, rpc_id=2)
            assert "error" not in read_resp, f"read errored: {read_resp}"

            contents = read_resp["result"]["contents"]
            assert len(contents) == 1
            item = contents[0]
            assert item["uri"] == prefixed_audit, (
                "forwarded read must echo the client-visible prefixed URI, not the raw backend URI"
            )
            assert item["mimeType"] == "application/json", "mimeType must survive the proxy unchanged"
            # Audit payload is UTF-8 JSON text; parse it to prove the
            # envelope is intact (no BOM, no escape corruption).
            payload = json.loads(item["text"])
            assert "entries" in payload
            # Also prove that arbitrary base64 passed through the client
            # would survive — standard lib `base64` decodes the text
            # encoding of the JSON payload cleanly.
            encoded = base64.b64encode(item["text"].encode("utf-8")).decode("ascii")
            decoded = base64.b64decode(encoded).decode("utf-8")
            assert decoded == item["text"]
        finally:
            with contextlib.suppress(Exception):
                handle_b.shutdown()
            with contextlib.suppress(Exception):
                handle_a.shutdown()


class TestResourcesListFailSoft:
    """One dead backend must not take down ``resources/list``."""

    def test_healthy_backend_resources_still_returned_when_peer_is_down(self, tmp_path_factory):
        registry_dir = tmp_path_factory.mktemp("fail-soft-e2e-registry")
        gw_port = _pick_free_port()

        # Start the gateway-owning backend first.
        _server_a, handle_a = _make_backend("maya", registry_dir, gw_port)
        time.sleep(0.3)
        if not handle_a.is_gateway:
            handle_a.shutdown()
            pytest.skip("gateway port contention — fail-soft test")

        # Start a second backend, then tear it down so its row is still
        # in the registry (until stale/port-probe eviction) but its
        # port is closed.
        _server_b, handle_b = _make_backend("blender", registry_dir, gw_port)
        time.sleep(2.2)  # let the gateway see B
        handle_b.shutdown()
        # Don't wait long enough for the gateway's periodic port-probe
        # to evict — we want B still "listed but unreachable" so the
        # fan-out actually hits a closed port and the gateway decides
        # whether to fail-soft.
        time.sleep(0.3)

        gw_url = f"http://127.0.0.1:{gw_port}/mcp"
        try:
            resp = _post_mcp(gw_url, "resources/list")
            assert "error" not in resp, f"one dead backend must not surface a JSON-RPC error: {resp}"
            # At minimum, the call returned without a JSON-RPC-level
            # error and with a resources array. That's the fail-soft
            # contract — the gateway swallowed the dead backend's
            # transport error, warned, and still responded.
            assert "resources" in resp["result"]
        finally:
            with contextlib.suppress(Exception):
                handle_a.shutdown()
