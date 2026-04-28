"""Internal collaborator classes that decompose :class:`DccServerBase` (#486).

These helpers split the responsibilities of the historical 912-line god
object into focused units that can be tested independently. They are
underscore-prefixed because they are an implementation detail; the public
contract remains :class:`dcc_mcp_core.server_base.DccServerBase`.
"""

from dcc_mcp_core._server.observability import FileLoggingManager
from dcc_mcp_core._server.observability import JobPersistenceManager
from dcc_mcp_core._server.observability import TelemetryManager
from dcc_mcp_core._server.skill_query import SkillQueryClient
from dcc_mcp_core._server.window_resolver import WindowResolver

__all__ = [
    "FileLoggingManager",
    "JobPersistenceManager",
    "SkillQueryClient",
    "TelemetryManager",
    "WindowResolver",
]
