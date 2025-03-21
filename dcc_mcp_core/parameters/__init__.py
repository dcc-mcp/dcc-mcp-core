"""Parameters package for DCC-MCP-Core.

This package contains modules related to parameter handling, including parameter validation,
conversion, and management of parameter groups and dependencies.
"""

from dcc_mcp_core.parameters.groups import ParameterGroup, ParameterDependency, with_parameter_groups
from dcc_mcp_core.parameters.validation import validate_and_convert_parameters
from dcc_mcp_core.parameters.models import with_parameter_validation, validate_function_parameters, ParameterValidationError
from dcc_mcp_core.parameters.processor import process_parameters, process_string_parameter, process_boolean_parameter

__all__ = [
    'ParameterGroup',
    'ParameterDependency',
    'with_parameter_groups',
    'validate_and_convert_parameters',
    'with_parameter_validation',
    'validate_function_parameters',
    'ParameterValidationError',
    'process_parameters',
    'process_string_parameter',
    'process_boolean_parameter',
]
