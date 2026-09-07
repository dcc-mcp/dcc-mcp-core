# ADR-035: Schema-driven modern parameter headers

Status: Proposed

## Decision

Enforce `x-mcp-header` only on the MCP 2026-07-28 HTTP `tools/call`
boundary. Arguments remain authoritative: headers never supply missing data.
The neutral JSON-RPC crate owns declaration scanning, primitive encoding, and
comparison. HTTP supplies case-insensitive lookup with duplicate fields joined
as Fetch does; the stateless service resolves the actual wire tool before
validation. Core and lazy meta-tools take precedence over registry aliases.
`call_action` has its own outer schema, not an invented inner-target contract.

Scan the original schema, not its client-compatible projection. Only a
`properties` chain may reach an annotated `string`, `integer`, or `boolean`.
Suffixes are nonempty RFC 9110 tokens and case-insensitively unique. An
annotation inside a union, conditional, array, map, or definition invalidates
the definition even if that branch is unused. Property path segments remain
exact; a dot in a property name is not a separator. No reference fetching occurs.

Modern listing preserves the entire source schema for valid annotated tools,
including unrelated constraints. Invalid annotated definitions are excluded;
direct calls fail in-band with HTTP 200 / JSON-RPC `-32603`. Local warnings
contain only a sanitized tool name and a fixed reason, never argument or
header values. Unannotated tools retain their prior compatible projection.
Modern duplicate wire names use the dispatcher's entire winning descriptor,
including safety annotations, output schema, and vendor metadata. Legacy listing,
registration, and dispatch are unchanged.

Non-null present values require a matching `Mcp-Param-{suffix}` header. Missing,
mismatched, or malformed encoded headers fail before execution with HTTP 400 /
`-32020`, preserving the request ID and redacting values. Absent/null values
expect no header, but a supplied recognized header must still have valid
characters/encoding; unknown headers are ignored. Invalid argument primitives and
unsafe integers fail in-band with `-32602`, not a misleading header error.

Encoding reuses the canonical UTF-8/Base64 standard-header codec. Integers are
limited to `[-(2^53-1), 2^53-1]`. Decimal header comparison normalizes digits
without floating-point conversion: `0042.0` equals `42`, but
`42.0000000000000001` does not. Body values follow the existing serde_json
numeric deserialization boundary, as JS clients do; this is not a guarantee
of arbitrary-precision raw JSON numeric lexemes.

## Official reference and explicit SDK differences

The authoritative [final transport specification](https://github.com/modelcontextprotocol/modelcontextprotocol/blob/e76e9c572c6f2bfcb730357101acc90f2f802e02/docs/specification/2026-07-28/basic/transports/streamable-http.mdx)
and [SDK scanner source](https://github.com/modelcontextprotocol/typescript-sdk/blob/5119ee7fd7790e335a3fb60ef36f85334e2a6326/packages/core-internal/src/shared/mcpParamHeaders.ts)
are pinned. The inspected scanner/producer/server-check behavior in published
SDK 2.0.0 matches that source; the following are deliberate Core policy or
strictness differences, not an assertion that source and release differ:

- SDK accepts `type: number` for a documented conformance-fixture exception;
  Core follows the final specification's integer/string/boolean restriction.
- SDK server warns and skips invalid definitions; Core excludes them from
  modern discovery and rejects direct modern calls as configuration errors.
- SDK skips unsafe numeric conversion; Core fails closed as invalid arguments.
- SDK's Base64 shape check accepts nonzero unused pad bits; Core requires a
  canonical decode/re-encode match.
- SDK ignores malformed recognized headers when the argument is absent/null;
  Core validates their characters and encoding even when no mirror is required.
- SDK's structural sweep omits `contentSchema`, legacy `additionalItems`, and
  schema-valued `dependencies`; Core rejects annotations in those locations too.
- SDK compares decimals numerically through JS Number; Core never rounds
  header decimals before comparing with a parsed safe integer.

## Validation and rollout boundary

Pure scanner/codec regressions, real HTTP invocation counters, and packaged
Python handler tests cover rejected and accepted requests, aliases/collisions,
source projection, and legacy behavior. Required Linux CI installs the locked
official SDK 2.0.0 packages without scripts, runs its public request oracle,
then exercises native response/request/parameter HTTP tests with SDK auto and
modern-pin clients. Parameter mirroring uses both cached and explicit tool
definitions and a bounded subprocess deadline.
These official-client tests run in Node. The SDK's browser environment skips
custom mirroring; that limitation is not an exemption from server validation.

The CLI still sends legacy `2025-06-18` MCP calls; its REST-first automatic
transport and updater do not change. No existing tools gain annotations and
no public Skills are rolled out by this change. Adapter/Skill authors may opt
in later after assessing whether a parameter is appropriate for HTTP exposure
and intermediary logging; no authorization or subscriptions are implemented.
Mirrored parameters are not authorization; intermediaries must also enforce
the protocol version before trusting header/body validation.
This completes only the parameter-header portion of #2436, not full modern
transport or multi-round-trip execution conformance.
