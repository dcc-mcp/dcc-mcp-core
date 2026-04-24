use super::*;

#[tokio::test]
async fn test_gateway_health_endpoint() {
    let server = TestServer::new(make_gateway_router());
    let resp = server.get("/health").await;
    resp.assert_status_ok();
    let body: serde_json::Value = resp.json();
    assert_eq!(body["ok"], true);
}

#[tokio::test]
async fn test_gateway_instances_endpoint_empty() {
    let server = TestServer::new(make_gateway_router());
    let resp = server.get("/instances").await;
    resp.assert_status_ok();
    let body: serde_json::Value = resp.json();
    assert_eq!(body["total"], 0);
    assert!(body["instances"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn test_gateway_instances_endpoint_with_entry() {
    let state = make_gateway_state();
    {
        let reg = state.registry.read().await;
        let entry = ServiceEntry::new("maya", "127.0.0.1", 18812);
        reg.register(entry).unwrap();
    }
    let server = TestServer::new(build_gateway_router(state));
    let resp = server.get("/instances").await;
    resp.assert_status_ok();
    let body: serde_json::Value = resp.json();
    assert_eq!(body["total"], 1);
}
