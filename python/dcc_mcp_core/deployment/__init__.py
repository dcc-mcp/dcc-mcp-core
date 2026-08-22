"""Import-light Rez deployment and sidecar lifecycle API.

The implementation remains available from ``dcc_mcp_core.install_lifecycle``
for compatibility.  New code should use this ownership-oriented namespace.
"""

from __future__ import annotations

from dcc_mcp_core.install_lifecycle import *  # noqa: F403
from dcc_mcp_core.install_lifecycle import __all__
