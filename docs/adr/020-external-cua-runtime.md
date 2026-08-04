# ADR-020: Consume application control through standalone dcc-mcp-cua

## Status

Accepted

## Context

Core contained a Windows-only Computer Use implementation, a private named-pipe
Host, capture worker behavior, release assets, wheel injection, and updater
coupling. The same repository also carried browser CDP and DCC policy. This made
platform automation release failures part of Core's failure domain and
duplicated capabilities now supplied by the CUA SDK and CLI.

## Decision

Move native application control to the independently versioned
`dcc-mcp/dcc-mcp-computer-use` project and its `dcc-mcp-cua` executable. Core
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
- trajectory recording/rendering and application-specific adapters.

Core neither embeds nor releases the CUA executable. It resolves
`DCC_MCP_CUA_BINARY` or `dcc-mcp-cua` on `PATH`, validates `manifest`, prefers
shared memory when available, and falls back to bounded binary attachments.
The previous synchronous `record_clip` and Core-specific system-operation tools
are removed rather than emulated.

## Consequences

- Core and CUA can release, fail, and evolve independently.
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
