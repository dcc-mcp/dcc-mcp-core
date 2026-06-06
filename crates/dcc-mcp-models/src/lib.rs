//! dcc-mcp-models: ActionResultModel, SkillMetadata, SkillScope, DccMcpError, DccName.

mod action_result;
mod dcc_name;
mod error;
pub mod registry;
mod skill_metadata;
pub mod skill_scope;

#[cfg(feature = "python-bindings")]
mod python;

pub use action_result::ActionResultModel as ToolResult;
pub use action_result::{ActionResultModel, ActionResultModelData, SerializeFormat};
pub use dcc_name::DccName;
pub use error::DccMcpError;
pub use registry::{DefaultRegistry, Registry, RegistryEntry, SearchQuery};
pub use skill_metadata::{
    CallExample, ExecutionMode, NextTools, Precondition, RecallContext, RiskLevel, SideEffects,
    SkillBranding, SkillDependencies, SkillDependency, SkillDependencyType, SkillGroup, SkillLinks,
    SkillMetadata, SkillPolicy, SkillRuntimeDescriptor, SkillRuntimeKind, SkillRuntimeReport,
    SkillRuntimeState, SkillRuntimeSummary, SuccessMetrics, ThreadAffinity, ToolAnnotations,
    ToolDeclaration, ToolRole, resolve_runtime_reports, summarize_runtime_reports,
};
pub use skill_scope::SkillScope;

#[cfg(feature = "python-bindings")]
pub use python::{
    py_deserialize_result, py_error_result, py_from_exception, py_serialize_result,
    py_success_result, py_validate_action_result,
};
