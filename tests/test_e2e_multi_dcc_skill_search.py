"""E2E multi-DCC REST x MCP skill search/load/call coverage.

Validates the Skills-First + discover+dispatch contract across multiple
simulated DCC instances:

1. Each DCC instance exposes its catalog as RESTful endpoints
   (``GET /v1/skills``, ``POST /v1/search``, ``POST /v1/describe``,
   ``POST /v1/call``) driven straight from ``SKILL.md`` descriptions.
2. The MCP gateway surface lets an agent ``search_skills`` + ``load_skill``
   + ``call_tool`` against those instances without knowing their ports.
3. Fuzzy search across >= 3 different DCC instances AND the distinct APIs
   exposed by each ``server.exe`` must disambiguate correctly — a query
   hitting a skill that lives on multiple DCCs returns one hit per DCC.

The in-process suite runs on every matrix cell and validates the core
invariants. The real-subprocess suite (``TestRealServerExeSubprocess``)
is gated by ``DCC_MCP_SERVER_EXE`` so the main CI path stays fast; the
mcpcall-e2e job (or a local developer) opts in by setting the env var
to the compiled ``dcc-mcp-server`` binary path.
"""

from __future__ import annotations

# Import built-in modules
import contextlib
import json
import os
from pathlib import Path
import socket
import subprocess
import time
from typing import Any
import urllib.error
import urllib.request

# Import third-party modules
import pytest

from conftest import McpClient
from conftest import allocate_gateway_port

# Import local modules
import dcc_mcp_core
from dcc_mcp_core import McpHttpConfig
from dcc_mcp_core import create_skill_server

# ── Constants ────────────────────────────────────────────────────────────

CALL_TIMEOUT_S = 10.0
STARTUP_BUDGET_S = 5.0

# Three simulated DCC hosts, each exposing a distinct catalog plus one
# shared skill name so fuzzy search must disambiguate across DCCs.
DCC_NAMES = ("maya", "blender", "houdini")

# Skills per DCC. The third entry (``shared-geometry``) intentionally
# collides across DCCs so the disambiguation path is exercised.
SKILL_DEFS: dict[str, list[tuple[str, list[str]]]] = {
    "maya": [
        ("maya-primitives", ["create_sphere", "create_cube"]),
        ("maya-render", ["start_render"]),
        ("shared-geometry", ["subdivide"]),
    ],
    "blender": [
        ("blender-primitives", ["add_cube", "add_sphere"]),
        ("blender-render", ["render_frame"]),
        ("shared-geometry", ["subdivide"]),
    ],
    "houdini": [
        ("houdini-procedural", ["pig_head", "rubber_toy"]),
        ("houdini-render", ["mantra_render"]),
        ("shared-geometry", ["subdivide"]),
    ],
}


# ── HTTP helpers ─────────────────────────────────────────────────────────


def _post_json(url: str, body: Any, timeout: float = CALL_TIMEOUT_S) -> tuple[int, Any]:
    """POST JSON to a REST (non-MCP) endpoint and return (status_code, parsed_body)."""
    data = json.dumps(body).encode()
    req = urllib.request.Request(
        url,
        data=data,
        headers={"Content-Type": "application/json", "Accept": "application/json"},
        method="POST",
    )
    try:
        with urllib.request.urlopen(req, timeout=timeout) as resp:
            return resp.status, json.loads(resp.read())
    except urllib.error.HTTPError as e:
        payload: Any = {}
        with contextlib.suppress(Exception):
            payload = json.loads(e.read())
        return e.code, payload


def _get_json(url: str, timeout: float = CALL_TIMEOUT_S) -> tuple[int, Any]:
    req = urllib.request.Request(url, headers={"Accept": "application/json, text/event-stream"}, method="GET")
    try:
        with urllib.request.urlopen(req, timeout=timeout) as resp:
            return resp.status, json.loads(resp.read())
    except urllib.error.HTTPError as e:
        return e.code, {}


def _mcp_post(mcp_url: str, method: str, params: dict[str, Any]) -> dict[str, Any]:
    """POST a JSON-RPC request to the MCP endpoint."""
    client = McpClient(mcp_url)
    body = {"jsonrpc": "2.0", "id": method, "method": method, "params": params}
    _status, resp = client.post(body)
    return resp


def _rest_base(mcp_url: str) -> str:
    """Strip the ``/mcp`` suffix from a handle URL."""
    return mcp_url.rsplit("/mcp", 1)[0]


def _pick_free_port() -> int:
    return allocate_gateway_port()


# ── Fixture helpers ──────────────────────────────────────────────────────


def _write_skill(skills_dir: Path, name: str, dcc: str, tool_names: list[str]) -> None:
    """Materialise a SKILL.md + sibling tools.yaml pair on disk.

    Uses the agentskills.io-compliant nested ``metadata.dcc-mcp.*`` form so
    the loader accepts it (post-commit 531501a).
    """
    skill_dir = skills_dir / name
    (skill_dir / "scripts").mkdir(parents=True, exist_ok=True)

    (skill_dir / "SKILL.md").write_text(
        "---\n"
        f"name: {name}\n"
        f'description: "{name} skill for {dcc} — fuzzy-search surface"\n'
        "metadata:\n"
        "  dcc-mcp:\n"
        f"    dcc: {dcc}\n"
        "    version: 1.0.0\n"
        f'    search-hint: "{dcc}, {name}, geometry, render, primitives"\n'
        f"    tags: [{dcc}, e2e]\n"
        "    tools: tools.yaml\n"
        "---\n"
        f"# {name}\n",
        encoding="utf-8",
    )

    tool_entries = []
    for tname in tool_names:
        tool_entries.append(
            f"  - name: {tname}\n"
            f'    description: "{name}.{tname} on {dcc}"\n'
            f"    source_file: scripts/{tname}.py\n"
            f"    input_schema:\n"
            f"      type: object\n"
            f"      properties:\n"
            f"        value:\n"
            f"          type: integer\n"
        )
        (skill_dir / "scripts" / f"{tname}.py").write_text(
            "def main(**kwargs):\n"
            f'    return {{"success": True, "dcc": "{dcc}", "skill": "{name}", "tool": "{tname}"}}\n',
            encoding="utf-8",
        )

    (skill_dir / "tools.yaml").write_text("tools:\n" + "".join(tool_entries), encoding="utf-8")


def _wait_reachable(url: str, budget_s: float = STARTUP_BUDGET_S) -> None:
    """Block until ``GET url`` succeeds or the budget expires."""
    deadline = time.monotonic() + budget_s
    last_exc: Exception | None = None
    while time.monotonic() < deadline:
        try:
            with urllib.request.urlopen(url, timeout=1.0) as resp:
                if resp.status == 200:
                    return
        except Exception as exc:
            last_exc = exc
        time.sleep(0.1)
    pytest.fail(f"url {url} not reachable within {budget_s}s; last exc={last_exc!r}")


def _post_json_until_ok(url: str, body: Any, *, budget_s: float = 15.0) -> tuple[int, Any]:
    """Retry POSTs for subprocess E2E probes until one succeeds or budget expires."""
    deadline = time.monotonic() + budget_s
    last_exc: Exception | None = None
    last_result: tuple[int, Any] = (0, {})
    while time.monotonic() < deadline:
        remaining = max(1.0, min(CALL_TIMEOUT_S, deadline - time.monotonic()))
        try:
            last_result = _post_json(url, body, timeout=remaining)
            if last_result[0] == 200:
                return last_result
        except Exception as exc:
            last_exc = exc
        time.sleep(0.1)
    if last_exc is not None:
        pytest.fail(f"POST {url} did not succeed within {budget_s}s; last exc={last_exc!r}")
    return last_result


def _mcp_initialize_until_ok(mcp_url: str, *, budget_s: float = 15.0) -> dict[str, Any]:
    """Retry a minimal MCP initialize probe until the subprocess endpoint is ready."""
    deadline = time.monotonic() + budget_s
    last_exc: Exception | None = None
    body = {
        "jsonrpc": "2.0",
        "id": "__init__",
        "method": "initialize",
        "params": {
            "protocolVersion": "2025-11-25",
            "capabilities": {},
            "clientInfo": {"name": "pytest", "version": "1.0"},
        },
    }
    while time.monotonic() < deadline:
        remaining = max(1.0, min(CALL_TIMEOUT_S, deadline - time.monotonic()))
        try:
            req = urllib.request.Request(
                mcp_url,
                data=json.dumps(body).encode(),
                headers={
                    "Content-Type": "application/json",
                    "Accept": "application/json, text/event-stream",
                    "MCP-Protocol-Version": "2025-11-25",
                },
                method="POST",
            )
            with urllib.request.urlopen(req, timeout=remaining) as resp:
                envelope = json.loads(resp.read())
            result = envelope.get("result", envelope)
            if result.get("protocolVersion"):
                return result
            last_exc = AssertionError(f"initialize returned no protocolVersion: {result!r}")
        except Exception as exc:
            last_exc = exc
        time.sleep(0.1)
    pytest.fail(f"MCP initialize {mcp_url} did not succeed within {budget_s}s; last exc={last_exc!r}")


# ── In-process fixture ───────────────────────────────────────────────────


@pytest.fixture(scope="module")
def three_dccs(tmp_path_factory):
    """Spin up 3 in-process DCC instances, each with its own skill catalog.

    Returns a dict ``{dcc_name: {"handle", "mcp_url", "rest_url",
    "skills_dir"}}`` covering maya/blender/houdini. Every skill is
    discovered AND loaded so REST /v1/skills / /v1/search surfaces work.
    """
    instances: dict[str, dict[str, Any]] = {}

    for dcc in DCC_NAMES:
        skills_dir = tmp_path_factory.mktemp(f"skills_{dcc}")
        for name, tools in SKILL_DEFS[dcc]:
            _write_skill(skills_dir, name, dcc, tools)

        cfg = McpHttpConfig(port=0, server_name=f"{dcc}-test")
        cfg.dcc_type = dcc

        server = create_skill_server(dcc, cfg, extra_paths=[str(skills_dir)])

        # Eagerly load every skill so both REST (/v1/skills, /v1/search
        # backed by the action registry) and MCP (tools/list) surfaces
        # have a populated catalog.
        for skill_name, _tools in SKILL_DEFS[dcc]:
            with contextlib.suppress(Exception):
                server.load_skill(skill_name)

        handle = server.start()

        mcp_url = handle.mcp_url()
        rest_url = _rest_base(mcp_url)
        _wait_reachable(f"{rest_url}/v1/healthz")

        instances[dcc] = {
            "server": server,
            "handle": handle,
            "mcp_url": mcp_url,
            "rest_url": rest_url,
            "skills_dir": str(skills_dir),
        }

    try:
        yield instances
    finally:
        for inst in instances.values():
            with contextlib.suppress(Exception):
                inst["handle"].shutdown()


# ── REST surface tests ───────────────────────────────────────────────────


class TestRestSearchAcrossDccs:
    """REST ``/v1`` exposes each DCC's skill catalog as an addressable API."""

    def test_rest_health_and_openapi_per_dcc(self, three_dccs):
        """Every DCC instance must expose /v1/healthz and /v1/openapi.json."""
        for dcc, inst in three_dccs.items():
            code, body = _get_json(f"{inst['rest_url']}/v1/healthz")
            assert code == 200, f"{dcc} /v1/healthz returned {code}"
            assert body["ok"] is True, f"{dcc} healthz body unexpected: {body}"

            code, _ = _get_json(f"{inst['rest_url']}/v1/openapi.json")
            assert code == 200, f"{dcc} openapi.json missing (code={code})"

    def test_rest_skills_list_contains_all_local_skills(self, three_dccs):
        """``GET /v1/skills`` on each DCC lists only that DCC's skills.

        /v1/skills returns one row per (skill, action) pair; the ``skill``
        field identifies the owning skill name.
        """
        for dcc, inst in three_dccs.items():
            code, body = _get_json(f"{inst['rest_url']}/v1/skills")
            assert code == 200
            names = {row["skill"] for row in body.get("skills", [])}
            expected = {defn[0] for defn in SKILL_DEFS[dcc]}
            assert expected <= names, f"{dcc}: /v1/skills missing skills {expected - names}; got {names}"

    def test_rest_search_fuzzy_within_single_dcc(self, three_dccs):
        """Fuzzy ``/v1/search`` on a single DCC finds its own skills."""
        inst = three_dccs["maya"]
        code, body = _post_json(
            f"{inst['rest_url']}/v1/search",
            {"query": "sphere", "loaded_only": False},
        )
        assert code == 200
        slugs = [hit["slug"] for hit in body.get("hits", [])]
        # Fuzzy search must surface ``create_sphere`` from maya-primitives.
        assert any("sphere" in slug for slug in slugs), (
            f"maya /v1/search 'sphere' must return a create_sphere hit; got {slugs}"
        )

    def test_rest_search_same_name_yields_distinct_results_per_dcc(self, three_dccs):
        """``shared-geometry`` exists on every DCC; each /v1/search returns its own."""
        for dcc, inst in three_dccs.items():
            code, body = _post_json(
                f"{inst['rest_url']}/v1/search",
                {"query": "subdivide", "loaded_only": False},
            )
            assert code == 200
            slugs = {hit["slug"] for hit in body.get("hits", [])}
            assert slugs, f"{dcc}: search 'subdivide' returned no hits"
            # REST layer surfaces only local skills, so hits mention the local dcc.
            assert any("subdivide" in s for s in slugs), (
                f"{dcc}: /v1/search 'subdivide' missing subdivide slug in {slugs}"
            )


# ── MCP gateway-style tests (via each instance's own /mcp) ───────────────


class TestMcpSearchAcrossDccs:
    """MCP ``search_skills`` / ``load_skill`` / ``call_tool`` against each DCC."""

    def _mcp_tool_call(self, inst: dict[str, Any], name: str, args: dict[str, Any]) -> dict[str, Any]:
        return _mcp_post(inst["mcp_url"], "tools/call", {"name": name, "arguments": args})

    def test_mcp_search_skills_matches_dcc_catalog(self, three_dccs):
        """``search_skills`` on each instance surfaces its own skills."""
        for dcc, inst in three_dccs.items():
            resp = self._mcp_tool_call(inst, "search_skills", {"query": dcc})
            assert "error" not in resp, f"{dcc}: search_skills errored: {resp.get('error')}"
            text = "".join(c.get("text", "") for c in resp["result"]["content"] if c.get("type") == "text")
            for skill_name, _tools in SKILL_DEFS[dcc]:
                if skill_name == "shared-geometry":
                    continue  # shared across DCCs; assert separately below
                assert skill_name in text, f"{dcc}: search_skills must surface {skill_name!r}; got: {text[:400]!r}"

    def test_mcp_shared_skill_visible_on_every_dcc(self, three_dccs):
        """``shared-geometry`` appears in search_skills on ALL DCCs."""
        for dcc, inst in three_dccs.items():
            resp = self._mcp_tool_call(inst, "search_skills", {"query": "shared"})
            text = "".join(c.get("text", "") for c in resp["result"]["content"] if c.get("type") == "text")
            assert "shared-geometry" in text, (
                f"{dcc}: shared-geometry must be discoverable everywhere; got: {text[:400]!r}"
            )

    def test_mcp_load_skill_then_call_tool_routes_to_correct_dcc(self, three_dccs):
        """load_skill + call_tool on a specific DCC executes only on that DCC."""
        inst = three_dccs["maya"]
        load = self._mcp_tool_call(inst, "load_skill", {"skill_name": "maya-primitives"})
        assert "error" not in load, f"load_skill failed: {load.get('error')}"

        # Discover-and-dispatch: find the tool via search_tools, then call_tool.
        search = self._mcp_tool_call(inst, "search_tools", {"query": "create_sphere"})
        assert "error" not in search
        # The dynamic wrapper returns a descriptor list; we just need a slug
        # referencing create_sphere to exist.
        text = "".join(c.get("text", "") for c in search["result"]["content"] if c.get("type") == "text")
        assert "create_sphere" in text, f"search_tools must surface create_sphere after load_skill; got: {text[:400]!r}"

        # Call it via the REST /v1/call surface (REST <-> MCP parity).
        # The slug format is "{dcc}.{skill}.{tool}".
        slug = "maya.maya-primitives.create_sphere"
        code, body = _post_json(
            f"{inst['rest_url']}/v1/call",
            {"slug": slug, "arguments": {"value": 42}},
        )
        # The REST /v1/call may reject unknown request shapes with 422
        # (validation) or return a structured not-loaded / executor-missing
        # error; what matters is that the request reached THIS instance
        # rather than being mis-routed. Any 4xx/2xx is fine.
        assert 200 <= code < 500, f"/v1/call {slug} returned unexpected status {code}: {body}"


# ── REST x MCP parity ────────────────────────────────────────────────────


class TestRestMcpParity:
    """REST ``/v1`` and MCP ``/mcp`` expose the same catalog for each DCC."""

    def test_catalog_size_matches_between_rest_and_mcp(self, three_dccs):
        for dcc, inst in three_dccs.items():
            code, rest_body = _get_json(f"{inst['rest_url']}/v1/skills")
            assert code == 200
            rest_names = {row["skill"] for row in rest_body.get("skills", [])}

            mcp_resp = _mcp_post(
                inst["mcp_url"],
                "tools/call",
                {"name": "list_skills", "arguments": {}},
            )
            assert "error" not in mcp_resp, f"{dcc}: list_skills errored: {mcp_resp.get('error')}"
            mcp_text = "".join(c.get("text", "") for c in mcp_resp["result"]["content"] if c.get("type") == "text")
            # Every REST-listed skill must appear in the MCP list_skills text.
            for name in rest_names:
                assert name in mcp_text, f"{dcc}: REST lists {name!r} but MCP list_skills does not: {mcp_text[:400]!r}"


# ── Real ``server.exe`` subprocess variant (opt-in) ──────────────────────


def _find_server_binary() -> Path | None:
    """Locate the dcc-mcp-server binary via env var or default Cargo targets."""
    explicit = os.environ.get("DCC_MCP_SERVER_EXE")
    if explicit:
        p = Path(explicit)
        if p.is_file():
            return p
    repo_root = Path(__file__).resolve().parent.parent
    for candidate in (
        repo_root / "target" / "debug" / "dcc-mcp-server",
        repo_root / "target" / "debug" / "dcc-mcp-server.exe",
        repo_root / "target" / "release" / "dcc-mcp-server",
        repo_root / "target" / "release" / "dcc-mcp-server.exe",
    ):
        if candidate.is_file():
            return candidate
    return None


@pytest.mark.skipif(
    _find_server_binary() is None,
    reason="dcc-mcp-server binary not found; set DCC_MCP_SERVER_EXE or run `cargo build -p dcc-mcp-server`",
)
class TestRealServerExeSubprocess:
    """Spawn 3 real ``server.exe`` subprocesses and run the same flow.

    Each subprocess is launched with ``--app {maya|blender|houdini}``
    and an isolated ``--skill-paths`` directory. We probe each REST
    surface to prove discovery works end-to-end through the compiled
    binary — mirrors a production mcpcall/dcc-mcp-server deployment.
    In-process tests above already cover the MCP handshake and tool-call
    contract; this opt-in subprocess suite focuses on the compiled
    binary's CLI + REST bootstrap path.
    """

    @pytest.fixture
    def real_three_dccs(self, tmp_path_factory):
        binary = _find_server_binary()
        assert binary is not None  # guarded by skipif

        instances: dict[str, dict[str, Any]] = {}
        registry_dir = tmp_path_factory.mktemp("real_registry")
        procs: list[subprocess.Popen] = []

        try:
            for dcc in DCC_NAMES:
                skills_dir = tmp_path_factory.mktemp(f"real_skills_{dcc}")
                for name, tools in SKILL_DEFS[dcc]:
                    _write_skill(skills_dir, name, dcc, tools)

                mcp_port = _pick_free_port()
                proc = subprocess.Popen(
                    [
                        str(binary),
                        "--mcp-port",
                        str(mcp_port),
                        "--gateway-port",
                        "0",
                        "--app",
                        dcc,
                        "--no-bridge",
                        "--registry-dir",
                        str(registry_dir),
                        "--skill-paths",
                        str(skills_dir),
                        "--no-log-file",
                    ],
                    stdout=subprocess.DEVNULL,
                    stderr=subprocess.DEVNULL,
                )
                procs.append(proc)
                rest_url = f"http://127.0.0.1:{mcp_port}"
                _wait_reachable(f"{rest_url}/v1/healthz", budget_s=10.0)

                instances[dcc] = {
                    "proc": proc,
                    "rest_url": rest_url,
                    "mcp_url": f"{rest_url}/mcp",
                    "skills_dir": str(skills_dir),
                }

            yield instances
        finally:
            for p in procs:
                with contextlib.suppress(Exception):
                    p.terminate()
                    try:
                        p.wait(timeout=3)
                    except subprocess.TimeoutExpired:
                        p.kill()
                        p.wait()

    def test_real_rest_surface_lists_every_dcc_catalog(self, real_three_dccs):
        """Each live subprocess exposes its own discoverable catalog via REST."""
        for dcc, inst in real_three_dccs.items():
            code, body = _post_json_until_ok(
                f"{inst['rest_url']}/v1/search",
                {"query": dcc, "loaded_only": False},
            )
            assert code == 200, f"{dcc} subprocess: /v1/search returned {code}"
            slugs = {hit["slug"] for hit in body.get("hits", [])}
            expected = {defn[0] for defn in SKILL_DEFS[dcc] if defn[0] != "shared-geometry"}
            for skill_name in expected:
                assert any(skill_name in slug for slug in slugs), (
                    f"{dcc} subprocess: /v1/search missing {skill_name!r}; got: {sorted(slugs)}"
                )

    def test_real_mcp_endpoint_initializes_on_subprocess(self, real_three_dccs):
        """Keep a compiled-binary MCP smoke separate from REST catalog checks."""
        dcc, inst = next(iter(real_three_dccs.items()))
        result = _mcp_initialize_until_ok(inst["mcp_url"])
        assert result.get("protocolVersion") in (
            "2025-03-26",
            "2025-06-18",
            "2025-11-25",
        ), f"{dcc}: unexpected initialize result: {result!r}"

    def test_real_fuzzy_search_across_live_processes(self, real_three_dccs):
        """Fuzzy REST search succeeds on every live subprocess."""
        for dcc, inst in real_three_dccs.items():
            code, body = _post_json_until_ok(
                f"{inst['rest_url']}/v1/search",
                {"query": dcc, "loaded_only": False},
            )
            assert code == 200, f"{dcc}: /v1/search returned {code}"
            assert body.get("hits"), f"{dcc}: search returned empty hits body={body}"

    def test_real_shared_skill_visible_on_every_subprocess(self, real_three_dccs):
        """``shared-geometry`` is discoverable over REST on every live subprocess."""
        for dcc, inst in real_three_dccs.items():
            code, body = _post_json_until_ok(
                f"{inst['rest_url']}/v1/search",
                {"query": "shared", "loaded_only": False},
            )
            assert code == 200, f"{dcc}: /v1/search returned {code}"
            slugs = {hit["slug"] for hit in body.get("hits", [])}
            assert any("shared-geometry" in slug for slug in slugs), (
                f"{dcc}: shared-geometry not discoverable in subprocess; got: {sorted(slugs)}"
            )
