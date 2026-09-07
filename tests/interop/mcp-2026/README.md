# MCP 2026 bounded interoperability

This bounded smoke exercises `server/discover`, `tools/list`, and `tools/call`
through the official `@modelcontextprotocol/client` **2.0.0**, using both auto
negotiation and a `2026-07-28` pin. The Rust HTTP test owns the server and shuts
it down; no installed wheel or live DCC is substituted.

```sh
npm ci --prefix tests/interop/mcp-2026 --ignore-scripts
DCC_MCP_SDK_SMOKE=1 cargo test -p dcc-mcp-http --no-default-features \
  --features mcp-2026-07-28 --test stateless_response_contract -- --nocapture
```

On PowerShell, set `$env:DCC_MCP_SDK_SMOKE = "1"` before the Cargo command.
The Node subprocess is opt-in locally and has a 30-second deadline. Required
Linux CI enables it explicitly for response, request, and parameter tests.
The ordinary Rust tests still check raw HTTP without Node dependencies.

The upstream discovery fixture is pinned to official SDK source commit
`5119ee7fd7790e335a3fb60ef36f85334e2a6326`, at
`packages/core-internal/test/corpus/fixtures/2026-07-28/DiscoverResultResponse/discover-result-response.json`.
The authoritative specification schema is pinned to
`modelcontextprotocol/modelcontextprotocol` commit
`e76e9c572c6f2bfcb730357101acc90f2f802e02`, at
`schema/2026-07-28/schema.ts`.

## Request boundary

Run `node tests/interop/mcp-2026/request-oracle.mjs` for the public server SDK
**2.0.0** fixture oracle. The same cases drive native HTTP and Python-binding
tests; `--test stateless_request_boundary` also exercises configured body
limits and, with `DCC_MCP_SDK_SMOKE=1`, both official client modes above.

Of 29 cases, 28 match the published SDK. One explicitly asserted release
difference remains: 2.0.0 accepts a modern body missing the version header,
whereas the pinned official source requires it (Core returns 400/-32020).
The all-legacy batch fixture uses public `isLegacyRequest`, because strict
modern-only rejection is not the dual-era forwarding contract under test.

## Parameter headers

`--test stateless_param_headers` covers source-schema projection and a real
handler invocation counter, including rejected mirrors, alias/core resolution,
and unchanged legacy behavior. With `DCC_MCP_SDK_SMOKE=1`, `param-client.mjs`
checks the public client's cached and explicit tool-definition mirroring in
both auto and pinned modern modes, without any private SDK imports.

Core deliberately follows the final specification's string/integer/boolean
restriction rather than SDK 2.0.0's documented `number` fixture exception.
Other intentional strictness differences (invalid server definitions, unsafe
integers, canonical Base64, exact header decimals, and malformed recognized
headers with absent/null arguments) are detailed in
[ADR-035](../../../docs/adr/035-schema-driven-parameter-headers.md).

This is **not** a full conformance claim. Subscriptions, multi-round-trip
execution, and full transport / authorization behavior require separate acceptance.
