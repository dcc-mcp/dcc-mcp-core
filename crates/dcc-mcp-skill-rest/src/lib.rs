//! Per-DCC REST skill API surface (#658 + #660).
//!
//! This module exposes the skills already discovered by an individual DCC
//! process as a small, stable HTTP API — so non-MCP agents and enterprise
//! remote callers can search, describe, and invoke DCC capabilities
//! directly without going through the multi-instance MCP gateway.
//!
//! # Architecture — SOLID-first
//!
//! ```text
//! ┌──────────────────────────────────────────────────────────────────┐
//! │                      SkillRestRouter                             │
//! │  (facade — single responsibility: mount axum routes)             │
//! └──────────────────────────┬───────────────────────────────────────┘
//!                            │
//!                            ▼
//!               ┌───────────────────────────┐
//!               │      SkillRestService      │
//!               │ ── pure logic, no axum ──  │
//!               └──┬──────┬──────┬──────┬────┘
//!                  │      │      │      │
//!                  ▼      ▼      ▼      ▼
//!            SkillCatalog  ToolInvoker  AuthGate  AuditSink
//!            (trait)       (trait)      (trait)   (trait)
//! ```
//!
//! Every collaborator is a **trait** — defaults wire to the existing
//! [`dcc_mcp_skills::SkillCatalog`] and [`dcc_mcp_actions::ActionDispatcher`]
//! but adapters (Maya/Blender/Houdini) may swap in their own impls to
//! enforce host-specific thread affinity, auth, or audit semantics
//! (Open/Closed + DIP). The router layer depends only on the service
//! trait, so tests can drive it with in-memory fakes.
//!
//! # Exposed routes (#658)
//!
//! | Method | Path                       | Purpose                             |
//! |--------|----------------------------|-------------------------------------|
//! | GET    | `/v1/skills`              | List discovered skills              |
//! | POST   | `/v1/search`              | Keyword + filter search             |
//! | POST   | `/v1/describe`            | Describe a single tool slug         |
//! | GET    | `/v1/tools/{slug}`        | Alias of describe                   |
//! | POST   | `/v1/call`                | Invoke a tool by slug               |
//! | GET    | `/v1/context`             | Current DCC scene/document summary  |
//! | GET    | `/v1/healthz`             | Liveness                            |
//! | GET    | `/v1/readyz`              | Three-state readiness               |
//! | GET    | `/v1/openapi.json`        | Machine-readable API contract       |
//!
//! # Enterprise standards (#660)
//!
//! - **Versioned paths** (`/v1/*`) — API contract is stable and
//!   forward-compatible.
//! - **Structured errors** — every failure is a [`ServiceError`] with
//!   `{kind, message, hint, request_id}`; clients dispatch on `kind`.
//! - **Auth gate** — pluggable [`AuthGate`]; defaults to a *localhost-only*
//!   allow policy. Enabling remote binding requires an explicit
//!   [`BearerTokenGate`].
//! - **Audit sink** — every call produces a structured [`AuditEvent`]
//!   with `request_id`, `slug`, `outcome`, latency.
//! - **Readiness three-state** — distinguishes process alive vs DCC
//!   ready vs dispatcher ready.
//! - **Low token overhead** — `/v1/search` returns compact hits
//!   (≤ `SEARCH_HIT_BUDGET_BYTES` per hit); describe is opt-in for the
//!   full schema via `{"include_schema": true}`.

mod audit;
mod auth;
mod errors;
pub mod openapi;
mod readiness;
mod router;
mod service;

#[cfg(test)]
mod tests;

pub use audit::{AuditEvent, AuditOutcome, AuditSink, NoopAuditSink, VecAuditSink};
pub use auth::{AllowLocalhostGate, AuthGate, BearerTokenGate, Principal};
pub use errors::{ServiceError, ServiceErrorKind};
pub use readiness::{ReadinessProbe, ReadinessReport, StaticReadiness};
pub use router::{SkillRestConfig, build_skill_rest_router};
pub use service::{
    CallOutcome, CallRequest, ContextSnapshot, DescribeRequest, DescribeResponse, SearchRequest,
    SearchResponse, SkillCatalogSource, SkillListEntry, SkillRestService, ToolInvoker, ToolSlug,
};

/// Upper bound on per-hit serialised bytes produced by `/v1/search` —
/// enforced in tests to prevent accidental schema expansion in the
/// compact search response and the token waste that comes with it.
pub const SEARCH_HIT_BUDGET_BYTES: usize = 512;
