"""End-to-end fixture for Rez-resolved context bundle skill exposure."""

from __future__ import annotations

import contextlib
import json
import os
from pathlib import Path
import shutil
import socket
import subprocess
import sys
import time
import urllib.request

import pytest

from dcc_mcp_core import McpHttpConfig
from dcc_mcp_core import McpHttpServer
from dcc_mcp_core import ToolRegistry
from dcc_mcp_core import scan_and_load_lenient

REZ_CONTEXT_CASES = [
    {
        "package": "film_animation_blocking",
        "dcc": "maya",
        "skill_env": "DCC_MCP_MAYA_SKILL_PATHS",
        "skill": "film-animation-blocking",
        "tool": "summarize_blocking",
        "task": "animation",
        "summary": "Animation blocking package active.",
    },
    {
        "package": "film_fx_cache_review",
        "dcc": "houdini",
        "skill_env": "DCC_MCP_HOUDINI_SKILL_PATHS",
        "skill": "film-fx-cache-review",
        "tool": "review_cache_manifest",
        "task": "fx",
        "summary": "FX cache review package active.",
    },
    {
        "package": "commercial_2d_layers",
        "dcc": "photoshop",
        "skill_env": "DCC_MCP_PHOTOSHOP_SKILL_PATHS",
        "skill": "commercial-2d-layers",
        "tool": "summarize_layers",
        "task": "motion-2d",
        "summary": "Layer groups are ready for motion handoff.",
    },
    {
        "package": "game_level_layout",
        "dcc": "unreal",
        "skill_env": "DCC_MCP_UNREAL_SKILL_PATHS",
        "skill": "game-level-layout",
        "tool": "summarize_level_layout",
        "task": "level-layout",
        "summary": "Game level layout package active.",
    },
    {
        "package": "asset_material_authoring",
        "dcc": "python",
        "skill_env": "DCC_MCP_SKILL_PATHS",
        "skill": "asset-material-authoring",
        "tool": "validate_material_export",
        "task": "material-authoring",
        "summary": "Material authoring package active.",
    },
]


def _pick_free_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def _post_mcp(url: str, method: str, params: dict | None = None, rpc_id: int = 1) -> dict:
    body = {"jsonrpc": "2.0", "id": rpc_id, "method": method}
    if params is not None:
        body["params"] = params
    req = urllib.request.Request(
        url,
        data=json.dumps(body).encode(),
        headers={"Content-Type": "application/json", "Accept": "application/json"},
        method="POST",
    )
    with urllib.request.urlopen(req, timeout=10) as resp:
        return json.loads(resp.read())


def _backend(
    dcc: str,
    tool_name: str,
    metadata: dict[str, str],
    registry_dir: Path,
    gateway_port: int,
) -> tuple[McpHttpServer, object]:
    registry = ToolRegistry()
    registry.register(name=tool_name, description=f"{dcc}:{tool_name}", dcc=dcc, version="1.0.0")

    cfg = McpHttpConfig(port=0, server_name=f"{dcc}-context-fixture")
    cfg.gateway_port = gateway_port
    cfg.registry_dir = str(registry_dir)
    cfg.dcc_type = dcc
    cfg.heartbeat_secs = 1
    cfg.stale_timeout_secs = 10
    cfg.instance_metadata = metadata

    server = McpHttpServer(registry, cfg)
    return server, server.start()


def _tool_suffixes(gateway_url: str) -> set[str]:
    tools = _post_mcp(gateway_url, "tools/list")["result"]["tools"]
    suffixes = set()
    for tool in tools:
        name = tool["name"]
        if "." in name and not name.startswith("__"):
            suffixes.add(name.split(".", 1)[1])
    return suffixes


def _tool_names(mcp_url: str) -> set[str]:
    return {tool["name"] for tool in _post_mcp(mcp_url, "tools/list")["result"]["tools"]}


def _write_rez_stub_packages(root: Path) -> None:
    for name in [
        "dcc_mcp_core",
        "dcc_mcp_maya",
        "dcc_mcp_houdini",
        "dcc_mcp_photoshop",
        "dcc_mcp_blender",
        "dcc_mcp_unreal",
    ]:
        package_dir = root / name
        package_dir.mkdir(parents=True, exist_ok=True)
        (package_dir / "package.py").write_text(
            f'name = "{name}"\nversion = "1.0.0"\n\n\ndef commands():\n    pass\n',
            encoding="utf-8",
        )


def _write_rez_verifier(path: Path) -> None:
    path.write_text(
        r"""
import json
import os
from pathlib import Path
import subprocess
import sys

from dcc_mcp_core import SkillCatalog
from dcc_mcp_core import ToolRegistry

expected = json.loads(os.environ["DCC_MCP_E2E_EXPECTED"])
skill_paths = [
    Path(item)
    for item in os.environ[expected["skill_env"]].split(os.pathsep)
    if item and (Path(item) / expected["skill"]).exists()
]
assert len(skill_paths) == 1, skill_paths
skill_root = skill_paths[0]

registry = ToolRegistry()
catalog = SkillCatalog(registry)
discovered = catalog.discover(extra_paths=[str(skill_root)], dcc_name=expected["dcc"])
assert discovered == 1, discovered
assert len(catalog.list_skills()) == 1
assert catalog.loaded_count() == 0
assert len(registry.list_actions()) == 0

search = catalog.search_skills(query=expected["task"], dcc=expected["dcc"], limit=5)
assert [item.name for item in search] == [expected["skill"]]

actions = catalog.load_skill(expected["skill"])
assert len(actions) == 1, actions
assert catalog.loaded_count() == 1
assert len(registry.list_actions()) == 1

script = skill_root / expected["skill"] / "scripts" / f"{expected['tool']}.py"
completed = subprocess.run(
    [sys.executable, str(script)],
    check=True,
    capture_output=True,
    text=True,
)
result = json.loads(completed.stdout)
assert result["summary"] == expected["summary"], result

removed = catalog.unload_skill(expected["skill"])
assert removed == 1
assert catalog.loaded_count() == 0
assert len(registry.list_actions()) == 0

print(json.dumps({
    "package": expected["package"],
    "discovered": discovered,
    "actions_after_load": len(actions),
    "actions_after_unload": len(registry.list_actions()),
    "summary": result["summary"],
}))
""".lstrip(),
        encoding="utf-8",
    )


def test_rez_context_bundle_fixture_exposes_distinct_instance_metadata(tmp_path: Path) -> None:
    registry_dir = tmp_path / "registry"
    gateway_port = _pick_free_port()
    backends = [
        (
            "maya",
            "summarize_blocking",
            {
                "context_bundle": "show-a.seq010.shot020.animation",
                "production_domain": "film",
                "context_kind": "shot",
                "task": "animation",
                "toolset_profile": "film-shot-animation",
                "package_provenance": "film_animation_blocking-1.0.0",
            },
        ),
        (
            "houdini",
            "review_cache_manifest",
            {
                "context_bundle": "show-a.seq010.shot020.fx",
                "production_domain": "film",
                "context_kind": "shot",
                "task": "fx",
                "toolset_profile": "film-shot-fx",
                "package_provenance": "film_fx_cache_review-1.0.0",
            },
        ),
        (
            "photoshop",
            "summarize_layers",
            {
                "context_bundle": "ad-spot.deliverable.social-16x9.motion",
                "production_domain": "advertising",
                "context_kind": "deliverable",
                "task": "motion-2d",
                "toolset_profile": "commercial-2d-motion",
                "package_provenance": "commercial_2d_layers-1.0.0",
            },
        ),
        (
            "blender",
            "summarize_level_layout",
            {
                "context_bundle": "game-demo.level.city-block.layout",
                "production_domain": "game",
                "context_kind": "level",
                "task": "level-layout",
                "toolset_profile": "game-level-layout",
                "package_provenance": "game_level_layout-1.0.0",
            },
        ),
    ]

    handles = []
    try:
        for dcc, tool_name, metadata in backends:
            handles.append(_backend(dcc, tool_name, metadata, registry_dir, gateway_port)[1])
            time.sleep(0.25)
        time.sleep(2.2)

        gateway_url = f"http://127.0.0.1:{gateway_port}/mcp"
        resp = _post_mcp(gateway_url, "tools/call", {"name": "list_dcc_instances", "arguments": {}})
        instances = json.loads(resp["result"]["content"][0]["text"])["instances"]
        by_task = {item["metadata"]["task"]: item for item in instances}

        assert by_task["animation"]["metadata"]["context_bundle"] == "show-a.seq010.shot020.animation"
        assert by_task["fx"]["metadata"]["production_domain"] == "film"
        assert by_task["motion-2d"]["metadata"]["context_kind"] == "deliverable"
        assert by_task["level-layout"]["metadata"]["toolset_profile"] == "game-level-layout"

        suffixes = _tool_suffixes(gateway_url)
        context_tools = {"summarize_blocking", "review_cache_manifest", "summarize_layers", "summarize_level_layout"}
        assert not context_tools <= suffixes
        assert "validate_material_export" not in suffixes

        selected = by_task["fx"]
        assert selected["dcc_type"] == "houdini"
        assert selected["metadata"]["package_provenance"] == "film_fx_cache_review-1.0.0"
        selected_tools = _tool_names(selected["mcp_url"])
        assert "review_cache_manifest" in selected_tools
        assert "summarize_blocking" not in selected_tools
        assert "summarize_layers" not in selected_tools
    finally:
        for handle in reversed(handles):
            with contextlib.suppress(Exception):
                handle.shutdown()


def test_rez_example_skills_are_searchable_by_context_metadata() -> None:
    examples = Path(__file__).resolve().parents[1] / "examples" / "rez-skills"
    skill_roots = [str(path / "skills") for path in examples.iterdir() if (path / "skills").exists()]

    skills, skipped = scan_and_load_lenient(extra_paths=skill_roots)
    names = {skill.name for skill in skills}

    assert skipped == []
    assert "film-animation-blocking" in names
    assert "film-fx-cache-review" in names
    assert "commercial-2d-layers" in names
    assert "game-level-layout" in names

    by_profile = {skill.metadata["dcc-mcp.toolset-profile"]: skill.name for skill in skills}
    assert by_profile["film-shot-animation"] == "film-animation-blocking"
    assert by_profile["film-shot-fx"] == "film-fx-cache-review"


@pytest.mark.parametrize("case", REZ_CONTEXT_CASES, ids=[case["package"] for case in REZ_CONTEXT_CASES])
def test_rez_env_composes_loads_executes_and_unloads_single_context(tmp_path: Path, case: dict) -> None:
    rez_env = shutil.which("rez-env")
    if rez_env is None:
        local_rez_env = Path(sys.executable).with_name("rez-env.exe")
        if local_rez_env.exists():
            rez_env = str(local_rez_env)
    if rez_env is None:
        pytest.skip("rez-env is not installed")

    repo_root = Path(__file__).resolve().parents[1]
    examples = repo_root / "examples" / "rez-skills"
    stubs = tmp_path / "rez-stubs"
    _write_rez_stub_packages(stubs)
    verifier = tmp_path / "verify_rez_context.py"
    _write_rez_verifier(verifier)

    env = os.environ.copy()
    existing_packages = env.get("REZ_PACKAGES_PATH", "")
    env["REZ_PACKAGES_PATH"] = os.pathsep.join(item for item in [str(examples), str(stubs), existing_packages] if item)
    env["DCC_MCP_E2E_EXPECTED"] = json.dumps(case)
    python_path = str(repo_root / "python")
    if env.get("PYTHONPATH"):
        python_path = os.pathsep.join([python_path, env["PYTHONPATH"]])
    env["PYTHONPATH"] = python_path

    completed = subprocess.run(
        [rez_env, case["package"], "--", sys.executable, str(verifier)],
        check=True,
        capture_output=True,
        text=True,
        env=env,
    )
    result = json.loads(completed.stdout.strip().splitlines()[-1])

    assert result["package"] == case["package"]
    assert result["discovered"] == 1
    assert result["actions_after_load"] == 1
    assert result["actions_after_unload"] == 0
    assert result["summary"] == case["summary"]
