# UI — Common Errors

Diagnostic patterns for UI interaction failures.

## Widget not found

**Symptom**: `find_widget` returns no results for an obviously visible widget.

Causes:
1. Widget is in a different window than searched
2. Widget type filter is wrong (QToolButton vs QPushButton)
3. Widget text is dynamic/localized
4. Widget hasn't been created yet (lazy-loaded)

```bash
# Broaden search
dcc-mcp-cli call <instance>__ui__find_widget \
  --json '{"query": "", "window_title": "Maya", "limit": 50}'

# Try without type filter
dcc-mcp-cli call <instance>__ui__find_widget \
  --json '{"query": "Save"}'

# Full inspection first
dcc-mcp-cli call <instance>__ui__inspect_ui \
  --json '{"window_title": "Maya", "max_depth": -1}'
```

## Click has no effect

**Symptom**: `interact_widget` returns success but nothing happens.

Causes:
1. Widget is disabled or hidden
2. Wrong widget targeted (overlay or parent container)
3. DCC is busy processing another operation
4. The control ID did not come from the current scoped UI Control session

```bash
# Check widget state before clicking
dcc-mcp-cli call <instance>__qt_ui_inspector__describe_widget \
  --json '{"widget_path": "<path>"}'
# Look for: enabled=true, visible=true

# Verify with screenshot
dcc-mcp-cli call <instance>__dcc_diagnostics__screenshot \
  --json '{"format": "png"}'
```

## Desktop unavailable

**Symptom**: UI control tools fail with `desktop_unavailable`.

The DCC is running headless, minimized, or on a locked screen.
UI automation requires an active desktop session.

- Ensure the DCC window is visible and not minimized
- For headless/CI: UI tools are unavailable — use structured MCP tools only
- Never try to fall back to another input path after `desktop_unavailable`

## User interrupted

**Symptom**: UI action returns `user_interrupted`.

The operator pressed Esc or otherwise cancelled the UI session.

- Stop immediately — do NOT retry
- Do NOT fall back to another input path
- Report to user and wait for explicit instruction to retry

## "requires_in_process: true" error

**Symptom**: UI Control tool fails because it requires in-process execution.

Some UI tools must run inside the DCC process. The adapter must be configured
with `DCC_MCP_UI_CONTROL_UIA_PROCESS_ID` or `DCC_MCP_UI_CONTROL_UIA_WINDOW_HANDLE`.

## Widget tree is stale

**Symptom**: Widget path from earlier inspection no longer exists.

UI state changes between inspection and action. Always:
1. Inspect immediately before acting
2. Verify widget exists with `wait_for_ui` before interactive operations
3. Re-inspect after state-changing actions

## DPI scaling issues

**Symptom**: Clicks miss by a consistent offset.

On high-DPI displays, coordinate scaling may be incorrect.

- Prefer semantic targeting (control_id, widget_path) over raw coordinates
- If coordinates are unavoidable, capture a fresh screenshot and derive
  coordinates from the current frame
- Never reuse coordinates from a previous snapshot
