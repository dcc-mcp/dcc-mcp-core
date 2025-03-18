DCC Implementation Examples
=========================

This directory contains example implementations of the ``PluginManager`` class for various DCCs.

These examples demonstrate how to extend the base ``PluginManager`` class to provide DCC-specific functionality.

Maya Plugin Manager
-----------------

The ``maya_plugin_manager.py`` file provides an example implementation for Maya. It shows how to:

- Override the ``func_call`` method to process parameters according to Maya's requirements
- Implement Maya-specific versions of ``get_scene_info``, ``execute_command``, and ``execute_script``
- Handle Maya-specific error cases and logging

Houdini Plugin Manager
--------------------

The ``houdini_plugin_manager.py`` file provides an example implementation for Houdini. It shows how to:

- Override the ``func_call`` method to process parameters according to Houdini's requirements
- Implement Houdini-specific versions of ``get_scene_info``, ``execute_command``, and ``execute_script``
- Handle Houdini-specific error cases and logging

Creating Your Own Implementation
-----------------------------

To create your own DCC implementation, follow these steps:

1. Create a new Python file for your DCC (e.g., ``blender_plugin_manager.py``)
2. Import the base ``PluginManager`` class
3. Create a new class that inherits from ``PluginManager``
4. Override the necessary methods to provide DCC-specific functionality
5. Register your implementation in the ``get_plugin_manager`` function in ``plugin_manager.py``

Here's a template to get you started:

.. code-block:: python

    """
    DCC-specific implementation of the DCC plugin manager.
    
    This module provides a DCC-specific implementation of the PluginManager class,
    with functionality tailored to the DCC environment.
    """
    
    import os
    import sys
    import traceback
    from typing import Dict, Any, Optional, List, Union
    
    # Configure logging
    from dcc_mcp_core.logg_config import setup_logging
    # Import base plugin manager
    from dcc_mcp_core.plugin_manager import PluginManager
    # Import parameter processing
    from dcc_mcp_core.parameters import process_parameters
    
    logger = setup_logging("your_dcc_plugin_manager")
    
    class YourDCCPluginManager(PluginManager):
        """DCC-specific implementation of the plugin manager.
        
        This class extends the base PluginManager with DCC-specific functionality.
        """
        
        def __init__(self, plugin_dirs: List[str] = None):
            """Initialize the DCC plugin manager.
            
            Args:
                plugin_dirs: List of directories to search for plugins
            """
            super().__init__(plugin_dirs)
            self.bound_logger = logger.bind(name="your_dcc_plugin_manager")
            self.bound_logger.info(f"DCC plugin manager initialized with dirs: {self.plugin_dirs}")
        
        def func_call(self, plugin_name: str, context: Dict[str, Any]) -> Dict[str, Any]:
            """Call a plugin function in the DCC.
            
            Override this method to provide DCC-specific functionality.
            
            Args:
                plugin_name: Name of the plugin
                context: Context dictionary containing parameters and other information
                
            Returns:
                Dict with the result of the function execution and scene info
            """
            # Your DCC-specific implementation here
            pass
        
        def get_scene_info(self) -> Dict[str, Any]:
            """Get information about the current DCC scene.
            
            Override this method to provide DCC-specific scene information.
            
            Returns:
                Dict with DCC scene information
            """
            # Your DCC-specific implementation here
            pass
        
        def execute_command(self, command: str, *args, **kwargs) -> Any:
            """Execute a DCC command.
            
            Override this method to execute DCC-specific commands.
            
            Args:
                command: Name of the DCC command to execute
                *args: Positional arguments for the command
                **kwargs: Keyword arguments for the command
                
            Returns:
                Result of the command execution
            """
            # Your DCC-specific implementation here
            pass
        
        def execute_script(self, script: str) -> Any:
            """Execute a script in the DCC.
            
            Override this method to execute DCC-specific scripts.
            
            Args:
                script: Script to execute
                
            Returns:
                Result of the script execution
            """
            # Your DCC-specific implementation here
            pass

For more details, see the :doc:`../../api_reference` and the :doc:`../../usage_guide`.
