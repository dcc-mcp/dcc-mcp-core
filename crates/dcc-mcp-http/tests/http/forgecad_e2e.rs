//! Third-party skill ecosystem E2E coverage using a ForgeCAD-style skill.
//!
//! ForgeCAD publishes public `SKILL.md` packages under `skills/*`. This test
//! keeps the CI fixture local and deterministic while preserving the public
//! ForgeCAD metadata shape: a non-DCC-specific skill directory, a `SKILL.md`,
//! script-backed tools, MCP progressive loading, and the REST `/v1/call` API.

use std::sync::Arc;
use std::time::{Duration, Instant};

use dcc_mcp_actions::{ActionDispatcher, ActionRegistry};
use dcc_mcp_http::{McpHttpConfig, McpHttpServer};
use dcc_mcp_skills::SkillCatalog;
use serde_json::{Value, json};

async fn wait_reachable(addr: &str) -> bool {
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        if tokio::net::TcpStream::connect(addr).await.is_ok() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    false
}

fn write_forgecad_skill(root: &std::path::Path) {
    let skill_dir = root.join("forgecad-make-a-model");
    std::fs::create_dir_all(skill_dir.join("scripts")).unwrap();
    std::fs::write(
        skill_dir.join("SKILL.md"),
        r#"---
name: forgecad-make-a-model
description: Create new ForgeCAD (.forge.js) models in the active CAD project. Handles file placement, invokes the forgecad skill for API guidance, and validates the result.
dcc: forgecad
forgecad-public: true
tags: [forgecad, cad, third-party]
tools:
  - name: create_model
    description: Create a ForgeCAD model from a brief and return the generated file path.
    source_file: scripts/create_model.py
    input_schema:
      type: object
      properties:
        brief:
          type: string
      required: [brief]
---
# Make a Model

Create new ForgeCAD models in the user's active ForgeCAD project.
"#,
    )
    .unwrap();
    std::fs::write(
        skill_dir.join("scripts/create_model.py"),
        "# The E2E uses an in-process executor; this file proves source resolution.\n",
    )
    .unwrap();
}

async fn mcp_post(
    client: &reqwest::Client,
    addr: &str,
    id: u64,
    method: &str,
    params: Value,
) -> Value {
    let resp = client
        .post(format!("http://{addr}/mcp"))
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .header(reqwest::header::ACCEPT, "application/json")
        .json(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        }))
        .send()
        .await
        .expect("MCP POST must complete");
    assert!(
        resp.status().is_success(),
        "MCP POST returned {}",
        resp.status()
    );
    resp.json::<Value>()
        .await
        .expect("MCP response must be JSON")
}

fn tool_text(response: &Value) -> &str {
    response["result"]["content"][0]["text"]
        .as_str()
        .expect("tool result text")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn forgecad_skill_discovers_loads_and_calls_over_mcp_and_rest_http() {
    let tmp = tempfile::tempdir().unwrap();
    write_forgecad_skill(tmp.path());

    let registry = Arc::new(ActionRegistry::new());
    let dispatcher = Arc::new(ActionDispatcher::new((*registry).clone()));
    let catalog = Arc::new(SkillCatalog::new_with_dispatcher(
        registry.clone(),
        dispatcher.clone(),
    ));
    catalog.set_in_process_executor(|script_path, params, context| {
        Ok(json!({
            "success": true,
            "ecosystem": "forgecad",
            "script_path": script_path,
            "action_name": context.action_name,
            "brief": params.get("brief").and_then(Value::as_str).unwrap_or_default(),
            "generated": "models/acceptance-test.forge.js",
        }))
    });

    let discovered = catalog.discover(Some(&[tmp.path().to_string_lossy().to_string()]), None);
    assert_eq!(discovered, 1, "ForgeCAD fixture should be discovered");

    let server = McpHttpServer::with_catalog(registry, catalog, McpHttpConfig::new(0));
    let handle = server.start().await.expect("server must start");
    let addr = handle.bind_addr.clone();
    assert!(wait_reachable(&addr).await, "server unreachable");

    let client = reqwest::Client::new();

    let listed_before = mcp_post(
        &client,
        &addr,
        1,
        "tools/call",
        json!({
            "name": "list_skills",
            "arguments": {"status": "discovered"}
        }),
    )
    .await;
    assert!(
        tool_text(&listed_before).contains("forgecad-make-a-model"),
        "discovered ForgeCAD skill missing from list_skills: {listed_before}"
    );

    let loaded = mcp_post(
        &client,
        &addr,
        2,
        "tools/call",
        json!({
            "name": "load_skill",
            "arguments": {"skill_name": "forgecad-make-a-model"}
        }),
    )
    .await;
    let load_text = tool_text(&loaded);
    assert!(
        load_text.contains("create_model"),
        "load_skill output: {load_text}"
    );

    let tools = mcp_post(&client, &addr, 3, "tools/list", json!({})).await;
    let tool_names: Vec<&str> = tools["result"]["tools"]
        .as_array()
        .expect("tools array")
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect();
    assert!(
        tool_names.contains(&"create_model"),
        "loaded ForgeCAD tool missing from tools/list: {tool_names:?}"
    );

    let called = mcp_post(
        &client,
        &addr,
        4,
        "tools/call",
        json!({
            "name": "create_model",
            "arguments": {"brief": "parametric bracket"}
        }),
    )
    .await;
    let call_text = tool_text(&called);
    assert!(
        call_text.contains("parametric bracket"),
        "MCP call output: {call_text}"
    );
    assert!(
        call_text.contains("acceptance-test.forge.js"),
        "MCP call output: {call_text}"
    );

    let search = client
        .post(format!("http://{addr}/v1/search"))
        .json(&json!({"query": "forgecad", "loaded_only": true}))
        .send()
        .await
        .expect("REST search must complete");
    assert_eq!(search.status(), 200);
    let search_json = search.json::<Value>().await.unwrap();
    let slug = search_json["hits"]
        .as_array()
        .unwrap()
        .iter()
        .find_map(|hit| {
            let slug = hit["slug"].as_str()?;
            slug.contains("forgecad-make-a-model")
                .then_some(slug.to_string())
        })
        .expect("REST search should expose ForgeCAD tool slug");

    let rest_call = client
        .post(format!("http://{addr}/v1/call"))
        .json(&json!({
            "tool_slug": slug,
            "params": {"brief": "rest-driven model"}
        }))
        .send()
        .await
        .expect("REST call must complete");
    assert_eq!(rest_call.status(), 200);
    let rest_json = rest_call.json::<Value>().await.unwrap();
    assert_eq!(rest_json["output"]["ecosystem"], "forgecad");
    assert_eq!(rest_json["output"]["brief"], "rest-driven model");

    handle.shutdown().await;
}
