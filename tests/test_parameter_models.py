"""Tests for the parameters.models module."""

# Import built-in modules
from typing import Any
from typing import Dict
from typing import List
from typing import Optional

# Import third-party modules
from pydantic import Field
from pydantic import ValidationError
import pytest

# Import local modules
from dcc_mcp_core.models import ActionResultModel
from dcc_mcp_core.parameters.models import create_parameter_model_from_function
from dcc_mcp_core.parameters.models import validate_function_parameters
from dcc_mcp_core.parameters.models import with_parameter_validation
from dcc_mcp_core.utils.exceptions import ParameterValidationError


def test_create_parameter_model_from_function():
    """Test creating a Pydantic model from a function's signature."""
    # Test with a simple function
    def simple_func(name: str, age: int = 30, is_active: bool = True):
        return name, age, is_active

    model = create_parameter_model_from_function(simple_func)

    # Check model properties
    assert model.__name__ == "simple_funcParameters"
    assert hasattr(model, "__function__")
    assert model.__function__ == simple_func

    # Create an instance of the model
    params = model(name="John")
    assert params.name == "John"
    assert params.age == 30
    assert params.is_active is True

    # Test validation
    with pytest.raises(ValidationError):
        model()

    with pytest.raises(ValidationError):
        model(name=123)  # Type error

    # Test with a method
    class TestClass:
        def test_method(self, name: str, age: int = 30):
            return name, age

    instance = TestClass()
    model = create_parameter_model_from_function(instance.test_method)

    # Check that 'self' is not included in the model
    params = model(name="John")
    assert params.name == "John"
    assert params.age == 30
    assert not hasattr(params, "self")

    # Test with *args and **kwargs
    def func_with_var_args(name: str, *args: Any, **kwargs: Any):
        return name, args, kwargs

    model = create_parameter_model_from_function(func_with_var_args)
    params = model(name="John", args=[1, 2, 3], kwargs={"key": "value"})
    assert params.name == "John"
    assert params.args == [1, 2, 3]
    assert params.kwargs == {"key": "value"}


def test_validate_function_parameters():
    """Test validating and converting function parameters."""
    # Test with a simple function
    def simple_func(name: str, age: int = 30, is_active: bool = True):
        return name, age, is_active

    # Test with valid parameters
    validated = validate_function_parameters(simple_func, "John", age=25, is_active=False)
    assert validated == {"name": "John", "age": 25, "is_active": False}

    # Test with positional arguments
    validated = validate_function_parameters(simple_func, "John", 25, False)
    assert validated == {"name": "John", "age": 25, "is_active": False}

    # Test with default values
    validated = validate_function_parameters(simple_func, "John")
    assert validated == {"name": "John", "age": 30, "is_active": True}

    # Test with invalid parameters
    with pytest.raises(ParameterValidationError):
        validate_function_parameters(simple_func)

    with pytest.raises(ParameterValidationError):
        validate_function_parameters(simple_func, 123)  # Type error

    # Test with a method
    class TestClass:
        def test_method(self, name: str, age: int = 30):
            return name, age

    instance = TestClass()
    # use validate_function_parameters to verify normal return value
    validated = validate_function_parameters(instance.test_method, instance, "John", age=25)
    assert validated == {"name": "John", "age": 25}

    # verify parameter validation failure
    with pytest.raises(ParameterValidationError):
        validate_function_parameters(instance.test_method, instance, 123, age=25)  # name should be a string, but an integer was passed


def test_with_parameter_validation():
    """Test the with_parameter_validation decorator."""
    # Test with a simple function
    @with_parameter_validation
    def simple_func(name: str, age: int = 30, is_active: bool = True):
        return f"{name}, {age}, {is_active}"

    # Test with valid parameters
    result = simple_func(name="John", age=25, is_active=False)
    print(f"Result 1: {result}")
    assert result.success is True
    assert result.context['result'] == "John, 25, False"

    # Test with positional arguments
    result = simple_func("John", 25, False)
    print(f"Result 2: {result}")
    assert result.success is True
    assert result.context['result'] == "John, 25, False"

    # Test with default values
    result = simple_func("John")
    print(f"Result 3: {result}")
    assert result.success is True
    assert result.context['result'] == "John, 30, True"

    # Test with invalid parameters
    result = simple_func()
    print(f"Result 4: {result}")
    assert result.success is False
    assert "Parameter validation failed" in result.message
    assert "name" in result.error

    result = simple_func(name=123)  # Type error
    print(f"Result 5: {result}")
    assert result.success is False
    assert "Parameter validation failed" in result.message
    assert "name" in result.error

    # Test with a method
    class TestClass:
        @with_parameter_validation
        def test_method(self, name: str, age: int = 30):
            return f"{name}, {age}"

    instance = TestClass()
    result = instance.test_method("John", age=25)
    print(f"Result 6: {result}")
    assert result.success is True
    assert result.context['result'] == "John, 25"

    # Test parameter validation failure
    result = instance.test_method()
    print(f"Result 7: {result}")
    assert result.success is False
    assert "Parameter validation failed" in result.message
    assert "name" in result.error

    # Test parameter validation failure with incorrect type
    result = instance.test_method(123, age=25)  # name should be a string, but an integer was passed
    print(f"Result 8: {result}")
    assert result.success is False
    assert "Parameter validation failed" in result.message
    assert "name" in result.error

    # Test with complex types
    @with_parameter_validation
    def complex_func(names: List[str], data: Dict[str, Any], optional: Optional[int] = None):
        return len(names), len(data), optional

    result = complex_func(names=["John", "Jane"], data={"key": "value"}, optional=42)
    print(f"Result 9: {result}")
    assert result.success is True
    assert result.context['result'] == (2, 1, 42)

    result = complex_func(names="not a list", data={"key": "value"})
    print(f"Result 10: {result}")
    assert result.success is False
    assert "Parameter validation failed" in result.message
    assert "names" in result.error
