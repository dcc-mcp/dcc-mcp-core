# UI Control workflows

UI Control is the application-interface fallback for work that a typed DCC
tool cannot perform. It is not a replacement for adapter APIs.

## Routing order

1. Call the typed DCC-MCP tool.
2. For browser or webview content, use the `chrome`/`edge` CDP backend.
3. For native application UI, use the standalone `dcc-cua` backend.

CDP remains first for browser content because DOM semantics and selectors are
more stable and can work without foreground visibility. CUA covers browser
chrome, native dialogs, canvas-only content, and non-browser applications.

## Standalone CUA setup

The official `dcc-mcp-cli` installer also reconciles the independently released
`dcc-cua` companion. Inspect or repair it explicitly with
`dcc-mcp-cli components status dcc-cua` and
`dcc-mcp-cli components ensure dcc-cua --yes`. Core probes
`DCC_MCP_INSTALL_DIR`, the standard dcc-mcp bin directory, versioned standalone
installs, and `PATH`. Set `DCC_MCP_CUA_BINARY` to an absolute executable path
only for a custom layout.

`DccServerBase` automatically carries its resolved `DccServerOptions` PID,
window handle, title, and DCC type into `ui-control` calls. A dedicated or
custom server can provide explicit operator overrides instead:

```text
DCC_MCP_UI_CONTROL_BACKEND=cua
DCC_MCP_UI_CONTROL_PROCESS_ID=<pid>
DCC_MCP_UI_CONTROL_WINDOW_HANDLE=<native-handle>
```

Request arguments can only narrow the trusted binding. A `window_title`
constraint is sent to the CUA Host together with the trusted PID, so a
multi-window Qt process can resolve one current window without pre-binding a
startup-dialog handle. The Host then mints an exact window capability; if the
title is still ambiguous, bind an exact handle.

Raw mouse and keyboard input are enabled by default inside that exact bound
scope. Operators can disable them with:

```text
DCC_MCP_CUA_ALLOW_RAW_INPUT=false
```

Core validates `dcc-cua manifest`, ensures the shared Host, and keeps a
persistent JSONL bridge. Native Core builds prefer shared-memory screenshots;
Python 3.7 pure wheels use bounded binary attachments. The CUA Host owns the
visible target border/banner/cursor, native accessibility, input queue, and
Escape broadcast.

## Observe and act

1. `ui_control__snapshot`
2. `ui_control__find` when a semantic control is available
3. `ui_control__act` once with the latest `snapshot_id`
4. `ui_control__wait_for` or another snapshot
5. `ui_control__stop_computer_use`

After `dcc-mcp-cli load-skill ui-control`, local
`dcc-mcp-cli search --query "ui control snapshot"` includes loaded
`ui_control__*` tool slugs; the load response remains an immediate compact
schema shortcut.

Each action is fenced by CUA observation and accessibility-state IDs. Take a
new snapshot after every mutation. Prefer semantic element tokens; coordinate
input is a gated fallback for custom-drawn interfaces.

Multiple agents can hold independent sessions for different applications.
Session grants, window capabilities, observations, recording state, and cleanup
remain isolated. Native input is serialized by the shared Host, while Escape
interrupts all active sessions for the user.

## Recording

Use trajectory recording around the actions rather than blocking for a fixed
clip:

```text
ui_control__recording_start(output_dir=<absolute-path>, record_video=true)
ui_control__act(...)
ui_control__recording_state()
ui_control__recording_stop()
```

Preserve the finalized artifacts and structured state returned by CUA. Core
does not duplicate its recording format.

## Semantic profiles and trusted handoff

Run `dcc-cua profiles`, then inspect the selected application with
`dcc-cua profile --id <id>`. Stable profile, surface, and target IDs do not
change with the UI language; localized aliases only match visible text. Route a
`browser_dom` surface through the exact-bound `dcc-cua` browser path instead of
the in-app Browser skill. When `ue/fab/download` falls back to
`fab/launcher_download`, bind Epic Games Launcher as a new exact target and take
a fresh observation. Cloudflare challenges, authentication, purchases, and OS
security confirmations require trusted human action even under full agent
access.

## Safety and evidence

- Scope every session to the exact process/window.
- Never automate credentials, authentication prompts, or secure desktops.
- Treat `user_interrupted`, `permission_denied`, and `policy_disabled` as hard
  stops.
- Preserve `capture_provenance` with screenshots used as evidence.
- Audit logs redact entered text and sensitive action payloads.

See [ADR-020](../adr/020-external-cua-runtime.md) for the ownership boundary.
