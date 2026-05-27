---
name: qt-ui-inspector
description: >-
  Infrastructure skill — agent-facing Qt UI introspection for any DCC that
  embeds a Qt-based interface (Maya, Houdini, Nuke, Substance, Katana, custom
  PySide tools). List top-level windows, find widgets by object name or class,
  describe widget state, snapshot the widget tree, and wait for a widget to
  appear. Lazily imports qtpy / PySide / PyQt and returns "qt-binding-unavailable"
  when no Qt binding is installed — never crashes the host.
license: MIT
metadata:
  dcc-mcp:
    dcc: python
    version: "0.1.0"
    layer: infrastructure
    risk: low
    tool_role: read_only
    search-hint: "qt ui introspection widget window pyside pyqt katana nuke houdini substance maya inspector"
    tags: "ui, qt, introspection, debug, infrastructure, widget, window"
    tools: tools.yaml
    relationships:
      depends_on: []
      compatible_with: ["dcc-diagnostics"]
      fallback_for: ["maya-ui-inspector", "houdini-ui-inspector"]
---

# Qt UI Inspector

Read-only, DCC-agnostic Qt UI introspection. Every tool returns structured
results, never raises into the host event loop, and reports a clear
``qt-binding-unavailable`` error when ``qtpy`` / ``PySide6`` / ``PySide2`` /
``PyQt6`` / ``PyQt5`` is not importable.

## Tools

### `qt_ui_inspector__list_windows`

Return every top-level Qt window with object name, class, visibility,
geometry, and child widget count. Use this first to discover what's on
screen.

### `qt_ui_inspector__find_widgets`

Locate widgets by object name (exact or substring), class name, or a
property predicate. Returns a bounded list of widget descriptors so the
result stays JSON-safe even when the host has thousands of widgets.

### `qt_ui_inspector__describe_widget`

Return a single widget's structured state — class, object name, geometry,
enabled/visible flags, accessible name/description, and a bounded
properties snapshot. The widget is identified by the stable ``widget_id``
returned by ``list_windows`` / ``find_widgets``.

### `qt_ui_inspector__snapshot_tree`

Walk the widget tree under a root (window, panel, or whole top-level set)
and return the structure as a JSON-safe tree. Depth is clamped so large
hosts stay responsive.

### `qt_ui_inspector__wait_for_widget`

Poll for a widget matching the given criteria, returning as soon as it
exists (visible / enabled options) or timing out cleanly. Useful when an
agent kicks off an action that opens a dialog and needs to wait for the
dialog before continuing.

## Safety model

* Every tool is read-only (`tool_role: read_only`, `risk: low`,
  `read_only_hint: true`).
* Qt bindings are imported lazily inside each tool so importing this
  skill never pulls Qt into the host process.
* When no Qt binding is available, the tool returns
  ``error_result(..., error_code="qt-binding-unavailable")`` with a
  remediation hint listing the supported bindings.
* All identifiers are stable strings (object name + class + memory id
  fingerprint), never raw ``PyObject*`` handles — agents can pass them
  back across calls without risking dangling pointers.

## Acceptance criteria (issue #1332)

* Discoverable through the standard `search_skills` / `load_skill` flow.
* Read-only; never mutates host state.
* Works in any DCC that embeds Qt without DCC-specific imports.
* Fails fast and structured when Qt is absent.
