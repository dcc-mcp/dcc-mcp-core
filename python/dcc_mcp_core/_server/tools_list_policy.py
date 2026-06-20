"""Progressive-loading stub policy for ``tools/list`` (issues #174 / #238).

Adapters share one resolver so each DCC can opt in via:

* Global: ``DCC_MCP_EXCLUDE_STUBS_FROM_TOOLS_LIST``
* Per-DCC: ``DCC_MCP_<DCC>_EXCLUDE_STUBS_FROM_TOOLS_LIST`` (e.g. ``DCC_MCP_MAYA_…``)
* Explicit :class:`ToolsListStubPolicy` on :class:`~dcc_mcp_core.McpHttpConfig`

Per-DCC env wins over global env. Discovery when stubs are hidden:
``search_skills``, ``search_tools``, ``list_skills``, capability manifests,
gateway ``/v1/search``.
"""

from __future__ import annotations

from dataclasses import dataclass
import os
import re
from typing import Any
from typing import Optional

__all__ = [
    "ENV_EXCLUDE_STUBS_FROM_TOOLS_LIST",
    "ToolsListStubPolicy",
    "apply_tools_list_stub_policy",
    "dcc_exclude_stubs_env_name",
    "env_truthy",
    "resolve_tools_list_stub_policy",
]

#: Global env var — applies to every DCC unless overridden per-DCC.
ENV_EXCLUDE_STUBS_FROM_TOOLS_LIST = "DCC_MCP_EXCLUDE_STUBS_FROM_TOOLS_LIST"


def env_truthy(name: str) -> bool:
    """Return True when *name* is set to a conventional truthy token."""
    return os.environ.get(name, "").strip().lower() in ("1", "true", "yes", "on")


def dcc_exclude_stubs_env_name(dcc_name: str) -> str:
    """Build ``DCC_MCP_<DCC>_EXCLUDE_STUBS_FROM_TOOLS_LIST`` for *dcc_name*."""
    slug = re.sub(r"[^A-Za-z0-9]+", "_", (dcc_name or "").strip()).strip("_").upper()
    if not slug:
        raise ValueError("dcc_name must be non-empty")
    return f"DCC_MCP_{slug}_EXCLUDE_STUBS_FROM_TOOLS_LIST"


@dataclass(frozen=True)
class ToolsListStubPolicy:
    """Controls which progressive-loading stubs appear in ``tools/list``."""

    exclude_skill_stubs: bool = False
    exclude_group_stubs: bool = False

    @classmethod
    def exclude_all_progressive_stubs(cls) -> ToolsListStubPolicy:
        """Hide both ``__skill__*`` and ``__group__*`` stubs."""
        return cls(exclude_skill_stubs=True, exclude_group_stubs=True)

    @classmethod
    def from_exclude_all_flag(cls, exclude: bool) -> ToolsListStubPolicy:
        """Map the documented single env knob to both stub kinds."""
        if exclude:
            return cls.exclude_all_progressive_stubs()
        return cls()

    def apply_to_config(self, config: Any) -> None:
        """Write this policy into a :class:`McpHttpConfig` instance."""
        config.exclude_skill_stubs_from_tools_list = self.exclude_skill_stubs
        config.exclude_group_stubs_from_tools_list = self.exclude_group_stubs


def resolve_tools_list_stub_policy(
    dcc_name: str,
    *,
    config: Optional[Any] = None,
    explicit: Optional[ToolsListStubPolicy] = None,
) -> ToolsListStubPolicy:
    """Resolve stub visibility for *dcc_name*.

    Precedence (highest first):

    1. *explicit* argument when provided
    2. Per-DCC env ``DCC_MCP_<DCC>_EXCLUDE_STUBS_FROM_TOOLS_LIST``
    3. Global ``DCC_MCP_EXCLUDE_STUBS_FROM_TOOLS_LIST``
    4. Values already set on *config* (if any stub flag is True)
    5. Default — include all stubs
    """
    if explicit is not None:
        return explicit

    dcc_env = dcc_exclude_stubs_env_name(dcc_name)
    if env_truthy(dcc_env):
        return ToolsListStubPolicy.exclude_all_progressive_stubs()

    if env_truthy(ENV_EXCLUDE_STUBS_FROM_TOOLS_LIST):
        return ToolsListStubPolicy.exclude_all_progressive_stubs()

    if config is not None:
        skill = bool(getattr(config, "exclude_skill_stubs_from_tools_list", False))
        group = bool(getattr(config, "exclude_group_stubs_from_tools_list", False))
        if skill or group:
            return ToolsListStubPolicy(exclude_skill_stubs=skill, exclude_group_stubs=group)

    return ToolsListStubPolicy()


def apply_tools_list_stub_policy(
    config: Any,
    dcc_name: str,
    *,
    explicit: Optional[ToolsListStubPolicy] = None,
) -> ToolsListStubPolicy:
    """Resolve and apply stub policy to *config*; return the policy used."""
    policy = resolve_tools_list_stub_policy(dcc_name, config=config, explicit=explicit)
    policy.apply_to_config(config)
    return policy
