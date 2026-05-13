//! Canonical MCP/gateway wire serialization and argument validation.
//!
//! This crate owns the shared types and helpers so that HTTP servers, the
//! multi-DCC gateway, and future transports do **not** each re-implement
//! serde quirks, double-stringify hazards, or partial object checks.
//!
//! # Dependency direction (issue #969)
//!
//! ```text
//! dcc-mcp-wire  →  dcc-mcp-protocols  (one edge, no circular)
//! ```
//!
//! `dcc-mcp-jsonrpc` depends on this crate and re-exports `coerce_tool_arguments_object`.
//!
//! # Module map
//!
//! | Module            | Purpose                                              |
//! |-------------------|------------------------------------------------------|
//! | `wire`            | Canonical encode/decode for MCP JSON-RPC and REST  |
//! | `validate`       | Argument shape validation and schema checks        |
//! | `error`          | Structured validation errors for clients/middleware|
//! | `normalize`      | Outer argument object normalisation              |

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod error;
pub mod normalize;
pub mod validate;
pub mod wire;

// Re-exports — consumers only need `use dcc_mcp_wire::*`.
pub use error::{WireError, WireResult};
pub use normalize::{normalize_arguments, normalize_meta};
pub use validate::{validate_arguments, validate_call_tool_params};
pub use wire::{decode_call_tool, encode_call_tool_result, parse_json_rpc_batch};

/// Re-export of [`dcc_mcp_protocols::ToolAnnotations`] so consumers
/// depending on this crate alone still have access to the annotation type.
pub use dcc_mcp_protocols::ToolAnnotations;
