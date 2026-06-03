# dcc-mcp-http-server

Runtime support layer for the DCC MCP HTTP server — no axum, tower,
PyO3, or top-level `dcc-mcp-http` dependency.

This crate is the reusable server-runtime layer of the Clean Architecture
split called for in issue [#852]. The dependency direction is strictly
inward:

```text
dcc-mcp-http  (axum server + Python bindings + compatibility facade)
        │
        ▼
dcc-mcp-http-server  (runtime support — this crate)
        │
        ▼
dcc-mcp-http-types  (wire/config/value types)
```

## What lives here

- fixed core-tool descriptor builders (`build_core_tools`),
- host/main-thread execution bridges (`DccExecutorHandle`, `DeferredExecutor`),
- session state and connection-scoped `tools/list` cache,
- in-flight request cancellation and progress routing,
- job/workflow notifications,
- workspace-root resolution helpers.

The public `dcc-mcp-http` crate re-exports this surface from historical
module paths for source compatibility while the split continues.

## Stability

Semver follows the workspace version. Runtime support moves here from
`dcc-mcp-http` one self-contained subsystem at a time so each step can be
reviewed independently and the dependency direction stays easy to audit.

[#852]: https://github.com/dcc-mcp/dcc-mcp-core/issues/852
