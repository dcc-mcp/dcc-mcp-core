"""Import-light skill path helpers used when ``_core`` is unavailable."""

from __future__ import annotations

import os
from pathlib import Path

from dcc_mcp_core._runtime.core_availability import is_core_extension_available


def get_skill_paths_from_env() -> list[str]:
    if is_core_extension_available():
        from dcc_mcp_core._core import get_skill_paths_from_env as _impl

        return list(_impl())
    raw = os.environ.get("DCC_MCP_SKILL_PATHS", "")
    if not raw.strip():
        return []
    return [part.strip() for part in raw.split(os.pathsep) if part.strip()]


def get_app_skill_paths_from_env(dcc_name: str) -> list[str]:
    if is_core_extension_available():
        from dcc_mcp_core._core import get_app_skill_paths_from_env as _impl

        return list(_impl(dcc_name))
    env_name = "DCC_MCP_{}_SKILL_PATHS".format(str(dcc_name or "").upper())
    raw = os.environ.get(env_name, "")
    if not raw.strip():
        return []
    return [part.strip() for part in raw.split(os.pathsep) if part.strip()]


def get_local_skills_dir(dcc_name: str) -> str:
    if is_core_extension_available():
        from dcc_mcp_core._core import get_local_skills_dir as _impl

        return str(_impl(dcc_name))
    slug = str(dcc_name or "dcc").strip().lower() or "dcc"
    return str(Path.home() / ".dcc-mcp" / slug / "skills")


def get_skills_dir() -> str | None:
    if is_core_extension_available():
        from dcc_mcp_core._core import get_skills_dir as _impl

        return _impl()
    default = Path.home() / ".dcc-mcp" / "skills"
    return str(default)
