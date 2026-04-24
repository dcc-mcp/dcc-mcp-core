//! MCP Protocol type definitions as Rust structs.
//!
//! Serde-backed `#[pyclass]` types exposed to Python via PyO3.
//! Reference: https://modelcontextprotocol.io/specification/2025-11-25

#[path = "types_prompts.rs"]
mod prompts;
#[path = "types_resources.rs"]
mod resources;
#[path = "types_tools.rs"]
mod tools;

pub use prompts::{PromptArgument, PromptDefinition};
pub use resources::{ResourceAnnotations, ResourceDefinition, ResourceTemplateDefinition};
pub use tools::{ToolAnnotations, ToolDefinition};
