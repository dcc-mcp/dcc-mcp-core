"""Version-compatibility contracts between core and its adapters.

Owns the rule that an adapter's supported core range is declared once — in its
Python distribution metadata — and that every other declaration of the same
range, such as the package-environment requirement a studio resolver reads, is
derived from it. See :mod:`dcc_mcp_core.version_compat.core_requirement`.
"""

from __future__ import annotations

# Import local modules
from dcc_mcp_core.version_compat.core_requirement import CORE_IMPORT_NAME
from dcc_mcp_core.version_compat.core_requirement import CORE_PACKAGE
from dcc_mcp_core.version_compat.core_requirement import ENV_ENFORCE
from dcc_mcp_core.version_compat.core_requirement import CoreRequirementError
from dcc_mcp_core.version_compat.core_requirement import Requirement
from dcc_mcp_core.version_compat.core_requirement import check_requirement
from dcc_mcp_core.version_compat.core_requirement import compare_requirements
from dcc_mcp_core.version_compat.core_requirement import compare_versions
from dcc_mcp_core.version_compat.core_requirement import distribution_requires
from dcc_mcp_core.version_compat.core_requirement import enforce_core_compatibility
from dcc_mcp_core.version_compat.core_requirement import open_ceiling
from dcc_mcp_core.version_compat.core_requirement import package_environment_name
from dcc_mcp_core.version_compat.core_requirement import package_environment_requirement_for
from dcc_mcp_core.version_compat.core_requirement import requirement_for_distribution
from dcc_mcp_core.version_compat.core_requirement import requirement_from_package_environment
from dcc_mcp_core.version_compat.core_requirement import requirement_from_pep440
from dcc_mcp_core.version_compat.core_requirement import requirement_from_text
from dcc_mcp_core.version_compat.core_requirement import runtime_core_version

__all__ = [
    "CORE_IMPORT_NAME",
    "CORE_PACKAGE",
    "ENV_ENFORCE",
    "CoreRequirementError",
    "Requirement",
    "check_requirement",
    "compare_requirements",
    "compare_versions",
    "distribution_requires",
    "enforce_core_compatibility",
    "open_ceiling",
    "package_environment_name",
    "package_environment_requirement_for",
    "requirement_for_distribution",
    "requirement_from_package_environment",
    "requirement_from_pep440",
    "requirement_from_text",
    "runtime_core_version",
]
