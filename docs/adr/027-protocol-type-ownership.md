# ADR 027: Assign One Owner to Every Cross-Crate Protocol Type

## Status

Accepted

## Context

The workspace has accumulated structurally similar request, response, error,
configuration, and search types in multiple crates. Identical names do not
guarantee identical semantics, while identical wire shapes implemented twice
can drift without a compiler error. The marketplace WebSocket bridge, for
example, declared a second set of JSON-RPC envelopes even though the gateway
already depends on `dcc-mcp-jsonrpc`.

A single catch-all protocol crate would reverse the workspace's dependency
boundaries and recreate the former `utils` problem. Ownership therefore needs
to follow semantic responsibility rather than type-name similarity.

## Decision

Every cross-crate type has one canonical definition. Other crates consume that
type directly, re-export it for compatibility, or define an explicit adapter
with `From`/`TryFrom`; they do not repeat its fields.

| Concern | Canonical owner |
| --- | --- |
| DCC domain models and domain failures | `dcc-mcp-models` |
| MCP tool, resource, prompt, and adapter models | `dcc-mcp-protocols` |
| Generic JSON-RPC envelopes, standard codes, and builders | `dcc-mcp-jsonrpc` |
| MCP/REST call-envelope normalization | `dcc-mcp-wire` |
| Transport-neutral HTTP configuration DTOs | `dcc-mcp-http-types` |
| IPC/network transport mechanics and their failures | `dcc-mcp-transport` |
| Tunnel registration and relay wire contract | `dcc-mcp-tunnel-protocol` |
| Pure gateway capability/search domain | `dcc-mcp-gateway-core` |
| Leaf gateway scoring implementation | `dcc-mcp-gateway-search` |
| Product-catalog search results | `dcc-mcp-catalog` |

Same-named types may remain distinct only when their invariants differ. The
distinction must be documented at the definition and conversion must be
explicit. Application crates such as `dcc-mcp-gateway` own orchestration and
state, not copies of generic protocol envelopes.

As the first migration, the marketplace WebSocket bridge now serializes and
deserializes `dcc-mcp-jsonrpc` envelopes directly. Marketplace method names,
parameters, operation phases, and application error codes remain gateway-owned
because they are specific to that application protocol.

`dcc_mcp_models::DccMcpError` is the coarse domain classification used while
errors bubble across crate boundaries. The structurally different MCP
`tools/call` wire projection is named
`dcc_mcp_protocols::ToolCallErrorEnvelope`; its former `DccMcpError` name is a
deprecated compatibility alias. The two types are intentionally distinct and
must be mapped at the boundary that knows the layer, public code, hint, and
trace context.

`dcc_mcp_protocols::ToolAnnotations` owns the camelCase MCP wire contract.
Skill authoring needs different serde behavior, so the models-layer projection
is named `dcc_mcp_models::SkillToolAnnotations`; its former `ToolAnnotations`
name is a deprecated compatibility alias. `ToolDeclaration.annotations` uses
the source projection, and the HTTP boundary projects only spec fields into
the protocol type while keeping DCC extensions in `_meta`.

`dcc_mcp_tunnel_protocol::tokio_io` owns async I/O for the tunnel frame
contract when its `tokio-io` feature is enabled. Its `FrameIoError` is narrow:
it covers stream I/O and tunnel frame decoding only. The agent and relay
re-export the shared implementation and retain their former `TransportError`
names as deprecated aliases. The broader `dcc_mcp_transport::TransportError`
continues to own IPC sessions, pools, reconnects, and registry failures.

Crate consolidation is not required by this decision. A future merge of
`wire`, `jsonrpc`, or `protocols` needs separate dependency and compatibility
evidence; removing duplicate definitions is sufficient.

## Consequences

- Wire-shape fixes and JSON-RPC helpers have one implementation and one test
  surface.
- Type migrations can proceed one family at a time without a flag-day rewrite.
- Compatibility re-exports are allowed, but new field-for-field wrappers are
  rejected during review.
- Similar names such as gateway capability hits and catalog product hits are
  not merged unless their invariants and consumers are actually the same.
- Application-specific error codes stay next to the application while their
  outer JSON-RPC envelope stays generic.

## Alternatives Considered

- **Merge every protocol-related crate immediately:** rejected because the
  crates have different dependency ceilings and compatibility surfaces.
- **Choose ownership by the shortest dependency path:** rejected because that
  moves semantics into incidental consumers.
- **Keep duplicate DTOs and compare JSON in tests:** rejected because tests do
  not provide compile-time identity and both implementations can drift in the
  same direction.
