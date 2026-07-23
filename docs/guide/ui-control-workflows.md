# UI Control Agent Workflows

`ui_control` is a scoped fallback for interface-only work. Prefer native DCC
skills first: they usually carry stronger schemas, better undo semantics, and
host-aware dispatch. Use `ui_control__*` when the state you need only exists in a
window, modal dialog, webview, launcher, license tool, or settings panel.

## Decision Rule

Use a native DCC tool when:

- The host API exposes the state or action directly.
- The action changes scene data, files, packages, renders, or project state.
- You need reliable batch execution, undo integration, or main-thread host
  semantics.

Use `ui_control` when:

- A typed DCC tool returned `unsupported` or `capability_missing` and the only
  remaining control path is a visible UI surface.
- You need to unblock a DCC-owned modal, wizard, webview, or sidecar control
  that has no typed host capability.

Do not use `ui_control` as a shortcut around a missing typed tool. If the workflow
is common and stable, add a native skill/API first and keep `ui_control` as the
diagnostic or emergency path.

Policy denial, user interruption, lock/disconnect, authentication, and safety
boundaries are stop conditions, not reasons to try another UI path.

## Standard Loop

Every workflow should keep the same shape:

1. `ui_control__snapshot` observes the scoped window and returns `snapshot_id`.
2. `ui_control__find` resolves a control id by label, text, role, or object name.
3. `ui_control__act` performs one action. Pass `snapshot_id` to detect stale
   controls before acting.
4. `ui_control__wait_for` polls inside one tool call until the UI reaches the
   expected state or returns a structured `timeout`.
5. `ui_control__snapshot` verifies the final state.
6. `ui_control__stop_computer_use` releases native input ownership, removes the
   visible DCC UI Control effects, and invalidates the final observation.

Treat step 6 like a `finally` block. Call it when the workflow succeeds, when
any tool fails, and when the agent or user abandons the workflow. The stop tool
is idempotent and remains safe while the desktop is unavailable. A
`cleanup_pending=true` result means Windows has not yet confirmed every pending
key/button release; retry cleanup and do not start another session. The global
input owner stays held across adapter processes until reconnect allows those
releases to drain.

## Evidence Attribution

Every successful snapshot or recording includes `capture_provenance`. Keep it
beside any materialized image or frame sequence instead of inferring origin
from the filename or picture contents. Native Windows screenshot evidence must
report `backend="windows-ui-control-host"` and `pixels_captured=true`; mock and
accessibility-only results do not prove that the native Host captured pixels.

The provenance also reports the logical `session_id`, `snapshot_id`, exact
PID/HWND when available, image dimensions, native capture backend, and whether
the bounded PNG was downscaled. The matching redacted
`ui_control_operation` audit event carries the same `snapshot_id`. It does not
record typed text or screenshot coordinates.

The exact-window PNG intentionally excludes UI Control's capsule, brackets,
and cursor marker because those are separate safety overlay windows. Verify
activation from provenance and audit rather than expecting the overlay inside
the captured DCC pixels. For acceptance telemetry, route every call through
the gateway with `--require-gateway` and one stable `--agent-session-id`; keep
that gateway id distinct from the logical UI Control `session_id`.

For gateway clients, discover and inspect tools before calling:

```json
{"name": "search", "arguments": {"query": "ui_control snapshot", "dcc_type": "maya"}}
{"name": "describe", "arguments": {"tool_slug": "<slug from search>"}}
```

REST clients use the same sequence through `/v1/search`, `/v1/describe`, and
`/v1/call`.

## DCC UI Control Fallback

**DCC UI Control** is the public cross-platform capability name. Shell agents
use `dcc-mcp-cli ui-control`; MCP-native clients use the canonical
`ui_control__*` namespace. Do not use the generic name “Computer Use” for this
DCC-scoped feature.

This release makes a hard cut: the former skill, tool prefix, environment
prefix, and Python/Rust contract names are not accepted. Migrate to
`ui-control`, `ui_control__*`, and `DCC_MCP_UI_CONTROL_*`, then rediscover saved
tool slugs before upgrading. See
[ADR-016](../adr/016-unify-ui-control-naming.md).

The CLI contract is platform-neutral:

```bash
dcc-mcp-cli ui-control snapshot --instance-id <id> --json '{"session_id":"ui","process_id":1234}'
dcc-mcp-cli ui-control act --instance-id <id> --json '{"session_id":"ui","control_id":"ok","action":"click","snapshot_id":"<snapshot_id>"}'
dcc-mcp-cli ui-control record-clip --instance-id <id> --json '{"session_id":"pv","process_id":1234,"duration_ms":5000,"frames_per_second":30,"jpeg_quality":92}'
dcc-mcp-cli ui-control stop --instance-id <id> --json '{"session_id":"ui"}'
```

Route identity has two levels. Always select the intended DCC `instance_id`
first; `session_id` is then logical only inside that adapter connection. Two
Unity, Unreal, Maya, or other DCC instances may both use `session_id="default"`
without colliding: the host creates an opaque connection namespace and keeps
their capabilities, observations, recordings, and disconnect cleanup separate.
When more than one matching DCC is ready, never omit `--instance-id`; verify the
returned route and exact PID/HWND before acting.

Multiple exact-window sessions can remain active in the same Windows logon
session. They share one host-owned input coordinator, one cross-process input
owner, and the same global Esc latch. Requests and native input mutations are
serialized, so concurrent agents cannot inject into different windows at the
same time. Stopping one logical session does not stop another; pressing Esc is
the explicit global stop and invalidates all of them until user-approved resume.

The wrapper prints compact machine-readable JSON by default: routing ids,
message/error, observation ids, bounded snapshot metadata, semantic matches,
and materialized image paths. It omits the repeated MCP envelope and full UIA
tree. Add `--full-output` to one subcommand only when raw protocol or tree
diagnostics are required.

Windows supplies an isolated, per-logon-session
`dcc-mcp-ui-control-host.exe`. The adapter is a versioned named-pipe proxy; the
host owns exact-window selection, shared-memory screenshots, UIA, input,
visible safety effects, interruption, confirmation, and redacted audit.
macOS and Linux adapters keep the same CLI and tool contract while implementing
equivalent platform boundaries.

Enter `ui_control` only after a typed DCC tool returns `unsupported` or
`capability_missing`. Keep every action scoped to the exact DCC window. The
adapter or operator must bind that target with
`DCC_MCP_UI_CONTROL_UIA_PROCESS_ID` or `DCC_MCP_UI_CONTROL_UIA_WINDOW_HANDLE` before a
Windows UIA mutation; request arguments may narrow that scope but must not
widen it. The host starts the prominent, non-modal control capsule before it
mints an opaque capability; routine session start does not open a confirmation
dialog. The bound session supplies the visible capsule, shared-memory screenshot,
and user interruption monitor even for semantic UIA. Raw pointer and keyboard input has
a second gate: the operator must also set
`DCC_MCP_COMPUTER_USE_ALLOW_RAW_INPUT=true`.

The native session treats the bound PID/HWND as a separate authorization
scope. It refuses unbound construction and revalidates the resolved process and
window before the capsule, every capture, and every action; title-only and
process-name scopes never authorize native input.

`ui_control__record_clip` is the exact-window evidence path for gameplay and
other time-based UI proof. It uses one continuous Windows.Graphics.Capture
session and returns a host-owned JPEG sequence plus a manifest containing
per-frame SHA-256 values. Duration is limited to 1–180 seconds, output is
1–60 FPS, and requests cannot select a directory. Esc, stop, desktop loss,
target replacement, size change, or an incomplete write removes the partial
artifact. Recording captures no audio and is not a finished trailer: use the
`game-pv-capture` Skill for shot provenance, then HyperFrames for editing,
titles, original or licensed audio, and delivery encoding. Do not substitute
whole-desktop/title-matched recorders or GPT/OpenAI Computer Use.

Use native coordinate input only when semantic UIA returns
`unsupported_action`. Host, UIA, or capture unavailability never selects an
alternate input path. Do not fall back after
`policy_disabled`, `permission_denied`, `invalid_target`, `missing_window`,
`user_interrupted`, or `desktop_unavailable`.

The Windows backend hard-denies raw `type`. Enter non-sensitive text only with
semantic `set_text` against the exact `control_id` returned by the current UIA
observation. Passwords, authentication codes, and other credential material
require a user hand-off or host-owned secure credential flow. Mock and CDP may
retain backend-specific `type`; it is not a portable Windows action.
Windows `keypress` also rejects ordinary printable characters, Shift-modified
text, and AltGr text. Use it only for navigation/control/function keys or a
genuine Ctrl/Alt shortcut; it cannot be used as one-character text entry.

Use this loop:

1. Call `ui_control__snapshot` for the exact PID or HWND and inspect the returned
   screenshot and `snapshot_id`.
2. Prefer `ui_control__find` and a semantic control action. Use screenshot
   coordinates only when no stable semantic control exists.
3. Perform exactly one `ui_control__act` with the latest `snapshot_id`.
4. Take a new `ui_control__snapshot` immediately after the action. Never reuse
   coordinates, control ids, or observation ids from an older screenshot.
5. Repeat one action at a time, then call `ui_control__stop_computer_use` on every
   success, failure, cancellation, or abandonment path.

If the exact target still exists but is minimized or hidden, the first
snapshot can fail before producing an id. Do not widen the scope or use desktop
input. Call `ui_control__act(action="get_window_state")` with the same trusted
PID/HWND, then use only the required `restore_window`, `show_window`, and
`activate_window` operations. These pre-snapshot operations address the
already confirmed opaque window capability, are revalidated and audited by
the host, and invalidate any older observation. Take a fresh snapshot after
the window becomes visible, non-minimized, and foreground.

The visible corner brackets, control capsule, and pointer effects belong to the adapter
host's interactive Windows logon session. The user stops all active sessions by pressing `Esc`
and the tool returns `user_interrupted`; stop immediately. Do not retry, change
`session_id`, or start another session after that interruption. Set `resume_computer_use=true` only
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

`ui_control__act` carries a destructive annotation and may declare an `intent` that
only raises policy. The isolated host also classifies the UIA/focused/pointed
control and keyboard chord. It presents a host-owned Windows dialog for tier
2/3 actions and returns `approval_required` when confirmation is denied or
unavailable. No model-supplied `confirmed=true`, `approved`, or environment
flag can resolve confirmation.

## Typed Windows System Configuration

Use `ui_control__system_operation` only for plug-in setup that cannot be expressed
through a DCC API or an in-application control. Unlike `ui_control__act`, it opens
a short-lived windowless system session and needs no PID, HWND, or snapshot.
Before the UI Control host starts, the operator must provide an exact grant
catalog through `DCC_MCP_UI_CONTROL_SYSTEM_GRANTS_FILE` and select one grant
through `DCC_MCP_UI_CONTROL_SYSTEM_GRANT_ID`.

```bash
dcc-mcp-cli ui-control system-operation --instance-id <id> \
  --json '{"operation_id":"enable-remote-control"}'
```

The agent supplies only a non-sensitive operation id. The host resolves the
typed operation from its operator-owned catalog, so values and paths do not
enter model context, tool arguments, or the host pipe. The native dialog shows
the resolved target and asks for confirmation; successful calls return
`created`, `updated`, or `unchanged`. The tool supports only HKCU String/DWORD
values and file/directory symbolic links. It cannot run a command, delete or
replace an object, use another registry hive, elevate, or handle credentials. Stop on
`elevation_required`, `approval_required`, or
`system_operation_not_granted`; never fall back to PowerShell, a terminal, or
UAC automation. See [ADR-015](../adr/015-bounded-ui-control-system-operations.md).

After system setup, enable an application's own remote-control checkbox with a
semantic `set_checked`/`click` and `intent: "account_or_access_change"`. The
host independently recognizes remote-control labels and always confirms them.
Non-sensitive account fields may be filled with semantic `set_text` against an
exact `control_id`. Passwords and authentication codes remain a user/host
secure hand-off or an application-owned OAuth/browser flow; raw `type` is not
available on Windows.

## Example: Custom DCC Control or Face Shaping

Prefer a native rig/parameter tool, then try `ui_control__find` for a semantic
slider or handle. If a custom canvas or viewport manipulator exposes no stable
semantic control, the operator may enable raw input and the agent may perform
one observation-fenced drag. Build `path` from the latest bounded screenshot;
`keys` may hold Ctrl, Shift, or Alt when the DCC uses a pointer modifier.

```json
{
  "session_id": "face-shape",
  "action": "drag",
  "button": "left",
  "keys": ["Shift"],
  "path": [{"x": 612, "y": 428}, {"x": 628, "y": 424}, {"x": 646, "y": 419}],
  "duration_ms": 350,
  "snapshot_id": "<latest-snapshot-id>"
}
```

Take a new snapshot immediately after the drag and verify the result before
deriving another path. Never reuse the consumed observation.

## Example: Modal Dialog

Use this when a DCC-native action opened a confirmation dialog that has no host
API equivalent.

Call `ui_control__snapshot` and verify that the root window is the expected dialog.
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

Prefer `ui_control__wait_for` over agent-side polling loops. It keeps retries near
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

`stale_control`: restart at `ui_control__snapshot`, then repeat `find` and `act`
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

For code changes touching `ui_control`, include at least one executable path:

- Unit tests for contract mapping and structured errors.
- A mock-backend workflow test for snapshot -> find -> act -> wait -> verify.
- A VRS trace when gateway `/v1/*` routing or REST envelopes are involved.

The VRS trace `tests/vrs/traces/core-1134-ui-control-mock-workflow.jsonl` pins the
mock backend workflow and recovery envelopes for live gateway runs. It skips
cleanly when no `ui_control__snapshot` capability is registered.
