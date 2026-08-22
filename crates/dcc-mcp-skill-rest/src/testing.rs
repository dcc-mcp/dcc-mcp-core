//! Reusable in-memory fixtures for the per-DCC REST contracts.
//!
//! The implementations live beside the traits so `dcc-mcp-skill-rest` can use
//! them in its own unit tests without a dependency cycle. Downstream workspace
//! tests use the canonical `dcc_mcp_test_utils::skill_rest` re-export.

use std::sync::Arc;

use axum::Router;
use axum_test::TestServer;
use parking_lot::Mutex;
use serde_json::{Value, json};

use crate::audit::VecAuditSink;
use crate::auth::{AuthContext, AuthGate, Principal};
use crate::errors::{ServiceError, ServiceErrorKind};
use crate::readiness::StaticReadiness;
use crate::router::{SkillRestConfig, build_skill_rest_router};
use crate::service::{
    CallOutcome, CatalogAction, SkillCatalogSource, SkillRestService, ToolInvoker, ToolSlug,
};

/// Thread-safe in-memory implementation of [`SkillCatalogSource`].
#[derive(Default)]
pub struct InMemorySkillCatalog {
    actions: Mutex<Vec<CatalogAction>>,
}

impl InMemorySkillCatalog {
    #[must_use]
    pub fn new(actions: impl IntoIterator<Item = CatalogAction>) -> Self {
        Self {
            actions: Mutex::new(actions.into_iter().collect()),
        }
    }

    pub fn push(&self, action: CatalogAction) {
        self.actions.lock().push(action);
    }

    #[must_use]
    pub fn actions(&self) -> Vec<CatalogAction> {
        self.actions.lock().clone()
    }
}

impl SkillCatalogSource for InMemorySkillCatalog {
    fn list_actions(&self) -> Vec<CatalogAction> {
        self.actions()
    }

    fn is_loaded(&self, skill_name: &str) -> bool {
        self.actions
            .lock()
            .iter()
            .any(|action| action.skill_name == skill_name && action.loaded)
    }

    fn load_skill(&self, skill_name: &str) -> Result<Vec<String>, ServiceError> {
        let mut actions = self.actions.lock();
        let mut loaded = Vec::new();
        for action in actions
            .iter_mut()
            .filter(|action| action.skill_name == skill_name)
        {
            action.loaded = true;
            loaded.push(action.action_name.clone());
        }
        if loaded.is_empty() {
            Err(ServiceError::new(
                ServiceErrorKind::NotFound,
                format!("skill not found: {skill_name}"),
            ))
        } else {
            Ok(loaded)
        }
    }

    fn unload_skill(&self, skill_name: &str) -> Result<usize, ServiceError> {
        let mut actions = self.actions.lock();
        let mut removed = 0;
        for action in actions
            .iter_mut()
            .filter(|action| action.skill_name == skill_name)
        {
            action.loaded = false;
            removed += 1;
        }
        if removed == 0 {
            Err(ServiceError::new(
                ServiceErrorKind::NotFound,
                format!("skill not found: {skill_name}"),
            ))
        } else {
            Ok(removed)
        }
    }
}

/// One invocation captured by [`RecordingToolInvoker`].
#[derive(Debug, Clone, PartialEq)]
pub struct RecordedInvocation {
    pub action_name: String,
    pub params: Value,
    pub meta: Option<Value>,
}

/// In-memory invoker with a configurable one-shot result and call history.
#[derive(Default)]
pub struct RecordingToolInvoker {
    calls: Mutex<Vec<RecordedInvocation>>,
    next: Mutex<Option<Result<Value, ServiceError>>>,
}

impl RecordingToolInvoker {
    pub fn set_next(&self, result: Result<Value, ServiceError>) {
        *self.next.lock() = Some(result);
    }

    #[must_use]
    pub fn calls(&self) -> Vec<RecordedInvocation> {
        self.calls.lock().clone()
    }
}

#[async_trait::async_trait]
impl ToolInvoker for RecordingToolInvoker {
    async fn invoke(
        &self,
        action_name: &str,
        params: Value,
        meta: Option<Value>,
    ) -> Result<CallOutcome, ServiceError> {
        self.calls.lock().push(RecordedInvocation {
            action_name: action_name.to_owned(),
            params,
            meta,
        });
        self.next
            .lock()
            .take()
            .unwrap_or(Ok(Value::Null))
            .map(|output| CallOutcome {
                slug: ToolSlug(action_name.to_owned()),
                output,
                validation_skipped: false,
            })
    }
}

/// Deterministic auth fixture that either returns one principal or rejects.
#[derive(Debug, Clone)]
pub struct StaticAuthGate {
    principal: Option<Principal>,
    denial: String,
}

impl StaticAuthGate {
    #[must_use]
    pub fn allow(subject: impl Into<String>) -> Self {
        Self {
            principal: Some(Principal {
                subject: subject.into(),
                roles: vec!["test".to_string()],
            }),
            denial: String::new(),
        }
    }

    #[must_use]
    pub fn deny(message: impl Into<String>) -> Self {
        Self {
            principal: None,
            denial: message.into(),
        }
    }
}

impl AuthGate for StaticAuthGate {
    fn authorize(&self, _ctx: &AuthContext<'_>) -> Result<Principal, ServiceError> {
        self.principal
            .clone()
            .ok_or_else(|| ServiceError::new(ServiceErrorKind::Unauthorized, self.denial.clone()))
    }
}

/// Explicit test-oriented name for the production in-memory audit sink.
pub type RecordingAuditSink = VecAuditSink;

/// Minimal real-router harness with deterministic auth, readiness, and audit.
pub struct SkillRestTestHarness {
    pub server: TestServer,
    pub audit: Arc<VecAuditSink>,
}

impl SkillRestTestHarness {
    #[must_use]
    pub fn new(service: SkillRestService) -> Self {
        let audit = Arc::new(VecAuditSink::new());
        let config = SkillRestConfig::new(service)
            .with_audit(audit.clone())
            .with_readiness(Arc::new(StaticReadiness::fully_ready()))
            .with_auth(Arc::new(StaticAuthGate::allow("test")));
        let app: Router = build_skill_rest_router(config);
        Self {
            server: TestServer::new(app),
            audit,
        }
    }
}

/// Compact multi-DCC action fixture with production defaults.
#[must_use]
pub fn catalog_action(
    action_name: impl Into<String>,
    skill_name: impl Into<String>,
    dcc: impl Into<String>,
    loaded: bool,
) -> CatalogAction {
    CatalogAction {
        action_name: action_name.into(),
        skill_name: skill_name.into(),
        dcc: dcc.into(),
        description: "Fixture action".to_string(),
        tags: Vec::new(),
        search_aliases: Vec::new(),
        search_tokens: Vec::new(),
        input_schema: json!({"type": "object"}),
        loaded,
        scope: "repo".to_string(),
        layer: None,
        path_source: "unknown".to_string(),
        annotations: Default::default(),
        execution: Default::default(),
        timeout_hint_secs: None,
        job_strategy: Default::default(),
        thread_affinity: Default::default(),
        enforce_thread_affinity: false,
        available_groups: Vec::new(),
        runtime: None,
        next_tools: Default::default(),
        call_examples: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_fixture_loads_and_unloads_skills() {
        let catalog = InMemorySkillCatalog::new([
            catalog_action("create_sphere", "geometry", "maya", false),
            catalog_action("export_layer", "geometry", "photoshop", false),
        ]);

        assert_eq!(catalog.load_skill("geometry").unwrap().len(), 2);
        assert!(catalog.is_loaded("geometry"));
        assert_eq!(catalog.unload_skill("geometry").unwrap(), 2);
        assert!(!catalog.is_loaded("geometry"));
    }

    #[test]
    fn audit_alias_implements_sink_contract() {
        let sink = RecordingAuditSink::new();
        assert!(sink.is_empty());
        let _sink: &dyn crate::AuditSink = &sink;
    }
}
