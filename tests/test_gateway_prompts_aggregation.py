"""Python-side integration tests for the gateway's prompts aggregation (#731).

The Rust unit tests in ``crates/dcc-mcp-gateway/src/gateway/aggregator/tests.rs``
exercise the handler logic against mock backends. These tests instead drive the
full stack from a Python client's perspective:

* Two real ``McpHttpServer`` backends — each built via ``create_skill_server``
  with a disjoint fixture skill that ships a ``prompts.yaml`` sibling file.
* One gateway process elected by the first backend to bind the gateway port.
* A plain ``urllib`` client POSTs ``prompts/list`` / ``prompts/get`` against the
  gateway's ``/mcp`` endpoint and decodes the cursor-safe ``i_<id8>__<name>``
  namespace to verify routing.

Coverage:

1. Zero-backend gateway → ``prompts/list`` returns ``{"prompts": []}``.
2. Two backends with disjoint prompts → merged list with correct per-backend
   prefixes; each entry carries ``_instance_id`` / ``_dcc_type`` annotations.
3. ``prompts/get`` against a prefixed name → request reaches the owning backend
   and the rendered template references the decoded bare name.

The fixture skills live under ``tests/fixtures/prompts_skills/`` so they remain
hermetic to this test module — no bundled ``examples/skills`` prompt
regression can mask a gateway regression.
"""

from __future__ import annotations

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
from dcc_mcp_core import create_skill_server

REPO_ROOT = Path(__file__).resolve().parent.parent
FIXTURE_SKILLS_DIR = REPO_ROOT / "tests" / "fixtures" / "prompts_skills"
MAYA_SKILL_PARENT = str(FIXTURE_SKILLS_DIR / "maya-only")
BLENDER_SKILL_PARENT = str(FIXTURE_SKILLS_DIR / "blender-only")


# ── helpers ───────────────────────────────────────────────────────────────────


def _pick_free_port() -> int:
    """Return a port that is currently free on 127.0.0.1."""
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def _post_mcp(url: str, method: str, params: dict | None = None, rpc_id: int = 1, timeout: float = 10.0) -> dict:
    body: dict = {"jsonrpc": "2.0", "id": rpc_id, "method": method}
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


def _unescape_cursor_safe(escaped: str) -> str | None:
    """Inverse of the ``escape_cursor_safe`` helper from gateway/namespace (#656)."""
    out: list[str] = []
    i = 0
    n = len(escaped)
    while i < n:
        ch = escaped[i]
        if ch == "_":
            if i + 2 >= n or escaped[i + 2] != "_":
                return None
            mapped = {"U": "_", "D": ".", "H": "-"}.get(escaped[i + 1])
            if mapped is None:
                return None
            out.append(mapped)
            i += 3
        elif ch.isascii() and ch.isalnum():
            out.append(ch)
            i += 1
        else:
            return None
    return "".join(out)


def _split_gateway_prefixed(name: str) -> tuple[str, str] | None:
    """Decode ``i_<id8>__<escaped>`` or legacy ``<id8>.<name>`` into (prefix, bare)."""
    if name.startswith("i_"):
        rest = name[2:]
        sep_idx = rest.find("__")
        if sep_idx == 8:
            prefix = rest[:8]
            escaped = rest[10:]
            if all(ch in "0123456789abcdef" for ch in prefix):
                decoded = _unescape_cursor_safe(escaped)
                if decoded is not None:
                    return prefix, decoded
    prefix, sep, suffix = name.partition(".")
    if sep and len(prefix) == 8 and all(ch in "0123456789abcdef" for ch in prefix):
        return prefix, suffix
    return None


def _start_backend(dcc: str, skill_parent: Path, registry_dir: Path, gw_port: int) -> object:
    """Start an ``McpHttpServer`` via ``create_skill_server`` discovering ``skill_parent``."""
    cfg = McpHttpConfig(port=0, server_name=f"{dcc}-prompts-backend")
    cfg.gateway_port = gw_port
    cfg.registry_dir = str(registry_dir)
    cfg.dcc_type = dcc
    cfg.heartbeat_secs = 1
    cfg.stale_timeout_secs = 10
    # Disable accumulated user/team skills so the test stays hermetic:
    # a stray `~/.dcc-mcp/skills` must not introduce additional prompts
    # that confuse the aggregated set assertions.
    server = create_skill_server(
        dcc,
        cfg,
        extra_paths=[str(skill_parent)],
        accumulated=False,
    )
    return server.start()


# ── fixture: zero-backend gateway ────────────────────────────────────────────


@pytest.fixture()
def empty_gateway(tmp_path):
    """Spin up a gateway with zero backends.

    The fixture binds a throwaway backend just to win the gateway election
    (the gateway needs somebody to host ``/mcp``), then disables backend
    fan-out by pointing the instance at a DCC filter that matches no
    fixture skill — so the aggregated prompts set is empty from the
    client's perspective.
    """
    registry_dir = tmp_path / "registry"
    registry_dir.mkdir()
    gw_port = _pick_free_port()

    # "python" backend with no fixture skill — nothing to aggregate.
    cfg = McpHttpConfig(port=0, server_name="empty-gateway-host")
    cfg.gateway_port = gw_port
    cfg.registry_dir = str(registry_dir)
    cfg.dcc_type = "python"
    cfg.heartbeat_secs = 1
    cfg.stale_timeout_secs = 10
    server = create_skill_server("python", cfg, accumulated=False)
    handle = server.start()
    assert handle.is_gateway, "host backend must win gateway election"

    try:
        yield f"http://127.0.0.1:{gw_port}/mcp"
    finally:
        with contextlib.suppress(Exception):
            handle.shutdown()


# ── fixture: two backends with disjoint prompts ──────────────────────────────


@pytest.fixture(scope="module")
def prompts_cluster(tmp_path_factory):
    """Two backends + gateway, each backend exposes one fixture prompts-demo skill.

    We build per-DCC fixture-parent directories (``maya-only/`` and
    ``blender-only/``) so each backend's ``create_skill_server`` discovers
    exactly its own skill — the gateway aggregator is what merges them.
    """
    # Layout the fixture parents on disk if not already present.
    # ``tests/fixtures/prompts_skills/<dcc>-only/<skill-name>/`` contains
    # symlinks or direct files; for portability we materialise directories.
    maya_parent = FIXTURE_SKILLS_DIR / "maya-only"
    blender_parent = FIXTURE_SKILLS_DIR / "blender-only"
    if not (maya_parent / "maya-prompts-demo" / "SKILL.md").exists():
        pytest.skip(f"fixture skill missing at {maya_parent}")
    if not (blender_parent / "blender-prompts-demo" / "SKILL.md").exists():
        pytest.skip(f"fixture skill missing at {blender_parent}")

    registry_dir = tmp_path_factory.mktemp("prompts-registry")
    gw_port = _pick_free_port()

    # First backend wins the gateway election.
    handle_a = _start_backend("maya", maya_parent, registry_dir, gw_port)
    # Give the gateway a moment to bind its sentinel before #2 starts.
    time.sleep(0.25)
    handle_b = _start_backend("blender", blender_parent, registry_dir, gw_port)

    # Give the gateway's 2-second instance watcher + 3-second tools
    # watcher enough time to see both registrations.
    time.sleep(2.5)

    gateway_url = f"http://127.0.0.1:{gw_port}/mcp"

    # Load each fixture skill **on its owning backend directly** rather
    # than via the gateway. Each backend discovered only its own skill
    # (different ``extra_paths`` per DCC) so a cross-backend load_skill
    # would fail. Hitting the backend's own /mcp endpoint is the
    # unambiguous path.
    for handle, skill in ((handle_a, "maya-prompts-demo"), (handle_b, "blender-prompts-demo")):
        backend_url = handle.mcp_url()
        resp = _post_mcp(
            backend_url,
            "tools/call",
            {"name": "load_skill", "arguments": {"skill_name": skill}},
        )
        assert "error" not in resp, f"load_skill({skill}) on {backend_url} failed: {resp.get('error')}"

    # One more tick so the prompts watcher picks up the newly loaded
    # prompts before the first assertion runs.
    time.sleep(3.2)

    try:
        yield {
            "gateway_url": gateway_url,
            "handle_a": handle_a,
            "handle_b": handle_b,
        }
    finally:
        for h in (handle_b, handle_a):
            with contextlib.suppress(Exception):
                h.shutdown()


# ── tests ─────────────────────────────────────────────────────────────────────


class TestPromptsListEmptyGateway:
    """A gateway with no prompt-bearing backend must still answer prompts/list."""

    def test_prompts_list_returns_empty_array_not_method_not_found(self, empty_gateway):
        """Hard acceptance criterion from #731 — must not be -32601."""
        resp = _post_mcp(empty_gateway, "prompts/list")
        assert "error" not in resp, f"unexpected JSON-RPC error: {resp.get('error')}"
        assert resp["result"] == {"prompts": []}

    def test_initialize_advertises_prompts_capability(self, empty_gateway):
        """`prompts: {listChanged: true}` must appear in the capabilities object."""
        resp = _post_mcp(
            empty_gateway,
            "initialize",
            {
                "protocolVersion": "2025-03-26",
                "capabilities": {},
                "clientInfo": {"name": "prompts-empty-test", "version": "0.1"},
            },
        )
        caps = resp["result"]["capabilities"]
        assert caps.get("prompts", {}).get("listChanged") is True, (
            f"initialize must advertise prompts.listChanged=true; got: {caps}"
        )


class TestPromptsListAggregatesBackends:
    """Two backends with disjoint prompt sets merge into one namespaced list."""

    def test_merged_list_contains_every_backend_prompt(self, prompts_cluster):
        resp = _post_mcp(prompts_cluster["gateway_url"], "prompts/list")
        assert "error" not in resp, f"prompts/list error: {resp.get('error')}"
        names = [p["name"] for p in resp["result"]["prompts"]]

        decoded = [(n, _split_gateway_prefixed(n)) for n in names]
        bare_names = {bare for (_, split) in decoded if split is not None for bare in (split[1],)}

        for expected in ("bake_animation", "render_preview", "export_gltf"):
            assert expected in bare_names, f"expected bare prompt {expected!r} in merged list; names={names}"

    def test_backend_prompts_carry_instance_metadata(self, prompts_cluster):
        resp = _post_mcp(prompts_cluster["gateway_url"], "prompts/list")
        for prompt in resp["result"]["prompts"]:
            assert "_instance_id" in prompt, f"prompt missing _instance_id: {prompt!r}"
            assert "_instance_short" in prompt
            assert "_dcc_type" in prompt
            split = _split_gateway_prefixed(prompt["name"])
            assert split is not None, f"prompt name not cursor-safe-prefixed: {prompt['name']}"
            prefix, _ = split
            assert prompt["_instance_short"] == prefix, (
                f"prefix {prefix!r} doesn't match _instance_short {prompt['_instance_short']!r}"
            )

    def test_prompts_get_routes_to_owning_backend(self, prompts_cluster):
        """prompts/get on a namespaced name must reach the owning backend and render its template."""
        resp = _post_mcp(prompts_cluster["gateway_url"], "prompts/list")
        prompts = resp["result"]["prompts"]

        # Pick the blender-side prompt so we exercise cross-backend
        # routing (the gateway winner hosts the maya backend).
        target = next(
            (p for p in prompts if _split_gateway_prefixed(p["name"])[1] == "export_gltf"),
            None,
        )
        assert target is not None, f"export_gltf prompt missing from {[p['name'] for p in prompts]}"

        get_resp = _post_mcp(
            prompts_cluster["gateway_url"],
            "prompts/get",
            {
                "name": target["name"],
                "arguments": {
                    "output_path": "/tmp/demo.glb",
                    "include_animations": "true",
                },
            },
        )
        assert "error" not in get_resp, f"prompts/get error: {get_resp.get('error')}"
        messages = get_resp["result"]["messages"]
        assert len(messages) >= 1, f"prompts/get returned no messages: {get_resp}"
        text = messages[0]["content"]["text"]
        # The backend-side PromptRegistry renders the template — the
        # rendered output must have substituted the arguments, proving
        # the request actually reached the right backend.
        assert "/tmp/demo.glb" in text, f"rendered prompt missing output_path arg: {text!r}"
        assert "true" in text, f"rendered prompt missing include_animations arg: {text!r}"

    def test_prompts_get_with_unknown_prefix_returns_routing_error(self, prompts_cluster):
        """An unknown 8-hex prefix must surface -32602 without hitting any backend."""
        resp = _post_mcp(
            prompts_cluster["gateway_url"],
            "prompts/get",
            {"name": "i_deadbeef__bake_U_animation"},
        )
        err = resp.get("error")
        assert err is not None, f"expected routing error, got: {resp}"
        assert err["code"] == -32602
        assert "deadbeef" in err["message"]


# A tiny helper to defeat linter warnings for unused imports in environments
# where ``ToolRegistry`` is not referenced directly — we rely on
# ``create_skill_server`` building registries for us, but importing the
# symbol keeps parity with sibling gateway tests.
_ = ToolRegistry
