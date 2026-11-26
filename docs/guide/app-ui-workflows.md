# App UI Workflows

This document surveys the application-level UI surfaces in dcc-mcp-core: the
built-in Admin Dashboard, the `ui_control` automation tool suite, and CLI
subcommands that interact with UI features. Each section links to the detailed
reference for deeper reading.

## Admin Dashboard

The gateway ships an embedded `/admin` web dashboard (React/Vite source in
`admin-ui/`). At runtime it is a single HTML payload served from the binary;
contributors edit the source under `admin-ui/`, and the build system embeds the
built asset during Cargo compilation.

The dashboard is enabled by default on the elected gateway. It provides:

- **Instances** — connected DCC adapter overview with version diagnostics
- **Tools** — registered tool/action catalogue with search and describe
- **Calls** — live and historical tool call log
- **Traces** — distributed trace viewer
- **Stats** — gateway-level metrics and throughput
- **Workers** — background worker pool status
- **Merged Logs** — aggregated log viewer across all gateway log files
- **Health** — readiness and liveness probes

See [admin-ui.md](admin-ui.md) for activation flags, environment variables,
Python/Rust API configuration, and the full feature walkthrough.

### Enabling and Disabling

```bash
# Default: gateway election on :9765; elected process serves /admin
dcc-mcp-server --app maya

# Disable gateway entirely (also disables admin)
dcc-mcp-server --gateway-port 0

# Keep gateway but disable admin
dcc-mcp-server --no-admin

# Move admin under another prefix
dcc-mcp-server --admin-path /dcc-admin
```

Equivalent environment variables:

| Env var | Default | Description |
|---------|---------|-------------|
| `DCC_MCP_GATEWAY_PORT` | `9765` | Gateway election port. `0` disables gateway/admin. |
| `DCC_MCP_NO_ADMIN` | `false` | Disable the Admin UI on the elected gateway. |
| `DCC_MCP_ADMIN_PATH` | `/admin` | Admin URL prefix. |
| `DCC_MCP_GATEWAY_AUDIT_DIR` | unset | Optional JSONL directory for durable audit and trace persistence. |
| `DCC_MCP_GATEWAY_AUDIT_MAX_ROWS` | `5000` | Max JSONL rows retained per durable file. |
| `DCC_MCP_GATEWAY_AUDIT_MAX_BYTES` | `52428800` | Approx. 50 MiB byte cap per durable JSONL file. |
| `DCC_MCP_LOG_DIR` | platform log dir | Directory scanned by `/admin/api/logs` for `*.log` files. |

**DCC UI Control** is the public cross-platform capability name. DCCMCP is the
control-plane layer and `dcc-cua` is the project-owned execution layer behind
the stable `dccmcp ui-control` route. The DCC-CUA host owns exact-window
binding, screenshots, UI Automation, native input, visible safety effects,
user interruption, and audit records. The `app_ui__*` names remain the
compatibility tool namespace for MCP-native clients and existing adapters. Do
not use the generic name “Computer Use” for this DCC-scoped feature.

A separate analytics dashboard provides KPI timeseries, heatmaps, top-tool
rankings, and CSV/JSONL export. See [analytics-dashboard.md](analytics-dashboard.md).

## UI Control Automation

When a DCC exposes state only through a window, modal dialog, webview, launcher,
or settings panel — rather than a typed API — the `ui_control` tool suite
provides a scoped fallback for interface-only automation.

The canonical workflow follows this loop:

1. **`ui_control__snapshot`** — capture the scoped DCC window, returns `snapshot_id`
2. **`ui_control__find`** — resolve a control by label, text, role, or name
3. **`ui_control__act`** — perform one action (click, set_text, drag, etc.)
4. **`ui_control__wait_for`** — poll inside one tool call until the UI reaches expected state
5. **`ui_control__snapshot`** — verify the final state
6. **`ui_control__stop_computer_use`** — release native input, remove visual effects

Always treat step 6 like a `finally` block — call it on success, failure,
cancellation, or abandonment.

For native application menu bars, `ui_control__act` also supports
`action="invoke_menu"` with an explicit top-to-leaf `menu_path` when a
semantic click or Alt mnemonic cannot prove that a Qt popup opened. This
negotiated exact-window route invalidates current observations; honor
`verification_required` and verify with a fresh snapshot.

See [ui-control-workflows.md](ui-control-workflows.md) for the full reference:
decision rules, evidence attribution, system configuration operations, recovery
patterns, and verification requirements.

### CLI Interface

The `dccmcp ui-control` subcommand exposes the same capabilities through
the shell:

```bash
dccmcp ui-control snapshot --instance-id <id> --json '{"session_id":"ui","process_id":1234}'
dccmcp ui-control act --instance-id <id> --json '{"session_id":"ui","control_id":"ok","action":"click","snapshot_id":"<snapshot_id>"}'
dccmcp ui-control stop --instance-id <id> --json '{"session_id":"ui"}'
```

Before the first snapshot or input action, the DCC-CUA host must report
`provider=dcc-cua`, its runtime version, and the exact target PID and HWND.
The CLI validates and routes these parameters. It does not implement desktop
input. An explicit DCC-CUA request for a browser or another non-DCC application
still uses this project-owned `ui-control` route and the same binding contract.

Windows supplies an isolated, per-logon-session
`dcc-mcp-ui-control-host.exe`. The adapter is a versioned named-pipe proxy; the
host owns exact-window selection, shared-memory screenshots, UIA, input,
visible safety effects, interruption, confirmation, and redacted audit.
macOS and Linux adapters keep the same CLI and tool contract while implementing
equivalent platform boundaries.

Enter `app_ui` only after a typed DCC tool returns `unsupported` or
`capability_missing`. Keep every action scoped to the exact DCC window. The
adapter or operator must bind that target with
`DCC_MCP_APP_UI_UIA_PROCESS_ID` or `DCC_MCP_APP_UI_UIA_WINDOW_HANDLE` before a
Windows UIA mutation; request arguments may narrow that scope but must not
widen it. The host visibly asks the user to approve the resolved exact window
before it mints an opaque capability. The bound session supplies the visible
capsule, shared-memory screenshot, and user interruption monitor even for
semantic UIA. Raw pointer and keyboard input has
a second gate: the operator must also set
`DCC_MCP_COMPUTER_USE_ALLOW_RAW_INPUT=true`.

The native session treats the bound PID/HWND as a separate authorization
scope. It refuses unbound construction and revalidates the resolved process and
window before the capsule, every capture, and every action; title-only and
process-name scopes never authorize native input.

Use native coordinate input only when semantic UIA returns
`unsupported_action`. Host, UIA, or capture unavailability never selects an
alternate input path. Do not fall back after
`policy_disabled`, `permission_denied`, `invalid_target`, `missing_window`,
`user_interrupted`, or `desktop_unavailable`.

Use this loop:

1. Call `app_ui__snapshot` for the exact PID or HWND and inspect the returned
   screenshot and `snapshot_id`.
2. Prefer `app_ui__find` and a semantic control action. Use screenshot
   coordinates only when no stable semantic control exists.
3. Perform exactly one `app_ui__act` with the latest `snapshot_id`.
4. Take a new `app_ui__snapshot` immediately after the action. Never reuse
   coordinates, control ids, or observation ids from an older screenshot.
5. Repeat one action at a time, then call `app_ui__stop_computer_use` on every
   success, failure, cancellation, or abandonment path.

The required lifecycle is `snapshot -> find -> one act -> snapshot -> stop`.
Results such as `policy_disabled`, `permission_denied`, `authentication`,
`confirmation`, `desktop_unavailable`, and `user_interrupted` stop the
workflow. Do not switch to a generic Computer Use provider, retry after
`Ctrl+Alt+Esc`, or change the target session.

If the exact target still exists but is minimized or hidden, the first
snapshot can fail before producing an id. Do not widen the scope or use desktop
input. Call `app_ui__act(action="get_window_state")` with the same trusted
PID/HWND, then use only the required `restore_window`, `show_window`, and
`activate_window` operations. These pre-snapshot operations address the
already confirmed opaque window capability, are revalidated and audited by
the host, and invalidate any older observation. Take a fresh snapshot after
the window becomes visible, non-minimized, and foreground.

The visible corner brackets, control capsule, and pointer effects belong to the adapter
host's interactive Windows logon session. If the user presses `Ctrl+Alt+Esc`
and the tool returns `user_interrupted`, stop immediately. Ordinary `Esc`
remains available to the target DCC. Do not retry, change
`session_id`, or start another session. Set `resume_computer_use=true` only
after the user explicitly asks to resume. The flag cannot approve resumption:
the isolated host always presents its own trusted confirmation before clearing
the global stop latch.

### Lock, RDP, and Display Changes

- Treat `desktop_unavailable` as a pause. Windows is locked, disconnected, or
  showing a secure desktop, so no UIA or raw input runs. Stop issuing UI calls,
  ask the user to unlock or reconnect, and do not poll autonomously. Keep the
  logical `session_id` and continue only with non-UI tools whose host remains
  ready.
- After unlock or RDP reconnect, discard every prior snapshot, observation,
  control id, and coordinate. Take a fresh exact-target snapshot before acting;
  the successful snapshot restores the visible DCC UI Control effects.
- DCC UI Control runs on the adapter host in the interactive logon session that
  owns the DCC. The gateway only routes calls. Never reuse coordinates captured
  on the gateway, another host, or another Windows session.
- A screenshot is bounded to the scoped target window, never the whole desktop.
  That window may span monitors with negative virtual-desktop origins or
  different DPI. Any monitor topology, resolution, DPI/scaling,
  window-position, or window-size change invalidates the observation and
  requires a fresh snapshot. Coordinates are relative to the returned PNG,
  not global desktop coordinates.

Never target LockApp, Windows Security, credential/authentication/password
manager windows, the Windows Run dialog, terminals, PowerShell, or `cmd`. These
backend-enforced boundaries cannot be bypassed with another UI automation
method. A script editor hosted inside the bound DCC process is not a terminal
target.

`app_ui__act` carries a destructive annotation and may declare an `intent` that
only raises policy. The isolated host also classifies the UIA/focused/pointed
control and keyboard chord. It presents a host-owned Windows dialog for tier
2/3 actions and returns `approval_required` when confirmation is denied or
unavailable. No model-supplied `confirmed=true`, `approved`, or environment
flag can resolve confirmation.

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
