"""Import-light Rez deployment and sidecar lifecycle API.

The implementation remains available from ``dcc_mcp_core.install_lifecycle``
for compatibility.  New code should use this ownership-oriented namespace.
"""

from __future__ import annotations

import warnings

from dcc_mcp_core.deployment.core_bounds import CODE_DESCRIPTIONS
from dcc_mcp_core.deployment.core_bounds import CODE_INVERTED_RANGE
from dcc_mcp_core.deployment.core_bounds import CODE_MISSING_LOWER_BOUND
from dcc_mcp_core.deployment.core_bounds import CODE_MISSING_UPPER_BOUND
from dcc_mcp_core.deployment.core_bounds import CODE_UNPARSABLE_DECLARATION
from dcc_mcp_core.deployment.core_bounds import CODE_UNSUPPORTED_SPECIFIER
from dcc_mcp_core.deployment.core_bounds import CODE_UPPER_BOUND_TOO_WIDE
from dcc_mcp_core.deployment.core_bounds import CORE_DISTRIBUTION
from dcc_mcp_core.deployment.core_bounds import CORE_IMPORT_NAME
from dcc_mcp_core.deployment.core_bounds import MAX_MINOR_LINES
from dcc_mcp_core.deployment.core_bounds import REQUIRE_LOWER_BOUND
from dcc_mcp_core.deployment.core_bounds import REQUIRE_UPPER_BOUND
from dcc_mcp_core.deployment.core_bounds import VERDICT_CORE_NEWER_THAN_DECLARED
from dcc_mcp_core.deployment.core_bounds import VERDICT_CORE_OLDER_THAN_DECLARED
from dcc_mcp_core.deployment.core_bounds import VERDICT_DECLARATION_UNUSABLE
from dcc_mcp_core.deployment.core_bounds import VERDICT_SUPPORTED
from dcc_mcp_core.deployment.core_bounds import VERDICT_UNKNOWN_CORE_VERSION
from dcc_mcp_core.deployment.core_bounds import CoreRequirement
from dcc_mcp_core.deployment.core_bounds import CoreVersion
from dcc_mcp_core.deployment.core_bounds import check_runtime
from dcc_mcp_core.deployment.core_bounds import compare_declarations
from dcc_mcp_core.deployment.core_bounds import derive_requirement
from dcc_mcp_core.deployment.core_bounds import evaluate
from dcc_mcp_core.deployment.core_bounds import installed_core_requirement
from dcc_mcp_core.deployment.core_bounds import parse_requirement
from dcc_mcp_core.deployment.install_sop import _DEPRECATED_INSTALL_SOP_SCHEMA_VERSION
from dcc_mcp_core.deployment.install_sop import INSTALL_EXIT_ACQUIRE
from dcc_mcp_core.deployment.install_sop import INSTALL_EXIT_CODES
from dcc_mcp_core.deployment.install_sop import INSTALL_EXIT_INSTALL
from dcc_mcp_core.deployment.install_sop import INSTALL_EXIT_OK
from dcc_mcp_core.deployment.install_sop import INSTALL_EXIT_PREFLIGHT
from dcc_mcp_core.deployment.install_sop import INSTALL_EXIT_REQUIRES_RESTART
from dcc_mcp_core.deployment.install_sop import INSTALL_EXIT_VERIFY
from dcc_mcp_core.deployment.install_sop import INSTALL_SOP_SCHEMA_REVISION
from dcc_mcp_core.deployment.install_sop import install_sop_report_schema_version
from dcc_mcp_core.deployment.install_sop import load_install_sop_schema
from dcc_mcp_core.deployment.install_sop import validate_install_sop_report
from dcc_mcp_core.install_lifecycle import *  # noqa: F403
from dcc_mcp_core.install_lifecycle import __all__ as _LIFECYCLE_EXPORTS

__all__ = [
    *_LIFECYCLE_EXPORTS,
    "CODE_DESCRIPTIONS",
    "CODE_INVERTED_RANGE",
    "CODE_MISSING_LOWER_BOUND",
    "CODE_MISSING_UPPER_BOUND",
    "CODE_UNPARSABLE_DECLARATION",
    "CODE_UNSUPPORTED_SPECIFIER",
    "CODE_UPPER_BOUND_TOO_WIDE",
    "CORE_DISTRIBUTION",
    "CORE_IMPORT_NAME",
    "CoreRequirement",
    "CoreVersion",
    "INSTALL_EXIT_ACQUIRE",
    "INSTALL_EXIT_CODES",
    "INSTALL_EXIT_INSTALL",
    "INSTALL_EXIT_OK",
    "INSTALL_EXIT_PREFLIGHT",
    "INSTALL_EXIT_REQUIRES_RESTART",
    "INSTALL_EXIT_VERIFY",
    "INSTALL_SOP_SCHEMA_REVISION",
    # Deprecated alias; served by __getattr__ with a DeprecationWarning.
    "INSTALL_SOP_SCHEMA_VERSION",  # noqa: F405
    "MAX_MINOR_LINES",
    "REQUIRE_LOWER_BOUND",
    "REQUIRE_UPPER_BOUND",
    "VERDICT_CORE_NEWER_THAN_DECLARED",
    "VERDICT_CORE_OLDER_THAN_DECLARED",
    "VERDICT_DECLARATION_UNUSABLE",
    "VERDICT_SUPPORTED",
    "VERDICT_UNKNOWN_CORE_VERSION",
    "check_runtime",
    "compare_declarations",
    "derive_requirement",
    "evaluate",
    "install_sop_report_schema_version",
    "installed_core_requirement",
    "load_install_sop_schema",
    "parse_requirement",
    "validate_install_sop_report",
]


def __getattr__(name: str) -> object:
    """Serve the deprecated ``INSTALL_SOP_SCHEMA_VERSION`` alias with a warning."""
    if name == "INSTALL_SOP_SCHEMA_VERSION":
        warnings.warn(_DEPRECATED_INSTALL_SOP_SCHEMA_VERSION, DeprecationWarning, stacklevel=2)
        return INSTALL_SOP_SCHEMA_REVISION
    raise AttributeError(f"module {__name__!r} has no attribute {name!r}")
