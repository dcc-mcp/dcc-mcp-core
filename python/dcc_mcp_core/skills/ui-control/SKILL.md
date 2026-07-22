---
name: ui-control
description: >-
  Infrastructure skill - application UI observation and scoped action tools for
  DCC-adjacent workflows. Use ui_control__snapshot, ui_control__find, ui_control__act,
  ui_control__wait_for, and ui_control__stop_computer_use for DCC UI Control when a
  host UI state is not exposed through native DCC APIs. Use
  ui_control__record_clip for exact-window, hash-verified gameplay capture. Use the separate,
  operator-granted ui_control__system_operation only for bounded Windows plug-in
  setup. Prefer DCC-native skills first, then use ui_control as a policy-controlled
  fallback.
license: MIT
metadata:
  dcc-mcp:
    dcc: python
    version: "0.4.0"
    layer: infrastructure
    search-hint: "dcc ui control, ui-control, ui control, ui automation, exact window recording, gameplay capture, game pv capture, jpeg sequence, frame hash, operate control menu dialog window button click keyboard, windows uia, chrome cdp, edge cdp, agent-browser, modal, settings panel, screenshot, snapshot, find control, custom control, face shaping, sculpt slider, modifier drag, registry, symlink, plugin setup, remote control, scroll, type, keypress, wait for ui, stale control, dcc debugging, operate maya menu, click button in dialog, fill form in 3ds max, 操作, 控制, 界面, 菜单, 弹窗, 窗口, 按钮, 点击, 键盘, 录制游戏, 游戏PV, 捏脸, 操控界面, 点击菜单, 自动化窗口, 界面自动化, 窗口操作, 控件识别"
    tags: "ui-control, dcc-ui-control, ui-automation, exact-window-recording, gameplay-capture, game-pv, windows-uia, chrome-cdp, edge-cdp, agent-browser, diagnostics, infrastructure, mock, maya-ui, blender-ui, 3dsmax-ui, houdini-ui, photoshop-ui, unreal-ui, unity-ui, zbrush-ui"
    tools: tools.yaml
---

# DCC UI Control

Application UI automation primitives for cases where native DCC tools cannot
observe or drive the interface state directly.

**DCC UI Control** is the public capability name. The canonical skill is
`ui-control`, its tools use the `ui_control__*` prefix, and configuration uses
`DCC_MCP_UI_CONTROL_*`. Shell agents use `dcc-mcp-cli ui-control`; MCP-native
agents call the underlying tools after search and describe.

`ui-control` is an escape hatch, not the first tool choice. Discover and call a
structured DCC skill, host API, or adapter script first. Enter `ui-control` only
when that path reports `unsupported` or `capability_missing`. Policy denial,
user interruption, authentication, or desktop unavailability are stop
conditions, not fallback signals.

The default backend is deterministic mock state for CI and adapter authoring.
Set `DCC_MCP_UI_CONTROL_BACKEND=chrome` to use the experimental CDP backend through
the same `ui_control__*` contract.

Set `DCC_MCP_UI_CONTROL_BACKEND=windows-uia` on Windows to use the isolated
`dcc-mcp-ui-control-host.exe`. Bind it at adapter startup with exactly one
`DCC_MCP_UI_CONTROL_UIA_PROCESS_ID` or `DCC_MCP_UI_CONTROL_UIA_WINDOW_HANDLE`;
request parameters may narrow that scope but cannot create or widen it.
Whole-desktop and title/process-name-only native sessions are disabled.

Starting with dcc-mcp-core 0.19.65, Host resolution is version-exact and
fail-closed. If `DCC_MCP_UI_CONTROL_HOST` is present, it must be an absolute
path to a Windows PE whose `--version` output exactly matches the running
dcc-mcp-core version. If the variable is absent, the client downloads only
`dcc-mcp-update-manifest-windows-x86_64.json` and
`dcc-mcp-ui-control-host-windows-x86_64.exe` from the matching
`dcc-mcp/dcc-mcp-core` GitHub Release tag, requires the manifest version and
asset URL to match that tag, verifies SHA-256, and stores the Host in a
per-user, per-version cache. Concurrent adapter processes share a download
lock. Offline use is allowed only when that exact cached Host and manifest
still pass the same checks; network, proxy, checksum, or version failures
return `backend_unavailable` and never fall back to another Host or input path.
The discovery pipe and per-session singleton are bound to protocol v2, the
strict package version, and the full Host binary SHA-256. An older detached
Host or a different same-version binary therefore cannot capture a new
client; byte-identical copies at different paths may share one Host. Discovery
coexistence does not widen native input authority: the input-owner mutex and
Esc interruption latch remain version-neutral and shared across all Hosts.

## Windows Reference Backend

The Windows backend exposes DCC UI Control through the existing `ui_control` tools:

| DCC UI Control operation | DCC-MCP tool |
|------------------------|--------------|
| `screenshot` | `ui_control__snapshot` |
| semantic `click`, `set_text`, `toggle`, `set_checked`, `select_option`, `focus` | `ui_control__act` with an exact `control_id` |
| raw `click`, `move`, `double_click`, `scroll`, `drag`, `keypress`, `game_navigation` | `ui_control__act` with the latest `snapshot_id` |
| typed HKCU value or symbolic-link ensure | `ui_control__system_operation` |
| exact-window JPEG frame sequence | `ui_control__record_clip` |
| `wait` | `ui_control__wait_for` (condition-based polling) |
| `stop` | `ui_control__stop_computer_use` |

Shell agents use the product-level wrapper, which maps to those tools without
requiring a hand-built slug:

```bash
dcc-mcp-cli ui-control snapshot --instance-id <id> --json '{"session_id":"ui","process_id":1234}'
dcc-mcp-cli ui-control find --instance-id <id> --json '{"session_id":"ui","label":"Settings"}'
dcc-mcp-cli ui-control act --instance-id <id> --json '{"session_id":"ui","control_id":"settings","action":"click","snapshot_id":"<snapshot_id>"}'
dcc-mcp-cli ui-control system-operation --instance-id <id> --json '{"operation_id":"enable-remote-control"}'
dcc-mcp-cli ui-control record-clip --instance-id <id> --json '{"session_id":"pv","process_id":1234,"duration_ms":5000,"frames_per_second":30,"jpeg_quality":92}'
dcc-mcp-cli ui-control wait --instance-id <id> --json '{"session_id":"ui","condition":{"kind":"control_exists","label":"Preferences"}}'
dcc-mcp-cli ui-control stop --instance-id <id> --json '{"session_id":"ui"}'
```

Treat `instance_id` and `session_id` as separate routing layers. Select the
exact DCC `instance_id` first; the logical `session_id` belongs only to that
adapter connection. Different DCC instances may reuse `default` because the
native host assigns a private connection namespace. Capabilities,
observations, recordings, stop, and disconnect cleanup never cross that
namespace. When multiple matching instances are ready, omitting
`--instance-id` is an error in the workflow even if a client could guess one.

Multiple exact-window sessions may stay active in one Windows logon session.
They share one native input coordinator and global Esc latch, and all input
mutations remain serialized. A normal stop releases only the selected logical
session; Esc interrupts every active session until explicit user-approved
resume. Never use multiple sessions as a way to run simultaneous keyboard or
pointer injection.

`system-operation` is a separate, windowless setup path. Before the shared
Windows-session host starts, the operator supplies an exact JSON catalog with
`DCC_MCP_UI_CONTROL_SYSTEM_GRANTS_FILE` and selects one entry with
`DCC_MCP_UI_CONTROL_SYSTEM_GRANT_ID`; changing either requires a host restart.
Each request carries only a non-sensitive `operation_id`; the native host
resolves the corresponding typed operation locally and still requires native
action-time confirmation. Registry values and link paths never enter model
context, tool arguments, or the host pipe. Only HKCU String/DWORD values and
file/directory symbolic links are supported. The operator-owned catalog is
trusted configuration, not a credential store; use an opaque credential broker
or a host-owned secure prompt for secrets. There is no command, deletion,
overwrite, alternate-hive, elevation, UAC, or security-settings form. Stop on
`system_operation_not_granted`, `approval_required`, or `elevation_required`.

The CLI prints compact JSON by default: it keeps routing ids, messages/errors,
observation ids, snapshot metadata, semantic matches, and materialized image
paths while omitting the repeated MCP envelope and full UIA tree. Add
`--full-output` only for targeted raw protocol or tree diagnostics.

`ui_control__record_clip` is the canonical evidence-capture primitive, not an
alternate UI-input path. It records only the exact PID/HWND already bound by the
operator, for 1 to 180 seconds at 1 to 60 FPS, through one continuous
Windows.Graphics.Capture session. The native host chooses the output directory,
writes numbered JPEG frames and per-frame SHA-256 values, then commits the
manifest last. Requests cannot choose a path or widen the target. Esc, stop,
desktop loss, target replacement, dimension change, or an incomplete write
fails closed and removes the partial recording. Recording consumes any prior
observation, so take a fresh snapshot before later UI actions.

The primitive intentionally captures no audio and does not edit or encode a
finished trailer. Use `game-pv-capture` to copy and hash approved shot ranges and
produce capture provenance; use HyperFrames afterward for editorial timing,
titles, transitions, original or licensed audio, and final delivery encoding.
Neither workflow may replace this primitive with title matching, whole-desktop
capture, an external recorder, or GPT/OpenAI Computer Use.

All ui-control tools require the adapter's persistent in-process executor so one
thin named-pipe client survives across snapshot/action calls. The independent
per-Windows-session host owns screenshots, UIA, observation ids, the
Esc stop latch, visible overlay, global input owner, confirmation, and
native input; adapters do not retain an alternate native path.
Every snapshot, recording, find, action, wait, stop, and rejected operation also appends a
redacted `ui_control_operation` event to the shared DCC-MCP log directory, so
the existing Admin Logs panel can display it without exposing entered text or
screenshot coordinates.

Use semantic UI Automation first: resolve a stable `control_id` with
`ui_control__find`, then use `click`, `set_text`, `toggle`, `set_checked`,
or `focus`. Use screenshot coordinates and native input only when the required
control is not exposed semantically.

### Visual Overlay Enhancements

The DCC UI Control visual overlay includes these features:

- **Session color coding**: When a `session_id` is provided to
  `ui_control__snapshot`, each session gets a distinct capsule, corner bracket,
  and cursor ring color from a 16-color palette. The same `session_id` always
  produces the same color. This makes multi-session scenarios (e.g., parallel
  Maya and Blender control) visually distinguishable at a glance.

- **Last-action marker**: A small fading dot (about 16px) appears at the last
  click, double-click, drag, or move point and fades to transparent over
  approximately 2 seconds. This gives the user immediate visual feedback on
  where the agent last acted.

- **Scope animation**: When a target window is first scoped with
  `ui_control__snapshot`, the corner brackets pulse briefly for about 1.5
  seconds before settling into the normal breathing rhythm. This provides
  clear visual confirmation that the DCC window has been captured.

The adapter/operator must bind a trusted DCC target with
`DCC_MCP_UI_CONTROL_UIA_PROCESS_ID` or `DCC_MCP_UI_CONTROL_UIA_WINDOW_HANDLE` before
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

The Windows backend hard-denies raw `type`. Enter non-sensitive text only with
semantic `set_text` against an exact `control_id`; passwords, authentication
codes, and other credential material require a user hand-off or a host-owned
secure credential flow. The mock and CDP backends may retain backend-specific
`type` support, so callers must not treat it as a portable Windows action.
Windows `keypress` also rejects ordinary printable characters, including
Shift-modified and AltGr text. Use it only for navigation/control/function
keys or a genuine Ctrl/Alt shortcut; it is not a one-character text-entry
bypass.

Windows `game_navigation` is a separate raw-input contract for non-editable
game surfaces. It accepts exactly one unmodified `W`, `A`, `S`, or `D` key and
an optional `duration_ms` from 0 through 500 (omitted means a tap). The native
host rechecks the exact PID/HWND, foreground window, focused non-editable UIA
ancestry, explicit absence of both UIA ValuePattern and TextPattern, and
observation immediately before key-down. Unknown pattern metadata fails closed.
This action does not relax the ordinary printable-key denial on `keypress`.

`ui_control__act` advertises a destructive annotation and accepts an optional
`intent` consequence hint. The native host independently classifies the UIA
control, focused/pointed control, keyboard chord, and requested intent; the hint
can only raise the tier. Tier 2/3 operations use a trusted host-owned Windows
confirmation dialog. There is no `confirmed`, `approved`, or environment-based
approval field. A missing or denied confirmation returns `approval_required`.

With that operator-bound exact scope, `ui_control__snapshot` returns a bounded PNG
through versioned shared memory plus a UIA tree, even when raw input is
disabled. Host absence, protocol mismatch, UIA failure, and capture failure
fail closed with no in-process or alternate-input fallback.

If that exact HWND is valid but minimized or hidden, do not search the desktop
or switch input backends. `ui_control__act` supports the pre-snapshot actions
`get_window_state`, `restore_window`, `show_window`, and `activate_window`.
They carry the existing task grant and opaque HWND capability, never accept a
replacement target, and use no pointer or keyboard input. The host revalidates
PID/HWND ownership and hard target policy, audits each operation, and
invalidates any old observation. After recovery, take a fresh snapshot before
interacting with content.

For native DCC UI Control actions, keep one `session_id` and use this loop:

1. Call `ui_control__snapshot` with the exact target scope. It returns a PNG image,
   a `snapshot_id`, and observation metadata for that window generation.
2. Inspect the screenshot and UIA tree. Prefer a semantic action when a stable
   control is available. On Windows, enter non-sensitive text only with
   `set_text` and that control's exact `control_id`.
3. For visual fallback, call `ui_control__act` with the same `snapshot_id` and
   screenshot-relative `x`/`y` or `path` values. Use `keys` for `keypress` or
   Ctrl/Shift/Alt modifiers held during a pointer action. Windows `keypress`
   accepts navigation/control/function keys and genuine Ctrl/Alt shortcuts,
   not printable text. Do not use raw `type` on Windows.
4. Call `ui_control__snapshot` after every native action. Each native action
   consumes its observation; a newer snapshot or a moved/resized window makes
   old coordinates stale.
5. Use `ui_control__wait_for` for a UI condition, then snapshot again to verify.
6. Call `ui_control__stop_computer_use` in the success, failure, and abandoned-task
   cleanup path so that logical session's capsule and corner brackets are
   released. The shared hotkey and global input owner remain active while any
   other exact-window session is active. If stop returns `cleanup_pending=true`,
   retry cleanup and do not start another session; the cross-process input owner
   remains fenced until every pending key/button release is confirmed. Stopping
   does not clear an Esc interruption latch created during a UI Control session.

`ui_control__wait_for` remains interruptible while polling: Esc, an
explicit `ui_control__stop_computer_use`, desktop loss, or backend cleanup cancels
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
`stale_observation`, restart from `ui_control__snapshot`.

The native session requires a visible, unlocked interactive desktop, a live
target window, and the adapter and DCC process at the same Windows integrity
level. While input control is active, click-through corner brackets mark the
target window and a bottom-center capsule reads `DCC UI Control · <app> |
Esc to stop`. The capsule, brackets, and cursor ring use a
session-specific color when a `session_id` is provided to
`ui_control__snapshot`, so multiple parallel sessions are visually distinct.
Pointer actions display a transient cursor marker (and a following marker
during drag) and a small fading dot at the action point so the user can see
where the agent is acting. The corner brackets briefly pulse when the target
is first scoped. The user stops control by pressing `Esc`. On
`user_interrupted`, stop immediately, do
not retry, do not switch to another input path, do not change `session_id`, and
do not automatically start a new DCC UI Control session. The stop is latched across
all DCC adapter processes in the same Windows logon session. Return control to
the user. Resume only through an explicit `ui_control__snapshot` call with
`resume_computer_use=true`. That flag only requests the flow: the native host
always displays its own confirmation surface before clearing the global latch,
so a model or adapter cannot approve itself. The native backend releases any held keys
or mouse buttons before allowing more input. If Windows disconnects after a
partial injection, those releases remain pending and retain the global input
owner until reconnect makes them confirmable.

### Windows desktop availability

Lock, disconnect, and secure-desktop transitions pause live UIA and raw input.
They return `desktop_unavailable` without sending input or ending the logical
`ui_control` session. Stop issuing UI operations, ask the user to unlock or
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
`ui_control__snapshot` again with the same exact target scope. A successful fresh
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

- `DCC_MCP_UI_CONTROL_CDP_PRESET=reuse` (default): attach to an existing DevTools
  endpoint first so the current browser profile, cookies, and tokens can be
  reused. Set `DCC_MCP_UI_CONTROL_CDP_URL` for an explicit HTTP or WebSocket CDP
  endpoint, or expose Chrome on `DCC_MCP_UI_CONTROL_CDP_PORT` / port `9222`.
- `DCC_MCP_UI_CONTROL_CDP_PRESET=isolated`: launch Chrome with a temporary
  `--user-data-dir` for hermetic tests and demos.
- `DCC_MCP_UI_CONTROL_CDP_PRESET=auroraview`: attach to AuroraView's CDP endpoint.
  It uses `DCC_MCP_UI_CONTROL_AURORAVIEW_CDP_PORT`, then `AURORAVIEW_CDP_PORT`,
  then `DCC_MCP_UI_CONTROL_CDP_PORT`, and finally port `9222`.
- `DCC_MCP_UI_CONTROL_CDP_PRESET=edge`: attach to or launch Microsoft Edge via
  CDP. It uses `DCC_MCP_UI_CONTROL_EDGE_CDP_URL` / `_PORT` before the shared CDP
  URL/port, and `DCC_MCP_UI_CONTROL_EDGE_PATH` when launching.
- `DCC_MCP_UI_CONTROL_CDP_PRESET=agent-browser`: use Vercel's `agent-browser`
  CLI, reading its CDP WebSocket URL through `agent-browser get cdp-url` after
  `agent-browser open about:blank`. Override the binary with
  `DCC_MCP_UI_CONTROL_AGENT_BROWSER_BIN`; this preset is suitable for CI when
  `agent-browser install` has provisioned Chrome for Testing.

## Agent Loop

Use this loop:

1. Try the structured DCC skill, host API, or adapter script.
2. If it returns `unsupported` or `capability_missing`,
   call `ui_control__snapshot` for the exact application window.
3. `ui_control__find` to resolve a control by label, role, text, or object name.
4. `ui_control__act` to perform one scoped action using the resolved control id or
   screenshot coordinates when no semantic control is available.
5. `ui_control__snapshot` immediately to verify the result before another action.
6. Use `ui_control__wait_for` only for a known UI condition, then snapshot again.
7. Call `ui_control__stop_computer_use` when the fallback is complete or abandoned.

For gameplay capture, start from a fresh exact-window snapshot, call
`ui_control__record_clip` once for a bounded shot, validate its manifest and
hashes through `game-pv-capture`, then stop the same session. Do not send UI
actions while recording and do not treat a completed frame sequence as a
finished PV.

If an action returns `stale_control`, restart at `ui_control__snapshot`. If an
action returns `policy_disabled`, prefer a native DCC skill or ask for an
explicit policy change. On `user_interrupted` or `desktop_unavailable`, stop;
do not follow a generic retry or fallback route.

## Workflow Examples

Custom-drawn DCC controls and face shaping: prefer a native rig/parameter tool,
then try `ui_control__find` for a semantic slider or handle. Only when the canvas,
viewport manipulator, or face control is not exposed semantically, enable raw
input at adapter startup and use one observation-fenced drag. Coordinates and
the path below must come from the latest returned PNG; never infer them from an
older frame. `keys` may hold Ctrl, Shift, or Alt for pointer actions when the
DCC uses those modifiers for fine adjustment or an alternate manipulation
mode.

```bash
dcc-mcp-cli ui-control snapshot --instance-id <id> --json '{"session_id":"face-shape","process_id":1234}'
dcc-mcp-cli ui-control act --instance-id <id> --json '{"session_id":"face-shape","process_id":1234,"action":"drag","intent":"ordinary_edit","button":"left","keys":["Shift"],"path":[{"x":612,"y":428},{"x":628,"y":424},{"x":646,"y":419}],"duration_ms":350,"snapshot_id":"<latest-snapshot-id>"}'
dcc-mcp-cli ui-control snapshot --instance-id <id> --json '{"session_id":"face-shape","process_id":1234}'
```

The second snapshot is mandatory immediately after the drag. Verify the visual
result before deriving the next path; one native action consumes the preceding
observation even when the face control moved only a few pixels.

Modal dialog: snapshot the scoped DCC/app window, find the button by label or
role, click with the returned `snapshot_id`, then `wait_for` the button or
dialog root to disappear. Verify completion through a native DCC skill when
possible.

Settings panel: snapshot, find the labeled field or checkbox, `set_text` /
`toggle` / `set_checked`, click Apply, then `wait_for` a status label such as
`Applied` and snapshot again. Typed text is redacted from audit unless policy
allows sensitive values. Use `intent: account_or_access_change` for a remote
control/connection switch; the host also recognizes that label and always
confirms it. Non-sensitive account fields are in scope, but password controls
remain a user hand-off or application-owned OAuth/browser flow.

Recovery: on `missing_window`, confirm process/window scope instead of widening
to the desktop. On `timeout`, inspect the last snapshot and either wait once
more with a justified budget or switch to host diagnostics.
