---
name: qt-ui-inspector
description: >-
  Infrastructure skill — DCC-agnostic Qt UI introspection: list top-level
  windows, find widgets by name/class, describe widget properties, walk the
  widget tree, and poll for availability. Works in any DCC with a Qt binding
  (Maya, Blender, Houdini, Unreal, etc.). All tools are read-only and
  lazy-loaded.
license: MIT
metadata:
  dcc-mcp:
    dcc: python
    version: \"1.0.0\"
    layer: infrastructure
    search-hint: \"qt ui, qt widgets, gui introspection, find widget, describe widget, list windows, snapshot tree, wait for widget, PySide, PyQt, qtpy\"
    tags: \"qt, ui, introspection, infrastructure, gui\"
    tools: tools.yaml
---

# Qt UI Inspector

Cross-DCC Qt UI introspection tools. All tools lazy-import the host's Qt binding
(qtpy, PySide6, PySide2, PyQt6, or PyQt5).

## Tools

### ``dcc_qt_ui_inspector__list_windows``
List every top-level Qt window with object name, class, visibility, geometry,
and child count.

### ``dcc_qt_ui_inspector__find_widgets``
Locate Qt widgets by object name (exact/substring/regex), class name, and
visibility. Bounded result count.

### ``dcc_qt_ui_inspector__describe_widget``
Return a single Qt widget's structured state - class, geometry, flags,
accessible name/description, and bounded property snapshot.

### ``dcc_qt_ui_inspector__snapshot_tree``
Walk the Qt widget tree under a root and return it as a JSON-safe tree with
depth and node-count budgets.

### ``dcc_qt_ui_inspector__wait_for_widget``
Poll for a Qt widget by name/class with visible/enabled gates and bounded
timeout.
