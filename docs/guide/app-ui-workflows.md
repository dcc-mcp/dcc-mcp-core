# App UI Agent Workflows

`app_ui` is a scoped fallback for interface-only work. Prefer native DCC
skills first: they usually carry stronger schemas, better undo semantics, and
host-aware dispatch. Use `app_ui__*` when the state you need only exists in a
window, modal dialog, webview, launcher, license tool, or settings panel.

## Decision Rule

Use a native DCC tool when:

- The host API exposes the state or action directly.
- The action changes scene data, files, packages, renders, or project state.
- You need reliable batch execution, undo integration, or main-thread host
  semantics.

Use `app_ui` when:

- The only available control path is a visible UI surface.
- You need to unblock a modal dialog, wizard, browser view, installer prompt,
  or adapter-owned sidecar app.
- A native tool already reported that manual UI confirmation is required.

Do not use `app_ui` as a shortcut around a missing typed tool. If the workflow
is common and stable, add a native skill/API first and keep `app_ui` as the
diagnostic or emergency path.

## Standard Loop

Every workflow should keep the same shape:

1. `app_ui__snapshot` observes the scoped window and returns `snapshot_id`.
2. `app_ui__find` resolves a control id by label, text, role, or object name.
3. `app_ui__act` performs one action. Pass `snapshot_id` to detect stale
   controls before acting.
4. `app_ui__wait_for` polls inside one tool call until the UI reaches the
   expected state or returns a structured `timeout`.
5. `app_ui__snapshot` verifies the final state.

For gateway clients, discover and inspect tools before calling:

```json
{"name": "search_tools", "arguments": {"query": "app_ui snapshot", "dcc_type": "maya"}}
{"name": "describe_tool", "arguments": {"tool_slug": "<slug from search>"}}
```

REST clients use the same sequence through `/v1/search`, `/v1/describe`, and
`/v1/call`.

## Example: Modal Dialog

Use this when a DCC-native action opened a confirmation dialog that has no host
API equivalent.

Call `app_ui__snapshot` and verify that the root window is the expected dialog.
Then find the confirmation button:

```json
{"session_id": "maya-confirm-export", "label": "Overwrite", "role": "button"}
```

Act only on the resolved control id and current snapshot:

```json
{
  "session_id": "maya-confirm-export",
  "control_id": "overwrite",
  "action": "click",
  "snapshot_id": "<snapshot_id>"
}
```

Wait for the modal to disappear or for the status text to change:

```json
{
  "session_id": "maya-confirm-export",
  "condition": {
    "kind": "control_missing",
    "control_id": "overwrite",
    "timeout_ms": 5000,
    "interval_ms": 100
  }
}
```

Finish by using the native DCC verification tool when one exists. For example,
verify that the exported file or scene state changed through a typed skill,
not only through the UI.

## Example: Settings Panel

Use this when the setting only exists in a preferences panel or webview.

1. Snapshot the scoped application window.
2. Find the setting by visible label, not by index.
3. Set the text, checkbox, or selection.
4. Click the panel's apply/save control.
5. Wait for a stable status message.
6. Snapshot again and verify the setting value.

Mock-backend payloads mirror the intended real workflow:

```json
{"session_id": "settings-demo", "label": "Project name"}
```

```json
{
  "session_id": "settings-demo",
  "control_id": "project-name",
  "action": "set_text",
  "text": "Hero",
  "snapshot_id": "<snapshot_id>"
}
```

```json
{
  "session_id": "settings-demo",
  "condition": {
    "kind": "value_equals",
    "control_id": "project-name",
    "value": "Hero",
    "timeout_ms": 1000,
    "interval_ms": 50
  }
}
```

Typed text should be redacted in audit records unless the adapter policy
explicitly allows sensitive values.

## Example: Wait For UI State

Prefer `app_ui__wait_for` over agent-side polling loops. It keeps retries near
the backend, avoids repeated MCP round trips, and returns one structured
timeout envelope if the state never appears.

Good wait conditions are stable and semantic:

- `text_equals` on a status label such as `Applied` or `Complete`.
- `value_equals` on a text field after an edit.
- `checked_equals` on a checkbox.
- `control_exists` or `control_missing` for modal lifecycle.
- `enabled` or `disabled` for controls that become actionable after work.

Avoid waiting on screen coordinates, pixel colors, or visual order unless the
backend has no accessibility tree and the adapter explicitly documents that
fallback.

## Recovery Examples

`stale_control`: restart at `app_ui__snapshot`, then repeat `find` and `act`
with the new `snapshot_id`. Never retry the same stale control id blindly.

`missing_window`: verify that the intended DCC/app process is still running and
that the backend is scoped to the right window title or process id. If the
window is gone because the workflow completed, switch to a native verification
tool.

`policy_disabled`: stop the UI action. Prefer a native skill, or ask the user
for a narrower policy change such as allowing text entry for one window. Do not
silently broaden to whole-desktop access.

`timeout`: take a fresh snapshot and inspect the last observed UI state. If the
state is still progressing, call `wait_for` once more with a justified timeout.
If the state is blocked, surface the current control/status text to the user or
switch to a host diagnostic skill.

## Verification

For code changes touching `app_ui`, include at least one executable path:

- Unit tests for contract mapping and structured errors.
- A mock-backend workflow test for snapshot -> find -> act -> wait -> verify.
- A VRS trace when gateway `/v1/*` routing or REST envelopes are involved.

The VRS trace `tests/vrs/traces/core-1134-app-ui-mock-workflow.jsonl` pins the
mock backend workflow and recovery envelopes for live gateway runs. It skips
cleanly when no `app_ui__snapshot` capability is registered.
