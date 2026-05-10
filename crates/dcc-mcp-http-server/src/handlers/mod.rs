//! Handler helper functions that do not depend on `AppState`.
//!
//! Full JSON-RPC routing still lives in `dcc-mcp-http` until its `AppState`
//! dependencies are split. This module hosts low-coupling handler support code
//! that can be shared from the server-support crate without depending on the
//! top-level HTTP runtime.

pub mod tool_builder_core;

pub use tool_builder_core::{build_core_tools, build_core_tools_inner};
