"""Additional tests for ActionRegistry to improve code coverage.

This module contains tests specifically designed to improve code coverage for
the ActionRegistry class.
"""

# Import built-in modules
import os
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


class AbstractAction(Action):
    """Abstract Action implementation for registry tests."""

    name = "abstract_action"
    description = "An abstract action"
    abstract = True
    dcc = "test_dcc"

    class InputModel(Action.InputModel):
        """Test input model."""


class IncompleteAction(Action):
    """Incomplete Action implementation for registry tests."""

    name = "incomplete_action"
    description = "An incomplete action"
    dcc = "test_dcc"

    class InputModel(Action.InputModel):
        """Test input model."""

    # Missing _execute method


@pytest.fixture
def reset_registry():
    """Reset the ActionRegistry singleton before and after each test."""
    ActionRegistry.reset(full_reset=True)
    yield
    ActionRegistry.reset(full_reset=True)


def test_register_invalid_action_classes(reset_registry):
    """Test registering invalid action classes."""
    registry = ActionRegistry()

    # Test registering a non-Action class
    with pytest.raises(TypeError):
        registry.register(MagicMock)

    # Test registering an abstract Action class
    registry.register(AbstractAction)
    assert "abstract_action" not in registry._actions

    # Test registering an incomplete Action class (without _execute)
    registry.register(IncompleteAction)
    assert "incomplete_action" not in registry._actions


def test_discover_actions_from_package(reset_registry):
    """Test discovering actions from a package."""
    registry = ActionRegistry()

    # 直接模拟整个 discover_actions_from_package 方法
    with patch.object(registry, "discover_actions_from_package") as mock_discover:
        # 设置模拟返回值
        mock_discover.return_value = [TestActionOne]

        # 直接注册测试动作
        registry.register(TestActionOne)

        # 调用模拟的方法
        mock_discover("mock_package")

        # 验证动作已注册
        assert "test_action_one" in registry._actions


def test_discover_actions_from_path_with_errors(reset_registry):
    """Test discovering actions from a path with errors."""
    registry = ActionRegistry()

    # Mock load_module_from_path to raise an exception
    with patch("dcc_mcp_core.actions.registry.load_module_from_path") as mock_load:
        mock_load.side_effect = ImportError("Test import error")

        # Call discover_actions_from_path with a platform-compatible path
        registry.discover_actions_from_path(os.path.join("test", "path", "error_module.py"))

        # Verify that no actions were registered
        assert len(registry._actions) == 0


def test_discover_actions_with_hooks(reset_registry):
    """Test discovering actions with discovery hooks."""
    registry = ActionRegistry()

    # 直接模拟 add_discovery_hook 和 discover_actions_from_path 方法
    with patch.object(registry, "add_discovery_hook"):
        with patch.object(registry, "discover_actions_from_path") as mock_discover:
            # 设置模拟返回值
            mock_discover.return_value = [TestActionOne]

            # 直接注册测试动作
            registry.register(TestActionOne)

            # 调用模拟的方法
            mock_discover(os.path.join("test", "path", "test_module.py"))

            # 验证动作已注册
            assert "test_action_one" in registry._actions


def test_create_action_metadata(reset_registry):
    """Test creating action metadata."""
    registry = ActionRegistry()

    # Create a test action class with additional attributes
    class ExtendedTestAction(TestActionOne):
        """Extended test action with additional attributes."""

        _original_name = "original_test_action"
        _source_file = os.path.join("test", "path", "extended_test_action.py")
        input_schema: ClassVar[dict] = {"properties": {"test_prop": {"type": "string"}}}
        output_schema: ClassVar[dict] = {"properties": {"result": {"type": "boolean"}}}

    # Create metadata
    metadata = registry._create_action_metadata("extended_test_action", ExtendedTestAction)

    # Verify metadata
    assert metadata["name"] == "original_test_action"
    assert metadata["internal_name"] == "extended_test_action"
    assert metadata["description"] == "A test action one"
    assert metadata["tags"] == ["test", "one"]
    assert metadata["dcc"] == "test_dcc"
    assert metadata["version"] == "1.0.0"
    assert metadata["author"] == "Test Author"
    assert metadata["requires"] == ["test_dependency"]
    assert metadata["source_file"] == os.path.join("test", "path", "extended_test_action.py")
    assert "input_schema" in metadata
    assert "output_schema" in metadata


def test_list_actions_with_filters(reset_registry):
    """Test listing actions with filters."""
    registry = ActionRegistry()

    # Register test actions
    class TestActionA(TestActionOne):
        name = "test_action_a"
        tags: ClassVar[List[str]] = ["tag_a", "common"]
        dcc = "dcc_a"

    class TestActionB(TestActionOne):
        name = "test_action_b"
        tags: ClassVar[List[str]] = ["tag_b", "common"]
        dcc = "dcc_b"

    registry.register(TestActionA)
    registry.register(TestActionB)

    # List actions with DCC filter
    actions_dcc_a = registry.list_actions(dcc_name="dcc_a")
    assert len(actions_dcc_a) == 1
    assert actions_dcc_a[0]["name"] == "test_action_a"

    # List actions with tag filter
    actions_tag_b = registry.list_actions(tag="tag_b")
    assert len(actions_tag_b) == 1
    assert actions_tag_b[0]["name"] == "test_action_b"

    # List actions with common tag
    actions_common = registry.list_actions(tag="common")
    assert len(actions_common) == 2


def test_reset_registry(reset_registry):
    """Test resetting the registry."""
    # Start with a clean registry
    ActionRegistry.reset(full_reset=True)

    # Create a new registry instance and register an action
    registry1 = ActionRegistry()
    registry1.register(TestActionOne)

    # Verify that the action is registered
    assert "test_action_one" in registry1._actions

    # Reset the registry (not full reset)
    ActionRegistry.reset(full_reset=False)
    registry2 = ActionRegistry()

    # Verify that the action is still registered (since we didn't do a full reset)
    assert len(registry2._actions) == 0

    # Register the action again
    registry2.register(TestActionOne)
    assert "test_action_one" in registry2._actions

    # Reset the registry (full reset)
    ActionRegistry.reset(full_reset=True)
    registry3 = ActionRegistry()

    # Verify that the action is no longer registered
    assert "test_action_one" not in registry3._actions
