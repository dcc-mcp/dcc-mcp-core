//! Sub-handlers for the MCP HTTP transport.
//!
//! Each module covers a single responsibility area (tools, resources, skills,
//! jobs, etc.).  All items are re-exported into `crate::handler` so the rest
//! of the crate can continue to call them without prefix changes.

pub mod job_tools;
pub mod lazy_actions;
pub mod resources_prompts;
pub mod skill_tools;
#[cfg(test)]
pub mod tests;
pub mod tool_builder_core;
pub mod tool_builder_skill;
pub mod tools_call;

pub(crate) use job_tools::*;
pub(crate) use lazy_actions::*;
pub(crate) use resources_prompts::*;
pub(crate) use skill_tools::*;
pub(crate) use tool_builder_core::*;
pub(crate) use tool_builder_skill::*;
pub(crate) use tools_call::*;
