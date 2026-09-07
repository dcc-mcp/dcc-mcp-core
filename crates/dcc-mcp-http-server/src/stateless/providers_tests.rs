//! Provider parity regressions, not full 2026 wire conformance tests.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use dcc_mcp_actions::{ToolDispatcher, ToolRegistry};
use dcc_mcp_http_types::config::FeatureFlags;
use dcc_mcp_jsonrpc::{
    GetPromptResult, JsonRpcRequest, JsonRpcRequestBuilder, McpPrompt, McpPromptArgument,
    McpPromptContent, McpPromptMessage, McpResource, ReadResourceResult, ResourceContents,
    encode_cursor,
};
use dcc_mcp_skill_rest::StaticReadiness;
use dcc_mcp_skills::SkillCatalog;
use serde_json::{Value, json};

use crate::rmcp_providers::{PromptProvider, ProviderError, ResourceProvider};
use crate::rmcp_registry_context::RegistryContext;
use crate::server_state::ServerState;
use crate::stateless::{StatelessDispatchOutcome, StatelessMcpService};

#[derive(Default)]
struct FixtureProvider {
    resources: Vec<McpResource>,
    contents: HashMap<String, ResourceContents>,
    prompts: Vec<McpPrompt>,
    diagnostics: Option<Value>,
    error: Option<ProviderError>,
    calls: AtomicUsize,
}

impl ResourceProvider for FixtureProvider {
    fn list_resources(&self, _catalog: &Arc<SkillCatalog>) -> Vec<McpResource> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.resources.clone()
    }

    fn read_resource(
        &self,
        uri: &str,
        _catalog: &Arc<SkillCatalog>,
    ) -> Result<ReadResourceResult, ProviderError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if let Some(error) = &self.error {
            return Err(error.clone());
        }
        self.contents
            .get(uri)
            .cloned()
            .map(|content| ReadResourceResult {
                contents: vec![content],
            })
            .ok_or_else(|| ProviderError::NotFound(uri.into()))
    }
}

impl PromptProvider for FixtureProvider {
    fn list_prompts(&self, _catalog: &Arc<SkillCatalog>) -> Vec<McpPrompt> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.prompts.clone()
    }

    fn prompt_diagnostics(&self, _catalog: &Arc<SkillCatalog>) -> Option<Value> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.diagnostics.clone()
    }

    fn get_prompt(
        &self,
        name: &str,
        arguments: &HashMap<String, String>,
        _catalog: &Arc<SkillCatalog>,
    ) -> Result<GetPromptResult, ProviderError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if let Some(error) = &self.error {
            return Err(error.clone());
        }
        if !self.prompts.iter().any(|prompt| prompt.name == name) {
            return Err(ProviderError::NotFound(name.into()));
        }
        let asset = arguments
            .get("asset")
            .ok_or_else(|| ProviderError::MissingArg("asset".into()))?;
        let note = arguments
            .get("note")
            .map(String::as_str)
            .unwrap_or("no note");
        Ok(GetPromptResult {
            description: Some("Review the named DCC asset".into()),
            messages: vec![McpPromptMessage {
                role: "user".into(),
                content: McpPromptContent::text(format!("{name}: {asset}; {note}")),
            }],
        })
    }
}

fn resource(uri: &str, name: &str, mime_type: &str) -> McpResource {
    McpResource {
        uri: uri.into(),
        name: name.into(),
        description: Some(format!("Current {name}")),
        mime_type: Some(mime_type.into()),
    }
}

fn prompt(name: &str) -> McpPrompt {
    McpPrompt {
        name: name.into(),
        description: Some("Review a scene asset".into()),
        arguments: vec![McpPromptArgument {
            name: "asset".into(),
            description: Some("Asset to inspect".into()),
            required: true,
        }],
        meta: Some(json!({"dcc.source": {"skill": "scene-review"}})),
    }
}

fn populated_provider() -> Arc<FixtureProvider> {
    let text = ResourceContents {
        uri: "scene://blender/current".into(),
        mime_type: Some("application/json".into()),
        text: Some(r#"{"objects":["Birch"],"dcc":"blender"}"#.into()),
        blob: None,
    };
    let blob = ResourceContents {
        uri: "capture://photoshop/current".into(),
        mime_type: Some("image/png".into()),
        text: None,
        blob: Some("cG5n".into()),
    };
    Arc::new(FixtureProvider {
        resources: vec![
            resource(&text.uri, "Blender scene", "application/json"),
            resource(&blob.uri, "Photoshop capture", "image/png"),
        ],
        contents: [(text.uri.clone(), text), (blob.uri.clone(), blob)]
            .into_iter()
            .collect(),
        prompts: vec![prompt("maya_review"), prompt("blender_review")],
        ..FixtureProvider::default()
    })
}

fn service(
    provider: Option<Arc<FixtureProvider>>,
    enable_resources: bool,
    enable_prompts: bool,
) -> StatelessMcpService {
    let registry = Arc::new(ToolRegistry::new());
    let dispatcher = Arc::new(ToolDispatcher::new((*registry).clone()));
    let catalog = Arc::new(SkillCatalog::new_with_dispatcher(
        Arc::clone(&registry),
        Arc::clone(&dispatcher),
    ));
    let state = ServerState::builder(registry, dispatcher, catalog)
        .with_features(FeatureFlags {
            enable_resources,
            enable_prompts,
            ..FeatureFlags::default()
        })
        .build();
    let context = Arc::new(RegistryContext {
        resource_provider: provider
            .clone()
            .map(|value| value as Arc<dyn ResourceProvider>),
        prompt_provider: provider.map(|value| value as Arc<dyn PromptProvider>),
        readiness: Arc::new(StaticReadiness::fully_ready()),
        on_skill_catalog_mutated: Arc::new(|| {}),
    });
    StatelessMcpService::new(state, context)
}

fn request(method: &str, params: Option<Value>) -> JsonRpcRequest {
    let mut params = params
        .filter(|value| !value.is_null())
        .unwrap_or_else(|| json!({}));
    if let Some(object) = params.as_object_mut() {
        object.insert(
            "_meta".into(),
            json!({
                dcc_mcp_jsonrpc::PROTOCOL_VERSION_META_KEY: "2026-07-28",
                dcc_mcp_jsonrpc::CLIENT_CAPABILITIES_META_KEY: {}
            }),
        );
    }
    serde_json::from_value(
        JsonRpcRequestBuilder::new("provider-request", method)
            .with_params(params)
            .to_value(),
    )
    .unwrap()
}

async fn call(service: &StatelessMcpService, method: &str, params: Option<Value>) -> Value {
    let request = request(method, params);
    let response = service.handle_request(&request).await.unwrap();
    assert_eq!(response["id"], "provider-request");
    assert_eq!(response["jsonrpc"], "2.0");
    if let Some(result) = response.get("result") {
        assert_eq!(result["resultType"], "complete");
        assert!(result["_meta"][dcc_mcp_jsonrpc::SERVER_INFO_META_KEY].is_object());
        if dcc_mcp_jsonrpc::CACHEABLE_RESULT_METHODS.contains(&method) {
            assert_eq!(result["ttlMs"], 0);
            assert_eq!(result["cacheScope"], "private");
        } else {
            assert!(result.get("ttlMs").is_none());
        }
    }
    response
}

fn assert_error(response: &Value, code: i64) {
    assert_eq!(response["error"]["code"], code, "{response}");
    assert!(
        response.get("result").is_none(),
        "errors must not carry fake success: {response}"
    );
}

#[tokio::test]
async fn resources_list_preserves_provider_identity_and_metadata() {
    let service = service(Some(populated_provider()), true, true);
    let response = call(&service, "resources/list", None).await;

    assert_eq!(
        response["result"]["resources"],
        json!([
            {"uri": "capture://photoshop/current", "name": "Photoshop capture", "description": "Current Photoshop capture", "mimeType": "image/png"},
            {"uri": "scene://blender/current", "name": "Blender scene", "description": "Current Blender scene", "mimeType": "application/json"},
        ])
    );
    assert!(response["result"].get("nextCursor").is_none());
}

#[tokio::test]
async fn resources_read_preserves_semantic_text_and_blob_without_placeholder_success() {
    let service = service(Some(populated_provider()), true, true);
    let text = call(
        &service,
        "resources/read",
        Some(json!({"uri": "scene://blender/current"})),
    )
    .await;
    let content = &text["result"]["contents"][0];
    assert_eq!(content["uri"], "scene://blender/current");
    assert_eq!(content["mimeType"], "application/json");
    assert_eq!(
        serde_json::from_str::<Value>(content["text"].as_str().unwrap()).unwrap(),
        json!({"objects": ["Birch"], "dcc": "blender"})
    );
    assert!(content.get("blob").is_none());

    let blob = call(
        &service,
        "resources/read",
        Some(json!({"uri": "capture://photoshop/current"})),
    )
    .await;
    assert_eq!(
        blob["result"]["contents"],
        json!([{"uri": "capture://photoshop/current", "mimeType": "image/png", "blob": "cG5n"}])
    );
    assert!(blob["result"]["contents"][0].get("text").is_none());
}

#[tokio::test]
async fn prompts_list_preserves_required_arguments_and_source_metadata() {
    let service = service(Some(populated_provider()), true, true);
    let response = call(&service, "prompts/list", Some(json!({}))).await;
    let prompts = response["result"]["prompts"].as_array().unwrap();

    assert_eq!(prompts.len(), 2);
    assert_eq!(prompts[0]["name"], "blender_review");
    assert_eq!(prompts[1]["name"], "maya_review");
    assert_eq!(
        prompts[0]["arguments"],
        json!([{"name": "asset", "description": "Asset to inspect", "required": true}])
    );
    assert_eq!(
        prompts[0]["_meta"],
        json!({"dcc.source": {"skill": "scene-review"}})
    );
    assert!(response["result"].get("nextCursor").is_none());
}

#[tokio::test]
async fn prompts_get_renders_provider_output_using_exact_string_arguments() {
    let service = service(Some(populated_provider()), true, true);
    let response = call(
        &service,
        "prompts/get",
        Some(json!({
            "name": "maya_review", "arguments": {"asset": "Oak {literal}", "note": "caf\u{e9} 001"}
        })),
    )
    .await;

    assert_eq!(
        response["result"]["description"],
        "Review the named DCC asset"
    );
    assert_eq!(
        response["result"]["messages"],
        json!([
            {"role": "user", "content": {"type": "text", "text": "maya_review: Oak {literal}; caf\u{e9} 001"}}
        ])
    );
}

#[tokio::test]
async fn resource_and_prompt_lists_have_deterministic_complete_pagination() {
    let provider = Arc::new(FixtureProvider {
        resources: (0..130)
            .rev()
            .map(|index| {
                resource(
                    &format!("scene://blender/{index:03}"),
                    "Scene",
                    "application/json",
                )
            })
            .collect(),
        prompts: (0..130)
            .rev()
            .map(|index| prompt(&format!("maya_review_{index:03}")))
            .collect(),
        ..FixtureProvider::default()
    });
    let service = service(Some(provider), true, true);
    for (method, key, identity, prefix) in [
        ("resources/list", "resources", "uri", "scene://blender/"),
        ("prompts/list", "prompts", "name", "maya_review_"),
    ] {
        let mut cursor: Option<Value> = None;
        let mut collected = Vec::new();
        for expected_count in [64, 64, 2] {
            let params = cursor.clone().map(|cursor| json!({"cursor": cursor}));
            let response = call(&service, method, params.clone()).await;
            let repeated = call(&service, method, params).await;
            let result = &response["result"];
            let rows = result[key].as_array().unwrap();
            assert_eq!(rows.len(), expected_count);
            assert_eq!(repeated["result"][key], result[key]);
            assert_eq!(
                repeated["result"].get("nextCursor"),
                result.get("nextCursor")
            );
            collected.extend(
                rows.iter()
                    .map(|row| row[identity].as_str().unwrap().to_owned()),
            );
            cursor = result.get("nextCursor").cloned();
            if expected_count == 64 {
                assert!(cursor.as_ref().is_some_and(Value::is_string));
            }
        }
        assert!(cursor.is_none());
        let expected: Vec<String> = (0..130)
            .map(|index| format!("{prefix}{index:03}"))
            .collect();
        assert_eq!(collected, expected);

        let past_end = call(
            &service,
            method,
            Some(json!({"cursor": encode_cursor(131)})),
        )
        .await;
        assert_error(&past_end, -32602);
    }
}

#[tokio::test]
async fn invalid_cursor_types_unicode_and_overflow_fail_without_panics_or_fake_pages() {
    let overflow = (usize::MAX as u128 + 1)
        .to_string()
        .bytes()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let cursors = vec![
        Value::Null,
        json!(64),
        json!([]),
        json!({}),
        json!(""),
        json!("xyz"),
        json!("zz"),
        json!("0\u{e9}0"),
        json!("a".repeat(42)),
        json!(overflow),
        json!(encode_cursor(usize::MAX)),
    ];
    let service = service(Some(populated_provider()), true, true);
    for method in ["resources/list", "prompts/list"] {
        for cursor in &cursors {
            let response = call(&service, method, Some(json!({"cursor": cursor}))).await;
            assert_error(&response, -32602);
        }
        // The shared request model normalizes params:null to absent params.
        for params in [json!([]), json!("cursor")] {
            let response = call(&service, method, Some(params)).await;
            assert_error(&response, -32602);
        }
    }
}

#[tokio::test]
async fn malformed_resource_parameters_are_rejected_before_provider_access() {
    let provider = populated_provider();
    let service = service(Some(Arc::clone(&provider)), true, true);
    for params in [
        None,
        Some(Value::Null),
        Some(json!([])),
        Some(json!({})),
        Some(json!({"uri": 42})),
        Some(json!({"uri": " \t"})),
    ] {
        assert_error(&call(&service, "resources/read", params).await, -32602);
    }
    assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn prompt_parameters_reject_non_string_arguments_without_stringification() {
    let provider = populated_provider();
    let service = service(Some(Arc::clone(&provider)), true, true);
    for value in [Value::Null, json!(true), json!(42), json!([]), json!({})] {
        let response = call(
            &service,
            "prompts/get",
            Some(json!({
                "name": "maya_review", "arguments": {"asset": value}
            })),
        )
        .await;
        assert_error(&response, -32602);
    }
    for params in [
        None,
        Some(Value::Null),
        Some(json!([])),
        Some(json!({})),
        Some(json!({"name": 42})),
        Some(json!({"name": " \t"})),
        Some(json!({"name": "maya_review", "arguments": null})),
        Some(json!({"name": "maya_review", "arguments": []})),
    ] {
        assert_error(&call(&service, "prompts/get", params).await, -32602);
    }
    assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
}

fn provider_requests() -> [(&'static str, Value); 4] {
    [
        ("resources/list", json!({})),
        ("resources/read", json!({"uri": "scene://blender/current"})),
        ("prompts/list", json!({})),
        (
            "prompts/get",
            json!({"name": "maya_review", "arguments": {"asset": "Oak"}}),
        ),
    ]
}

#[tokio::test]
async fn disabled_features_never_invoke_installed_providers() {
    let provider = populated_provider();
    let service = service(Some(Arc::clone(&provider)), false, false);
    for (method, params) in provider_requests() {
        assert_error(&call(&service, method, Some(params)).await, -32601);
        // Capability gating precedes business-parameter validation, after
        // the required modern request envelope has already been validated.
        assert_error(
            &call(
                &service,
                method,
                Some(json!({"cursor": 42, "uri": 42, "name": 42})),
            )
            .await,
            -32601,
        );
    }
    assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn absent_providers_return_method_not_found_instead_of_empty_success() {
    let service = service(None, true, true);
    for (method, params) in provider_requests() {
        assert_error(&call(&service, method, Some(params)).await, -32601);
    }
}

#[tokio::test]
async fn unavailable_provider_methods_preserve_method_not_found_origin() {
    for service in [
        service(None, true, true),
        service(Some(populated_provider()), false, false),
    ] {
        for (method, params) in provider_requests() {
            let outcome = service
                .handle_request_with_outcome(&request(method, Some(params)))
                .await;
            assert!(
                matches!(outcome, StatelessDispatchOutcome::MethodNotFound(_)),
                "{method}: {outcome:?}"
            );
        }
    }
    let service = service(Some(populated_provider()), true, true);
    let outcome = service
        .handle_request_with_outcome(&request(
            "resources/read",
            Some(json!({"uri": "scene://missing"})),
        ))
        .await;
    assert!(matches!(outcome, StatelessDispatchOutcome::Response(_)));
    assert_error(&outcome.into_response().unwrap(), -32602);
}

#[tokio::test]
async fn resource_and_prompt_feature_flags_are_independent() {
    for (resources_enabled, prompts_enabled) in [(true, false), (false, true)] {
        let service = service(
            Some(populated_provider()),
            resources_enabled,
            prompts_enabled,
        );
        for (method, params) in provider_requests() {
            let response = call(&service, method, Some(params)).await;
            if method.starts_with("resources/") == resources_enabled {
                assert!(response.get("result").is_some(), "{response}");
                assert!(response.get("error").is_none(), "{response}");
            } else {
                assert_error(&response, -32601);
            }
        }
    }
}

#[tokio::test]
async fn provider_errors_keep_their_classification_and_redact_internal_details() {
    const PRIVATE_DETAIL: &str = "provider-private-detail-must-not-leak";
    for (error, code) in [
        (ProviderError::NotFound(PRIVATE_DETAIL.into()), -32602),
        (ProviderError::MissingArg(PRIVATE_DETAIL.into()), -32602),
        (ProviderError::NotEnabled(PRIVATE_DETAIL.into()), -32002),
        (ProviderError::Internal(PRIVATE_DETAIL.into()), -32603),
    ] {
        let provider = Arc::new(FixtureProvider {
            error: Some(error),
            ..FixtureProvider::default()
        });
        let service = service(Some(provider), true, true);
        for (method, params) in [
            ("resources/read", json!({"uri": "scene://blender/current"})),
            (
                "prompts/get",
                json!({"name": "maya_review", "arguments": {"asset": "Oak"}}),
            ),
        ] {
            let response = call(&service, method, Some(params)).await;
            assert_error(&response, code);
            assert!(!response.to_string().contains(PRIVATE_DETAIL));
            assert!(
                response["error"]["message"]
                    .as_str()
                    .is_some_and(|message| !message.is_empty())
            );
        }
    }
}

#[tokio::test]
async fn missing_targets_and_required_prompt_arguments_are_not_successes() {
    let service = service(Some(populated_provider()), true, true);
    for (method, params) in [
        ("resources/read", json!({"uri": "scene://blender/missing"})),
        (
            "prompts/get",
            json!({"name": "missing_review", "arguments": {"asset": "Oak"}}),
        ),
        ("prompts/get", json!({"name": "maya_review"})),
        (
            "prompts/get",
            json!({"name": "maya_review", "arguments": {}}),
        ),
    ] {
        assert_error(&call(&service, method, Some(params)).await, -32602);
    }
}

#[tokio::test]
async fn empty_prompt_lists_preserve_only_object_diagnostics() {
    for diagnostics in [
        None,
        Some(json!({"status": "no_loaded_skills"})),
        Some(json!(["invalid"])),
        Some(json!("invalid")),
    ] {
        let provider = Arc::new(FixtureProvider {
            diagnostics: diagnostics.clone(),
            ..FixtureProvider::default()
        });
        let service = service(Some(provider), true, true);
        let response = call(&service, "prompts/list", None).await;
        assert_eq!(response["result"]["prompts"], json!([]));
        if diagnostics.as_ref().is_some_and(Value::is_object) {
            assert_eq!(
                response["result"]["_meta"]["dcc.prompt_diagnostics"],
                diagnostics.unwrap()
            );
        } else {
            assert!(
                response["result"]["_meta"]
                    .get("dcc.prompt_diagnostics")
                    .is_none()
            );
        }
    }
}

#[tokio::test]
async fn discover_advertises_only_wired_enabled_provider_capabilities() {
    for (resources_enabled, prompts_enabled) in
        [(true, true), (true, false), (false, true), (false, false)]
    {
        let provider = populated_provider();
        let service = service(
            Some(Arc::clone(&provider)),
            resources_enabled,
            prompts_enabled,
        );
        let response = call(&service, "server/discover", None).await;
        let capabilities = &response["result"]["capabilities"];
        assert_eq!(capabilities.get("resources").is_some(), resources_enabled);
        assert_eq!(capabilities.get("prompts").is_some(), prompts_enabled);
        if resources_enabled {
            assert_eq!(capabilities["resources"]["subscribe"], false);
            assert_eq!(capabilities["resources"]["listChanged"], false);
        }
        if prompts_enabled {
            assert_eq!(capabilities["prompts"]["listChanged"], false);
        }
        assert_eq!(capabilities["tools"]["listChanged"], false);
        assert!(capabilities.get("tasks").is_none());
        assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
    }
}

#[tokio::test]
async fn discover_does_not_advertise_absent_resource_or_prompt_providers() {
    let service = service(None, true, true);
    let response = call(&service, "server/discover", None).await;
    let capabilities = &response["result"]["capabilities"];

    assert!(capabilities.get("resources").is_none());
    assert!(capabilities.get("prompts").is_none());
    assert!(capabilities.get("tasks").is_none());
}
