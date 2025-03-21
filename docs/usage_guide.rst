Usage Guide
===========

This guide explains how to use the DCC-MCP-Core library to create DCC-agnostic plugins and applications.

Overview
--------

DCC-MCP-Core provides a flexible abstraction layer for working with different Digital Content Creation (DCC) applications such as Maya, Houdini, and others. The core of this abstraction is the ``PluginManager`` which defines a standard set of methods that all DCC implementations must provide.

Getting Started
--------------

To use DCC-MCP-Core in your project, you first need to create a plugin manager for your target DCC:

.. code-block:: python

    from dcc_mcp_core.action_manager import create_dcc_action_manager

    # Create a plugin manager for Maya
    maya_pm = create_dcc_action_manager("maya")

    # Or for Houdini
    houdini_pm = create_dcc_action_manager("houdini")

Working with Plugins
------------------

Once you have a plugin manager, you can use it to load and execute plugins:

.. code-block:: python

    # Load a plugin
    plugin = maya_pm.load_plugin("/path/to/my_plugin.py")

    # Call a plugin function
    result = maya_pm.func_call("my_plugin", {
        "parameters": {
            "value": 42,
            "name": "test"
        }
    })

    print(result)

DCC Commands and Scripts
----------------------

You can also execute DCC-specific commands and scripts:

.. code-block:: python

    # Execute a Maya command
    maya_pm.execute_command("ls", "-l")

    # Execute a MEL script in Maya
    maya_pm.execute_script("sphere -name mySphere;")

    # Execute a Python script in Houdini
    houdini_pm.execute_script("hou.node('/obj').createNode('geo', 'myGeo')")

Getting Scene Information
-----------------------

To get information about the current scene:

.. code-block:: python

    # Get Maya scene info
    maya_scene = maya_pm.get_scene_info()
    print(maya_scene)

    # Get Houdini scene info
    houdini_scene = houdini_pm.get_scene_info()
    print(houdini_scene)

Writing DCC-Agnostic Code
-----------------------

One of the main benefits of DCC-MCP-Core is the ability to write code that works with multiple DCCs without modification:

.. code-block:: python

    from dcc_mcp_core.action_manager import create_dcc_action_manager, get_supported_dccs

    def process_scene(dcc_name):
        # Create a plugin manager for the specified DCC
        pm = create_dcc_action_manager(dcc_name)

        # Get scene information (works with any DCC)
        scene_info = pm.get_scene_info()
        print(f"Scene info for {dcc_name}: {scene_info}")

        # Execute a command (implementation differs by DCC)
        pm.execute_command("help")

        # Call a plugin (would work if the plugin exists)
        pm.func_call("process_scene", {"parameters": {"operation": "cleanup"}})

    # Run the function with all supported DCCs
    for dcc in get_supported_dccs():
        process_scene(dcc)

Extending with New DCCs
--------------------

To add support for a new DCC, you need to create a new implementation of the ``DCCPluginInterface``. See the example implementations in the ``docs/examples/dcc_implementations`` directory for reference.

The basic steps are:

1. Create a new class that inherits from ``PluginManager``
2. Override the necessary methods to provide DCC-specific functionality
3. Register your implementation in the ``get_action_manager`` function

See the :doc:`api_reference` for more details on the available methods and classes.
