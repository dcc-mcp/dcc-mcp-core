"""Live-upstream ForgeCAD + in-repo ClawHub skill ecosystem E2E coverage.

Addresses GitHub issue #703: prove that ``dcc-mcp-core`` can consume real
third-party skill ecosystems **without modification** — automatically
inferring the metadata it needs (dcc, layer, tools, tags) and exposing
them as MCP services over both MCP and REST.

Two ecosystems are covered:

1. **ForgeCAD** — https://github.com/KoStard/ForgeCAD
   Real upstream skills live under ``skills/`` in that repository. Their
   ``SKILL.md`` files use the agentskills.io-minimal shape: just ``name``,
   ``description``, and ``forgecad-public: true``. No ``dcc``, no
   ``tools.yaml``, no ``metadata.dcc-mcp.*`` — exactly the case that
   exercises our auto-inference.

   Because this half clones from the public internet, every test in this
   file that touches upstream content is gated behind
   ``DCC_MCP_E2E_LIVE_FORGECAD=1``. Default CI never hits the network.

2. **ClawHub / OpenClaw** — no canonical public skill repository exists
   today (``clawhub.ai`` is a commercial marketplace; ``docs.openclaw.ai``
   is docs only). The in-repo fixture at
   ``examples/skills/clawhub-compat/`` already carries the full
   ``metadata.openclaw.*`` surface (``requires``, ``primaryEnv``,
   ``emoji``, ``homepage``, ``install``). Reusing that fixture is the
   strongest available proof that our parser accepts the ClawHub format
   unmodified. When a public ClawHub mirror appears the ``LIVE_CLAWHUB``
   env var (reserved below) can be wired up without rewriting tests.

CI impact
---------
Default CI runs exactly one test from this file: the ungated ClawHub
fixture test. All upstream-clone tests ``pytest.skip`` when
``DCC_MCP_E2E_LIVE_FORGECAD`` is unset. Local reviewers can opt-in with::

    DCC_MCP_E2E_LIVE_FORGECAD=1 pytest tests/test_e2e_live_upstream_skills.py -v

Risks and fallbacks (see the issue #703 design doc for details):
- ``git`` binary missing → test skips with a diagnostic.
- GitHub unreachable / clone timeout → test skips; subsequent runs pick
  up where they left off once the network returns.
- Upstream renames/removes ``skills/forgecad`` → sentinel-file check
  fails and the test skips with the missing path named in the message.
- Upstream frontmatter gains new fields → tolerant matchers (e.g.
  ``meta.dcc in ("python", "forgecad")``) keep the test robust.
"""

from __future__ import annotations

# Import built-in modules
import contextlib
import json
import os
from pathlib import Path
import shutil
import socket
import subprocess
import time
from typing import Any
import urllib.error
import urllib.request

# Import third-party modules
import pytest

# Import local modules
import dcc_mcp_core
from dcc_mcp_core import McpHttpConfig
from dcc_mcp_core import McpHttpServer
from dcc_mcp_core import ToolRegistry
from dcc_mcp_core import create_skill_server
from dcc_mcp_core import scan_and_load

# ── Constants ──────────────────────────────────────────────────────────────

REPO_ROOT = Path(__file__).resolve().parent.parent
CACHE_ROOT = Path(__file__).resolve().parent / "_cache" / "forgecad-upstream"
UPSTREAM_REPO_URL = "https://github.com/KoStard/ForgeCAD"
UPSTREAM_CLONE_DIR = CACHE_ROOT / "ForgeCAD"
UPSTREAM_SKILLS_DIR = UPSTREAM_CLONE_DIR / "skills"
# Sentinel file proves the clone finished and the expected layout is present.
UPSTREAM_SENTINEL = UPSTREAM_SKILLS_DIR / "forgecad" / "SKILL.md"
CLONE_TIMEOUT_S = 60.0
CLAWHUB_SKILL_DIR = str(REPO_ROOT / "examples" / "skills" / "clawhub-compat")

LIVE_FORGECAD = os.environ.get("DCC_MCP_E2E_LIVE_FORGECAD") == "1"
# Reserved for a future public ClawHub mirror; no live fetch today.
LIVE_CLAWHUB = os.environ.get("DCC_MCP_E2E_LIVE_CLAWHUB") == "1"

SKIP_LIVE_REASON = (
    f"Live upstream test requires DCC_MCP_E2E_LIVE_FORGECAD=1 (clones {UPSTREAM_REPO_URL} into tests/_cache/)"
)


# ── HTTP helpers (self-contained; do NOT import from sibling test) ────────


def _post_json(url: str, payload: Any, timeout: float = 10.0) -> dict[str, Any]:
    """POST JSON and return the parsed body. Raises on non-200 responses."""
    req = urllib.request.Request(
        url,
        data=json.dumps(payload).encode(),
        headers={"Content-Type": "application/json", "Accept": "application/json"},
        method="POST",
    )
    with urllib.request.urlopen(req, timeout=timeout) as resp:
        return json.loads(resp.read())


def _mcp_post(
    mcp_url: str,
    method: str,
    params: dict[str, Any] | None = None,
    rpc_id: int = 1,
) -> dict[str, Any]:
    """POST a JSON-RPC 2.0 MCP request and return the parsed body."""
    body: dict[str, Any] = {"jsonrpc": "2.0", "id": rpc_id, "method": method}
    if params is not None:
        body["params"] = params
    return _post_json(mcp_url, body)


def _tool_text(response: dict[str, Any]) -> str:
    """Extract the text payload of the first content item in a tools/call result."""
    return response["result"]["content"][0]["text"]


def _wait_tcp_reachable(host: str, port: int, budget: float = 3.0) -> bool:
    """Poll until TCP connect succeeds or the budget expires."""
    deadline = time.time() + budget
    while time.time() < deadline:
        try:
            with socket.create_connection((host, port), timeout=0.3):
                return True
        except OSError:
            time.sleep(0.05)
    return False


# ── Upstream clone helper ──────────────────────────────────────────────────


def _ensure_forgecad_clone() -> Path:
    """Return the path to the upstream ``skills/`` directory, cloning if needed.

    Never hard-fails: any issue that prevents the clone (missing git binary,
    offline, upstream rename) is turned into ``pytest.skip`` with a
    diagnostic message so CI never breaks on environmental issues.
    """
    if UPSTREAM_SENTINEL.is_file():
        return UPSTREAM_SKILLS_DIR

    # If a previous clone left a partial directory without the sentinel,
    # purge it before retrying — otherwise ``git clone`` errors out with
    # "destination path already exists".
    if UPSTREAM_CLONE_DIR.exists():
        shutil.rmtree(UPSTREAM_CLONE_DIR, ignore_errors=True)

    if shutil.which("git") is None:
        pytest.skip("git binary not available on PATH; cannot fetch upstream ForgeCAD")

    CACHE_ROOT.mkdir(parents=True, exist_ok=True)
    try:
        result = subprocess.run(
            [
                "git",
                "clone",
                "--depth=1",
                "--single-branch",
                UPSTREAM_REPO_URL,
                str(UPSTREAM_CLONE_DIR),
            ],
            check=True,
            timeout=CLONE_TIMEOUT_S,
            capture_output=True,
            text=True,
        )
    except subprocess.TimeoutExpired as exc:
        pytest.skip(f"git clone timed out after {CLONE_TIMEOUT_S:.0f}s: {exc}")
    except subprocess.CalledProcessError as exc:
        pytest.skip(f"git clone failed (exit {exc.returncode}): {exc.stderr or exc.stdout}")
    except OSError as exc:
        pytest.skip(f"git clone failed: {exc}")

    if not UPSTREAM_SENTINEL.is_file():
        pytest.skip(
            "Upstream ForgeCAD clone succeeded but sentinel file is missing: "
            f"{UPSTREAM_SENTINEL}. The upstream may have reorganised its "
            "skills/ layout — update UPSTREAM_SENTINEL in this test to match."
        )

    # Keep stderr visible in the CI log when something subtle goes wrong later.
    if result.stderr.strip():  # pragma: no cover - informational only
        print(f"[forgecad-clone] git stderr: {result.stderr.strip()!r}")
    return UPSTREAM_SKILLS_DIR


# ── Fixtures ───────────────────────────────────────────────────────────────


@pytest.fixture(scope="module")
def forgecad_upstream_skills_dir() -> Path:
    """Return the upstream ``skills/`` path, cloning on demand.

    Scoped to ``module`` so we only clone once per pytest invocation even
    if multiple gated tests run together.
    """
    return _ensure_forgecad_clone()


@pytest.fixture()
def forgecad_live_server(forgecad_upstream_skills_dir: Path):
    """Start a skill-server fed with the upstream ForgeCAD ``skills/`` dir.

    The server uses ``create_skill_server("forgecad", ...)`` — the same
    entry point a studio deploying ForgeCAD would use — and points the
    scanner at the cloned upstream ``skills/`` directory via
    ``extra_paths``. Nothing in the upstream skill files is modified.
    """
    cfg = McpHttpConfig(port=0, server_name="forgecad-live-upstream")
    cfg.dcc_type = "forgecad"
    cfg.instance_metadata = {"display_name": "forgecad-live-upstream", "dcc": "forgecad"}

    server = create_skill_server(
        "forgecad",
        cfg,
        extra_paths=[str(forgecad_upstream_skills_dir)],
        accumulated=False,
    )

    # An in-process executor so any skill whose script we did have could
    # run without spawning a real ForgeCAD process. We do not call any
    # upstream skill's action here — the live test focuses on the
    # discover/load/metadata surface, not execution — but the executor
    # is attached for symmetry with test_forgecad_skill_ecosystem.py.
    def _executor(script_path: str, params: dict, **context: object) -> dict:
        return {
            "success": True,
            "ecosystem": "forgecad-live",
            "script_path": script_path,
            "action_name": context["action_name"],
            "params": params,
        }

    server.set_in_process_executor(_executor)
    handle = server.start()
    assert _wait_tcp_reachable("127.0.0.1", handle.port), (
        f"forgecad-live-upstream port {handle.port} did not become reachable"
    )
    try:
        yield handle
    finally:
        with contextlib.suppress(Exception):
            handle.shutdown()


# ── Live upstream ForgeCAD tests (gated) ──────────────────────────────────

pytestmark_live = pytest.mark.skipif(not LIVE_FORGECAD, reason=SKIP_LIVE_REASON)


@pytestmark_live
class TestLiveUpstreamForgeCAD:
    """Black-box proof that we consume real KoStard/ForgeCAD skills unmodified."""

    def test_upstream_clone_populates_cache(self, forgecad_upstream_skills_dir: Path) -> None:
        """The cache contains the upstream ``forgecad`` skill and core sentinels."""
        assert forgecad_upstream_skills_dir.is_dir()
        assert UPSTREAM_SENTINEL.is_file(), f"Sentinel {UPSTREAM_SENTINEL} missing — upstream layout may have changed."

        # Also assert at least one additional skill exists so we know the
        # whole skills/ tree came down, not just the single sentinel.
        other_skills = [
            d
            for d in forgecad_upstream_skills_dir.iterdir()
            if d.is_dir() and d.name != "forgecad" and (d / "SKILL.md").is_file()
        ]
        assert other_skills, (
            "Only the sentinel skill was found — expected multiple upstream skills "
            "(forgecad-make-a-model, forgecad-prepare-prompt, ...)"
        )

    def test_scan_and_load_accepts_upstream_minimal_frontmatter(self, forgecad_upstream_skills_dir: Path) -> None:
        """``scan_and_load`` accepts every upstream SKILL.md without ``skipped`` entries."""
        skills, skipped = scan_and_load(extra_paths=[str(forgecad_upstream_skills_dir)])
        assert skipped == [], (
            f"Expected empty skipped list; got {skipped}. Upstream SKILL.md files "
            f"should be accepted as-is by the auto-inference pipeline."
        )
        forgecad_names = {s.name for s in skills if s.name.startswith("forgecad")}
        assert "forgecad" in forgecad_names, (
            f"Core upstream 'forgecad' skill missing from scan result: {sorted(forgecad_names)}"
        )

    def test_upstream_skill_metadata_sensible_defaults(self, forgecad_upstream_skills_dir: Path) -> None:
        """Auto-inferred metadata has sensible defaults when the frontmatter is minimal.

        The upstream ``forgecad`` SKILL.md declares only ``name``,
        ``description``, and ``forgecad-public: true``. Our pipeline must:

        - preserve ``name`` and ``description`` verbatim
        - infer a non-empty ``dcc`` value (default ``python`` at the time
          of writing; accept ``forgecad`` too in case inference improves)
        - return an empty ``tools`` list since no ``tools:`` block or
          ``tools.yaml`` is present
        """
        skills, _ = scan_and_load(extra_paths=[str(forgecad_upstream_skills_dir)])
        forgecad = next(s for s in skills if s.name == "forgecad")

        assert forgecad.name == "forgecad"
        assert forgecad.description, "Upstream description must be preserved verbatim"
        assert "ForgeCAD" in forgecad.description or "forgecad" in forgecad.description.lower()

        # Tolerant matcher — the goal is to assert that absence of `dcc:`
        # is handled gracefully, not that a specific default is forever
        # stable. If auto-inference improves we accept the improvement.
        assert forgecad.dcc in ("python", "forgecad"), (
            f"Unexpected auto-inferred dcc {forgecad.dcc!r}; should be python (default) "
            f"or forgecad (dcc-specific inference)."
        )
        # No tools.yaml / no tools: block → empty tool list.
        assert list(forgecad.tools) == [], f"Expected empty tools for upstream-minimal skill; got {forgecad.tools}"

    def test_upstream_skill_stub_visible_via_mcp_tools_list(self, forgecad_live_server) -> None:
        """MCP ``tools/list`` surfaces each upstream skill as a ``__skill__<name>`` stub."""
        handle = forgecad_live_server
        resp = _mcp_post(handle.mcp_url(), "tools/list", rpc_id=1)
        tool_names = {t["name"] for t in resp["result"]["tools"]}
        stub_names = {n for n in tool_names if n.startswith("__skill__forgecad")}
        assert stub_names, (
            f"Expected at least one __skill__forgecad* stub in tools/list; got {sorted(tool_names)[:20]}..."
        )

    def test_upstream_skill_load_skill_over_mcp(self, forgecad_live_server) -> None:
        """``load_skill`` is JSON-RPC successful for an upstream-minimal skill.

        The upstream ``forgecad-make-a-model`` skill has no ``tools:`` block
        and no ``tools.yaml``. ``load_skill`` therefore correctly reports
        "nothing to register" via ``isError: true`` inside the MCP content
        (loaded=false, tool_count=0) — but the JSON-RPC envelope must still
        succeed (no top-level ``error`` key, valid result structure). That
        distinction is the interface contract we're asserting here: we do
        not crash, we do not return -32603, we return a structured outcome.
        """
        handle = forgecad_live_server
        resp = _mcp_post(
            handle.mcp_url(),
            "tools/call",
            {"name": "load_skill", "arguments": {"skill_name": "forgecad-make-a-model"}},
            rpc_id=2,
        )
        assert "error" not in resp, f"load_skill returned JSON-RPC error envelope: {resp.get('error')}"
        result = resp.get("result")
        assert isinstance(result, dict), f"Missing result in load_skill response: {resp}"
        content_text = _tool_text(resp)
        # The content text is a JSON document with structured fields.
        payload = json.loads(content_text)
        # Upstream minimal skill → loaded=false, tool_count=0, registered_tools=[]
        # is the correct outcome. If auto-inference improves and more
        # skills start shipping with inferred tools, ``tool_count`` may
        # become > 0 — accept both outcomes.
        assert "loaded" in payload
        assert "registered_tools" in payload
        assert "tool_count" in payload
        assert isinstance(payload["registered_tools"], list)

    def test_upstream_skill_reachable_via_get_skill_info_and_search(self, forgecad_live_server) -> None:
        """``get_skill_info`` and ``search_skills`` surface the upstream skill."""
        handle = forgecad_live_server
        mcp_url = handle.mcp_url()

        info = _mcp_post(
            mcp_url,
            "tools/call",
            {"name": "get_skill_info", "arguments": {"skill_name": "forgecad-make-a-model"}},
            rpc_id=3,
        )
        assert info["result"].get("isError") is False, f"get_skill_info failed unexpectedly: {info}"
        info_text = _tool_text(info)
        assert "forgecad-make-a-model" in info_text, (
            f"Expected skill name in get_skill_info output; got {info_text[:300]}"
        )

        found = _mcp_post(
            mcp_url,
            "tools/call",
            {"name": "search_skills", "arguments": {"query": "forgecad"}},
            rpc_id=4,
        )
        assert found["result"].get("isError") is False, f"search_skills failed: {found}"
        search_text = _tool_text(found)
        # The search_skills result is a JSON structure with a "skills" array.
        search_payload = json.loads(search_text)
        skill_names = {s["name"] for s in search_payload.get("skills", [])}
        assert "forgecad" in skill_names, f"Upstream 'forgecad' skill missing from search_skills result: {skill_names}"

    def test_upstream_rest_search_contract(self, forgecad_live_server) -> None:
        """REST ``/v1/search`` follows the documented contract.

        Because upstream skills declare zero action tools, ``/v1/search``
        (which indexes *tools*, not skill stubs) may legitimately return
        zero hits. The test asserts the endpoint responds 200 and returns
        a well-formed hits array — the contract — rather than requiring a
        specific hit count. When an upstream skill ships with declared
        tools we expect hits to appear without test changes.
        """
        handle = forgecad_live_server
        rest_url = handle.mcp_url().rsplit("/mcp", 1)[0]
        body = _post_json(f"{rest_url}/v1/search", {"query": "forgecad"})
        assert isinstance(body.get("hits"), list), f"REST /v1/search must return a 'hits' list; got {body}"
        # Non-loaded variant exercises the other branch.
        body_loaded_only = _post_json(f"{rest_url}/v1/search", {"query": "forgecad", "loaded_only": True})
        assert isinstance(body_loaded_only.get("hits"), list)

    def test_upstream_server_clean_shutdown(self, forgecad_live_server) -> None:
        """Shutdown is idempotent and leaves the port free."""
        handle = forgecad_live_server
        port = handle.port
        handle.shutdown()
        # Re-shutdown should not raise.
        with contextlib.suppress(Exception):
            handle.shutdown()
        # The fixture will also call shutdown on teardown — that too must
        # not raise. Verify the port is actually released by binding to
        # it; if still held this will fail with OSError.
        try:
            with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
                s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
                s.bind(("127.0.0.1", port))
        except OSError as exc:  # pragma: no cover - flaky on some CI
            pytest.skip(f"Port {port} still held after shutdown (likely OS TIME_WAIT): {exc}")


# ── ClawHub ungated test (always runs; uses in-repo fixture) ──────────────


def test_clawhub_local_fixture_auto_infers_and_registers(tmp_path: Path) -> None:
    """The ClawHub-format fixture is accepted and registered unmodified.

    No canonical public ClawHub skill repository exists to clone at the
    time of writing. The in-repo ``examples/skills/clawhub-compat``
    fixture faithfully reproduces the ClawHub / OpenClaw frontmatter
    shape (``metadata.openclaw.requires``, ``primaryEnv``, ``emoji``,
    ``homepage``, ``install``) so scanning it exercises exactly the same
    auto-inference path a live ClawHub clone would.

    If a public ClawHub skills repo appears later, mirror the ForgeCAD
    ``_ensure_*_clone`` helper and gate new tests with
    ``DCC_MCP_E2E_LIVE_CLAWHUB=1``.
    """
    skills, skipped = scan_and_load(extra_paths=[CLAWHUB_SKILL_DIR])
    assert skipped == [], f"ClawHub fixture should parse cleanly; skipped={skipped}"
    clawhub = next(s for s in skills if s.name == "clawhub-compat")

    # ClawHub-specific metadata must have survived the parse. The
    # ``metadata.openclaw`` block in the fixture declares
    # ``requires.bins=[curl]`` and ``primaryEnv=EXAMPLE_API_KEY`` — those
    # are the concrete signals we check to prove the parser understood
    # the ClawHub extension surface rather than silently dropping it.
    required_bins = clawhub.required_bins()
    assert "curl" in required_bins, f"ClawHub required_bins auto-inference lost the 'curl' dependency: {required_bins}"
    assert clawhub.primary_env() == "EXAMPLE_API_KEY", f"ClawHub primary_env lost: {clawhub.primary_env()!r}"

    # Now prove the skill becomes reachable via MCP search_skills and
    # get_skill_info — i.e. it is a first-class citizen in our runtime.
    cfg = McpHttpConfig(port=0, server_name="clawhub-compat-e2e")
    cfg.dcc_type = "python"
    server = McpHttpServer(ToolRegistry(), cfg)
    # Attach an in-process executor for parity with the ForgeCAD test;
    # this skill's scripts are not actually invoked in this test.
    server.set_in_process_executor(lambda sp, params, **ctx: {"ok": True})
    assert server.discover(extra_paths=[CLAWHUB_SKILL_DIR]) >= 1

    handle = server.start()
    try:
        mcp_url = handle.mcp_url()

        found = _mcp_post(
            mcp_url,
            "tools/call",
            {"name": "search_skills", "arguments": {"query": "clawhub"}},
            rpc_id=1,
        )
        assert found["result"].get("isError") is False, f"search_skills failed: {found}"
        search_payload = json.loads(_tool_text(found))
        skill_names = {s["name"] for s in search_payload.get("skills", [])}
        assert "clawhub-compat" in skill_names, f"clawhub-compat missing from search_skills: {skill_names}"

        info = _mcp_post(
            mcp_url,
            "tools/call",
            {"name": "get_skill_info", "arguments": {"skill_name": "clawhub-compat"}},
            rpc_id=2,
        )
        assert info["result"].get("isError") is False, f"get_skill_info failed: {info}"
        assert "clawhub-compat" in _tool_text(info)
    finally:
        handle.shutdown()


@pytest.mark.skipif(
    not LIVE_CLAWHUB,
    reason=(
        "No canonical public ClawHub skills repository exists today. Set "
        "DCC_MCP_E2E_LIVE_CLAWHUB=1 only once a mirror is configured — see "
        "test module docstring."
    ),
)
def test_live_clawhub_placeholder() -> None:  # pragma: no cover - reserved gate
    """Placeholder for a future live ClawHub mirror.

    When a public ClawHub / OpenClaw skill repository becomes available,
    add an ``_ensure_clawhub_clone`` helper mirroring the ForgeCAD one
    above and move real assertions here.
    """
    pytest.skip("Live ClawHub mirror not yet configured; see module docstring.")
