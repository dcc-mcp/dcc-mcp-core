//! dcc-mcp-models: ActionResultModel, SkillMetadata.

mod action_result;
mod skill_metadata;

pub use action_result::{ActionResultModel, ActionResultModelData, SerializeFormat};
pub use skill_metadata::{SkillMetadata, ToolDeclaration};

#[cfg(feature = "python-bindings")]
pub use action_result::{
    py_deserialize_result, py_error_result, py_from_exception, py_serialize_result,
    py_success_result, py_validate_action_result,
};
