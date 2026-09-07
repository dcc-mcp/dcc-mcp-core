# MCP 2026 response interoperability

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
The Node subprocess is opt-in and has a 30-second deadline. The ordinary Rust
test still checks the same raw HTTP result fields without Node dependencies.

The upstream discovery fixture is pinned to official SDK source commit
`5119ee7fd7790e335a3fb60ef36f85334e2a6326`, at
`packages/core-internal/test/corpus/fixtures/2026-07-28/DiscoverResultResponse/discover-result-response.json`.
The authoritative specification schema is pinned to
`modelcontextprotocol/modelcontextprotocol` commit
`e76e9c572c6f2bfcb730357101acc90f2f802e02`, at
`schema/2026-07-28/schema.ts`.

This is **not** a full conformance claim. Request-envelope validation,
header/body consistency, standard errors, parameter header mirroring,
subscriptions, and multi-round-trip execution require separate acceptance.
