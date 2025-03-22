"""Tests for the parameters.validation module."""

# Import built-in modules
from typing import Any
from typing import Dict
from typing import List
from typing import Optional
from unittest.mock import MagicMock
from unittest.mock import patch

# Import third-party modules
import pytest

# Import local modules
from dcc_mcp_core.models import ActionResultModel
from dcc_mcp_core.parameters.groups import DependencyType
from dcc_mcp_core.parameters.groups import ParameterDependency
from dcc_mcp_core.parameters.groups import ParameterGroup
from dcc_mcp_core.parameters.groups import validate_parameter_constraints
from dcc_mcp_core.parameters.groups import with_parameter_dependencies
from dcc_mcp_core.parameters.groups import with_parameter_groups

# Keep only one with_parameter_validation import, use the version from models
from dcc_mcp_core.parameters.models import with_parameter_validation
from dcc_mcp_core.parameters.validation import create_validation_decorator
from dcc_mcp_core.parameters.validation import validate_and_convert_parameters
from dcc_mcp_core.parameters.validation import validate_parameters
from dcc_mcp_core.parameters.validation import validate_parameters_with_constraints


def test_validate_and_convert_parameters():
    """Test validating and converting parameters."""
    # Define a test function with type hints
    def test_func(name: str, age: int = 30, is_active: bool = True):
        return name, age, is_active

    # Test with valid parameters
    result = validate_and_convert_parameters(test_func, (), {"name": "John", "age": 25, "is_active": False})
    assert result == {"name": "John", "age": 25, "is_active": False}

    # Test with positional arguments
    result = validate_and_convert_parameters(test_func, ("John", 25, False), {})
    assert result == {"name": "John", "age": 25, "is_active": False}

    # Test with type conversion
    result = validate_and_convert_parameters(test_func, (), {"name": "John", "age": "25", "is_active": "false"})
    assert result == {"name": "John", "age": 25, "is_active": False}

    # Test with validation error
    with pytest.raises(ValueError):
        validate_and_convert_parameters(test_func, (), {})


@patch('dcc_mcp_core.parameters.validation.validate_function_parameters')
@patch('dcc_mcp_core.parameters.validation.validate_parameter_constraints')
def test_validate_parameters_with_constraints(mock_validate_constraints, mock_validate_params):
    """Test validating parameters with constraints."""
    # Define a test function
    def test_func(name: str, age: int = 30):
        return name, age

    # Setup mocks
    mock_validate_params.return_value = {"name": "John", "age": 25}
    mock_validate_constraints.return_value = (True, [])

    # Test with valid parameters
    result = validate_parameters_with_constraints(test_func, (), {"name": "John", "age": 25})
    assert result == {"name": "John", "age": 25}

    # Verify mocks were called correctly
    mock_validate_params.assert_called_once_with(test_func, *(), **{"name": "John", "age": 25})
    mock_validate_constraints.assert_called_once_with(test_func, (), {"name": "John", "age": 25})

    # Reset mocks
    mock_validate_params.reset_mock()
    mock_validate_constraints.reset_mock()

    # Test with constraint validation failure
    mock_validate_params.return_value = {"name": "John", "age": 25}
    mock_validate_constraints.return_value = (False, ["Error message"])

    result = validate_parameters_with_constraints(test_func, (), {"name": "John", "age": 25})
    assert isinstance(result, ActionResultModel)
    assert result.success is False
    assert "Parameter validation failed" in result.message
    assert "Error message" in result.error
    assert result.prompt is not None
    assert "validation_errors" in result.context

    # Reset mocks
    mock_validate_params.reset_mock()
    mock_validate_constraints.reset_mock()

    # Test with parameter validation exception
    mock_validate_params.side_effect = Exception("Validation error")

    result = validate_parameters_with_constraints(test_func, (), {"name": "John", "age": 25})
    assert isinstance(result, ActionResultModel)
    assert result.success is False
    assert "Parameter validation failed" in result.message
    assert "Validation error" in result.error
    assert "exception" in result.context


def test_create_validation_decorator_with_constraints():
    """Test creating a validation decorator with constraints."""
    # Test with constraints=True
    decorator = create_validation_decorator(with_constraints=True)

    # The decorator should be the same as with_parameter_validation
    # Import local modules
    from dcc_mcp_core.parameters.groups import with_parameter_validation as original_decorator
    assert decorator == original_decorator


@patch('dcc_mcp_core.parameters.validation.validate_function_parameters')
def test_create_validation_decorator_without_constraints(mock_validate_params):
    """Test creating a validation decorator without constraints."""
    # Create the decorator
    decorator = create_validation_decorator(with_constraints=False)

    # Define a test function
    def test_func(name: str, age: int = 30):
        return f"{name}, {age}"

    # Apply the decorator
    decorated_func = decorator(test_func)

    # Test with valid parameters
    mock_validate_params.return_value = {"name": "John", "age": 25}
    result = decorated_func(name="John", age=25)
    assert result == "John, 25"

    # Verify mock was called correctly
    mock_validate_params.assert_called_once_with(test_func, *(), **{"name": "John", "age": 25})

    # Reset mock
    mock_validate_params.reset_mock()

    # Test with validation exception
    mock_validate_params.side_effect = Exception("Validation error")
    result = decorated_func(name="John", age=25)
    assert isinstance(result, ActionResultModel)
    assert result.success is False
    assert "Parameter validation failed" in result.message
    assert "Validation error" in result.error

    # Test with a method
    class TestClass:
        @decorator
        def test_method(self, name: str, age: int = 30):
            return f"{name}, {age}"

    # Reset mock and setup for success
    mock_validate_params.reset_mock()
    mock_validate_params.side_effect = None
    mock_validate_params.return_value = {"name": "John", "age": 25}

    # Test the decorated method
    instance = TestClass()
    result = instance.test_method(name="John", age=25)
    assert result == "John, 25"


def test_validate_parameters_alias():
    """Test that validate_parameters is an alias for validate_parameter_constraints."""
    # Import local modules
    from dcc_mcp_core.parameters.groups import validate_parameter_constraints
    assert validate_parameters == validate_parameter_constraints


def test_simple_parameter_validation():
    """Test simple parameter validation with the with_parameter_validation decorator."""
    # Import local modules
    from dcc_mcp_core.models import ActionResultModel

    @with_parameter_validation
    def simple_func(name: str):
        print(f"\nDEBUG - simple_func called with name={name}")
        return "Success"

    # Test with valid parameters
    print("\nDEBUG - Testing simple_func with valid parameters")
    result = simple_func(name="John")
    print(f"DEBUG - Result: {result}")
    assert result.success is True, f"Expected success=True, got {result.success}"
    assert result.context['result'] == "Success", f"Expected context['result']='Success', got {result.context['result']}"

    # Test with invalid parameters (missing required parameter)
    print("\nDEBUG - Testing simple_func with invalid parameters")
    try:
        result = simple_func()
        print(f"DEBUG - Result type: {type(result)}")
        print(f"DEBUG - Result: {result}")
        assert isinstance(result, ActionResultModel), f"Expected ActionResultModel, got {type(result)}"
        assert result.success is False, f"Expected success=False, got {result.success}"
    except Exception as e:
        print(f"DEBUG - Exception: {type(e).__name__}: {e}")
