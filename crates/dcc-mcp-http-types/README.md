# dcc-mcp-http-types

Wire-level value types for the DCC MCP HTTP server — no axum, tokio,
reqwest, or pyo3.

This crate is the innermost layer of the `dcc-mcp-http` Clean-Architecture
split called for in issue [#852]. The dependency direction is strictly
inward:

```text
dcc-mcp-http  (server + pyo3 bindings)
        │
        ▼
dcc-mcp-http-types  (types — this crate)
```

Consumers in `dcc-mcp-http` re-export the types under their historical
paths so existing call sites keep compiling while the migration is in
flight.

Application UI observation/action contracts intentionally live in
`dcc-mcp-app-ui`, not here. That keeps the `app_ui` schema independent from the
HTTP wire/config layer and from any host-specific UI automation backend.

## Stability

Semver follows the workspace version. Types move here from
`dcc-mcp-http` one at a time so each move can be reviewed in isolation
and the dependency direction verified.

[#852]: https://github.com/dcc-mcp/dcc-mcp-core/issues/852
