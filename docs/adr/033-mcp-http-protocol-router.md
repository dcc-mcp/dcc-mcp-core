# ADR-033: Explicit MCP HTTP protocol routing

Status: Proposed

## Context

The 2026-07-28 MCP transport removes lifecycle sessions and introduces
request-level protocol headers.  Existing DCC servers and gateways still need
to accept 2025.x clients, so changing the `/mcp` default would be a breaking
change.

## Decision

HTTP routers use `MCP-Protocol-Version` as the authoritative route signal:

- `2026-07-28` routes to the stateless handler;
- `2025-06-18`, `2025-03-26`, an absent header, or an unknown version routes to
  the existing session-compatible handler;
- `Mcp-Session-Id` never upgrades a request to stateless mode;
- `Accept`, `Mcp-Method`, and `Mcp-Name` are optional middleware hints and are
  not sufficient to upgrade an unversioned request.

The shared `dcc-mcp-jsonrpc::select_protocol_mode_from_headers` helper owns
this decision so the embedded HTTP server, gateway, and CLI cannot drift.
Phase 2 HTTP integration may add validation of the optional hints, but must
preserve the legacy default until the stateless handler is enabled by the
caller and covered by end-to-end tests.

## Consequences

This is an additive seam: current clients retain their existing route, while
2026 clients can opt in explicitly.  A later release can change the default
only after all adapters and the gateway advertise and exercise the stateless
handler.
