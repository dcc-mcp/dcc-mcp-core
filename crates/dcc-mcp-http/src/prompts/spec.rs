//! Prompt spec compatibility facade (issue #852).
//!
//! Pure prompt specification types now live in `dcc-mcp-http-types` so callers
//! can parse and validate prompt YAML without depending on the HTTP runtime
//! crate. This module re-exports the historical `crate::prompts::spec::*` path
//! for source compatibility.

pub use dcc_mcp_http_types::prompts::{
    PromptArgumentSpec, PromptError, PromptResult, PromptSpec, PromptsSpec, WorkflowPromptRef,
};
