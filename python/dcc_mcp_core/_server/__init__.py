"""Internal collaborator classes that decompose :class:`DccServerBase` (#486).

These helpers split the responsibilities of the historical 912-line god
object into focused units that can be tested independently. They are
underscore-prefixed because they are an implementation detail; the public
contract remains :class:`dcc_mcp_core.server_base.DccServerBase`.
"""

from dcc_mcp_core._server.inprocess_executor import BaseDccCallableDispatcher
from dcc_mcp_core._server.inprocess_executor import build_inprocess_executor
from dcc_mcp_core._server.inprocess_executor import run_skill_script
from dcc_mcp_core._server.minimal_mode import MinimalModeConfig
from dcc_mcp_core._server.observability import FileLoggingManager
from dcc_mcp_core._server.observability import JobPersistenceManager
from dcc_mcp_core._server.observability import TelemetryManager
from dcc_mcp_core._server.skill_query import SkillQueryClient
from dcc_mcp_core._server.window_resolver import WindowResolver

__all__ = [
    "BaseDccCallableDispatcher",
    "FileLoggingManager",
    "JobPersistenceManager",
    "MinimalModeConfig",
    "SkillQueryClient",
    "TelemetryManager",
    "WindowResolver",
    "build_inprocess_executor",
    "run_skill_script",
]
