"""verify_capability — Check if a specific tool or capability is available.

Searches the skill catalog via dcc-mcp-cli search and reports availability.
"""
from __future__ import annotations

import json
import subprocess
from typing import Any


def _run_search(query: str, dcc_type: str | None = None, limit: int = 10, timeout: int = 15) -> dict[str, Any] | None:
    """Run dcc-mcp-cli search."""
    args = ["dcc-mcp-cli", "search", "--query", query, "--limit", str(limit)]
    if dcc_type:
        args.extend(["--dcc-type", dcc_type])
    try:
        result = subprocess.run(
            args + ["--output", "json"],
            capture_output=True,
            text=True,
            timeout=timeout,
        )
        if result.returncode != 0:
            return None
        return json.loads(result.stdout)
    except Exception:
        return None


def verify_capability(
    query: str,
    dcc_type: str | None = None,
    instance_id: str | None = None,
) -> dict[str, Any]:
    """Check if a capability/tool is available.

    Args:
        query: Capability to search for (e.g. 'create sphere', 'import fbx').
        dcc_type: Filter by DCC type.
        instance_id: Specific instance (informational only).

    Returns:
        Structured availability report.
    """
    # Try with original query
    result = _run_search(query, dcc_type=dcc_type, limit=10)
    if result is None:
        return {
            "success": False,
            "available": False,
            "tools": [],
            "recommendation": "CLI search failed. Check dcc-mcp-cli is on PATH and gateway is running.",
        }

    tools = result.get("tools", result.get("results", []))
    available = len(tools) > 0

    tool_entries = []
    for t in tools:
        tool_entries.append({
            "slug": t.get("slug", t.get("name", "")),
            "name": t.get("name", ""),
            "description": t.get("description", ""),
            "dcc_type": t.get("dcc_type", dcc_type or "unknown"),
        })

    recommendation = ""
    if not available:
        # Try broader search
        short_query = query.split()[0] if " " in query else query
        if short_query != query:
            retry = _run_search(short_query, dcc_type=dcc_type, limit=10)
            if retry and (retry.get("tools") or retry.get("results")):
                retry_tools = retry.get("tools", retry.get("results", []))
                tool_entries = [
                    {"slug": t.get("slug", t.get("name", "")),
                     "name": t.get("name", ""),
                     "description": t.get("description", ""),
                     "dcc_type": t.get("dcc_type", dcc_type or "unknown")}
                    for t in retry_tools
                ]
                available = True
                recommendation = f"No exact match for '{query}'. Broader search found {len(tool_entries)} related tool(s)."
            else:
                recommendation = f"No tools found for '{query}'. Try a different query, check loaded skills, or install the required skill from the marketplace."

    return {
        "success": True,
        "available": available,
        "tools": tool_entries[:10],
        "recommendation": recommendation,
    }
