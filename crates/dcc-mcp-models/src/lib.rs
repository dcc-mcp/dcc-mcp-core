//! dcc-mcp-models: ActionResultModel, SkillMetadata.

mod action_result;
mod skill_metadata;

pub use action_result::ActionResultModel;
pub use action_result::ActionResultModelData;
pub use skill_metadata::SkillMetadata;

#[cfg(feature = "python-bindings")]
pub use action_result::py_error_result;
#[cfg(feature = "python-bindings")]
pub use action_result::py_from_exception;
#[cfg(feature = "python-bindings")]
pub use action_result::py_success_result;
#[cfg(feature = "python-bindings")]
pub use action_result::py_validate_action_result;
