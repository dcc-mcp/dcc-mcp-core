//! dcc-mcp-actions: ActionRegistry, EventBus, ActionDispatcher, ActionValidator, VersionedRegistry, ActionPipeline.

pub mod dispatcher;
pub mod events;
pub mod pipeline;
#[cfg(feature = "python-bindings")]
pub mod python;
pub mod registry;
pub mod validator;
pub mod versioned;

pub use dispatcher::{ActionDispatcher, DispatchError, DispatchResult, HandlerFn};
pub use events::EventBus;
pub use pipeline::{
    ActionMiddleware, ActionPipeline, AuditMiddleware, AuditRecord, LoggingMiddleware,
    MiddlewareContext, RateLimitMiddleware, TimingMiddleware,
};
pub use registry::ActionMeta;
pub use registry::ActionRegistry;
pub use validator::{ActionValidator, ValidationError, ValidationResult};
pub use versioned::{
    CompatibilityRouter, SemVer, VersionConstraint, VersionParseError, VersionedRegistry,
};
