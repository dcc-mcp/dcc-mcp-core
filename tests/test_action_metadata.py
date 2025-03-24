"""Tests for action metadata extraction.

This module contains tests for the action metadata extraction functionality,
including loading test actions and verifying the extracted metadata.
"""

# Import built-in modules
import os
import sys
import types
from typing import Any
from typing import Callable
from typing import ClassVar
from typing import Dict
from typing import List

# Import third-party modules
from pyfakefs import fake_filesystem
import pytest

# Import local modules
from dcc_mcp_core.actions.metadata import create_action_model
from dcc_mcp_core.actions.metadata import create_actions_info_model
from dcc_mcp_core.actions.metadata import extract_action_metadata
from dcc_mcp_core.models import ActionModel
from dcc_mcp_core.models import ActionResultModel
from dcc_mcp_core.models import FunctionModel
from dcc_mcp_core.models import ParameterModel
from dcc_mcp_core.utils.constants import ACTION_METADATA


def test_basic_action_metadata(action_manager, test_data_dir):
    """Test metadata extraction from a basic action with complete metadata."""
    # Load the basic action
    action_path = os.path.join(test_data_dir, "basic_plugin.py")
    action_result = action_manager.load_action(action_path)

    assert action_result.success is True
    action_module = action_result.context["module"]
    assert action_module is not None

    # Get action info
    action_result = action_manager.get_action_info("basic_plugin")

    # Verify the returned object is an ActionResultModel
    assert isinstance(action_result, ActionResultModel)
    assert action_result.success is True

    # Get the actual ActionModel from the context
    action_info = action_result.context["result"]
    assert isinstance(action_info, ActionModel)

    # Verify metadata
    assert action_info.name == "basic_plugin"
    assert action_info.version == "1.0.0"
    assert action_info.description == "A basic test plugin with complete metadata"
    assert action_info.author == "Test Author"
    assert action_info.requires == ["dependency1", "dependency2"]

    # Verify functions
    assert "hello_world" in action_info.functions
    assert "add_numbers" in action_info.functions
    assert "process_data" in action_info.functions

    # Verify function details
    hello_func = action_info.functions["hello_world"]
    assert hello_func.return_type in ["<class 'str'>", "str"]
    assert len(hello_func.parameters) == 0

    add_func = action_info.functions["add_numbers"]
    assert len(add_func.parameters) == 2

    process_func = action_info.functions["process_data"]
    assert len(process_func.parameters) == 2
    assert process_func.parameters[0].name == "data"
    assert process_func.parameters[0].required is True
    assert process_func.parameters[1].name == "verbose"
    assert process_func.parameters[1].required is False
    assert process_func.parameters[1].default is False


def test_minimal_action_metadata(action_manager, test_data_dir):
    """Test metadata extraction from a minimal action with only required fields."""
    # Load the minimal action
    action_path = os.path.join(test_data_dir, "minimal_plugin.py")
    action_module = action_manager.load_action(action_path)

    assert action_module is not None

    # Get action info
    action_result = action_manager.get_action_info("minimal_plugin")

    # Verify the returned object is an ActionResultModel
    assert isinstance(action_result, ActionResultModel)
    assert action_result.success is True

    # Get the actual ActionModel from the context
    action_info = action_result.context["result"]
    assert isinstance(action_info, ActionModel)

    # Verify metadata (with defaults)
    assert action_info.name == "minimal_plugin"
    assert action_info.version == ACTION_METADATA["version"]["default"]
    assert action_info.description == ACTION_METADATA["description"]["default"]
    assert action_info.author == ACTION_METADATA["author"]["default"]
    assert action_info.requires == ACTION_METADATA["requires"]["default"]

    # Verify functions
    assert "minimal_function" in action_info.functions

    # Verify function details
    min_func = action_info.functions["minimal_function"]
    assert len(min_func.parameters) == 0


def test_maya_action_metadata(action_manager, test_data_dir):
    """Test metadata extraction from a Maya action with context parameter."""
    # Load the Maya action
    action_path = os.path.join(test_data_dir, "maya_plugin.py")
    action_module = action_manager.load_action(action_path)

    assert action_module is not None

    # Get action info
    action_result = action_manager.get_action_info("maya_plugin")

    # Verify the returned object is an ActionResultModel
    assert isinstance(action_result, ActionResultModel)
    assert action_result.success is True

    # Get the actual ActionModel from the context
    action_info = action_result.context["result"]
    assert isinstance(action_info, ActionModel)

    # Verify metadata
    assert action_info.name == "maya_plugin"
    assert action_info.requires == ["maya"]

    # Verify functions
    assert "create_cube" in action_info.functions
    assert "list_objects" in action_info.functions

    # Verify function details with context parameter
    create_func = action_info.functions["create_cube"]
    assert len(create_func.parameters) == 2
    assert create_func.parameters[0].name == "size"
    assert create_func.parameters[0].default == 1.0
    assert create_func.parameters[1].name == "context"
    assert create_func.parameters[1].required is False


def test_advanced_types_action_metadata(action_manager, test_data_dir):
    """Test metadata extraction from an action with advanced type annotations."""
    # Load the advanced types action
    action_path = os.path.join(test_data_dir, "advanced_types_plugin.py")
    action_module = action_manager.load_action(action_path)

    assert action_module is not None

    # Get action info
    action_result = action_manager.get_action_info("advanced_types_plugin")

    # Verify the returned object is an ActionResultModel
    assert isinstance(action_result, ActionResultModel)
    assert action_result.success is True

    # Get the actual ActionModel from the context
    action_info = action_result.context["result"]
    assert isinstance(action_info, ActionModel)

    # Verify functions
    assert "process_complex_data" in action_info.functions
    assert "async_operation" in action_info.functions

    # Verify complex function details
    complex_func = action_info.functions["process_complex_data"]
    assert len(complex_func.parameters) == 3

    # Check return type (should contain "Tuple" or "tuple")
    assert "Tuple" in complex_func.return_type or "tuple" in complex_func.return_type.lower()


def test_internal_helpers_action_metadata(action_manager, test_data_dir):
    """Test that internal helper functions are not auto-registered."""
    # Load the internal helpers action
    action_path = os.path.join(test_data_dir, "internal_helpers_plugin.py")
    action_module = action_manager.load_action(action_path)

    assert action_module is not None

    # Get action info
    action_result = action_manager.get_action_info("internal_helpers_plugin")

    # Verify the returned object is an ActionResultModel
    assert isinstance(action_result, ActionResultModel)
    assert action_result.success is True

    # Get the actual ActionModel from the context
    action_info = action_result.context["result"]
    assert isinstance(action_info, ActionModel)

    # Verify only public functions are registered
    assert "get_calculated_value" in action_info.functions
    assert "get_formatted_calculation" in action_info.functions

    # Verify internal helpers are NOT registered
    assert "_calculate_value" not in action_info.functions
    assert "_format_result" not in action_info.functions


def test_extract_action_metadata_function(test_data_dir):
    """Test the extract_action_metadata utility function directly."""
    # Create a mock module with metadata
    # Import built-in modules

    class MockModule:
        __action_name__ = "mock_plugin"
        __action_version__ = "2.0.0"
        __action_description__ = "A mock plugin for testing"
        __action_author__ = "Mock Author"
        __action_requires__: ClassVar[List[str]] = ["mock_dependency"]

    # Extract metadata
    metadata = extract_action_metadata(MockModule)

    # Verify metadata
    assert metadata["name"] == "mock_plugin"
    assert metadata["version"] == "2.0.0"
    assert metadata["description"] == "A mock plugin for testing"
    assert metadata["author"] == "Mock Author"
    assert metadata["requires"] == ["mock_dependency"]

    # Test with default values when attributes are missing
    class MockModuleMinimal:
        pass

    # Extract metadata with defaults
    metadata = extract_action_metadata(MockModuleMinimal)

    # Verify default metadata
    assert metadata["name"] == ""
    assert metadata["version"] == "0.1.0"
    assert metadata["description"] == "No description provided."
    assert metadata["author"] == "mcp"
    assert metadata["requires"] == []


def test_create_action_model():
    """Test the create_action_model function."""

    # Create mock functions dictionary with a test function
    def test_function(param1: str) -> str:
        """A test function.

        Args:
            param1: First parameter

        Returns:
            A string result

        """
        return param1

    functions: Dict[str, Callable] = {"test_function": test_function}

    # Create a dummy module with metadata
    test_module = types.ModuleType("test_module")
    test_module.__action_name__ = "test_action"
    test_module.__action_version__ = "1.0.0"
    test_module.__action_description__ = "Test action description"
    test_module.__action_author__ = "Test Author"
    test_module.__file__ = "/path/to/test_action.py"

    # Create action model
    action_model = create_action_model(
        action_name="test_action", action_module=test_module, action_functions=functions, dcc_name="test_dcc"
    )

    # Verify action model
    assert isinstance(action_model, ActionModel)
    assert action_model.name == "test_action"
    assert action_model.version == "1.0.0"
    assert action_model.description == "Test action description"
    assert action_model.author == "Test Author"
    assert action_model.file_path == "/path/to/test_action.py"
    assert action_model.dcc == "test_dcc"

    # Verify functions
    assert "test_function" in action_model.functions
    test_func = action_model.functions["test_function"]
    assert isinstance(test_func, FunctionModel)
    assert test_func.name == "test_function"
    assert "A test function" in test_func.description
    assert test_func.return_type == "<class 'str'>" or test_func.return_type == "str"

    # Verify parameters
    assert len(test_func.parameters) == 1
    param = test_func.parameters[0]
    assert isinstance(param, ParameterModel)
    assert param.name == "param1"
    assert param.type_hint == "<class 'str'>" or param.type_hint == "str"
    assert param.type == "positional_or_keyword"
    assert param.required is True
    assert param.default is None
    assert "First parameter" in param.description


def test_create_actions_info_model():
    """Test the create_actions_info_model function."""
    # Create mock action models
    action1 = ActionModel(
        name="action1",
        version="1.0.0",
        description="First test action",
        author="Test Author",
        requires=[],
        dcc="test_dcc",
        file_path="/path/to/action1.py",
        functions={"func1": FunctionModel(name="func1", description="Function 1", return_type="int", parameters=[])},
        documentation_url=None,
        tags=[],
        capabilities=[],
    )

    action2 = ActionModel(
        name="action2",
        version="2.0.0",
        description="Second test action",
        author="Another Author",
        requires=["dep1"],
        dcc="test_dcc",
        file_path="/path/to/action2.py",
        functions={
            "func2": FunctionModel(
                name="func2",
                description="Function 2",
                return_type="str",
                parameters=[
                    ParameterModel(
                        name="arg1",
                        type_hint="str",
                        type="string",
                        required=True,
                        default=None,
                        description="Argument 1",
                    )
                ],
            )
        },
        documentation_url=None,
        tags=[],
        capabilities=[],
    )

    actions: Dict[str, ActionModel] = {"action1": action1, "action2": action2}

    # Create actions info model
    dcc_name = "test_dcc"
    actions_info = create_actions_info_model(dcc_name, actions)

    # Verify actions info model
    assert actions_info.dcc_name == "test_dcc"
    assert len(actions_info.actions) == 2

    # Verify action1
    action1_result = actions_info.actions["action1"]
    assert isinstance(action1_result, ActionModel)
    assert action1_result.name == "action1"
    assert action1_result.version == "1.0.0"
    assert len(action1_result.functions) == 1
    assert "func1" in action1_result.functions

    # Verify action2
    action2_result = actions_info.actions["action2"]
    assert isinstance(action2_result, ActionModel)
    assert action2_result.name == "action2"
    assert action2_result.version == "2.0.0"
    assert len(action2_result.functions) == 1
    assert "func2" in action2_result.functions

    # Verify function parameters
    func2 = action2_result.functions["func2"]
    assert len(func2.parameters) == 1
    param = func2.parameters[0]
    assert param.name == "arg1"
    assert param.type_hint == "str"
    assert param.type == "string"
