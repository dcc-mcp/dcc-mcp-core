# ADR-009: Migrate MCP Transport to rmcp SDK

- **Status:** Accepted
- **Date:** 2026-05-14
- **Issue:** [#985](https://github.com/dcc-mcp/dcc-mcp-core/issues/985)

## Context

dcc-mcp-core maintains ~4k lines of MCP protocol framing (session lifecycle, SSE
streaming, JSON-RPC method dispatch, protocol version negotiation) across
`dcc-mcp-jsonrpc`, `dcc-mcp-http-server`, and `dcc-mcp-http`. Each MCP spec
revision (2025-03-26, 2025-06-18, 2025-11-25) requires manual updates to types,
dispatch logic, and transport headers.

The official Rust MCP SDK [`rmcp`](https://crates.io/crates/rmcp) now provides a
production-quality implementation of Streamable HTTP transport via its
`transport-streamable-http-server` feature. It tracks spec versions, handles
session lifecycle, and exposes a `ServerHandler` trait for method dispatch.

## Decision

Replace the MCP transport layer with rmcp's `StreamableHttpService` behind the
`rmcp-transport` Cargo feature flag. DCC-specific business logic remains in our
crates; only the wire-level concerns move to rmcp.

## Boundary Definition

| Concern | Owner after migration |
|---------|----------------------|
| HTTP transport, SSE framing, `Mcp-Session-Id` header | rmcp |
| Session lifecycle (create / resume / evict) | rmcp `SessionManager` |
| JSON-RPC frame parsing and method routing | rmcp |
| Protocol version negotiation | rmcp (`ProtocolVersion::KNOWN_VERSIONS`) |
| Tool registry, dispatch, readiness gating | dcc-mcp-http-server (unchanged) |
| DCC executor, thread-affinity routing | dcc-mcp-http-server (unchanged) |
| Skill catalog, lazy actions, progressive disclosure | dcc-mcp-http-server (unchanged) |
| Vendor extensions (deltaToolsUpdate, dynamic tools) | Adapter layer (new) |
| Multi-DCC gateway routing, `/v1/*` REST, admin, audit | dcc-mcp-http (unchanged) |
| Python/PyO3 bindings | dcc-mcp-core (unchanged, public API stable) |

## Approach

1. **Feature-flagged**: `rmcp-transport` feature in `dcc-mcp-http` and
   `dcc-mcp-http-server`. Off by default initially; default-on after parity is
   proven.
2. **Adapter pattern**: `DccMcpHandler` implements rmcp's `ServerHandler` trait
   and delegates to our existing `ServerState` (registry, dispatcher, catalog).
3. **Type conversion**: Thin `rmcp_adapter` module converts between our types
   (`McpTool`, `CallToolResult`, `ToolContent`) and rmcp's (`Tool`,
   `CallToolResult`, `Content`).
4. **Parallel mount (spike)**: `/mcp-next` endpoint runs rmcp alongside the
   existing `/mcp`; once stable, the paths merge.
5. **Version routing (later)**: Middleware routes `2025-11-25` sessions to rmcp
   and `2025-06-18`/`2025-03-26` sessions to the legacy stack until legacy is
   removed.

## Consequences

### Positive

- Spec protocol updates are handled by upgrading the `rmcp` dependency version.
- Session management, SSE keep-alive, and MCP-Protocol-Version header validation
  are maintained upstream.
- Reduced internal code surface to maintain and test.

### Negative

- Vendor extensions (`dcc_mcp_core/deltaToolsUpdate`, elicitation, dynamic tools)
  require adapter hooks outside rmcp's standard trait methods.
- An additional dependency (~20 transitive crates) in the compile graph when the
  feature is enabled.
- Breaking rmcp major version bumps require adapter updates.

### Neutral

- Tool authors' workflow is unchanged: register tools via `ToolRegistry` /
  `SKILL.md` as before.
- Python binding consumers see no change (PyO3 surface uses public Rust API).

## Alternatives Considered

1. **Fork rmcp**: Full control but doubles maintenance burden.
2. **Partial adoption** (types only, no transport): Saves less code, still need
   to maintain SSE / session logic.
3. **Stay on in-house stack**: Higher maintenance cost per spec revision, but
   zero new dependencies.

Option 2 was rejected because the transport layer is the primary maintenance cost.
Option 1 is a fallback if rmcp's extension points prove insufficient.
