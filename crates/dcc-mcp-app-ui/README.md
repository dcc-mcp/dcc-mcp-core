# dcc-mcp-app-ui

DCC-agnostic `app_ui` observation, action, wait, policy, and audit contract
types.

This crate defines schemas only. It does not implement a universal UI
automation backend, and it intentionally has no axum, tokio, reqwest, pyo3, or
OS accessibility dependency. Each adapter owns its host-specific implementation
for Qt, native accessibility, webviews, or DCC APIs.

The stable downstream Python-facing surface remains
`dcc_mcp_core.adapter_contracts`. Rust callers should import these contract
types from this crate directly.
