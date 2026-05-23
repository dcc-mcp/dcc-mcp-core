---
name: app-ui
description: >-
  Infrastructure skill - application UI observation and scoped action tools for
  DCC-adjacent workflows. Use app_ui__snapshot, app_ui__find, app_ui__act, and
  app_ui__wait_for when a host UI state is not exposed through native DCC APIs.
  Prefer DCC-native skills first, then use app_ui as a policy-controlled UI
  fallback.
license: MIT
metadata:
  dcc-mcp:
    dcc: python
    version: "0.1.0"
    layer: infrastructure
    search-hint: "app ui, ui automation, chrome cdp, edge cdp, agent-browser, dialog, modal, settings panel, snapshot, find control, click, set text, wait for ui, stale control, dcc debugging"
    tags: "app-ui, ui-automation, chrome-cdp, edge-cdp, agent-browser, diagnostics, infrastructure, mock"
    tools: tools.yaml
---

# App UI

Application UI automation primitives for cases where native DCC tools cannot
observe or drive the interface state directly.

The default backend is deterministic mock state for CI and adapter authoring.
Set `DCC_MCP_APP_UI_BACKEND=chrome` to use the experimental CDP backend through
the same `app_ui__*` contract.

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

1. `app_ui__snapshot` to observe the scoped application window.
2. `app_ui__find` to resolve a control by label, role, text, or object name.
3. `app_ui__act` to perform one scoped action using the resolved control id.
4. `app_ui__wait_for` to poll until the UI reaches the expected state.
5. `app_ui__snapshot` again to verify the result.

If an action returns `stale_control`, restart at `app_ui__snapshot`. If an
action returns `policy_disabled`, prefer a native DCC skill or ask for an
explicit policy change.
