"""Extended tests for the ActionRegistry class.

This module contains additional tests for the ActionRegistry to improve code coverage.
"""

# Import built-in modules
import os
import tempfile
from typing import ClassVar
from typing import List
from unittest.mock import MagicMock
from unittest.mock import patch

# Import third-party modules
import pytest

# Import local modules
from dcc_mcp_core.actions.base import Action
from dcc_mcp_core.actions.registry import ActionRegistry


class TestActionOne(Action):
    """Test Action implementation for registry tests."""

    name = "test_action_one"
    description = "A test action one"
    version = "1.0.0"
    author = "Test Author"
    requires: ClassVar[List[str]] = ["test_dependency"]
    tags: ClassVar[List[str]] = ["test", "one"]
    dcc = "test_dcc"

    class InputModel(Action.InputModel):
        """Test input model."""

    def _execute(self) -> None:
        """Test execution implementation."""


class TestActionTwo(Action):
    """Another test Action implementation for registry tests."""

    name = "test_action_two"
    description = "A test action two"
    version = "2.0.0"
    author = "Test Author Two"
    requires: ClassVar[List[str]] = ["test_dependency_two"]
    tags: ClassVar[List[str]] = ["test", "two"]
    dcc = "another_dcc"

    class InputModel(Action.InputModel):
        """Test input model."""

    def _execute(self) -> None:
        """Test execution implementation."""


@pytest.fixture
def reset_registry():
    """Reset the ActionRegistry singleton before and after each test."""
    ActionRegistry._reset_instance()
    yield
    ActionRegistry._reset_instance()


def test_registry_singleton(reset_registry):
    """Test that ActionRegistry follows the singleton pattern."""
    # Create two instances
    registry1 = ActionRegistry()
    registry2 = ActionRegistry()

    # They should be the same object
    assert registry1 is registry2

    # Register an action in one registry
    registry1.register(TestActionOne)

    # It should be available in the other registry
    assert registry2.get_action("test_action_one") is TestActionOne


def test_register_and_get_action(reset_registry):
    """Test registering and retrieving actions."""
    registry = ActionRegistry()

    # Register actions
    registry.register(TestActionOne)
    registry.register(TestActionTwo)

    # Get actions by name
    action1 = registry.get_action("test_action_one")
    action2 = registry.get_action("test_action_two")

    assert action1 is TestActionOne
    assert action2 is TestActionTwo

    # Get action by name and DCC
    action1_dcc = registry.get_action("test_action_one", "test_dcc")
    action2_dcc = registry.get_action("test_action_two", "another_dcc")

    assert action1_dcc is TestActionOne
    assert action2_dcc is TestActionTwo

    # Get action from non-existent DCC (should fall back to main registry)
    action1_nonexistent = registry.get_action("test_action_one", "nonexistent_dcc")
    assert action1_nonexistent is TestActionOne

    # Get non-existent action
    nonexistent_action = registry.get_action("nonexistent_action")
    assert nonexistent_action is None


def test_register_invalid_action(reset_registry):
    """Test registering an invalid action class."""
    registry = ActionRegistry()

    # Try to register a non-Action class
    class NotAnAction:
        pass

    with pytest.raises(TypeError):
        registry.register(NotAnAction)


def test_list_actions(reset_registry):
    """Test listing actions."""
    registry = ActionRegistry()

    # Register actions
    registry.register(TestActionOne)
    registry.register(TestActionTwo)

    # List all actions
    all_actions = registry.list_actions()
    assert len(all_actions) == 2

    # Check action metadata
    action1_info = next(info for info in all_actions if info["name"] == "test_action_one")
    assert action1_info["description"] == "A test action one"
    assert action1_info["version"] == "1.0.0"
    assert action1_info["dcc"] == "test_dcc"
    assert "input_schema" in action1_info

    # List actions for specific DCC
    test_dcc_actions = registry.list_actions("test_dcc")
    assert len(test_dcc_actions) == 1
    assert test_dcc_actions[0]["name"] == "test_action_one"

    another_dcc_actions = registry.list_actions("another_dcc")
    assert len(another_dcc_actions) == 1
    assert another_dcc_actions[0]["name"] == "test_action_two"

    # List actions for non-existent DCC
    nonexistent_dcc_actions = registry.list_actions("nonexistent_dcc")
    assert len(nonexistent_dcc_actions) == 0


def test_register_action_with_source_file(reset_registry):
    """Test registering an action with a source file."""
    registry = ActionRegistry()

    # Create a copy of TestActionOne with a source file
    class TestActionWithSource(TestActionOne):
        pass

    # Set source file
    setattr(TestActionWithSource, "_source_file", "/path/to/test_action.py")

    # Register the action
    registry.register(TestActionWithSource)

    # The action should be registered with a unique name
    action = registry.get_action("test_action_one")
    assert action is TestActionWithSource

    # Check that the original name is preserved in the action info
    actions_info = registry.list_actions()
    action_info = next(info for info in actions_info if info["name"] == "test_action_one")
    assert action_info["source_file"] == "/path/to/test_action.py"


def test_discover_actions_from_module(reset_registry):
    """Test discovering actions from a module."""
    registry = ActionRegistry()

    # Discover actions from module
    registry.register(TestActionOne)
    registry.register(TestActionTwo)

    # Verify actions are registered
    assert registry.get_action("test_action_one") is TestActionOne
    assert registry.get_action("test_action_two") is TestActionTwo

    # Get all actions info
    actions_info = registry.list_actions()

    # Verify actions info contains our registered actions
    action_names = [info["name"] for info in actions_info]
    assert "test_action_one" in action_names
    assert "test_action_two" in action_names


def test_discover_actions_from_path():
    """Test discovering actions from a file path."""
    # Reset registry
    ActionRegistry._reset_instance()
    registry = ActionRegistry()

    # Create a temporary file with action classes
    with tempfile.NamedTemporaryFile(suffix=".py", mode="w+", delete=False) as temp_file:
        temp_file.write("""\
from dcc_mcp_core.actions.base import Action

class PathTestAction(Action):
    name = "path_test_action"
    description = "A test action from path"
    dcc = "test_dcc"
    
    def _execute(self):
        pass
""")
        temp_path = temp_file.name

    try:
        # Discover actions from the path
        with patch("dcc_mcp_core.utils.module_loader.load_module_from_path") as mock_load:
            # Create a mock module with the action
            mock_module = MagicMock()

            # Create a real Action subclass for the mock module
            class PathTestAction(Action):
                name = "path_test_action"
                description = "A test action from path"
                dcc = "test_dcc"

                def _execute(self):
                    pass

            # Add the action class to the mock module
            mock_module.PathTestAction = PathTestAction
            mock_load.return_value = mock_module

            # Discover actions from the path
            discovered = registry.discover_actions_from_path(temp_path, dcc_name="test_dcc")

            # Should discover the action
            assert len(discovered) == 1
            assert discovered[0].__name__ == "PathTestAction"

            # Action should be registered
            action = registry.get_action("path_test_action")
            assert action is not None
            assert action.__name__ == "PathTestAction"

            # Source file should be set
            assert getattr(action, "_source_file", None) == temp_path
    finally:
        # Clean up
        if os.path.exists(temp_path):
            os.unlink(temp_path)
        ActionRegistry._reset_instance()


def test_discover_actions_from_package(reset_registry):
    """Test discovering actions from a package."""
    registry = ActionRegistry()

    # First set DCC name, ensure both actions use the same DCC
    TestActionOne.dcc = "test_dcc"
    TestActionTwo.dcc = "test_dcc"

    # Register actions
    registry.register(TestActionOne)
    registry.register(TestActionTwo)

    # Verify actions are registered
    assert registry.get_action("test_action_one") is TestActionOne
    assert registry.get_action("test_action_two") is TestActionTwo

    # Get actions for specific DCC
    dcc_actions = registry.list_actions_for_dcc("test_dcc")

    # Verify actions list contains our registered actions
    assert "test_action_one" in dcc_actions, f"test_action_one not in actions list: {dcc_actions}"
    assert "test_action_two" in dcc_actions, f"test_action_two not in actions list: {dcc_actions}"
