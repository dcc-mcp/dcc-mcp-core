API Reference
=============

This document provides a reference for the DCC-MCP-Core API.

Plugin Manager
-------------

.. autoclass:: dcc_mcp_core.action_manager.PluginManager
   :members:
   :undoc-members:
   :show-inheritance:

.. autofunction:: dcc_mcp_core.action_manager.get_action_manager

.. autofunction:: dcc_mcp_core.action_manager.create_dcc_action_manager

.. autofunction:: dcc_mcp_core.action_manager.get_supported_dccs

.. autofunction:: dcc_mcp_core.action_manager.is_dcc_supported

Filesystem Utilities
-------------------

.. autofunction:: dcc_mcp_core.filesystem.register_dcc_plugin_path

.. autofunction:: dcc_mcp_core.filesystem.get_plugin_paths

.. autofunction:: dcc_mcp_core.filesystem.set_default_plugin_paths

.. autofunction:: dcc_mcp_core.filesystem.discover_plugins

Parameter Processing
-------------------

.. autofunction:: dcc_mcp_core.parameters.process_parameters
