"""Resolve server-owned identity for canonical Finding v1 reports."""

from __future__ import annotations

import sys
from typing import Any
from typing import Optional

from dcc_mcp_core._version_util import package_version
from dcc_mcp_core.schemas.finding import FindingRuntimeContext


def finding_context_for_server(
    server: Any,
    *,
    core_version: Optional[str] = None,  # noqa: UP045 - imported by Python 3.7 hosts
) -> FindingRuntimeContext:
    """Build the shared runtime identity used by feedback and startup capture."""
    options = server._options
    adapter = options.server_name or options.sidecar.display_name or f"dcc-mcp-{server._dcc_name}"
    adapter_version = options.sidecar.adapter_version or options.server_version or "unknown"
    owning_repo = f"dcc-mcp/{adapter}" if adapter.startswith("dcc-mcp-") else adapter
    return FindingRuntimeContext(
        dcc_type=server._dcc_name,
        adapter=adapter,
        adapter_version=adapter_version,
        core_version=core_version or package_version(fallback="unknown", load_core=True),
        host_version=str(server._version_string() or "unknown"),
        os=sys.platform,
        owning_repo=owning_repo,
    )


__all__ = ["finding_context_for_server"]
