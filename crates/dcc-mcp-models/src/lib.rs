//! dcc-mcp-models: shared domain types for actions, skills, instance status,
//! sessions, errors, and observability.

mod action_result;
mod dcc_name;
mod error;
pub mod feedback;
mod instance_status;
pub mod registry;
pub mod session;
mod skill_metadata;
pub mod skill_scope;
pub mod state_delta;
pub mod tool_call_event;

#[cfg(feature = "python-bindings")]
mod python;

pub use action_result::ActionResultModel as ToolResult;
pub use action_result::{
    ActionResultModel, ActionResultModelData, LinkedAdapterJob, SerializeFormat,
    linked_adapter_job_from_result,
};
pub use dcc_name::DccName;
pub use error::DccMcpError;
pub use feedback::{
    FINDING_V1_JSON_SCHEMA, FINDING_V1_SCHEMA_VERSION, FeedbackReport, FeedbackSeverity,
    FeedbackValidationError, FindingEvidenceV1, FindingPhase, FindingRedactionMode,
    FindingRedactionStatusV1, FindingReproV1, FindingSeverity, FindingV1, finding_fingerprint,
};
pub use instance_status::{DispatchStatus, InstanceStatus, ServiceStatus};
pub use registry::{DefaultRegistry, Registry, RegistryEntry, SearchQuery};
pub use session::{Session, SessionEndReason, SessionStatus};
#[allow(deprecated)]
pub use skill_metadata::ToolAnnotations;
pub use skill_metadata::{
    CallExample, ExecutionMode, JobStrategy, NextTools, Precondition, RecallContext, RiskLevel,
    SideEffects, SkillBranding, SkillDependencies, SkillDependency, SkillDependencyType,
    SkillGroup, SkillLinks, SkillMetadata, SkillPolicy, SkillRuntimeDescriptor, SkillRuntimeKind,
    SkillRuntimeReport, SkillRuntimeState, SkillRuntimeSummary, SkillToolAnnotations,
    SuccessMetrics, ThreadAffinity, ToolDeclaration, ToolRole, resolve_runtime_reports,
    summarize_runtime_reports,
};
pub use skill_scope::SkillScope;
pub use state_delta::{
    DEFAULT_STATE_DELTA_MAX_CHANGES, StateChange, StateChangeKind, StateDelta, diff_json_state,
};
pub use tool_call_event::{
    ArtifactStats, CoverageStats, CrashStats, FunnelStats, SessionStats, ToolCallEvent,
    ToolCallStats,
};

#[cfg(feature = "python-bindings")]
pub use python::{
    py_deserialize_result, py_error_result, py_from_exception, py_serialize_result,
    py_success_result, py_validate_action_result,
};
