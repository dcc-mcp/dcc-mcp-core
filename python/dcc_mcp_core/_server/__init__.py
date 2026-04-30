"""Internal collaborator classes that decompose :class:`DccServerBase` (#486).

These helpers split the responsibilities of the historical 912-line god
object into focused units that can be tested independently. They are
underscore-prefixed because they are an implementation detail; the public
contract remains :class:`dcc_mcp_core.server_base.DccServerBase`.
"""

from dcc_mcp_core._server.callable_dispatcher import AdaptivePumpPolicy
from dcc_mcp_core._server.callable_dispatcher import AdaptivePumpStats
from dcc_mcp_core._server.callable_dispatcher import Affinity
from dcc_mcp_core._server.callable_dispatcher import BaseDccCallableDispatcherFull
from dcc_mcp_core._server.callable_dispatcher import BaseDccPump
from dcc_mcp_core._server.callable_dispatcher import DrainStats
from dcc_mcp_core._server.callable_dispatcher import InProcessCallableDispatcher
from dcc_mcp_core._server.callable_dispatcher import JobEntry
from dcc_mcp_core._server.callable_dispatcher import JobOutcome
from dcc_mcp_core._server.callable_dispatcher import PendingEnvelope
from dcc_mcp_core._server.callable_dispatcher import PumpStats
from dcc_mcp_core._server.callable_dispatcher import current_callable_job
from dcc_mcp_core._server.inprocess_executor import BaseDccCallableDispatcher
from dcc_mcp_core._server.inprocess_executor import DeferredToolResult
from dcc_mcp_core._server.inprocess_executor import HostExecutionBridge
from dcc_mcp_core._server.inprocess_executor import InProcessExecutionContext
from dcc_mcp_core._server.inprocess_executor import build_inprocess_executor
from dcc_mcp_core._server.inprocess_executor import exception_to_error_envelope
from dcc_mcp_core._server.inprocess_executor import run_skill_script
from dcc_mcp_core._server.minimal_mode import MinimalModeConfig
from dcc_mcp_core._server.observability import FileLoggingManager
from dcc_mcp_core._server.observability import JobPersistenceManager
from dcc_mcp_core._server.observability import TelemetryManager
from dcc_mcp_core._server.skill_query import SkillQueryClient
from dcc_mcp_core._server.window_resolver import WindowResolver

__all__ = [
    "AdaptivePumpPolicy",
    "AdaptivePumpStats",
    "Affinity",
    "BaseDccCallableDispatcher",
    "BaseDccCallableDispatcherFull",
    "BaseDccPump",
    "DeferredToolResult",
    "DrainStats",
    "FileLoggingManager",
    "HostExecutionBridge",
    "InProcessCallableDispatcher",
    "InProcessExecutionContext",
    "JobEntry",
    "JobOutcome",
    "JobPersistenceManager",
    "MinimalModeConfig",
    "PendingEnvelope",
    "PumpStats",
    "SkillQueryClient",
    "TelemetryManager",
    "WindowResolver",
    "build_inprocess_executor",
    "current_callable_job",
    "exception_to_error_envelope",
    "run_skill_script",
]
