//! Dry-run validation for declared skill execution modes.
//!
//! The probe registers a skill's complete tool table with deterministic mock
//! handlers, then calls every tool through the same Core routing path used by
//! MCP `tools/call`. It never executes adapter or DCC code.

use std::sync::Arc;

use dcc_mcp_actions::ToolDispatcher;
use dcc_mcp_actions::registry::{ToolMeta, ToolRegistry};
use dcc_mcp_models::{ExecutionMode, SkillMetadata};
use dcc_mcp_skill_rest::StaticReadiness;
use dcc_mcp_skills::SkillCatalog;
use serde::Serialize;
use serde_json::{Value, json};

use crate::rmcp_registry_context::RegistryContext;
use crate::rmcp_tool_call_dispatch::dispatch_rmcp_tool_call;
use crate::server_state::ServerState;

/// Result of probing one skill's complete declared tool table.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ExecutionContractReport {
    /// Number of tool declarations invoked through the mock dispatch lane.
    pub checked: usize,
    /// Contract mismatches or routing failures.
    pub issues: Vec<ExecutionContractIssue>,
}

/// One declared-versus-observed execution-mode mismatch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExecutionContractIssue {
    /// Tool name as declared in the skill.
    pub tool: String,
    /// `sync` or `async` from `tools.yaml`.
    pub declared: String,
    /// `result`, `job_envelope`, or `error` observed from Core routing.
    pub observed: String,
    /// Actionable failure description.
    pub message: String,
}

/// Invoke every declared tool through Core's real dispatch router using safe,
/// deterministic mock handlers.
///
/// The probe validates routing semantics only. Adapter code and DCC APIs are
/// never imported or executed, so it is safe for ordinary CI workers.
pub async fn probe_skill_execution_contracts(metadata: &SkillMetadata) -> ExecutionContractReport {
    let registry = ToolRegistry::new();
    let dispatcher = Arc::new(ToolDispatcher::new(registry.clone()));
    let mut probes = Vec::with_capacity(metadata.tools.len());

    for (index, declaration) in metadata.tools.iter().enumerate() {
        let route_name = format!("execution_contract_probe__{index}");
        registry.register_action(ToolMeta {
            name: route_name.clone(),
            description: format!("Execution contract probe for {}", declaration.name),
            dcc: metadata.dcc.clone(),
            input_schema: json!({"type": "object"}),
            execution: declaration.execution,
            timeout_hint_secs: declaration.timeout_hint_secs,
            job_strategy: declaration.job_strategy,
            thread_affinity: declaration.thread_affinity,
            enforce_thread_affinity: declaration.enforce_thread_affinity,
            ..Default::default()
        });
        let marker = json!({"execution_contract_probe": declaration.name});
        dispatcher.register_handler(&route_name, move |_| Ok(marker.clone()));
        probes.push((declaration, route_name));
    }

    let registry = Arc::new(registry);
    let catalog = Arc::new(SkillCatalog::new_with_dispatcher(
        Arc::clone(&registry),
        Arc::clone(&dispatcher),
    ));
    let state = ServerState::builder(registry, dispatcher, catalog)
        .with_standalone_main_thread_execution(true)
        .build();
    let context = ready_context();
    let mut report = ExecutionContractReport {
        checked: probes.len(),
        issues: Vec::new(),
    };

    for (declaration, route_name) in probes {
        let result =
            dispatch_rmcp_tool_call(&state, &context, None, &route_name, Some(json!({})), None)
                .await;

        let (observed, matches_contract) = match result {
            Ok(result) if result.is_error => ("error", false),
            Ok(result) if is_pending_job(result.structured_content.as_ref()) => (
                "job_envelope",
                matches!(declaration.execution, ExecutionMode::Async),
            ),
            Ok(_) => (
                "result",
                matches!(declaration.execution, ExecutionMode::Sync),
            ),
            Err(_) => ("error", false),
        };

        if !matches_contract {
            let declared = execution_label(declaration.execution);
            report.issues.push(ExecutionContractIssue {
                tool: declaration.name.clone(),
                declared: declared.to_string(),
                observed: observed.to_string(),
                message: format!(
                    "tool '{}' declares execution: {declared} but Core mock dispatch returned {observed}",
                    declaration.name
                ),
            });
        }
    }

    report
}

fn execution_label(mode: ExecutionMode) -> &'static str {
    match mode {
        ExecutionMode::Sync => "sync",
        ExecutionMode::Async => "async",
    }
}

fn is_pending_job(value: Option<&Value>) -> bool {
    value.is_some_and(|value| {
        value.get("job_id").and_then(Value::as_str).is_some()
            && value.get("status").and_then(Value::as_str) == Some("pending")
    })
}

fn ready_context() -> RegistryContext {
    RegistryContext {
        resource_provider: None,
        prompt_provider: None,
        readiness: Arc::new(StaticReadiness::fully_ready()),
        on_skill_catalog_mutated: Arc::new(|| {}),
    }
}
