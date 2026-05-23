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
    search-hint: "app ui, ui automation, dialog, modal, settings panel, snapshot, find control, click, set text, wait for ui, stale control, dcc debugging"
    tags: "app-ui, ui-automation, diagnostics, infrastructure, mock"
    tools: tools.yaml
---

# App UI

Application UI automation primitives for cases where native DCC tools cannot
observe or drive the interface state directly.

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
