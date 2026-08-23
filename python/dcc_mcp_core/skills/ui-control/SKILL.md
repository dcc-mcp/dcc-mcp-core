---
name: ui-control
description: >-
  Infrastructure skill for application UI observation, scoped action, waits,
  and CUA trajectory recording when a typed DCC tool cannot expose the needed
  interface. Use this skill when the user asks for CUA, UI Control, dcc-cua,
  or application UI automation in a DCC. Prefer typed DCC tools first and CDP
  for browsers; use the standalone dcc-cua backend for native applications.
license: MIT
compatibility: "dcc-cua 0.4.0+, Python 3.7+"
metadata:
  dcc-mcp:
    dcc: python
    version: "0.5.0"
    layer: infrastructure
    search-hint: "dcc ui control, ui control, UI Control, cua, CUA, dcc-cua, dcc cua, computer use, ui automation, chrome cdp, edge cdp, screenshot, click, type, keypress, scroll, wait, trajectory recording"
    tags: "ui-control, cua, dcc-cua, computer-use, ui-automation, chrome-cdp, diagnostics, infrastructure"
    tools: tools.yaml
---

# DCC UI Control

Use application UI automation only when a typed DCC skill, host API, or adapter
script reports `unsupported` or `capability_missing`. Policy denial, user
interruption, authentication, and desktop unavailability are stop conditions.

## Backend order

1. Use a typed DCC-MCP tool whenever one exists.
2. For Chrome, Edge, launchers, and embedded webviews with DevTools access, set
   `DCC_MCP_UI_CONTROL_BACKEND=chrome` (or `edge`) and use CDP.
3. For native application UI, set `DCC_MCP_UI_CONTROL_BACKEND=cua` and use the
   standalone `dcc-cua` CLI/Host.
4. `mock` is an explicit deterministic test backend only; production defaults
   to `dcc-cua` and never silently falls back to mock automation.

Core does not package an automation Host in its own release assets. The official
CLI installer reconciles the independently released `dcc-cua` companion; verify
it with `dcc-mcp-cli components status dcc-cua` or repair it with
`dcc-mcp-cli components ensure dcc-cua --yes`. For custom layouts, install
`dcc-cua` 0.4.0 or newer separately. Core probes the configured
`DCC_MCP_INSTALL_DIR`, the platform's standard dcc-mcp bin directory,
versioned standalone installs, and then `PATH`; set `DCC_MCP_CUA_BINARY` to an
absolute executable path only for a custom location. Core validates
`dcc-cua manifest`, ensures the shared Host, and keeps one
persistent JSONL bridge per active UI session. It prefers shared-memory image
transport when the native Core extension is present and otherwise uses bounded
binary attachments.

The CUA Host owns native accessibility, capture, banner/border/cursor markers,
input scheduling, Escape interruption, browser CDP routing, and platform
adapters. Core owns only DCC policy, normalized tool results, audit events, and
artifact publication.

## Exact application scope

`DccServerBase` injects its trusted `DccServerOptions` process/window context
into every in-process `ui-control` call. Dedicated or custom servers may
override that automatic binding with:

- `DCC_MCP_UI_CONTROL_PROCESS_ID`
- `DCC_MCP_UI_CONTROL_WINDOW_HANDLE`
- optional `DCC_MCP_UI_CONTROL_PROCESS_NAME`
- optional `DCC_MCP_UI_CONTROL_WINDOW_TITLE`

Request arguments may narrow that scope but cannot widen it. Native mouse and
keyboard input are enabled inside that exact scope by default; operators can
disable them with `DCC_MCP_CUA_ALLOW_RAW_INPUT=false`. Semantic accessibility
actions remain preferred. For a multi-window process, pass a `window_title`
that narrows the trusted PID to one window. The Host resolves that title once,
then returns and enforces an exact window capability; use an explicit handle if
the title still matches more than one window.

Multiple agents may control different applications concurrently. Each logical
session has its own grant, window capability, observation fences, and bridge.
The shared Host isolates session state, serializes raw input, and broadcasts
Escape as the operator stop signal.

## Observe, act, verify

Follow this loop:

1. Call `ui_control__snapshot`.
2. Use `ui_control__find` for semantic controls when needed.
3. Call `ui_control__act` once with the current `snapshot_id`.
4. Call `ui_control__wait_for` for an explicit condition or take a fresh
   snapshot.
5. Call `ui_control__stop_computer_use` when the application task is complete.

Never reuse an observation after an action, target change, display change, or
resume. CUA actions are fenced by both `observation_id` and
`accessibility_state_id`; stale actions fail closed.

Prefer semantic `control_id` actions. Coordinate clicks, drag paths, raw typing,
shortcuts, and game navigation are native input and require the raw-input gate.
Do not use UI Control to enter credentials, solve authentication challenges, or
cross a secure-desktop boundary.

## Recording

Trajectory recording is session-scoped and intentionally asynchronous:

1. Call `ui_control__recording_start` with an absolute `output_dir` and optional
   `record_video=true`.
2. Perform the UI actions that should become evidence.
3. Optionally call `ui_control__recording_state`.
4. Call `ui_control__recording_stop` to finalize the trajectory.
5. Preserve the finalized artifacts and structured state returned by CUA.

This replaces the removed synchronous JPEG-sequence `record_clip` contract.
Core does not duplicate CUA's recording format.

## Semantic profiles and trusted handoff

Run `dcc-cua profiles`, then inspect the chosen application with
`dcc-cua profile --id <id>`. Profile target IDs stay stable across languages;
localized aliases are matching inputs, not translated identifiers. Dispatch a
`browser_dom` surface through the exact-bound `dcc-cua` browser route, never the
in-app Browser skill. A fallback such as `ue/fab/download` to
`fab/launcher_download` requires a new Epic Games Launcher binding and fresh
observation. Cloudflare challenges, authentication, purchases, and operating
system security confirmations require trusted human action; full agent access
does not bypass them.

## Browser control

CDP is the first choice for browser content because it exposes DOM semantics,
stable selectors, network state, and background operation. Use native CUA only
for browser chrome, file pickers, permission prompts, canvas-only content, or
when CDP is unavailable. Never attach a second automation stack to the same
page concurrently.

## Error handling

Treat these as recoverable only when the stated precondition can be restored:

- `stale_observation`: take a fresh snapshot.
- `missing_window` or `invalid_target`: rediscover and rebind the exact app.
- `desktop_unavailable`: wait for the interactive desktop to return.
- `backend_unavailable`: install/configure `dcc-cua` or restore CDP.
- `user_interrupted`: stop until the operator explicitly resumes.
- `policy_disabled` or `permission_denied`: do not bypass policy.

Successful snapshots include `capture_provenance`. Preserve it with any visual
evidence. Audit logs redact typed text and other sensitive action payloads.
