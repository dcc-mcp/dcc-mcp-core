"""Tests for the ActionRegistry class.

This module contains tests for the ActionRegistry class functionality,
including registration, discovery, and retrieval of Action classes.
"""

# Import built-in modules
import os
from pathlib import Path
import sys
from typing import Any
from typing import Dict
from typing import List

# Import third-party modules
from pydantic import Field
import pytest

# Import local modules
from dcc_mcp_core.actions.base import Action
from dcc_mcp_core.actions.registry import ActionRegistry
from dcc_mcp_core.models import ActionResultModel


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
    # Save the original actions dictionary
    registry = ActionRegistry()
    original_actions = registry._actions.copy()

    # Clear the registry for the test
    registry._actions = {}

    yield registry

    # Restore the original actions after the test
    registry._actions = original_actions


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
