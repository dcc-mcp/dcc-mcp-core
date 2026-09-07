# Modern request boundary

The optional MCP 2026-07-28 path validates each request independently. This
is the request-boundary batch of #2436, not full protocol conformance.

## Routing and errors

The reserved `params._meta["io.modelcontextprotocol/protocolVersion"]` key
claims the modern envelope. A malformed claim cannot silently become legacy
traffic. The protocol version and `io.modelcontextprotocol/clientCapabilities`
are required; an empty capability object is valid. Client identity is optional,
but a present malformed identity is rejected. Optional progress, logging, and
capability fields are validated without inferring capabilities from sessions.

| Boundary failure | HTTP | JSON-RPC |
| --- | --- | --- |
| Malformed JSON / invalid modern JSON-RPC shape | 400 | -32700 / -32600 |
| Missing or malformed request envelope | 400 | -32602 |
| Required standard header absent, malformed, or disagreeing | 400 | -32020 |
| Unsupported modern version | 400 | -32022 |

Unsupported-version errors use `data.supported` and `data.requested`.
The shared error vocabulary also defines -32021 with `requiredCapabilities`;
this change does not add methods requiring sampling, elicitation, or roots.
Handler-produced invalid parameters remain ordinary HTTP 200 JSON-RPC errors.

`MCP-Protocol-Version` and `Mcp-Method` are required on modern request POSTs.
`Mcp-Name` mirrors the standard method-specific name/URI field. Its shared
codec supports canonical Base64 sentinels and strips only HTTP SP/HTAB around
raw header values. Duplicate disagreeing headers are not silently ignored.
Notification acceptance does not claim notification or cancellation support.

## Compatibility and shared producers

Legacy requests retain their original bytes and headers when forwarded to rmcp.
The existing CLI MCP routes explicitly use the legacy `MCP_PROTOCOL_VERSION`;
the gateway's backend client does not create modern envelope claims. Those
producers and adapter APIs are unchanged. A future modern client can opt into
the canonical `StatelessRequestMeta` through the existing
`JsonRpcRequestBuilder.with_stateless_metadata` method and use the shared
standard-header codec. It must not merely change a version header.

Modern protocol metadata is lifted before tool business dispatch; vendor
metadata, tracing, progress tokens, and tool arguments are preserved. The
historical `StatelessRequestMeta` Rust name now models required namespaced
fields; callers constructing its former optional RC shape must migrate.

The existing `queue.max_request_body_bytes` / `MCP_MAX_REQUEST_BODY_BYTES`
configuration now also covers the mounted `/mcp` route, returning 413 above
the configured limit. Default remains 4 MiB. Oversized legacy requests now
enforce this already-documented limit; within-limit requests are unchanged.
Configured limits above the former modern-only 16 MiB cap remain honored.

## Evidence and remaining work

The negative fixture is exercised by the shared classifier, a real native
HTTP server, and the official SDK HTTP entry. Auto and pinned official-client
discovery/list/call checks exercise the exact native build. Source anchors:
[SDK classifier](https://github.com/modelcontextprotocol/typescript-sdk/blob/5119ee7fd7790e335a3fb60ef36f85334e2a6326/packages/core-internal/src/shared/inboundClassification.ts)
and [final specification schema](https://github.com/modelcontextprotocol/modelcontextprotocol/blob/e76e9c572c6f2bfcb730357101acc90f2f802e02/schema/2026-07-28/schema.ts).

Schema-driven `Mcp-Param-*` mirroring/validation remains a separate #2436
batch. Subscriptions, multi-round-trip execution, and full transport/authorization
conformance are not established by these bounded tests. No new Skill authoring
contract is introduced; no adapter-wide or agent-plugins update is required.
