use std::sync::Arc;
use std::time::Duration;

use dcc_mcp_actions::{ToolDispatcher, ToolRegistry, registry::ToolMeta};
use dcc_mcp_models::{ExecutionMode, ThreadAffinity};
use dcc_mcp_skill_rest::StaticReadiness;
use dcc_mcp_skills::SkillCatalog;
use rmcp::model::{CallToolRequestParams, Meta, NumberOrString};
use rmcp::service::{RequestContext, serve_directly};
use rmcp::{RoleServer, ServerHandler};
use serde_json::{Value, json};

use super::{DccMcpHandler, RegistryContext};
use crate::server_state::ServerState;

struct ContextPeer;
impl ServerHandler for ContextPeer {}

fn handler() -> DccMcpHandler {
    let registry = Arc::new(ToolRegistry::new());
    registry.register_action(ToolMeta {
        name: "metadata_probe".into(),
        description: "Observe metadata delivered to a synchronous tool".into(),
        dcc: "custom-renderer".into(),
        input_schema: json!({"type":"object"}),
        execution: ExecutionMode::Sync,
        thread_affinity: ThreadAffinity::Any,
        ..Default::default()
    });
    let dispatcher = Arc::new(ToolDispatcher::new((*registry).clone()));
    dispatcher.register_handler("metadata_probe", |args| {
        Ok(json!({"marker":args["marker"], "meta":args.get("_meta")}))
    });
    let catalog = Arc::new(SkillCatalog::new_with_dispatcher(
        Arc::clone(&registry),
        Arc::clone(&dispatcher),
    ));
    DccMcpHandler::new(
        ServerState::builder(registry, dispatcher, catalog).build(),
        Arc::new(RegistryContext {
            resource_provider: None,
            prompt_provider: None,
            readiness: Arc::new(StaticReadiness::fully_ready()),
            on_skill_catalog_mutated: Arc::new(|| {}),
        }),
    )
}

fn meta(value: Value) -> Meta {
    serde_json::from_value(value).expect("metadata object")
}

async fn invoke(
    context_meta: Value,
    request_meta: Option<Value>,
    legacy_meta: Option<Value>,
) -> (Value, Value) {
    let handler = handler();
    let (stream, _remote) = tokio::io::duplex(1024);
    let service = serve_directly(ContextPeer, stream, None);
    let peer = service.peer().clone();
    let mut context = RequestContext::<RoleServer>::new(NumberOrString::Number(1), peer.clone());
    context.meta = meta(context_meta);
    let mut arguments = json!({"marker":"executed"});
    if let Some(value) = legacy_meta {
        arguments["_meta"] = value;
    }
    let mut request = CallToolRequestParams::new("metadata_probe")
        .with_arguments(arguments.as_object().unwrap().clone());
    request.meta = request_meta.map(meta);
    let response = handler
        .call_tool(request, context)
        .await
        .expect("tool call");
    assert_ne!(response.is_error, Some(true));
    let initial = response.structured_content.expect("structured result");
    let terminal = if let Some(job_id) = initial["job_id"].as_str() {
        assert_eq!(initial["status"], "pending");
        assert_eq!(handler.state.jobs.list().len(), 1);
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                let request = CallToolRequestParams::new("jobs_get_status").with_arguments(
                    json!({"job_id":job_id, "include_result":true})
                        .as_object()
                        .unwrap()
                        .clone(),
                );
                let response = handler
                    .call_tool(
                        request,
                        RequestContext::new(NumberOrString::Number(2), peer.clone()),
                    )
                    .await
                    .expect("job status");
                assert_ne!(response.is_error, Some(true));
                let status = response.structured_content.expect("structured job status");
                match status["status"].as_str() {
                    Some("completed") => break status["result"].clone(),
                    Some("pending" | "running") => tokio::task::yield_now().await,
                    _ => panic!("unexpected terminal job: {status}"),
                }
            }
        })
        .await
        .expect("job must complete")
    } else {
        assert!(handler.state.jobs.list().is_empty());
        initial.clone()
    };
    assert_eq!(terminal["marker"], "executed");
    service.cancel().await.expect("stop context peer");
    (initial, terminal)
}

#[tokio::test]
async fn direct_typed_request_meta_keeps_explicit_async() {
    let (initial, terminal) = invoke(json!({}), Some(json!({"dcc":{"async":true}})), None).await;
    assert_eq!(initial["status"], "pending");
    assert_eq!(terminal.pointer("/meta/dcc/async"), Some(&json!(true)));
}

#[tokio::test]
async fn context_null_token_alone_keeps_sync() {
    let (initial, _) = invoke(json!({"progressToken":null}), None, None).await;
    assert!(initial.get("job_id").is_none());
}

#[tokio::test]
async fn context_null_token_keeps_legacy_arguments_fallback() {
    let (initial, terminal) = invoke(
        json!({"progressToken":null}),
        None,
        Some(json!({"progressToken":"legacy"})),
    )
    .await;
    assert_eq!(initial["status"], "pending");
    assert_eq!(
        terminal.pointer("/meta/progressToken"),
        Some(&json!("legacy"))
    );
}

#[tokio::test]
async fn context_token_and_parent_win_over_request_and_legacy_metadata() {
    let (initial, terminal) = invoke(
        json!({"progressToken":"context", "dcc":{"parentJobId":"context-parent"}}),
        Some(json!({"progressToken":"request", "dcc":{"parentJobId":"request-parent"}})),
        Some(json!({"progressToken":"legacy", "dcc":{"parentJobId":"legacy-parent"}})),
    )
    .await;
    assert_eq!(initial["status"], "pending");
    assert_eq!(initial["parent_job_id"], "context-parent");
    assert_eq!(
        terminal.pointer("/meta/progressToken"),
        Some(&json!("context"))
    );
    assert_eq!(
        terminal.pointer("/meta/dcc/parentJobId"),
        Some(&json!("context-parent"))
    );
}
