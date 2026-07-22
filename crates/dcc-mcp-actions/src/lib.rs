//! dcc-mcp-actions: ToolRegistry, EventBus, ToolDispatcher, ToolValidator, VersionedRegistry, ToolPipeline, ActionChain.

pub mod chain;
pub mod dispatch_context;
pub mod dispatch_job_context;
pub mod dispatcher;
pub mod events;
pub mod pipeline;
#[cfg(feature = "python-bindings")]
pub mod python;
pub mod registry;
pub mod validation_strategy;
pub mod validator;
pub mod versioned;

pub use chain::{ActionChain, ChainResult, ChainStepResult, ErrorAction};
pub use dispatch_context::{
    DispatchExecutionContext, current_execution_context, with_execution_context,
};
pub use dispatch_job_context::{
    DispatchJobContext, current_dispatch_job_context, with_dispatch_job_context,
};
pub use dispatcher::{
    DispatchError, DispatchResult, HandlerFn, ToolDispatcher, current_thread_affinity,
    with_thread_affinity,
};
pub use events::{EventBus, EventVeto, VETOABLE_EVENTS, is_vetoable_event};
pub use pipeline::{
    ActionMiddleware, AuditMiddleware, AuditRecord, LoggingMiddleware, MiddlewareContext,
    RateLimitMiddleware, TimingMiddleware, ToolPipeline,
};
pub use registry::{ToolMeta, ToolRegistry};
pub use validator::{ToolValidator, ValidationError, ValidationResult};
pub use versioned::{
    CompatibilityRouter, SemVer, VersionConstraint, VersionParseError, VersionedRegistry,
};
