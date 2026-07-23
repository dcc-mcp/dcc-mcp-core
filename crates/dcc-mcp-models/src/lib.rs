//! dcc-mcp-models: ActionResultModel, SkillMetadata, SkillScope, DccMcpError, DccName,
//! Session, ToolCallEvent, and aggregate statistics types for observability (PIP-2751).

mod action_result;
mod dcc_name;
mod error;
pub mod registry;
pub mod session;
mod skill_metadata;
pub mod skill_scope;
pub mod tool_call_event;

#[cfg(feature = "python-bindings")]
mod python;

pub use action_result::ActionResultModel as ToolResult;
pub use action_result::{ActionResultModel, ActionResultModelData, SerializeFormat};
pub use dcc_name::DccName;
pub use error::DccMcpError;
pub use registry::{DefaultRegistry, Registry, RegistryEntry, SearchQuery};
pub use session::{Session, SessionEndReason, SessionStatus};
pub use skill_metadata::{
    CallExample, ExecutionMode, JobStrategy, NextTools, Precondition, RecallContext, RiskLevel,
    SideEffects, SkillBranding, SkillDependencies, SkillDependency, SkillDependencyType,
    SkillGroup, SkillLinks, SkillMetadata, SkillPolicy, SkillRuntimeDescriptor, SkillRuntimeKind,
    SkillRuntimeReport, SkillRuntimeState, SkillRuntimeSummary, SuccessMetrics, ThreadAffinity,
    ToolAnnotations, ToolDeclaration, ToolRole, resolve_runtime_reports, summarize_runtime_reports,
};
pub use skill_scope::SkillScope;
pub use tool_call_event::{
    ArtifactStats, CoverageStats, CrashStats, FunnelStats, SessionStats, ToolCallEvent,
    ToolCallStats,
};

#[cfg(feature = "python-bindings")]
pub use python::{
    py_deserialize_result, py_error_result, py_from_exception, py_serialize_result,
    py_success_result, py_validate_action_result,
};
