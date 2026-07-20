---
name: app-ui
description: >-
  Infrastructure skill - application UI observation and scoped action tools for
  DCC-adjacent workflows. Use app_ui__snapshot, app_ui__find, app_ui__act,
  app_ui__wait_for, and app_ui__stop_computer_use for DCC UI Control when
  a host UI state is not exposed through native DCC APIs. Prefer DCC-native
  skills first, then use app_ui as a policy-controlled UI fallback.
license: MIT
metadata:
  dcc-mcp:
    dcc: python
    version: "0.2.0"
    layer: infrastructure
    search-hint: "dcc ui control, ui-control, app ui, ui automation, operate control menu dialog window button click keyboard, windows uia, chrome cdp, edge cdp, agent-browser, modal, settings panel, screenshot, snapshot, find control, drag, scroll, type, keypress, wait for ui, stale control, dcc debugging, 操作, 控制, 界面, 菜单, 弹窗, 窗口, 按钮, 点击, 键盘"
    tags: "app-ui, dcc-ui-control, ui-control, ui-automation, windows-uia, chrome-cdp, edge-cdp, agent-browser, diagnostics, infrastructure, mock"
    tools: tools.yaml
---

# DCC UI Control

Application UI automation primitives for cases where native DCC tools cannot
observe or drive the interface state directly.

**DCC UI Control** is the public capability name. The skill directory and
`app_ui__*` tool identifiers remain unchanged for compatibility. Shell agents
use the stable `dcc-mcp-cli ui-control` command; MCP-native agents call the
underlying tools after search and describe.

`app-ui` is an escape hatch, not the first tool choice. Discover and call a
structured DCC skill, host API, or adapter script first. Enter `app-ui` only
when that path reports `unsupported` or `capability_missing`. Policy denial,
user interruption, authentication, or desktop unavailability are stop
conditions, not fallback signals.

The default backend is deterministic mock state for CI and adapter authoring.
Set `DCC_MCP_APP_UI_BACKEND=chrome` to use the experimental CDP backend through
the same `app_ui__*` contract.

Set `DCC_MCP_APP_UI_BACKEND=windows-uia` on Windows to use the isolated
`dcc-mcp-ui-control-host.exe`. Bind it at adapter startup with exactly one
`DCC_MCP_APP_UI_UIA_PROCESS_ID` or `DCC_MCP_APP_UI_UIA_WINDOW_HANDLE`;
request parameters may narrow that scope but cannot create or widen it.
Whole-desktop and title/process-name-only native sessions are disabled.

## Windows Reference Backend

The Windows backend exposes DCC UI Control through the existing `app_ui` tools:

| DCC UI Control operation | DCC-MCP tool |
|------------------------|--------------|
| `screenshot` | `app_ui__snapshot` |
| `click`, `move`, `double_click`, `scroll`, `drag`, `type`, `keypress` | `app_ui__act` |
| `wait` | `app_ui__wait_for` (condition-based polling) |
| `stop` | `app_ui__stop_computer_use` |

Shell agents use the product-level wrapper, which maps to those compatibility
tools without requiring a hand-built slug:

```bash
dcc-mcp-cli ui-control snapshot --instance-id <id> --json '{"session_id":"ui","process_id":1234}'
dcc-mcp-cli ui-control find --instance-id <id> --json '{"session_id":"ui","label":"Settings"}'
dcc-mcp-cli ui-control act --instance-id <id> --json '{"session_id":"ui","control_id":"settings","action":"click","snapshot_id":"<snapshot_id>"}'
dcc-mcp-cli ui-control wait --instance-id <id> --json '{"session_id":"ui","condition":{"kind":"control_exists","label":"Preferences"}}'
dcc-mcp-cli ui-control stop --instance-id <id> --json '{"session_id":"ui"}'
```

The CLI prints compact JSON by default: it keeps routing ids, messages/errors,
observation ids, snapshot metadata, semantic matches, and materialized image
paths while omitting the repeated MCP envelope and full UIA tree. Add
`--full-output` only for targeted raw protocol or tree diagnostics.

All app-ui tools require the adapter's persistent in-process executor so one
thin named-pipe client survives across snapshot/action calls. The independent
per-Windows-session host owns screenshots, UIA, observation ids, the
active-session Esc latch, visible overlay, global input owner, confirmation, and
native input; adapters do not retain an alternate native path.
Every snapshot, find, action, wait, stop, and rejected operation also appends a
redacted `ui_control_operation` event to the shared DCC-MCP log directory, so
the existing Admin Logs panel can display it without exposing entered text or
screenshot coordinates.

Use semantic UI Automation first: resolve a stable `control_id` with
`app_ui__find`, then use `click`, `set_text`, `toggle`, `set_checked`,
or `focus`. Use screenshot coordinates and native input only when the required
control is not exposed semantically.

The adapter/operator must bind a trusted DCC target with
`DCC_MCP_APP_UI_UIA_PROCESS_ID` or `DCC_MCP_APP_UI_UIA_WINDOW_HANDLE` before
any Windows snapshot or mutation. The host resolves an exact PID/HWND,
validates the caller's Windows session, user, and integrity level, then starts
the prominent non-modal control notice before minting its opaque capability.
Routine session start does not open a confirmation dialog.

The native DCC UI Control boundary imports that PID/HWND as a separate trusted
scope, rejects construction without it, and revalidates the resolved native
identity before the capsule, every capture, and every action. A request-supplied
title, process name, PID, or HWND cannot authorize a different process.

Before a semantic mutation, the host re-resolves the actual descendant
control and checks it plus every ancestor back to the scoped root. Password
controls, cross-process descendants, and authentication or credential subtrees
are hard denied even when the outer DCC window itself is allowed.

Raw pointer and keyboard input have a second gate and are disabled by default:
the operator must also set `DCC_MCP_COMPUTER_USE_ALLOW_RAW_INPUT=true` in the
adapter process. This environment variable is a hard ceiling: a request cannot
enable `allow_raw_coordinates` or `allow_keyboard_shortcuts` while it is unset
or false. Request PID, HWND, title, policy, and environment scopes are
intersected; a request can narrow the trusted target but cannot replace it
with another application. Process-name scopes are observation-only. Never
widen the scope to the desktop.

`app_ui__act` advertises a destructive annotation and accepts an optional
`intent` consequence hint. The native host independently classifies the UIA
control, focused/pointed control, keyboard chord, and requested intent; the hint
can only raise the tier. Tier 2/3 operations use a trusted host-owned Windows
confirmation dialog. There is no `confirmed`, `approved`, or environment-based
approval field. A missing or denied confirmation returns `approval_required`.

With that operator-bound exact scope, `app_ui__snapshot` returns a bounded PNG
through versioned shared memory plus a UIA tree, even when raw input is
disabled. Host absence, protocol mismatch, UIA failure, and capture failure
fail closed with no in-process or alternate-input fallback.

If that exact HWND is valid but minimized or hidden, do not search the desktop
or switch input backends. `app_ui__act` supports the pre-snapshot actions
`get_window_state`, `restore_window`, `show_window`, and `activate_window`.
They carry the existing task grant and opaque HWND capability, never accept a
replacement target, and use no pointer or keyboard input. The host revalidates
PID/HWND ownership and hard target policy, audits each operation, and
invalidates any old observation. After recovery, take a fresh snapshot before
interacting with content.

For native DCC UI Control actions, keep one `session_id` and use this loop:

1. Call `app_ui__snapshot` with the exact target scope. It returns a PNG image,
   a `snapshot_id`, and observation metadata for that window generation.
2. Inspect the screenshot and UIA tree. Prefer a semantic action when a stable
   control is available.
3. For visual fallback, call `app_ui__act` with the same `snapshot_id` and
   screenshot-relative `x`/`y` or `path` values. Use `text` for `type` and
   `keys` for `keypress`.
4. Call `app_ui__snapshot` after every native action. Each native action
   consumes its observation; a newer snapshot or a moved/resized window makes
   old coordinates stale.
5. Use `app_ui__wait_for` for a UI condition, then snapshot again to verify.
6. Call `app_ui__stop_computer_use` in the success, failure, and abandoned-task
   cleanup path so the capsule, corner brackets, hotkey, and global input owner are
   released. If it returns `cleanup_pending=true`, retry cleanup and do not
   start another session; the cross-process input owner remains fenced until
   every pending key/button release is confirmed. Stopping does not clear an
   Esc interruption latch created during an active UI Control session.

`app_ui__wait_for` remains interruptible while polling: active-session Esc, an
explicit `app_ui__stop_computer_use`, desktop loss, or backend cleanup cancels
the wait without waiting for its condition timeout.

Agents should enter this loop only after a structured DCC operation returns
`unsupported` or `capability_missing`; they should not ask the user to manually
perform that missing GUI step. Keep native calls scoped to the same
`process_id` or `window_handle`.
Perform one action at a time and re-observe after every action; never chain
guessed coordinates from an old image.

Coordinates are pixels in the returned PNG, which may be scaled down for a
bounded MCP payload; they are not desktop coordinates. Never reuse them across
actions or snapshots. On `stale_control` or
`stale_observation`, restart from `app_ui__snapshot`.

The native session requires a visible, unlocked interactive desktop, a live
target window, and the adapter and DCC process at the same Windows integrity
level. While input control is active, click-through corner brackets mark the
target window and a bottom-center capsule reads `DCC UI Control · <app> |
Esc to stop`. Pointer actions display a transient
cursor marker (and a following marker during drag) so the user can see where
the agent is acting. While UI Control is active, the user stops control with
`Esc`; outside an active session, `Esc` behaves normally. On
`user_interrupted`, stop immediately, do
not retry, do not switch to another input path, do not change `session_id`, and
do not automatically start a new DCC UI Control session. The Esc stop is latched across
all DCC adapter processes in the same Windows logon session. Return control to
the user. Resume only through an explicit `app_ui__snapshot` call with
`resume_computer_use=true`. That flag only requests the flow: the native host
always displays its own confirmation surface before clearing the global latch,
so a model or adapter cannot approve itself. The native backend releases any held keys
or mouse buttons before allowing more input. If Windows disconnects after a
partial injection, those releases remain pending and retain the global input
owner until reconnect makes them confirmable.

### Windows desktop availability

Lock, disconnect, and secure-desktop transitions pause live UIA and raw input.
They return `desktop_unavailable` without sending input or ending the logical
`app_ui` session. Stop issuing UI operations, ask the user to unlock or
reconnect, and do not poll autonomously. Keep the same `session_id`; structured
DCC skills and MCP calls may continue only while the host adapter remains
ready.

Never target LockApp, Windows Security, credential/authentication/password
manager windows, the Windows Run dialog, terminals, PowerShell, or `cmd`.
These are hard backend-enforced boundaries and must not be bypassed by another
UI automation path. A DCC application's own script editor remains in scope
because its target process is still the bound DCC process.

An ordinary user process cannot display the DCC UI Control capsule over the
Windows lock screen or secure desktop. After the user unlocks or reconnects,
discard all prior snapshot, observation, and control ids and call
`app_ui__snapshot` again with the same exact target scope. A successful fresh
snapshot re-establishes the corner brackets/capsule; `resume_computer_use` is still only
for an explicit post-interruption resume. Run the DCC in a dedicated, always-unlocked VM
when Windows GUI control must continue without interruption.

DCC UI Control executes on the adapter host, inside the specific interactive
Windows logon session that owns the DCC process. A central gateway routes the
tool call; it does not own the screenshot coordinate space. Never apply
coordinates captured on the gateway, another machine, or another logon session
to a remote DCC. An RDP disconnect or Windows session switch returns
`desktop_unavailable` and retains the logical session. Reconnect to the DCC's
session, then take a fresh exact-target snapshot before any UI action.

The returned image is bounded to the scoped target window; it is never a
whole-desktop screenshot. That window may occupy or span monitors whose virtual
desktop origins are negative and whose DPI scales differ. Continue to send
coordinates relative to the returned PNG; the backend maps them through that
observation's source rectangle and DPI metadata. A
monitor add/remove, display-layout change, resolution change, or DPI/scaling
change invalidates the observation. Discard its ids and take a fresh snapshot.
The capsule follows the scoped target window across monitors in that same logon
session.

CDP presets:

- `DCC_MCP_APP_UI_CDP_PRESET=reuse` (default): attach to an existing DevTools
  endpoint first so the current browser profile, cookies, and tokens can be
  reused. Set `DCC_MCP_APP_UI_CDP_URL` for an explicit HTTP or WebSocket CDP
  endpoint, or expose Chrome on `DCC_MCP_APP_UI_CDP_PORT` / port `9222`.
- `DCC_MCP_APP_UI_CDP_PRESET=isolated`: launch Chrome with a temporary
  `--user-data-dir` for hermetic tests and demos.
- `DCC_MCP_APP_UI_CDP_PRESET=auroraview`: attach to AuroraView's CDP endpoint.
  It uses `DCC_MCP_APP_UI_AURORAVIEW_CDP_PORT`, then `AURORAVIEW_CDP_PORT`,
  then `DCC_MCP_APP_UI_CDP_PORT`, and finally port `9222`.
- `DCC_MCP_APP_UI_CDP_PRESET=edge`: attach to or launch Microsoft Edge via
  CDP. It uses `DCC_MCP_APP_UI_EDGE_CDP_URL` / `_PORT` before the shared CDP
  URL/port, and `DCC_MCP_APP_UI_EDGE_PATH` when launching.
- `DCC_MCP_APP_UI_CDP_PRESET=agent-browser`: use Vercel's `agent-browser`
  CLI, reading its CDP WebSocket URL through `agent-browser get cdp-url` after
  `agent-browser open about:blank`. Override the binary with
  `DCC_MCP_APP_UI_AGENT_BROWSER_BIN`; this preset is suitable for CI when
  `agent-browser install` has provisioned Chrome for Testing.

## Agent Loop

Use this loop:

1. Try the structured DCC skill, host API, or adapter script.
2. If it returns `unsupported` or `capability_missing`,
   call `app_ui__snapshot` for the exact application window.
3. `app_ui__find` to resolve a control by label, role, text, or object name.
4. `app_ui__act` to perform one scoped action using the resolved control id or
   screenshot coordinates when no semantic control is available.
5. `app_ui__snapshot` immediately to verify the result before another action.
6. Use `app_ui__wait_for` only for a known UI condition, then snapshot again.
7. Call `app_ui__stop_computer_use` when the fallback is complete or abandoned.

If an action returns `stale_control`, restart at `app_ui__snapshot`. If an
action returns `policy_disabled`, prefer a native DCC skill or ask for an
explicit policy change. On `user_interrupted` or `desktop_unavailable`, stop;
do not follow a generic retry or fallback route.

## Workflow Examples

Modal dialog: snapshot the scoped DCC/app window, find the button by label or
role, click with the returned `snapshot_id`, then `wait_for` the button or
dialog root to disappear. Verify completion through a native DCC skill when
possible.

Settings panel: snapshot, find the labeled field or checkbox, `set_text` /
`toggle` / `set_checked`, click Apply, then `wait_for` a status label such as
`Applied` and snapshot again. Typed text is redacted from audit unless policy
allows sensitive values.

Recovery: on `missing_window`, confirm process/window scope instead of widening
to the desktop. On `timeout`, inspect the last snapshot and either wait once
more with a justified budget or switch to host diagnostics.
