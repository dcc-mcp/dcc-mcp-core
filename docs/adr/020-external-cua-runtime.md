# ADR-020: Consume application control through standalone dcc-cua

## Status

Accepted (amended 2026-08-13)

## Context

Core contained a Windows-only Computer Use implementation, a private named-pipe
Host, native screenshot implementation, release assets, wheel injection, and updater
coupling. The same repository also carried browser CDP and DCC policy. This made
platform automation release failures part of Core's failure domain and
duplicated capabilities now supplied by the CUA SDK and CLI.

## Decision

Move native application control to the independently versioned
`dcc-mcp/dcc-cua` project and its `dcc-cua` executable. Core
consumes its machine-readable manifest and persistent `host-jsonl` bridge.

Core keeps:

- typed-DCC-first and browser-CDP-first routing policy;
- exact application scoping and raw-input admission policy;
- normalized `ui_control__*` tools, audit events, and artifacts;
- one persistent bridge per logical session.

The standalone CUA project owns:

- CUA SDK integration and platform drivers;
- shared Host lifecycle and IPC;
- session capabilities, input queueing, and Escape broadcast;
- capture, shared-memory/binary image transport, visible control markers;
- trajectory recording and application-specific profiles.

Core neither embeds nor releases the CUA executable in Core release assets.
The official CLI installer reconciles the independently released executable,
and operators can run `dcc-mcp-cli components status dcc-cua` or
`dcc-mcp-cli components ensure dcc-cua --yes`. Component installation consumes
only the versionless per-target manifest from `dcc-mcp/dcc-cua`, requires a
SHA-256, strictly binds the version, target, asset name, and official asset URL,
extracts into a bounded transaction directory, validates the candidate runtime
manifest, and installs it beside `dcc-mcp-cli`. The Python bridge prefers this
CLI sibling before falling back to `dcc-cua` on `PATH`; an explicit
`DCC_MCP_CUA_BINARY` remains authoritative. Core validates `manifest`, prefers
shared memory when available, and falls back to bounded binary attachments.
Core requires stable `dcc-cua` 0.4.0 or newer so the external runtime includes
the hardened long-session health and recovery contract while preserving Host
protocol v1 compatibility.
The manifest must declare `runtime.separate_driver_required=false`; Core rejects
a runtime that would require a separately distributed `cua-driver`.
The previous synchronous `record_clip` and Core-specific system-operation tools
are removed rather than emulated.

## Consequences

- Core and CUA can release, fail, and evolve independently.
- CLI installation converges both executables, but the current two-step
  installer is not crash-atomic across the Core CLI and CUA binary. Each binary
  replacement is verified and recoverable independently; a future install-set
  journal may make the pair one crash-recoverable transaction.
- Windows, Linux, and macOS use one integration contract.
- Browser content continues to prefer CDP; native application surfaces use CUA.
- Existing consumers must install CUA separately and migrate recording calls to
  `recording_start`, `recording_state`, and `recording_stop`.
- ADR-014, ADR-015, and the Host-specific parts of ADR-016 are superseded.

## Alternatives considered

- Keep the old Host in Core: rejected because it duplicates CUA and preserves
  release/update coupling.
- Link CUA crates directly into Core: rejected because it recreates build and
  platform coupling.
- Spawn one CLI process per action: rejected because it loses session fences,
  efficient IPC, and multi-agent lifecycle ownership.
