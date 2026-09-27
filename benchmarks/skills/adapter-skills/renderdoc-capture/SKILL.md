---
name: renderdoc-capture
description: >-
  Domain skill — Discover RenderDoc and launch an explicit game or graphics executable under
  capture. Use for repeatable local or CI frame capture. Not for reading an existing capture —
  use renderdoc-analysis.
license: MIT
compatibility: "RenderDoc 1.45+; dcc-mcp-core 0.20.14+"
allowed-tools: "python"
metadata:
  dcc-mcp:
    dcc: renderdoc
    layer: domain
    version: "0.1.0"
    search-hint: "RenderDoc capture launch executable game graphics frame rdc"
    tags: "renderdoc,capture,graphics-debugging,game-development"
    tools: tools.yaml
    depends: "dcc-diagnostics"
---

# RenderDoc Capture

Call `get_version` before capture. `capture_program` starts only the explicit executable and
arguments, never a shell. Set `trigger_after_secs` to request one capture through RenderDoc's
official local Target Control API. This headless trigger requires `qrenderdoc` beside
`renderdoccmd`; it does not focus a window or synthesize keyboard input. `hook_children` remains a
RenderDoc launch option. Set `trigger_process_name` only with `hook_children=true`; the sidecar
checks the launched target first and, if its name differs, follows only its official `NewChild`
messages to find a unique matching child. A child name without child hooking fails before launch.

The official RenderDoc runtime supports Windows and Linux, not macOS (where this project runs
Python unit tests only). On headless Linux, run the adapter under Xvfb or provide another working
X/Wayland display; the official archive does not include Qt's `offscreen` platform plugin.

Capture tools inspect each new `.rdc` before reporting success. A capture with no frame work such
as Draw, Dispatch, Clear, Copy, Resolve, Blit, render-pass work, or command-list execution is
rejected with chunk diagnostics while the `.rdc` remains at the requested output path; retry while
the intended target is actively rendering or correct the selected child process.

Use `capture_process` when Steam or another platform client must launch the target first. It
injects into the explicit PID, connects to the returned Target Control ident, requests one capture
after the configured delay, and waits for the resulting file. It never takes foreground focus or
emits a hotkey. Inject before the target creates its graphics device; late injection cannot recover
resources that RenderDoc did not observe. Prefer `capture_program` when the executable can be
launched directly. Neither tool terminates the target process.
