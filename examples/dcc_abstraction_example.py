"""
Example demonstrating the use of DCC abstraction interfaces.

This example shows how to use the DCC abstraction interfaces to write code
that works with multiple DCCs without hardcoding DCC-specific logic.
"""

import sys
import os

# Add the parent directory to the Python path
sys.path.append(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from dcc_mcp_core.plugin_manager import create_dcc_plugin_manager, get_supported_dccs, is_dcc_supported


def main():
    """Main function demonstrating DCC abstraction."""
    print("Supported DCCs:", get_supported_dccs())
    
    # Example with Maya
    if is_dcc_supported("maya"):
        print("\nCreating Maya plugin manager...")
        maya_pm = create_dcc_plugin_manager("maya")
        print("Maya scene info:", maya_pm.get_scene_info())
        print("Executing Maya command:", maya_pm.execute_command("ls"))
        print("Executing Maya script:", maya_pm.execute_script("sphere -name mySphere;"))
    
    # Example with Houdini
    if is_dcc_supported("houdini"):
        print("\nCreating Houdini plugin manager...")
        houdini_pm = create_dcc_plugin_manager("houdini")
        print("Houdini scene info:", houdini_pm.get_scene_info())
        print("Executing Houdini command:", houdini_pm.execute_command("opfind"))
        print("Executing Houdini script:", houdini_pm.execute_script("hou.node('/obj').createNode('geo', 'myGeo')"))
    
    # Example with base implementation
    print("\nCreating base plugin manager...")
    base_pm = create_dcc_plugin_manager("base")
    print("Base scene info:", base_pm.get_scene_info())
    
    # Example of DCC-agnostic code
    print("\nDCC-agnostic function example:")
    for dcc_name in get_supported_dccs():
        print(f"\nRunning with {dcc_name}:")
        run_dcc_agnostic_function(dcc_name)


def run_dcc_agnostic_function(dcc_name: str):
    """Example of a function that works with any DCC.
    
    Args:
        dcc_name: Name of the DCC to use
    """
    # Create a plugin manager for the specified DCC
    pm = create_dcc_plugin_manager(dcc_name)
    
    # Get scene information (works with any DCC)
    scene_info = pm.get_scene_info()
    print(f"Scene info for {dcc_name}: {scene_info}")
    
    # Execute a command (implementation differs by DCC)
    command_result = pm.execute_command("help")
    print(f"Command result for {dcc_name}: {command_result}")
    
    # Try to call a plugin (would work if the plugin exists)
    plugin_result = pm.func_call("example_plugin", {"parameters": {"value": 42}})
    print(f"Plugin result for {dcc_name}: {plugin_result}")


if __name__ == "__main__":
    main()
