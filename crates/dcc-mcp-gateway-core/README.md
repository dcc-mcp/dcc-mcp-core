# dcc-mcp-gateway-core

Domain layer for the DCC MCP gateway — pure types with no HTTP, no async
runtime, and no dependency on `dcc-mcp-gateway`.

This crate is the innermost layer of the Clean-Architecture split called
for in issue [#845]. The dependency direction is strictly inward:

```text
dcc-mcp-gateway  (application + infrastructure)
        │
        ▼
dcc-mcp-gateway-core  (domain — this crate)
```

Consumers in `dcc-mcp-gateway` re-export the types under stable paths so
existing call sites keep compiling while the migration is in flight.

## Stability

Semver follows the workspace version. Types move here from
`dcc-mcp-gateway` one at a time so each move can be reviewed in isolation
and the dependency direction verified.

[#845]: https://github.com/dcc-mcp/dcc-mcp-core/issues/845
