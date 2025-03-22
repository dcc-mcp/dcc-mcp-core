"""Tests for the parameters.groups module."""

# Import built-in modules
from typing import Any
from typing import Dict
from typing import List

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
from dcc_mcp_core.parameters.groups import with_parameter_validation


def test_parameter_group_init():
    """Test initialization of ParameterGroup."""
    # Test with basic parameters
    group = ParameterGroup(
        name="test_group",
        description="Test group",
        parameters=["param1", "param2"],
        required=True,
        exclusive=True
    )

    assert group.name == "test_group"
    assert group.description == "Test group"
    assert group.parameters == ["param1", "param2"]
    assert group.required is True
    assert group.exclusive is True


def test_parameter_group_validate():
    """Test validation of parameters against group constraints."""
    # Test required group with no parameters provided
    group = ParameterGroup(
        name="required_group",
        description="Required group",
        parameters=["param1", "param2"],
        required=True
    )
    is_valid, error = group.validate({})
    assert is_valid is False
    assert "required" in error.lower()

    # Test required group with one parameter provided
    is_valid, error = group.validate({"param1": "value1"})
    assert is_valid is True
    assert error is None

    # Test exclusive group with multiple parameters provided
    group = ParameterGroup(
        name="exclusive_group",
        description="Exclusive group",
        parameters=["param1", "param2"],
        exclusive=True
    )
    is_valid, error = group.validate({"param1": "value1", "param2": "value2"})
    assert is_valid is False
    assert "only one parameter" in error.lower()

    # Test exclusive group with one parameter provided
    is_valid, error = group.validate({"param1": "value1"})
    assert is_valid is True
    assert error is None

    # Test non-required, non-exclusive group
    group = ParameterGroup(
        name="optional_group",
        description="Optional group",
        parameters=["param1", "param2"]
    )
    is_valid, error = group.validate({})
    assert is_valid is True
    assert error is None


def test_parameter_group_to_dict():
    """Test conversion of ParameterGroup to dictionary."""
    group = ParameterGroup(
        name="test_group",
        description="Test group",
        parameters=["param1", "param2"],
        required=True,
        exclusive=True
    )

    group_dict = group.to_dict()
    assert group_dict["name"] == "test_group"
    assert group_dict["description"] == "Test group"
    assert group_dict["parameters"] == ["param1", "param2"]
    assert group_dict["required"] is True
    assert group_dict["exclusive"] is True


def test_parameter_dependency_init():
    """Test initialization of ParameterDependency."""
    # Test with string depends_on
    dependency = ParameterDependency(
        parameter="param1",
        depends_on="param2",
        dependency_type=DependencyType.REQUIRES,
        error_message="Custom error"
    )

    assert dependency.parameter == "param1"
    assert dependency.depends_on == ["param2"]
    assert dependency.dependency_type == DependencyType.REQUIRES
    assert dependency.error_message == "Custom error"

    # Test with list depends_on
    dependency = ParameterDependency(
        parameter="param1",
        depends_on=["param2", "param3"],
        dependency_type=DependencyType.CONFLICTS
    )

    assert dependency.parameter == "param1"
    assert dependency.depends_on == ["param2", "param3"]
    assert dependency.dependency_type == DependencyType.CONFLICTS
    assert dependency.error_message is None


def test_parameter_dependency_validate_requires():
    """Test validation of 'requires' dependency."""
    dependency = ParameterDependency(
        parameter="param1",
        depends_on=["param2", "param3"],
        dependency_type=DependencyType.REQUIRES
    )

    # Test when parameter is not provided
    is_valid, error = dependency.validate({})
    assert is_valid is True
    assert error is None

    # Test when parameter is provided but dependencies are missing
    is_valid, error = dependency.validate({"param1": "value1"})
    assert is_valid is False
    assert "requires" in error.lower()

    # Test when parameter and some dependencies are provided
    is_valid, error = dependency.validate({"param1": "value1", "param2": "value2"})
    assert is_valid is False
    assert "param3" in error

    # Test when parameter and all dependencies are provided
    is_valid, error = dependency.validate({"param1": "value1", "param2": "value2", "param3": "value3"})
    assert is_valid is True
    assert error is None

    # Test with custom error message
    dependency = ParameterDependency(
        parameter="param1",
        depends_on="param2",
        dependency_type=DependencyType.REQUIRES,
        error_message="Custom error message"
    )
    is_valid, error = dependency.validate({"param1": "value1"})
    assert is_valid is False
    assert error == "Custom error message"


def test_parameter_dependency_validate_conflicts():
    """Test validation of 'conflicts' dependency."""
    dependency = ParameterDependency(
        parameter="param1",
        depends_on=["param2", "param3"],
        dependency_type=DependencyType.CONFLICTS
    )

    # Test when parameter is not provided
    is_valid, error = dependency.validate({})
    assert is_valid is True
    assert error is None

    # Test when parameter is provided and no conflicts are provided
    is_valid, error = dependency.validate({"param1": "value1"})
    assert is_valid is True
    assert error is None

    # Test when parameter and some conflicts are provided
    is_valid, error = dependency.validate({"param1": "value1", "param2": "value2"})
    assert is_valid is False
    assert "conflicts" in error.lower()

    # Test with custom error message
    dependency = ParameterDependency(
        parameter="param1",
        depends_on="param2",
        dependency_type=DependencyType.CONFLICTS,
        error_message="Custom error message"
    )
    is_valid, error = dependency.validate({"param1": "value1", "param2": "value2"})
    assert is_valid is False
    assert error == "Custom error message"


def test_parameter_dependency_validate_one_of():
    """Test validation of 'one_of' dependency."""
    dependency = ParameterDependency(
        parameter="param1",
        depends_on=["param2", "param3"],
        dependency_type=DependencyType.ONE_OF
    )

    # Test when no parameters are provided
    is_valid, error = dependency.validate({})
    assert is_valid is False
    assert "exactly one" in error.lower()

    # Test when one parameter is provided
    is_valid, error = dependency.validate({"param1": "value1"})
    assert is_valid is True
    assert error is None

    # Test when multiple parameters are provided
    is_valid, error = dependency.validate({"param1": "value1", "param2": "value2"})
    assert is_valid is False
    assert "exactly one" in error.lower()


def test_parameter_dependency_validate_at_least_one():
    """Test validation of 'at_least_one' dependency."""
    dependency = ParameterDependency(
        parameter="param1",
        depends_on=["param2", "param3"],
        dependency_type=DependencyType.AT_LEAST_ONE
    )

    # Test when no parameters are provided
    is_valid, error = dependency.validate({})
    assert is_valid is False
    assert "at least one" in error.lower()

    # Test when one parameter is provided
    is_valid, error = dependency.validate({"param1": "value1"})
    assert is_valid is True
    assert error is None

    # Test when multiple parameters are provided
    is_valid, error = dependency.validate({"param1": "value1", "param2": "value2"})
    assert is_valid is True
    assert error is None


def test_parameter_dependency_validate_mutually_exclusive():
    """Test validation of 'mutually_exclusive' dependency."""
    dependency = ParameterDependency(
        parameter="param1",
        depends_on=["param2", "param3"],
        dependency_type=DependencyType.MUTUALLY_EXCLUSIVE
    )

    # Test when no parameters are provided
    is_valid, error = dependency.validate({})
    assert is_valid is True
    assert error is None

    # Test when one parameter is provided
    is_valid, error = dependency.validate({"param1": "value1"})
    assert is_valid is True
    assert error is None

    # Test when multiple parameters are provided
    is_valid, error = dependency.validate({"param1": "value1", "param2": "value2"})
    assert is_valid is False
    assert "mutually exclusive" in error.lower()


def test_parameter_dependency_to_dict():
    """Test conversion of ParameterDependency to dictionary."""
    dependency = ParameterDependency(
        parameter="param1",
        depends_on=["param2", "param3"],
        dependency_type=DependencyType.REQUIRES,
        error_message="Custom error"
    )

    dependency_dict = dependency.to_dict()
    assert dependency_dict["parameter"] == "param1"
    assert dependency_dict["depends_on"] == ["param2", "param3"]
    assert dependency_dict["dependency_type"] == "requires"
    assert dependency_dict["error_message"] == "Custom error"


def test_with_parameter_groups():
    """Test with_parameter_groups decorator."""
    group = ParameterGroup(
        name="test_group",
        description="Test group",
        parameters=["param1", "param2"]
    )

    @with_parameter_groups(group)
    def test_func(param1=None, param2=None):
        return param1, param2

    assert hasattr(test_func, "__parameter_groups__")
    assert test_func.__parameter_groups__[0] == group


def test_with_parameter_dependencies():
    """Test with_parameter_dependencies decorator."""
    dependency = ParameterDependency(
        parameter="param1",
        depends_on="param2",
        dependency_type=DependencyType.REQUIRES
    )

    @with_parameter_dependencies(dependency)
    def test_func(param1=None, param2=None):
        return param1, param2

    assert hasattr(test_func, "__parameter_dependencies__")
    assert test_func.__parameter_dependencies__[0] == dependency


def test_validate_parameter_constraints():
    """Test validate_parameter_constraints function."""
    # Test function with no constraints
    def func_no_constraints(param1=None, param2=None):
        return param1, param2

    is_valid, errors = validate_parameter_constraints(func_no_constraints, (), {"param1": "value1"})
    assert is_valid is True
    assert errors == []

    # Test function with parameter groups
    group = ParameterGroup(
        name="exclusive_group",
        description="Exclusive group",
        parameters=["param1", "param2"],
        exclusive=True
    )

    @with_parameter_groups(group)
    def func_with_groups(param1=None, param2=None):
        return param1, param2

    # Test valid parameters
    is_valid, errors = validate_parameter_constraints(func_with_groups, (), {"param1": "value1"})
    assert is_valid is True
    assert errors == []

    # Test invalid parameters
    is_valid, errors = validate_parameter_constraints(func_with_groups, (), {"param1": "value1", "param2": "value2"})
    assert is_valid is False
    assert len(errors) == 1
    assert "only one parameter" in errors[0].lower()

    # Test function with parameter dependencies
    dependency = ParameterDependency(
        parameter="param1",
        depends_on="param2",
        dependency_type=DependencyType.REQUIRES
    )

    @with_parameter_dependencies(dependency)
    def func_with_dependencies(param1=None, param2=None):
        return param1, param2

    # Test valid parameters
    is_valid, errors = validate_parameter_constraints(func_with_dependencies, (), {"param1": "value1", "param2": "value2"})
    assert is_valid is True
    assert errors == []

    # Test invalid parameters
    is_valid, errors = validate_parameter_constraints(func_with_dependencies, (), {"param1": "value1"})
    assert is_valid is False
    assert len(errors) == 1
    assert "requires" in errors[0].lower()


def test_with_parameter_validation():
    """Test with_parameter_validation decorator."""
    # Test function with parameter groups
    group = ParameterGroup(
        name="exclusive_group",
        description="Exclusive group",
        parameters=["param1", "param2"],
        exclusive=True
    )

    @with_parameter_validation
    @with_parameter_groups(group)
    def func_with_groups(param1=None, param2=None):
        return "Success"

    # Test valid parameters
    result = func_with_groups(param1="value1")
    assert result == "Success"

    # Test invalid parameters
    result = func_with_groups(param1="value1", param2="value2")
    assert isinstance(result, ActionResultModel)
    assert result.success is False
    assert "Parameter validation failed" in result.message
    assert "only one parameter" in result.error.lower()

    # Test function with parameter dependencies
    dependency = ParameterDependency(
        parameter="param1",
        depends_on="param2",
        dependency_type=DependencyType.REQUIRES
    )

    @with_parameter_validation
    @with_parameter_dependencies(dependency)
    def func_with_dependencies(param1=None, param2=None):
        return "Success"

    # Test valid parameters
    result = func_with_dependencies(param1="value1", param2="value2")
    assert result == "Success"

    # Test invalid parameters
    result = func_with_dependencies(param1="value1")
    assert isinstance(result, ActionResultModel)
    assert result.success is False
    assert "Parameter validation failed" in result.message
    assert "requires" in result.error.lower()
