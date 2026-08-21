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

`dcc_mcp_gateway::GatewayConfig` owns the complete gateway runtime
configuration, including middleware, authentication, persistence, and
lifecycle controls. The transport-neutral settings nested under
`McpHttpConfig.gateway` are named
`dcc_mcp_http_types::config::GatewaySettings` and are explicitly projected
into the runtime type by `dcc-mcp-http`. The HTTP type's former
`GatewayConfig` name is a deprecated compatibility alias.

`dcc_mcp_catalog::CatalogSearchHit` is a lightweight index-and-score reference
into one product-catalog slice. It is intentionally distinct from the generic,
record-bearing `dcc_mcp_gateway_search::SearchHit<R>` ranking result. The
catalog type's former `SearchHit` name is a deprecated compatibility alias;
catalog APIs use the explicit name so imports cannot imply that the two search
domains share a contract.

`dcc-mcp-naming` owns ecosystem-wide validation for MCP tool names and internal
action ids. `dcc_mcp_gateway_core::capability_naming` owns the narrower gateway
projection policy: instance prefixes, skill-qualified tool codecs, bare-name
collision resolution, and its fixed vocabulary. The gateway module's former
`naming` path is a deprecated compatibility alias. Validation remains delegated
to `dcc-mcp-naming`; the gateway layer does not redefine its regex contract.

Python runtime version decisions use the import-light
`dcc_mcp_core._version_util.parse_semver` helper. Invalid numeric cores return
`None`; lifecycle planning reports unknown drift and gateway takeover fails
closed instead of coercing malformed segments to zero.

Python package-version reporting uses
`dcc_mcp_core._version_util.package_version`. It reuses an already-loaded core,
optionally loads the native extension for server construction, then checks
distribution metadata before applying the caller's explicit fallback.

Import-light lifecycle path coercion uses
`dcc_mcp_core._path_util.to_resolved_path`. It expands user paths and resolves
them when possible, while preserving the lexical absolute-path fallback for
inaccessible or transient filesystem entries.

The import-light lifecycle runtime module owns `default_registry_dir`.
Install and sidecar helpers re-export that callable instead of independently
reconstructing the environment and temporary-directory fallback contract.

Import-light environment parsing uses `dcc_mcp_core.env`: `env_flag`,
`env_int`, `env_float`, and `env_path`. Core runtime callers keep environment
names in `dcc_mcp_core.constants` and pass caller-specific truth tokens or
numeric bounds explicitly.

Public Python exceptions share the import-light `dcc_mcp_core.DccMcpError`
root. Specialized exceptions retain their prior built-in exception category
through multiple inheritance. The Python class is an API-level catch boundary,
not an alias for the Rust `dcc_mcp_models::DccMcpError` domain enum.

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
