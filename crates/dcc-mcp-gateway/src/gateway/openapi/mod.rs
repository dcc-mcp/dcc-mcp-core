//! OpenAPI-to-MCP mount helper (#773).
//!
//! Parses an OpenAPI 3.x spec (JSON or YAML) and emits one [`McpTool`] per
//! HTTP operation so that any existing REST service can be exposed through
//! the gateway's MCP surface without hand-writing tool definitions.
//!
//! # Quick start
//!
//! ```rust,ignore
//! use dcc_mcp_gateway::gateway::openapi::{OpenApiMount, AuthConfig};
//!
//! let mount = OpenApiMount::from_spec_json(spec_json)
//!     .base_url("https://api.example.com")
//!     .auth(AuthConfig::bearer("$MY_API_TOKEN"))
//!     .tool_prefix("example");
//!
//! let tools = mount.to_mcp_tools();
//! ```
//!
//! # Auth forwarding
//!
//! [`AuthConfig`] supports three kinds:
//!
//! - `Bearer` — `Authorization: Bearer <value>` header.
//! - `ApiKey` — custom header (e.g. `X-API-Key: <value>`).
//! - `Basic`  — `Authorization: Basic base64(<user>:<pass>)`.
//!
//! When `value` starts with `$`, the remainder is treated as an **env-var
//! name** and the actual secret is resolved at call time.  This prevents
//! secrets from being stored in config structs.

mod auth;
mod call;
mod spec;

pub use auth::{AuthConfig, AuthKind};
pub use call::call_operation;
pub use spec::{OpenApiMount, OperationInfo};
