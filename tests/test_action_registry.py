"""Tests for the ActionRegistry class.

This module contains tests for the ActionRegistry class functionality,
including registration, discovery, and retrieval of Action classes.
"""

# Import built-in modules
import os
import sys

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
    dcc = "test"

    class InputModel(Action.InputModel):
        """Input model for TestAction1."""

        value: int

    def _execute(self) -> None:
        self.output = self.OutputModel(prompt="Test Action 1 executed successfully")


class TestAction2(Action):
    """Test Action 2."""

    name = "test_action2"
    description = "Test Action 2"
    dcc = "maya"

    class InputModel(Action.InputModel):
        """Input model for TestAction2."""

        value: str

    def _execute(self) -> None:
        value = self.input.value
        self.output = self.OutputModel(prompt=f"Test Action 2 executed with value: {value}")


class TestAction(Action):
    """Test action for registry."""

    name = "test_action"
    description = "A test action"
    dcc = "test"

    class InputModel(Action.InputModel):
        """Input model for TestAction."""

        value: int


class MayaAction(Action):
    """Maya-specific test action."""

    name = "maya_action"
    description = "A Maya-specific test action"
    dcc = "maya"

    class InputModel(Action.InputModel):
        """Input model for MayaAction."""

        value: str


@pytest.fixture
def clean_registry():
    """Fixture to provide a clean ActionRegistry for each test."""
    # Reset ActionRegistry singleton instance
    ActionRegistry._reset_instance()

    # Get new instance
    registry = ActionRegistry()

    yield registry

    # Reset singleton instance after test
    ActionRegistry._reset_instance()


def test_action_registry_singleton():
    """Test that ActionRegistry follows the singleton pattern."""
    registry1 = ActionRegistry()
    registry2 = ActionRegistry()

    # Both instances should be the same object
    assert registry1 is registry2

    # Modifying one should affect the other
    registry1._actions["test"] = "value"
    assert registry2._actions["test"] == "value"


def test_action_registry_register(clean_registry):
    """Test registering Actions."""
    registry = clean_registry

    # Register test actions
    registry.register(TestAction1)
    registry.register(TestAction2)

    # Check registration
    assert len(registry._actions) == 2
    assert "test_action1" in registry._actions
    assert "test_action2" in registry._actions
    assert registry._actions["test_action1"] is TestAction1
    assert registry._actions["test_action2"] is TestAction2


def test_action_registry_register_invalid(clean_registry):
    """Test registering invalid Action classes."""
    registry = clean_registry

    # Try to register a non-Action class
    class NotAnAction:
        pass

    with pytest.raises(TypeError):
        registry.register(NotAnAction)


def test_action_registry_list_actions(clean_registry):
    """Test listing registered Actions."""
    registry = clean_registry

    # Register test actions
    registry.register(TestAction1)
    registry.register(TestAction2)

    # List all actions
    actions = registry.list_actions()
    assert len(actions) == 2

    # Check action metadata
    action1 = next(a for a in actions if a["name"] == "test_action1")
    assert action1["description"] == "Test Action 1"
    assert action1["dcc"] == "test"

    # Filter by DCC
    maya_actions = registry.list_actions(dcc_name="maya")
    assert len(maya_actions) == 1
    assert maya_actions[0]["name"] == "test_action2"

    test_actions = registry.list_actions(dcc_name="test")
    assert len(test_actions) == 1
    assert test_actions[0]["name"] == "test_action1"

    # Filter by non-existent DCC
    houdini_actions = registry.list_actions(dcc_name="houdini")
    assert len(houdini_actions) == 0


# Create a temporary module for testing discovery
@pytest.fixture
def setup_test_package(tmp_path):
    """Create a temporary package with Action classes for testing discovery."""
    # Create package structure
    pkg_dir = tmp_path / "test_pkg"
    pkg_dir.mkdir()
    (pkg_dir / "__init__.py").write_text("")

    # Create a module with an Action class
    module_content = """
# Import local modules
from dcc_mcp_core.actions.base import Action
from dcc_mcp_core.models import ActionResultModel

class DiscoveredAction(Action):
    name = "discovered_action"
    description = "Discovered during testing"
    dcc = "test"

    class InputModel(Action.InputModel):
        value: str

    def _execute(self) -> None:
        value = self.input.value
        self.output = self.OutputModel()
"""
    (pkg_dir / "module.py").write_text(module_content)

    # Create a subpackage with another Action class
    subpkg_dir = pkg_dir / "subpkg"
    subpkg_dir.mkdir()
    (subpkg_dir / "__init__.py").write_text("")

    submodule_content = """
# Import local modules
from dcc_mcp_core.actions.base import Action
from dcc_mcp_core.models import ActionResultModel

class SubpackageAction(Action):
    name = "subpackage_action"
    description = "Action in a subpackage"
    dcc = "maya"

    class InputModel(Action.InputModel):
        value: int

    def _execute(self) -> None:
        value = self.input.value
        self.output = self.OutputModel()
"""
    (subpkg_dir / "submodule.py").write_text(submodule_content)

    # Add to sys.path so it can be imported
    sys.path.insert(0, str(tmp_path))
    yield "test_pkg"

    # Clean up
    sys.path.remove(str(tmp_path))


@pytest.mark.skipif(os.environ.get("CI") == "true", reason="Skip in CI environment")
def test_action_registry_discover_actions(clean_registry, setup_test_package):
    """Test discovering Actions from a package."""
    registry = clean_registry
    pkg_name = setup_test_package

    # Discover actions
    registry.discover_actions(pkg_name)

    # Check discovered actions
    assert "discovered_action" in registry._actions
    assert "subpackage_action" in registry._actions

    # Check action metadata
    actions = registry.list_actions()
    discovered = next(a for a in actions if a["name"] == "discovered_action")
    assert discovered["description"] == "Discovered during testing"
    assert discovered["dcc"] == "test"

    subpackage = next(a for a in actions if a["name"] == "subpackage_action")
    assert subpackage["description"] == "Action in a subpackage"
    assert subpackage["dcc"] == "maya"


def test_action_registry_dcc_specific_registry(clean_registry):
    """Test DCC-specific action registry."""
    registry = clean_registry

    # Register test actions
    registry.register(TestAction1)  # dcc = "test"
    registry.register(TestAction2)  # dcc = "maya"
    registry.register(TestAction)  # dcc = "test"
    registry.register(MayaAction)  # dcc = "maya"

    assert len(registry._actions) == 4
    assert "test_action1" in registry._actions
    assert "test_action2" in registry._actions
    assert "test_action" in registry._actions
    assert "maya_action" in registry._actions

    # Check DCC-specific registry
    assert len(registry._dcc_actions) == 2
    assert "test" in registry._dcc_actions
    assert "maya" in registry._dcc_actions

    # Check test DCC registry
    assert len(registry._dcc_actions["test"]) == 2
    assert "test_action1" in registry._dcc_actions["test"]
    assert "test_action" in registry._dcc_actions["test"]

    # Check maya DCC registry
    assert len(registry._dcc_actions["maya"]) == 2
    assert "test_action2" in registry._dcc_actions["maya"]
    assert "maya_action" in registry._dcc_actions["maya"]


def test_action_registry_get_action_with_dcc(clean_registry):
    """Test getting action by DCC name."""
    registry = clean_registry

    # Register test actions
    registry.register(TestAction1)  # dcc = "test"
    registry.register(TestAction2)  # dcc = "maya"

    # Get action without DCC name
    action1 = registry.get_action("test_action1")
    assert action1 is TestAction1

    action2 = registry.get_action("test_action2")
    assert action2 is TestAction2

    # Specify correct DCC name
    action1_test = registry.get_action("test_action1", dcc_name="test")
    assert action1_test is TestAction1

    action2_maya = registry.get_action("test_action2", dcc_name="maya")
    assert action2_maya is TestAction2

    # Specify incorrect DCC name, should return None
    action1_maya = registry.get_action("test_action1", dcc_name="maya")
    assert action1_maya is None

    action2_test = registry.get_action("test_action2", dcc_name="test")
    assert action2_test is None

    # Specify non-existent DCC name, should return action from main registry
    action1_houdini = registry.get_action("test_action1", dcc_name="houdini")
    assert action1_houdini is TestAction1


def test_action_registry_get_actions_by_dcc(clean_registry):
    """Test getting all actions for a specific DCC."""
    registry = clean_registry

    # Register test actions
    registry.register(TestAction1)  # dcc = "test"
    registry.register(TestAction2)  # dcc = "maya"
    registry.register(TestAction)  # dcc = "test"
    registry.register(MayaAction)  # dcc = "maya"

    # Get all actions for a specific DCC
    test_actions = registry.get_actions_by_dcc("test")
    assert len(test_actions) == 2
    assert "test_action1" in test_actions
    assert "test_action" in test_actions
    assert test_actions["test_action1"] is TestAction1
    assert test_actions["test_action"] is TestAction

    maya_actions = registry.get_actions_by_dcc("maya")
    assert len(maya_actions) == 2
    assert "test_action2" in maya_actions
    assert "maya_action" in maya_actions
    assert maya_actions["test_action2"] is TestAction2
    assert maya_actions["maya_action"] is MayaAction

    # Get actions for non-existent DCC
    houdini_actions = registry.get_actions_by_dcc("houdini")
    assert len(houdini_actions) == 0


def test_action_registry_get_all_dccs(clean_registry):
    """Test getting all DCCs."""
    registry = clean_registry

    # Initial state should have no DCCs
    assert len(registry.get_all_dccs()) == 0

    # Register test actions
    registry.register(TestAction1)  # dcc = "test"
    registry.register(TestAction2)  # dcc = "maya"

    # Check DCC list
    dccs = registry.get_all_dccs()
    assert len(dccs) == 2
    assert "test" in dccs
    assert "maya" in dccs

    # Register more actions
    registry.register(TestAction)  # dcc = "test", already exists
    registry.register(MayaAction)  # dcc = "maya", already exists

    # DCC list should remain unchanged
    dccs = registry.get_all_dccs()
    assert len(dccs) == 2
    assert "test" in dccs
    assert "maya" in dccs


@pytest.mark.skipif(os.environ.get("CI") == "true", reason="Skip in CI environment")
def test_action_registry_discover_actions_return_value(clean_registry, setup_test_package):
    """Test action discovery mechanism return value."""
    registry = clean_registry
    pkg_name = setup_test_package

    # Discover actions and get return value
    discovered_actions = registry.discover_actions(pkg_name)

    # Check return value
    assert len(discovered_actions) == 2
    assert any(a.__name__ == "DiscoveredAction" for a in discovered_actions)
    assert any(a.__name__ == "SubpackageAction" for a in discovered_actions)

    # Check registry
    assert "discovered_action" in registry._actions
    assert "subpackage_action" in registry._actions

    # Check DCC-specific registry
    assert "test" in registry._dcc_actions
    assert "maya" in registry._dcc_actions
    assert "discovered_action" in registry._dcc_actions["test"]
    assert "subpackage_action" in registry._dcc_actions["maya"]


@pytest.mark.skipif(os.environ.get("CI") == "true", reason="Skip in CI environment")
def test_action_registry_discover_actions_with_dcc(clean_registry, setup_test_package):
    """Test action discovery mechanism with DCC name."""
    registry = clean_registry
    pkg_name = setup_test_package

    # Discover actions and specify DCC name
    discovered_actions = registry.discover_actions(pkg_name, dcc_name="custom_dcc")

    # Check return value
    assert len(discovered_actions) == 2
    assert any(a.__name__ == "DiscoveredAction" for a in discovered_actions)
    assert any(a.__name__ == "SubpackageAction" for a in discovered_actions)

    # Check registry
    assert "discovered_action" in registry._actions
    assert "subpackage_action" in registry._actions

    # Check DCC-specific registry
    assert "test" in registry._dcc_actions  # DiscoveredAction already has dcc = "test"
    assert "maya" in registry._dcc_actions  # SubpackageAction already has dcc = "maya"

    # Check action DCC name is not overwritten
    discovered_action = registry._actions.get("discovered_action")
    assert discovered_action is not None
    assert discovered_action.dcc == "test"

    subpackage_action = registry._actions.get("subpackage_action")
    assert subpackage_action is not None
    assert subpackage_action.dcc == "maya"
