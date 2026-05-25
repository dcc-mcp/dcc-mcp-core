//! Trait abstractions for resource and prompt access in the rmcp handler.
//!
//! These traits break the circular dependency between `dcc-mcp-http` (which owns
//! `ResourceRegistry` / `PromptRegistry`) and `dcc-mcp-http-server` (which
//! implements `ServerHandler`). The HTTP crate implements these traits and
//! passes trait-object pointers into the handler factory.
//!
//! # Gating
//!
//! This entire module is compiled only when the `rmcp-transport` feature is
//! enabled.

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use dcc_mcp_jsonrpc::{GetPromptResult, McpPrompt, McpResource, ReadResourceResult};
use dcc_mcp_skills::SkillCatalog;
use serde_json::Value;

// ── Error type ──────────────────────────────────────────────────────────────

/// Error variants returned by provider operations.
///
/// This deliberately mirrors the union of `ResourceError` and `PromptError`
/// from `dcc-mcp-http` without requiring a direct dependency on that crate.
#[derive(Debug, Clone)]
pub enum ProviderError {
    /// The requested resource or prompt was not found.
    NotFound(String),
    /// The feature is disabled (e.g. resources turned off in config).
    NotEnabled(String),
    /// A required argument was not provided (prompts only).
    MissingArg(String),
    /// An internal / IO / parse error occurred.
    Internal(String),
}

impl fmt::Display for ProviderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound(msg) => write!(f, "not found: {msg}"),
            Self::NotEnabled(msg) => write!(f, "not enabled: {msg}"),
            Self::MissingArg(msg) => write!(f, "missing required argument: {msg}"),
            Self::Internal(msg) => write!(f, "internal error: {msg}"),
        }
    }
}

impl std::error::Error for ProviderError {}

// ── Provider traits ─────────────────────────────────────────────────────────

/// Provides resource listing and reading for the rmcp handler.
///
/// Implemented by `dcc-mcp-http`'s `ResourceRegistryProvider` which wraps
/// the actual `ResourceRegistry`.
pub trait ResourceProvider: Send + Sync {
    /// List all resources, refreshing skill-declared resources first.
    fn list_resources(&self, catalog: &Arc<SkillCatalog>) -> Vec<McpResource>;

    /// Read a single resource by URI.
    fn read_resource(
        &self,
        uri: &str,
        catalog: &Arc<SkillCatalog>,
    ) -> Result<ReadResourceResult, ProviderError>;
}

/// Provides prompt listing and rendering for the rmcp handler.
///
/// Implemented by `dcc-mcp-http`'s `PromptRegistryProvider` which wraps
/// the actual `PromptRegistry`.
pub trait PromptProvider: Send + Sync {
    /// List all registered prompts.
    fn list_prompts(&self, catalog: &Arc<SkillCatalog>) -> Vec<McpPrompt>;

    /// Optional diagnostics for empty or surprising prompt lists.
    fn prompt_diagnostics(&self, catalog: &Arc<SkillCatalog>) -> Option<Value> {
        let _ = catalog;
        None
    }

    /// Look up and render a prompt by name with the given arguments.
    fn get_prompt(
        &self,
        name: &str,
        args: &HashMap<String, String>,
        catalog: &Arc<SkillCatalog>,
    ) -> Result<GetPromptResult, ProviderError>;
}
