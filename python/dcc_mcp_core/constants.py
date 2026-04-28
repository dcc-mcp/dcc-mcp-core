"""Shared string constants used by Python tool handlers (#487).

Centralises every ``"dcc-mcp"`` / ``"dcc-mcp.<feature>"`` metadata key,
SKILL layer name, and tool category that previously appeared as inline
string literals scattered across ``recipes.py``, ``feedback.py``,
``introspect.py``, ``docs_resources.py``, ``workflow_yaml.py``, and
``dcc_server.py``.

Renaming a key now means editing one line here instead of grepping the
whole codebase.
"""

from __future__ import annotations

# ── SKILL.md metadata keys (per agentskills.io spec) ────────────────────────
# All extension keys must live under ``metadata.dcc-mcp.<feature>``.
METADATA_DCC_MCP: str = "dcc-mcp"
METADATA_RECIPES_KEY: str = f"{METADATA_DCC_MCP}.recipes"
METADATA_WORKFLOWS_KEY: str = f"{METADATA_DCC_MCP}.workflows"
METADATA_LAYER_KEY: str = f"{METADATA_DCC_MCP}.layer"
METADATA_DCC_KEY: str = f"{METADATA_DCC_MCP}.dcc"
METADATA_VERSION_KEY: str = f"{METADATA_DCC_MCP}.version"

# ── SKILL layer taxonomy ────────────────────────────────────────────────────
# A skill's ``metadata.dcc-mcp.layer`` tag declares its architectural role.
LAYER_THIN_HARNESS: str = "thin-harness"
LAYER_DOMAIN: str = "domain"
LAYER_INFRASTRUCTURE: str = "infrastructure"
LAYER_EXAMPLE: str = "example"

# ── Tool categories ─────────────────────────────────────────────────────────
# Used as the ``category`` field on ``ToolRegistry.register(...)``.
CATEGORY_DIAGNOSTICS: str = "diagnostics"
CATEGORY_FEEDBACK: str = "feedback"
CATEGORY_INTROSPECT: str = "introspect"
CATEGORY_RECIPES: str = "recipes"
CATEGORY_WORKFLOWS: str = "workflows"
CATEGORY_DOCS: str = "docs"
CATEGORY_GENERAL: str = "general"

__all__ = [
    "CATEGORY_DIAGNOSTICS",
    "CATEGORY_DOCS",
    "CATEGORY_FEEDBACK",
    "CATEGORY_GENERAL",
    "CATEGORY_INTROSPECT",
    "CATEGORY_RECIPES",
    "CATEGORY_WORKFLOWS",
    "LAYER_DOMAIN",
    "LAYER_EXAMPLE",
    "LAYER_INFRASTRUCTURE",
    "LAYER_THIN_HARNESS",
    "METADATA_DCC_KEY",
    "METADATA_DCC_MCP",
    "METADATA_LAYER_KEY",
    "METADATA_RECIPES_KEY",
    "METADATA_VERSION_KEY",
    "METADATA_WORKFLOWS_KEY",
]
