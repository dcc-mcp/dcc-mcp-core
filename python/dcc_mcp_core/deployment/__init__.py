"""Import-light Rez deployment and sidecar lifecycle API.

The implementation remains available from ``dcc_mcp_core.install_lifecycle``
for compatibility.  New code should use this ownership-oriented namespace.
"""

from __future__ import annotations

from dcc_mcp_core.deployment.install_sop import INSTALL_EXIT_ACQUIRE
from dcc_mcp_core.deployment.install_sop import INSTALL_EXIT_CODES
from dcc_mcp_core.deployment.install_sop import INSTALL_EXIT_INSTALL
from dcc_mcp_core.deployment.install_sop import INSTALL_EXIT_OK
from dcc_mcp_core.deployment.install_sop import INSTALL_EXIT_PREFLIGHT
from dcc_mcp_core.deployment.install_sop import INSTALL_EXIT_REQUIRES_RESTART
from dcc_mcp_core.deployment.install_sop import INSTALL_EXIT_VERIFY
from dcc_mcp_core.deployment.install_sop import INSTALL_SOP_SCHEMA_VERSION
from dcc_mcp_core.deployment.install_sop import load_install_sop_schema
from dcc_mcp_core.install_lifecycle import *  # noqa: F403
from dcc_mcp_core.install_lifecycle import __all__ as _LIFECYCLE_EXPORTS

__all__ = [
    *_LIFECYCLE_EXPORTS,
    "INSTALL_EXIT_ACQUIRE",
    "INSTALL_EXIT_CODES",
    "INSTALL_EXIT_INSTALL",
    "INSTALL_EXIT_OK",
    "INSTALL_EXIT_PREFLIGHT",
    "INSTALL_EXIT_REQUIRES_RESTART",
    "INSTALL_EXIT_VERIFY",
    "INSTALL_SOP_SCHEMA_VERSION",
    "load_install_sop_schema",
]
