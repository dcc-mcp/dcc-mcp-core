//! dcc-mcp-models: ActionResultModel, SkillMetadata, SkillScope.

mod action_result;
mod skill_metadata;
pub mod skill_scope;

pub use action_result::ActionResultModel as ToolResult;
pub use action_result::{ActionResultModel, ActionResultModelData, SerializeFormat};
pub use skill_metadata::{
    SkillDependencies, SkillDependency, SkillDependencyType, SkillGroup, SkillMetadata,
    SkillPolicy, ToolDeclaration,
};
pub use skill_scope::SkillScope;

#[cfg(feature = "python-bindings")]
pub use action_result::{
    py_deserialize_result, py_error_result, py_from_exception, py_serialize_result,
    py_success_result, py_validate_action_result,
};
