# UI Recipes

Copy-pasteable UI interaction sequences.

## Find and click a button

```bash
# 1. Find the button
dcc-mcp-cli call <instance>__ui__find_widget \
  --json '{"query": "Save", "widget_type": "QPushButton"}'

# 2. Click it using the returned widget descriptor
dcc-mcp-cli call <instance>__ui__interact_widget \
  --json '{"widget": {"control_id": "saveButton"}, "action": "click"}'
```

## Type text into a field

```bash
# 1. Find the text field
dcc-mcp-cli call <instance>__ui__find_widget \
  --json '{"query": "", "widget_type": "QLineEdit", "limit": 5}'

# 2. Click to focus, then type
dcc-mcp-cli call <instance>__ui__interact_widget \
  --json '{"widget": {"widget_path": "MayaWindow/QLineEdit[name]"},"action":"click"}'

dcc-mcp-cli call <instance>__ui__interact_widget \
  --json '{"widget": {"widget_path": "MayaWindow/QLineEdit[name]"},"action":"type","value":"myObject"}'
```

## Wait for a dialog and dismiss it

```bash
# 1. Wait for the dialog
dcc-mcp-cli call <instance>__ui__wait_for_ui \
  --json '{"query": "Warning", "timeout_secs": 10}'

# 2. Find the OK button
dcc-mcp-cli call <instance>__ui__find_widget \
  --json '{"query": "OK", "window_title": "Warning"}'

# 3. Click OK
dcc-mcp-cli call <instance>__ui__interact_widget \
  --json '{"widget": {"control_id": "okButton"}, "action": "click"}'

# 4. Wait for dialog to disappear
dcc-mcp-cli call <instance>__ui__wait_for_ui \
  --json '{"query": "Warning", "condition": "disappears"}'
```

## Full computer-use fallback loop

```bash
# Use only when structured tools don't cover the operation

# 1. Snapshot current state
dcc-mcp-cli call <instance>__ui_control__snapshot \
  --json '{}'

# 2. Find target in snapshot
dcc-mcp-cli call <instance>__ui_control__find \
  --json '{"query": "render button"}'

# 3. Act on target
dcc-mcp-cli call <instance>__ui_control__act \
  --json '{"action": "click", "target": "..."}'

# 4. Verify with another snapshot
dcc-mcp-cli call <instance>__ui_control__snapshot \
  --json '{}'

# 5. Stop computer use session
dcc-mcp-cli call <instance>__ui_control__stop_computer_use \
  --json '{}'
```

## Inspect window layout (Qt-based DCCs)

```bash
# List all windows
dcc-mcp-cli call <instance>__qt_ui_inspector__list_windows --json '{}'

# Snapshot widget tree for a specific window
dcc-mcp-cli call <instance>__qt_ui_inspector__snapshot_tree \
  --json '{"window_title": "Maya"}'

# Describe a specific widget
dcc-mcp-cli call <instance>__qt_ui_inspector__describe_widget \
  --json '{"widget_path": "MayaWindow/QMenuBar"}'
```

## Debug UI with full state capture

```bash
# Capture everything
dcc-mcp-cli call <instance>__ui__capture_ui_state \
  --json '{"output_dir": "/tmp/ui-debug"}'

# Results saved as:
# /tmp/ui-debug/screenshot-<ts>.png
# /tmp/ui-debug/widget-tree-<ts>.json
# /tmp/ui-debug/windows-<ts>.json
```
