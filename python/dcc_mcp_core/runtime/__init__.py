"""Public runtime configuration and lightweight fallback contracts."""

from __future__ import annotations

from dcc_mcp_core._lite_fallback import DccCapabilities
from dcc_mcp_core._lite_fallback import GuiExecutableHint
from dcc_mcp_core._lite_fallback import PyPumpedDispatcher
from dcc_mcp_core._lite_fallback import ReadinessProbe
from dcc_mcp_core._lite_fallback import correct_python_executable
from dcc_mcp_core._lite_fallback import is_gui_executable
from dcc_mcp_core._lite_fallback import parse_skill_md
from dcc_mcp_core._lite_fallback import scan_and_load_strict
from dcc_mcp_core._runtime.config_bridge import resolve_mcp_http_config_class

McpHttpConfig = resolve_mcp_http_config_class()

__all__ = [
    "DccCapabilities",
    "GuiExecutableHint",
    "McpHttpConfig",
    "PyPumpedDispatcher",
    "ReadinessProbe",
    "correct_python_executable",
    "is_gui_executable",
    "parse_skill_md",
    "scan_and_load_strict",
]
