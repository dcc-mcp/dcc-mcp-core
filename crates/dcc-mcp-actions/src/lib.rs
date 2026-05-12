//! dcc-mcp-actions: ToolRegistry, EventBus, ToolDispatcher, ToolValidator, VersionedRegistry, ToolPipeline, ActionChain.

pub mod chain;
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
pub use dispatcher::{
    DispatchError, DispatchResult, HandlerFn, ToolDispatcher, current_thread_affinity,
    with_thread_affinity,
};
pub use events::EventBus;
pub use pipeline::{
    ActionMiddleware, AuditMiddleware, AuditRecord, LoggingMiddleware, MiddlewareContext,
    RateLimitMiddleware, TimingMiddleware, ToolPipeline,
};
pub use registry::{ToolMeta, ToolRegistry};
pub use validator::{ToolValidator, ValidationError, ValidationResult};
pub use versioned::{
    CompatibilityRouter, SemVer, VersionConstraint, VersionParseError, VersionedRegistry,
};
