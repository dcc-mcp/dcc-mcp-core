//! Real REST/MCP routes must agree on job admission, not wall-clock duration.

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;

use dcc_mcp_actions::{ToolDispatcher, ToolRegistry, registry::ToolMeta};
use dcc_mcp_http::{McpHttpConfig, McpHttpServer};
use dcc_mcp_models::ExecutionMode;
use serde_json::{Value, json};

async fn mcp_call(
    client: &reqwest::Client,
    base: &str,
    name: &str,
    arguments: Value,
    meta: Option<Value>,
) -> Value {
    static NEXT_REQUEST_ID: AtomicUsize = AtomicUsize::new(0);
    let request_id = format!(
        "admission-{}",
        NEXT_REQUEST_ID.fetch_add(1, Ordering::Relaxed)
    );
    let mut params = json!({"name": name, "arguments": arguments});
    if let Some(meta) = meta {
        params["_meta"] = meta;
    }
    let response = client
        .post(format!("{base}/mcp"))
        .header("Accept", "application/json, text/event-stream")
        .header("MCP-Protocol-Version", "2025-06-18")
        .json(&json!({"jsonrpc":"2.0", "id":request_id, "method":"tools/call", "params":params}))
        .send()
        .await
        .expect("MCP response");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let body: Value = response.json().await.expect("MCP JSON");
    assert_eq!(body["id"], request_id);
    assert!(body.get("error").is_none(), "{body}");
    assert_ne!(body["result"]["isError"], true, "{body}");
    body["result"]["structuredContent"].clone()
}

async fn terminal_result(
    client: &reqwest::Client,
    base: &str,
    output: Value,
    pending: bool,
) -> Value {
    if !pending {
        assert!(output.get("job_id").is_none(), "{output}");
        return output;
    }
    assert_eq!(
        output["status"], "pending",
        "expected a pending envelope: {output}"
    );
    let id = output["job_id"].as_str().expect("job id");
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let job = mcp_call(
                client,
                base,
                "jobs_get_status",
                json!({"job_id":id,"include_result":true}),
                None,
            )
            .await;
            assert_eq!(job["job_id"], id);
            match job["status"].as_str().expect("job status") {
                "completed" => return job["result"].clone(),
                "pending" | "running" => tokio::time::sleep(Duration::from_millis(5)).await,
                _ => panic!("unexpected terminal job: {job}"),
            }
        }
    })
    .await
    .expect("bounded job completion")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rest_and_mcp_use_explicit_execution_contracts() {
    let registry = Arc::new(ToolRegistry::new());
    let dispatcher = Arc::new(ToolDispatcher::new((*registry).clone()));
    let calls = Arc::new(AtomicUsize::new(0));
    for (name, execution) in [
        ("sync_probe", ExecutionMode::Sync),
        ("async_probe", ExecutionMode::Async),
    ] {
        registry.register_action(ToolMeta {
            name: name.into(), dcc: "blender".into(),
            description: "bounded execution probe".into(),
            input_schema: json!({"type":"object","properties":{"marker":{"type":"string"}},"required":["marker"]}),
            execution, timeout_hint_secs: Some(5), ..Default::default()
        });
        let calls = Arc::clone(&calls);
        dispatcher.register_handler(name, move |args| {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(json!({"marker":args["marker"]}))
        });
    }
    let mut config = McpHttpConfig::default();
    config.server.port = 0;
    config.gateway.gateway_port = 0;
    let handle = McpHttpServer::new(registry, config)
        .with_dispatcher(dispatcher)
        .start()
        .await
        .expect("server");
    let base = format!("http://127.0.0.1:{}", handle.port);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap();
    let cases = [
        ("sync_probe", None, false, false),
        (
            "sync_probe",
            Some(json!({"progressToken":null})),
            false,
            false,
        ),
        (
            "sync_probe",
            Some(json!({"progressToken":false})),
            false,
            false,
        ),
        (
            "sync_probe",
            Some(json!({"progressToken":[]})),
            false,
            false,
        ),
        (
            "sync_probe",
            Some(json!({"progressToken":{}})),
            false,
            false,
        ),
        (
            "sync_probe",
            Some(json!({"progressToken":"progress"})),
            true,
            true,
        ),
        ("sync_probe", Some(json!({"progressToken":0})), true, true),
        // Legacy numeric tokens are not narrowed by modern ingress rules here.
        ("sync_probe", Some(json!({"progressToken":0.5})), true, true),
        (
            "sync_probe",
            Some(json!({"dcc":{"async":true}})),
            true,
            true,
        ),
        (
            "sync_probe",
            Some(json!({"dcc":{"async":false}})),
            false,
            false,
        ),
        (
            "sync_probe",
            Some(json!({"dcc":{"async":false},"progressToken":"progress"})),
            true,
            true,
        ),
        ("async_probe", None, true, true),
        (
            "async_probe",
            Some(json!({"dcc":{"async":false}})),
            true,
            true,
        ),
        (
            "async_probe",
            Some(json!({"progressToken":null})),
            true,
            true,
        ),
        // REST's existing alias is not added to the MCP wire schema.
        (
            "sync_probe",
            Some(json!({"progress_token":"rest-alias"})),
            true,
            false,
        ),
        (
            "sync_probe",
            Some(json!({"progress_token":null})),
            false,
            false,
        ),
        (
            "sync_probe",
            Some(json!({"progressToken":null,"progress_token":"masked"})),
            false,
            false,
        ),
    ];
    let paired_calls = cases.len() * 2;
    for (index, (name, meta, rest_pending, mcp_pending)) in cases.into_iter().enumerate() {
        let marker = format!("case-{index}");
        let mut request =
            json!({"tool_slug":format!("blender.core.{name}"),"params":{"marker":marker}});
        if let Some(meta) = &meta {
            request["meta"] = meta.clone();
        }
        let response = client
            .post(format!("{base}/v1/call"))
            .json(&request)
            .send()
            .await
            .expect("REST response");
        assert_eq!(
            response.status().as_u16(),
            if rest_pending { 202 } else { 200 },
            "REST case {index}"
        );
        let body: Value = response.json().await.expect("REST JSON");
        let rest = terminal_result(&client, &base, body["output"].clone(), rest_pending).await;
        let mcp = mcp_call(&client, &base, name, json!({"marker":marker}), meta).await;
        let mcp = terminal_result(&client, &base, mcp, mcp_pending).await;
        assert_eq!(rest, json!({"marker":marker}), "REST case {index}");
        assert_eq!(mcp, rest, "MCP case {index}");
        assert_eq!(
            calls.load(Ordering::SeqCst),
            (index + 1) * 2,
            "one execution per call"
        );
    }
    // MCP's old arguments._meta fallback differs from REST's snake alias:
    // canonical null continues to accept a valid legacy progress token.
    for (index, meta) in [None, Some(json!({"progressToken":null}))]
        .into_iter()
        .enumerate()
    {
        let marker = format!("legacy-{index}");
        let output = mcp_call(
            &client,
            &base,
            "sync_probe",
            json!({
                "marker":marker, "_meta":{"progressToken":"legacy-progress"}
            }),
            meta,
        )
        .await;
        let result = terminal_result(&client, &base, output, true).await;
        assert_eq!(result, json!({"marker":marker}));
        assert_eq!(calls.load(Ordering::SeqCst), paired_calls + index + 1);
    }
    handle.shutdown().await;
}
