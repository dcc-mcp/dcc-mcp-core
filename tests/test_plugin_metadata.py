"""Tests for plugin metadata extraction.

This module contains tests for the plugin metadata extraction functionality,
including loading test plugins and verifying the extracted metadata.
"""

# Import built-in modules
import os
import sys
from typing import Any
from typing import Dict

# Import third-party modules
import pytest

# Import local modules
from dcc_mcp_core.constants import PLUGIN_METADATA
from dcc_mcp_core.utils import extract_module_metadata


def test_basic_plugin_metadata(plugin_manager, test_data_dir):
    """Test metadata extraction from a basic plugin with complete metadata."""
    # Load the basic plugin
    plugin_path = os.path.join(test_data_dir, "basic_plugin.py")
    plugin_module = plugin_manager.load_plugin(plugin_path)
    
    assert plugin_module is not None
    
    # Get plugin info
    plugin_info = plugin_manager.get_plugin_info("basic_plugin")
    
    # Verify metadata
    assert plugin_info["name"] == "basic_plugin"
    assert plugin_info["version"] == "1.0.0"
    assert plugin_info["description"] == "A basic test plugin with complete metadata"
    assert plugin_info["author"] == "Test Author"
    assert plugin_info["requires"] == ["dependency1", "dependency2"]
    
    # Verify functions
    assert "hello_world" in plugin_info["functions"]
    assert "add_numbers" in plugin_info["functions"]
    assert "process_data" in plugin_info["functions"]
    
    # Verify function details
    hello_func = plugin_info["functions"]["hello_world"]
    assert hello_func["return_type"] in ["<class 'str'>", "str"]
    assert len(hello_func["parameters"]) == 0
    
    add_func = plugin_info["functions"]["add_numbers"]
    assert len(add_func["parameters"]) == 2
    assert add_func["parameters"][0]["name"] == "a"
    assert add_func["parameters"][0]["type"] in ["<class 'int'>", "int"]
    assert add_func["parameters"][1]["name"] == "b"
    
    process_func = plugin_info["functions"]["process_data"]
    assert len(process_func["parameters"]) == 2
    assert process_func["parameters"][0]["name"] == "data"
    assert process_func["parameters"][0]["required"] is True
    assert process_func["parameters"][1]["name"] == "verbose"
    assert process_func["parameters"][1]["required"] is False
    assert process_func["parameters"][1]["default"] is False


def test_minimal_plugin_metadata(plugin_manager, test_data_dir):
    """Test metadata extraction from a minimal plugin with only required fields."""
    # Load the minimal plugin
    plugin_path = os.path.join(test_data_dir, "minimal_plugin.py")
    plugin_module = plugin_manager.load_plugin(plugin_path)
    
    assert plugin_module is not None
    
    # Get plugin info
    plugin_info = plugin_manager.get_plugin_info("minimal_plugin")
    
    # Verify metadata (with defaults)
    assert plugin_info["name"] == "minimal_plugin"
    assert plugin_info["version"] == PLUGIN_METADATA["version"]["default"]
    assert plugin_info["description"] == PLUGIN_METADATA["description"]["default"]
    assert plugin_info["author"] == PLUGIN_METADATA["author"]["default"]
    assert plugin_info["requires"] == PLUGIN_METADATA["requires"]["default"]
    
    # Verify functions
    assert "minimal_function" in plugin_info["functions"]
    
    # Verify function details
    min_func = plugin_info["functions"]["minimal_function"]
    assert len(min_func["parameters"]) == 0


def test_maya_plugin_metadata(plugin_manager, test_data_dir):
    """Test metadata extraction from a Maya plugin with context parameter."""
    # Load the Maya plugin
    plugin_path = os.path.join(test_data_dir, "maya_plugin.py")
    plugin_module = plugin_manager.load_plugin(plugin_path)
    
    assert plugin_module is not None
    
    # Get plugin info
    plugin_info = plugin_manager.get_plugin_info("maya_plugin")
    
    # Verify metadata
    assert plugin_info["name"] == "maya_plugin"
    assert plugin_info["requires"] == ["maya"]
    
    # Verify functions
    assert "create_cube" in plugin_info["functions"]
    assert "list_objects" in plugin_info["functions"]
    
    # Verify function details with context parameter
    create_func = plugin_info["functions"]["create_cube"]
    assert len(create_func["parameters"]) == 2
    assert create_func["parameters"][0]["name"] == "size"
    assert create_func["parameters"][0]["default"] == 1.0
    assert create_func["parameters"][1]["name"] == "context"
    assert create_func["parameters"][1]["required"] is False


def test_advanced_types_plugin_metadata(plugin_manager, test_data_dir):
    """Test metadata extraction from a plugin with advanced type annotations."""
    # Load the advanced types plugin
    plugin_path = os.path.join(test_data_dir, "advanced_types_plugin.py")
    plugin_module = plugin_manager.load_plugin(plugin_path)
    
    assert plugin_module is not None
    
    # Get plugin info
    plugin_info = plugin_manager.get_plugin_info("advanced_types_plugin")
    
    # Verify functions
    assert "process_complex_data" in plugin_info["functions"]
    assert "async_operation" in plugin_info["functions"]
    
    # Verify complex function details
    complex_func = plugin_info["functions"]["process_complex_data"]
    assert len(complex_func["parameters"]) == 3
    
    # Check return type (should contain "Tuple" or "tuple")
    assert ("Tuple" in complex_func["return_type"] or 
            "tuple" in complex_func["return_type"].lower())


def test_internal_helpers_plugin_metadata(plugin_manager, test_data_dir):
    """Test that internal helper functions are not auto-registered."""
    # Load the internal helpers plugin
    plugin_path = os.path.join(test_data_dir, "internal_helpers_plugin.py")
    plugin_module = plugin_manager.load_plugin(plugin_path)
    
    assert plugin_module is not None
    
    # Get plugin info
    plugin_info = plugin_manager.get_plugin_info("internal_helpers_plugin")
    
    # Verify only public functions are registered
    assert "get_calculated_value" in plugin_info["functions"]
    assert "get_formatted_calculation" in plugin_info["functions"]
    
    # Verify internal helpers are NOT registered
    assert "_calculate_value" not in plugin_info["functions"]
    assert "_format_result" not in plugin_info["functions"]


def test_extract_module_metadata_function(test_data_dir):
    """Test the extract_module_metadata utility function directly."""
    # Create a mock module with metadata
    # Import built-in modules
    from typing import ClassVar
    from typing import List
    
    class MockModule:
        __plugin_name__ = "mock_plugin"
        __plugin_version__ = "2.0.0"
        __plugin_description__ = "A mock plugin for testing"
        __plugin_author__ = "Mock Author"
        __plugin_requires__: ClassVar[List[str]] = ["mock_dependency"]
    
    # Extract metadata
    metadata = extract_module_metadata(MockModule)
    
    # Verify metadata
    assert metadata["name"] == "mock_plugin"
    assert metadata["version"] == "2.0.0"
    assert metadata["description"] == "A mock plugin for testing"
    assert metadata["author"] == "Mock Author"
    assert metadata["requires"] == ["mock_dependency"]
    
    # Test with default name
    class MockModuleNoName:
        pass
    
    metadata = extract_module_metadata(MockModuleNoName, "default_name")
    assert metadata["name"] == "default_name"
