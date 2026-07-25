"""inspect_state — Snapshot current DCC state."""
from __future__ import annotations

import json
import subprocess
from typing import Any


def _call_introspect(instance: str, code: str, timeout: int = 10) -> str:
    """Run an introspection eval and return the result string."""
    try:
        result = subprocess.run(
            ["dcc-mcp-cli", "call", f"{instance}.dcc_introspect__eval",
             "--json", json.dumps({"code": code}), "--output", "json"],
            capture_output=True, text=True, timeout=timeout,
        )
        if result.returncode == 0:
            data = json.loads(result.stdout)
            return str(data.get("result", data))
        return f"error: {result.stderr}"
    except Exception as e:
        return f"exception: {e}"


def _get_first_instance(dcc_name: str | None = None) -> str | None:
    """Get first ready instance."""
    try:
        result = subprocess.run(
            ["dcc-mcp-cli", "list", "--output", "json"],
            capture_output=True, text=True, timeout=10,
        )
        if result.returncode != 0:
            return None
        data = json.loads(result.stdout)
        for inst in data.get("instances", []):
            if dcc_name and inst.get("dcc_type") != dcc_name:
                continue
            if inst.get("direct_control", {}).get("ready"):
                return inst.get("instance_short") or inst.get("instance_id")
        if data.get("instances"):
            return data["instances"][0].get("instance_short")
    except Exception:
        pass
    return None


def inspect_state(
    dcc_name: str | None = None,
    include_scene_info: bool = True,
    include_modules: bool = False,
    include_tools: bool = True,
) -> dict[str, Any]:
    """Snapshot DCC state.

    Args:
        dcc_name: DCC to query.
        include_scene_info: Include scene details.
        include_modules: Include module list.
        include_tools: Include tool list.

    Returns:
        State snapshot.
    """
    instance = _get_first_instance(dcc_name)
    if not instance:
        return {"success": False, "error": "No ready DCC instance found."}

    state: dict[str, Any] = {"instance": instance, "dcc_type": dcc_name or "unknown"}

    if include_scene_info:
        # Try to get scene info via introspection
        scene_info = _call_introspect(instance, (
            "import sys; "
            "print('Python', sys.version.split()[0]); "
            "print('PID', __import__('os').getpid()); "
        ))
        state["scene_info"] = {"python_probe": scene_info}

    if include_modules:
        modules = _call_introspect(instance, (
            "import sys; "
            "dcc_mods = [m for m in sorted(sys.modules) if any("
            "   prefix in m.lower() for prefix in ['maya', 'bpy', 'hou', 'nuke', 'unreal', 'dcc_mcp']"
            ")]; "
            "print('\\n'.join(dcc_mods[:50]))"
        ))
        state["modules"] = modules.split("\n")[:50] if modules else []

    if include_tools:
        # Search for all tools
        try:
            result = subprocess.run(
                ["dcc-mcp-cli", "search", "--query", "", "--limit", "50", "--output", "json"],
                capture_output=True, text=True, timeout=15,
            )
            if result.returncode == 0:
                data = json.loads(result.stdout)
                tools = data.get("tools", data.get("results", []))
                state["tool_count"] = len(tools)
                state["tools"] = [
                    {"slug": t.get("slug", ""), "name": t.get("name", "")}
                    for t in tools[:50]
                ]
        except Exception:
            state["tools"] = []
            state["tool_count"] = 0

    state["success"] = True
    return state
