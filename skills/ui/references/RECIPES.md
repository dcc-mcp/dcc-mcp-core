# UI Recipes

Copy-pasteable UI interaction sequences.

## Find and click a button

```bash
# 1. Start one scoped UI Control session and find the button
dcc-mcp-cli call <instance>__ui_control__snapshot \
  --json '{"session_id":"save-scene"}'
dcc-mcp-cli call <instance>__ui_control__find \
  --json '{"session_id":"save-scene","query":"Save","role":"button"}'

# 2. Run one verified semantic action; this tool always stops the session
dcc-mcp-cli call <instance>__ui__interact_widget \
  --json '{"instance":"<instance>","session_id":"save-scene","widget":{"control_id":"saveButton"},"action":"click"}'
```

## Type text into a field

```bash
# Resolve the field with ui_control__snapshot/find, then set non-sensitive text
dcc-mcp-cli call <instance>__ui__interact_widget \
  --json '{"instance":"<instance>","session_id":"rename-object","widget":{"control_id":"nameField"},"action":"type","value":"myObject"}'
```

## Wait for a dialog and dismiss it

```bash
# 1. Wait for the dialog
dcc-mcp-cli call <instance>__ui__wait_for_ui \
  --json '{"query": "Warning", "timeout_secs": 10}'

# 2. Snapshot and find the OK button through the scoped UI Control session
dcc-mcp-cli call <instance>__ui_control__snapshot \
  --json '{"session_id":"dismiss-warning","window_title":"Warning"}'
dcc-mcp-cli call <instance>__ui_control__find \
  --json '{"session_id":"dismiss-warning","query":"OK","role":"button"}'

# 3. Click OK
dcc-mcp-cli call <instance>__ui__interact_widget \
  --json '{"instance":"<instance>","session_id":"dismiss-warning","widget":{"control_id":"okButton","window_title":"Warning"},"action":"click"}'

# 4. Wait for dialog to disappear
dcc-mcp-cli call <instance>__ui__wait_for_ui \
  --json '{"query": "Warning", "condition": "disappears"}'
```

## Full computer-use fallback loop

```bash
# Use only when structured tools don't cover the operation

# 1. Snapshot current state
dcc-mcp-cli call <instance>__ui_control__snapshot \
  --json '{"session_id":"render-button"}'

# 2. Find target in snapshot
dcc-mcp-cli call <instance>__ui_control__find \
  --json '{"session_id":"render-button","query":"render button"}'

# 3. Act on target
dcc-mcp-cli call <instance>__ui_control__act \
  --json '{"session_id":"render-button","snapshot_id":"<latest-snapshot-id>","action":"click","control_id":"<control-id>"}'

# 4. Verify with another snapshot
dcc-mcp-cli call <instance>__ui_control__snapshot \
  --json '{"session_id":"render-button"}'

# 5. Stop computer use session
dcc-mcp-cli call <instance>__ui_control__stop_computer_use \
  --json '{"session_id":"render-button"}'
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
  --json '{"instance":"<instance>","session_id":"ui-debug","output_dir":"/tmp/ui-debug"}'

# Results include the canonical snapshot and capture_provenance.
# Local artifacts are reused when the UI Control backend provides a path.
# Additional state is saved as:
# /tmp/ui-debug/widget-tree-<ts>.json
# /tmp/ui-debug/windows-<ts>.json
```
