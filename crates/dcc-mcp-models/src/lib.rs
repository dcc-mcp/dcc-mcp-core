//! dcc-mcp-models: ActionResultModel, SkillMetadata, SkillScope, DccMcpError.

mod action_result;
mod error;
mod skill_metadata;
pub mod skill_scope;

#[cfg(feature = "python-bindings")]
mod python;

pub use action_result::ActionResultModel as ToolResult;
pub use action_result::{ActionResultModel, ActionResultModelData, SerializeFormat};
pub use error::DccMcpError;
pub use skill_metadata::{
    ExecutionMode, NextTools, SkillDependencies, SkillDependency, SkillDependencyType, SkillGroup,
    SkillMetadata, SkillPolicy, ThreadAffinity, ToolAnnotations, ToolDeclaration,
};
pub use skill_scope::SkillScope;

#[cfg(feature = "python-bindings")]
pub use python::{
    py_deserialize_result, py_error_result, py_from_exception, py_serialize_result,
    py_success_result, py_validate_action_result,
};
