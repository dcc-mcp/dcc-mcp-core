"""Parameters package for DCC-MCP-Core.

This package contains modules related to parameter handling, including parameter validation,
conversion, and management of parameter groups and dependencies.
"""

# Import local modules
from dcc_mcp_core.parameters.groups import ParameterDependency
from dcc_mcp_core.parameters.groups import ParameterGroup
from dcc_mcp_core.parameters.groups import with_parameter_groups
from dcc_mcp_core.parameters.models import ParameterValidationError
from dcc_mcp_core.parameters.models import validate_function_parameters
from dcc_mcp_core.parameters.models import with_parameter_validation
from dcc_mcp_core.parameters.processor import process_boolean_parameter
from dcc_mcp_core.parameters.processor import process_parameters
from dcc_mcp_core.parameters.processor import process_string_parameter
from dcc_mcp_core.parameters.validation import validate_and_convert_parameters

__all__ = [
    "ParameterDependency",
    "ParameterGroup",
    "ParameterValidationError",
    "process_boolean_parameter",
    "process_parameters",
    "process_string_parameter",
    "validate_and_convert_parameters",
    "validate_function_parameters",
    "with_parameter_groups",
    "with_parameter_validation",
]
