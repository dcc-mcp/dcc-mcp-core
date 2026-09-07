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

The `initialize` body does not select a lifecycle. Its version negotiation
is restricted to `2025-06-18` and `2025-03-26`; requesting `2026-07-28` (or
another unsupported version) falls back to `2025-06-18`. Clients opting into
the stateless header use `server/discover`; that route rejects `initialize`
as an unsupported method.

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

## Staged stateless capabilities

The opt-in stateless endpoint advertises tools, with `listChanged: false`:
no subscription stream is implemented. Resources and prompts are advertised
only when their feature flag is enabled and a matching read/get provider is
registered (#2434). Both use the existing legacy providers; notification and
subscription flags remain false. The final 2026 revision does not include
the task wire surface, so tasks are not advertised (#2433).
Legacy capability behavior is unchanged. These declarations do not claim
full final-2026 wire compatibility; the independent SDK round-trip and wire
validation work is tracked in #2436.
