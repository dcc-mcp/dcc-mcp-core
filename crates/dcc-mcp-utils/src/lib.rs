//! dcc-mcp-utils: Filesystem helpers and shared constants.
//!
//! Logging was extracted into the dedicated `dcc-mcp-logging` crate
//! (see [issue #496](https://github.com/loonghao/dcc-mcp-core/issues/496))
//! so that pure data crates no longer transitively pull in
//! `tracing-appender` / `tracing-subscriber`.
//!
//! The Python<->Rust bridge helpers (`py_json`, `py_yaml`, `type_wrappers`)
//! were extracted into `dcc-mcp-pybridge`
//! (see [issue #497](https://github.com/loonghao/dcc-mcp-core/issues/497))
//! so that callers that only need filesystem helpers do not pull in
//! `pyo3` / `serde_yaml_ng`.

pub mod constants;
pub mod filesystem;
