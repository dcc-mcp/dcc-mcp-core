"""Plugin manifest generation for dcc-mcp-core (issue #410).

Claude Code Plugins bundle an MCP server URL, skill paths, and optional
sub-agents into a single JSON manifest that users can install with one click.

Reference: https://code.claude.com/docs/en/plugins-reference#plugin-components-reference

The MCP community is actively working on an extension for delivering skills
directly from servers (experimental-ext-skills):
https://github.com/modelcontextprotocol/experimental-ext-skills

This module provides:

1. :func:`build_plugin_manifest` — generate a Claude Code plugin manifest.
2. :func:`export_plugin_manifest` — write the manifest to a JSON file.
3. :class:`PluginManifest` — typed dataclass for the manifest.

Usage
-----
::

    from dcc_mcp_core.plugin_manifest import build_plugin_manifest, export_plugin_manifest

    # Via DccServerBase.plugin_manifest() (recommended)
    handle = server.start()
    manifest = server.plugin_manifest(version="1.0.0")

    # Or directly
    manifest = build_plugin_manifest(
        dcc_name="maya",
        mcp_url="http://localhost:8765/mcp",
        skill_paths=["/opt/skills/maya-geometry"],
        version="1.0.0",
    )
    export_plugin_manifest(manifest, "claude_plugin.json")
"""

from __future__ import annotations

import dataclasses
import logging
from pathlib import Path
from typing import Any

from dcc_mcp_core import json_dumps

logger = logging.getLogger(__name__)

__all__ = [
    "PluginManifest",
    "build_plugin_manifest",
    "export_plugin_manifest",
]


@dataclasses.dataclass
class PluginManifest:
    """Claude Code plugin manifest.

    Attributes:
        name: Plugin name (e.g. ``"maya-mcp"``).
        version: Plugin version string.
        description: Short description shown in the Claude Code UI.
        mcp_servers: List of MCP server entry dicts.
            Each entry has ``"url"`` and optional ``"headers"``.
        skills: List of skill directory paths included in the bundle.
        sub_agents: Optional list of sub-agent definitions.

    """

    name: str
    version: str
    description: str
    mcp_servers: list[dict[str, Any]]
    skills: list[str]
    sub_agents: list[dict[str, Any]] = dataclasses.field(default_factory=list)

    def to_dict(self) -> dict[str, Any]:
        """Return the manifest as a JSON-serialisable dict."""
        doc: dict[str, Any] = {
            "name": self.name,
            "version": self.version,
            "description": self.description,
            "mcp_servers": self.mcp_servers,
            "skills": self.skills,
        }
        if self.sub_agents:
            doc["sub_agents"] = self.sub_agents
        return doc

    def to_json(self, indent: int = 2) -> str:
        """Return the manifest as a formatted JSON string."""
        return json_dumps(self.to_dict(), indent=indent)


def build_plugin_manifest(
    dcc_name: str,
    mcp_url: str | None,
    skill_paths: list[str] | None = None,
    *,
    version: str = "0.1.0",
    description: str | None = None,
    api_key: str | None = None,
    extra_mcp_servers: list[dict[str, Any]] | None = None,
    sub_agents: list[dict[str, Any]] | None = None,
) -> dict[str, Any]:
    """Build a Claude Code plugin manifest dict.

    Args:
        dcc_name: Short DCC identifier (e.g. ``"maya"``).
        mcp_url: Full MCP endpoint URL (e.g. ``"http://localhost:8765/mcp"``).
            ``None`` is allowed but will produce a manifest without an MCP
            server entry (useful for skills-only bundles).
        skill_paths: Directories to include in the manifest's ``skills``
            list.  Paths that do not exist are filtered out with a warning.
        version: Plugin version string.
        description: Human-readable description.  Auto-generated if ``None``.
        api_key: Bearer token to include in MCP server headers.  ``None``
            omits the ``Authorization`` header.
        extra_mcp_servers: Additional MCP server entries beyond the primary.
        sub_agents: Sub-agent definitions to include in the manifest.

    Returns:
        JSON-serialisable dict.

    Example::

        manifest = build_plugin_manifest(
            dcc_name="maya",
            mcp_url="http://localhost:8765/mcp",
            skill_paths=["/opt/skills/maya-geometry"],
            version="1.0.0",
        )
        print(manifest["name"])  # "maya-mcp"

    """
    if description is None:
        description = f"MCP plugin for {dcc_name.capitalize()} — provides AI-accessible tools via dcc-mcp-core."

    mcp_servers: list[dict[str, Any]] = []
    if mcp_url:
        entry: dict[str, Any] = {"url": mcp_url}
        if api_key:
            entry["headers"] = {"Authorization": f"Bearer {api_key}"}
        mcp_servers.append(entry)

    if extra_mcp_servers:
        mcp_servers.extend(extra_mcp_servers)

    valid_skills: list[str] = []
    for path in skill_paths or []:
        p = Path(path)
        if p.exists():
            valid_skills.append(str(p))
        else:
            logger.debug("build_plugin_manifest: skill path %r does not exist — skipping", path)

    manifest = PluginManifest(
        name=f"{dcc_name}-mcp",
        version=version,
        description=description,
        mcp_servers=mcp_servers,
        skills=valid_skills,
        sub_agents=sub_agents or [],
    )

    logger.info(
        "build_plugin_manifest: generated manifest for %s — %d MCP server(s), %d skill(s)",
        dcc_name,
        len(mcp_servers),
        len(valid_skills),
    )

    return manifest.to_dict()


def export_plugin_manifest(
    manifest: dict[str, Any],
    path: str | Path,
    *,
    indent: int = 2,
) -> Path:
    """Write a plugin manifest dict to a JSON file.

    Args:
        manifest: Manifest dict from :func:`build_plugin_manifest`.
        path: Output file path.
        indent: JSON indentation.

    Returns:
        Resolved :class:`pathlib.Path` of the written file.

    Example::

        p = export_plugin_manifest(manifest, "claude_plugin.json")
        print(f"Plugin manifest written to {p}")

    """
    p = Path(path).resolve()
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_text(json_dumps(manifest, indent=indent), encoding="utf-8")
    logger.info("export_plugin_manifest: manifest written to %s", p)
    return p
