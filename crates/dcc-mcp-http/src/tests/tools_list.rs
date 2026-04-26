use super::*;

// ── tools/list ────────────────────────────────────────────────────────

#[tokio::test]
pub async fn test_tools_list() {
    let server = TestServer::new(make_router());

    let resp = server
        .post("/mcp")
        .add_header(
            axum::http::header::ACCEPT,
            "application/json".parse::<HeaderValue>().unwrap(),
        )
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/list"
        }))
        .await;

    resp.assert_status_ok();
    let body: Value = resp.json();
    let tools = body["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 16); // 14 core (11 + register_tool/deregister_tool/list_dynamic_tools #462) + 2 registered
    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert!(names.contains(&"get_scene_info"));
    assert!(names.contains(&"list_objects"));
    assert!(names.contains(&"search_skills"));
    assert!(names.contains(&"load_skill"));
    assert!(names.contains(&"search_skills"));
    assert!(names.contains(&"activate_tool_group"));
    assert!(names.contains(&"deactivate_tool_group"));
    assert!(names.contains(&"search_tools"));
    assert!(
        names.contains(&"jobs.get_status"),
        "tools/list must always expose the built-in jobs.get_status (#319)"
    );
}

#[tokio::test]
pub async fn test_tools_list_includes_unloaded_skill_stubs() {
    let server = TestServer::new(make_router_with_skills());

    let resp = server
        .post("/mcp")
        .add_header(
            axum::http::header::ACCEPT,
            "application/json".parse::<HeaderValue>().unwrap(),
        )
        .json(&json!({"jsonrpc": "2.0", "id": 15, "method": "tools/list"}))
        .await;

    resp.assert_status_ok();
    let body: Value = resp.json();
    let tools = body["result"]["tools"].as_array().unwrap();
    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert!(
        names.contains(&"__skill__maya-bevel"),
        "Expected stub __skill__maya-bevel, got: {names:?}"
    );
    assert!(
        names.contains(&"__skill__git-tools"),
        "Expected stub __skill__git-tools, got: {names:?}"
    );

    let maya_stub = tools
        .iter()
        .find(|t| t["name"] == "__skill__maya-bevel")
        .unwrap();
    assert_eq!(maya_stub["annotations"], serde_json::Value::Null);
}

#[tokio::test]
pub async fn test_loaded_tools_have_namespaced_names() {
    let server = TestServer::new(make_router_with_skill());
    server.post("/mcp")
            .add_header(axum::http::header::ACCEPT, "application/json".parse::<HeaderValue>().unwrap())
            .json(&json!({"jsonrpc":"2.0","id":100,"method":"tools/call","params":{"name":"load_skill","arguments":{"skill_name":"modeling-bevel"}}}))
            .await;
    let resp = server
        .post("/mcp")
        .add_header(
            axum::http::header::ACCEPT,
            "application/json".parse::<HeaderValue>().unwrap(),
        )
        .json(&json!({"jsonrpc":"2.0","id":101,"method":"tools/list"}))
        .await;
    let body: Value = resp.json();
    let tools = body["result"]["tools"].as_array().unwrap();
    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert!(
        names.contains(&"bevel"),
        "Expected bare `bevel`, got: {names:?}"
    );
    assert!(
        names.contains(&"chamfer"),
        "Expected bare `chamfer`, got: {names:?}"
    );
    assert!(
        !names.contains(&"modeling_bevel__bevel"),
        "Old __ name must not appear: {names:?}"
    );
}

#[tokio::test]
pub async fn test_core_tools_keep_bare_names() {
    let server = TestServer::new(make_router_with_skill());
    let resp = server
        .post("/mcp")
        .add_header(
            axum::http::header::ACCEPT,
            "application/json".parse::<HeaderValue>().unwrap(),
        )
        .json(&json!({"jsonrpc":"2.0","id":120,"method":"tools/list"}))
        .await;
    let body: Value = resp.json();
    let tools = body["result"]["tools"].as_array().unwrap();
    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    for core in &[
        "list_skills",
        "get_skill_info",
        "load_skill",
        "unload_skill",
        "search_skills",
        "activate_tool_group",
        "deactivate_tool_group",
        "search_tools",
    ] {
        assert!(
            names.contains(core),
            "Core '{core}' must be bare, got: {names:?}"
        );
    }
}
