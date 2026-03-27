"""Tests for the ActionRegistry interface with dcc-mcp-rpyc.

This module contains tests specifically focused on the interface methods
that ActionRegistry provides for dcc-mcp-rpyc, ensuring that they are
robust and handle error conditions gracefully.
"""

# Import built-in modules

# Import third-party modules
import pytest

# Import local modules
from dcc_mcp_core.actions.base import Action
from dcc_mcp_core.actions.registry import ActionRegistry


# Define test Action classes
class TestAction1(Action):
    """Test Action 1."""

    name = "test_action1"
    description = "Test Action 1"
    dcc = "test_dcc1"


class TestAction2(Action):
    """Test Action 2."""

    name = "test_action2"
    description = "Test Action 2"
    dcc = "test_dcc1"


class TestAction3(Action):
    """Test Action 3."""

    name = "test_action3"
    description = "Test Action 3"
    dcc = "test_dcc2"


@pytest.fixture
def registry_with_actions():
    """Fixture to provide a registry with test actions."""
    registry = ActionRegistry()
    registry.reset(full_reset=True)  # Ensure clean registry

    # Add _execute method to test action classes to ensure they can be registered
    def dummy_execute(self):
        pass

    TestAction1._execute = dummy_execute
    TestAction2._execute = dummy_execute
    TestAction3._execute = dummy_execute

    # Register test action classes
    registry.register(TestAction1)
    registry.register(TestAction2)
    registry.register(TestAction3)

    # Ensure they are also added to DCC-specific registry
    if "test_dcc1" not in registry._dcc_actions:
        registry._dcc_actions["test_dcc1"] = {}
    if "test_dcc2" not in registry._dcc_actions:
        registry._dcc_actions["test_dcc2"] = {}

    registry._dcc_actions["test_dcc1"]["test_action1"] = TestAction1
    registry._dcc_actions["test_dcc1"]["test_action2"] = TestAction2
    registry._dcc_actions["test_dcc2"]["test_action3"] = TestAction3

    return registry


def test_get_action_with_empty_name(registry_with_actions):
    """Test get_action with empty action name."""
    result = registry_with_actions.get_action("")
    assert result is None


def test_get_action_with_nonexistent_dcc(registry_with_actions):
    """Test get_action with non-existent DCC name."""
    result = registry_with_actions.get_action("test_action1", dcc_name="non_existent_dcc")
    assert result is None


def test_get_action_with_nonexistent_action(registry_with_actions):
    """Test get_action with non-existent action name."""
    result = registry_with_actions.get_action("non_existent_action")
    assert result is None


def test_get_action_with_valid_dcc(registry_with_actions):
    """Test get_action with valid DCC name."""
    result = registry_with_actions.get_action("test_action1", dcc_name="test_dcc1")
    assert result is TestAction1


def test_get_action_with_wrong_dcc(registry_with_actions):
    """Test get_action with wrong DCC name for an existing action."""
    result = registry_with_actions.get_action("test_action1", dcc_name="test_dcc2")
    assert result is None


def test_list_actions_with_nonexistent_dcc(registry_with_actions):
    """Test list_actions with non-existent DCC name."""
    result = registry_with_actions.list_actions(dcc_name="non_existent_dcc")
    assert result == []


def test_list_actions_with_valid_dcc(registry_with_actions):
    """Test list_actions with valid DCC name."""
    result = registry_with_actions.list_actions(dcc_name="test_dcc1")
    assert len(result) == 2
    action_names = [action["internal_name"] for action in result]
    assert "test_action1" in action_names
    assert "test_action2" in action_names


def test_list_actions_with_tag(registry_with_actions):
    """Test list_actions with tag filter."""
    # Add tags to TestAction1
    TestAction1.tags = ["tag1", "tag2"]

    result = registry_with_actions.list_actions(tag="tag1")
    assert len(result) == 1
    assert result[0]["internal_name"] == "test_action1"


def test_list_actions_with_dcc_and_tag(registry_with_actions):
    """Test list_actions with both DCC and tag filters."""
    # Add tags to TestAction1 and TestAction3
    TestAction1.tags = ["tag1"]
    TestAction3.tags = ["tag1"]

    result = registry_with_actions.list_actions(dcc_name="test_dcc1", tag="tag1")
    assert len(result) == 1
    assert result[0]["internal_name"] == "test_action1"


def test_list_actions_with_error_in_metadata(registry_with_actions):
    """Test list_actions handles errors in creating metadata."""
    # Create a mock _create_action_metadata that raises an exception for TestAction1
    original_create_metadata = registry_with_actions._create_action_metadata

    def mock_create_metadata(name, action_class):
        if name == "test_action1":
            raise RuntimeError("Error creating metadata")
        return original_create_metadata(name, action_class)

    registry_with_actions._create_action_metadata = mock_create_metadata

    # Should still return metadata for TestAction2 and TestAction3
    result = registry_with_actions.list_actions()
    assert len(result) == 2
    action_names = [action["internal_name"] for action in result]
    assert "test_action1" not in action_names
    assert "test_action2" in action_names
    assert "test_action3" in action_names


def test_get_actions_by_dcc_with_empty_name(registry_with_actions):
    """Test get_actions_by_dcc with empty DCC name."""
    result = registry_with_actions.get_actions_by_dcc("")
    assert result == {}


def test_get_actions_by_dcc_with_nonexistent_dcc(registry_with_actions):
    """Test get_actions_by_dcc with non-existent DCC name."""
    result = registry_with_actions.get_actions_by_dcc("non_existent_dcc")
    assert result == {}


def test_get_actions_by_dcc_with_valid_dcc(registry_with_actions):
    """Test get_actions_by_dcc with valid DCC name."""
    result = registry_with_actions.get_actions_by_dcc("test_dcc1")
    assert len(result) == 2
    assert "test_action1" in result
    assert "test_action2" in result
    assert result["test_action1"] is TestAction1
    assert result["test_action2"] is TestAction2


def test_get_all_dccs(registry_with_actions):
    """Test get_all_dccs returns all DCCs with registered actions."""
    result = registry_with_actions.get_all_dccs()
    assert len(result) == 2
    assert "test_dcc1" in result
    assert "test_dcc2" in result


def test_get_all_dccs_with_empty_registry():
    """Test get_all_dccs with empty registry."""
    registry = ActionRegistry()
    registry.reset(full_reset=True)  # Ensure clean registry
    result = registry.get_all_dccs()
    assert result == []
