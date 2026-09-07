# ADR-034: Final-revision modern response projection

Status: Proposed

## Decision

Keep MCP 2026-07-28 response encoding in one transport-neutral codec, applied
once at the stateless service's result boundary. The legacy serializer and
`initialize` behavior are unchanged. Business handlers continue returning
their existing content, structured content, and vendor metadata.

`DiscoverResult` is the single canonical discovery type. The historical
`ServerDiscoverResult` export is an alias, not a second schema. Rust callers
constructing the former RC shape must migrate `protocol_version` to
`supported_versions` and move `server_info` into result metadata. The final
wire result carries `supportedVersions`, `resultType`, `ttlMs`, `cacheScope`,
and optional `_meta`; it does not emit RC `protocolVersion` or `serverInfo`.

The modern codec stamps `resultType: complete` and the server identity under
`_meta["io.modelcontextprotocol/serverInfo"]`. Existing handler-authored
identity and vendor metadata are retained. A different result kind fails
closed; this change does not implement multi-round-trip input requests.
JSON-RPC error responses are not success results and bypass the codec.

Only discovery, tool/prompt/resource lists, resource-template lists, and
resource reads receive default cache fields (`ttlMs: 0`, `cacheScope:
private`). Valid authored hints take precedence; invalid hints fall back
conservatively. No positive TTL or shared authorization-context cache is
enabled by default.

## Validation and boundaries

Pure codec tests pin the closed method set, final discovery fixture,
metadata preservation, malformed-result rejection, and unchanged legacy
serialization. HTTP and opt-in official SDK 2.0.0 smoke tests cover discovery,
list, and call with auto and pinned modern negotiation.

This addresses the response portion of #2436, not full protocol conformance.
The request-routing decision in ADR-033, envelope/header validation, standard
errors, and parameter-header support remain separate follow-up work.
Capability truthfulness and resource/prompt provider wiring are separate
changes. No adapter execution, thread-affinity, or public Skill authoring
contract changes are required; no agent-plugins Skill update is needed for
this response-only correction.
