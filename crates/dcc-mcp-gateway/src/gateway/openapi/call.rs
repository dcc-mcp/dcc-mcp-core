//! HTTP forwarding for OpenAPI-mapped MCP tool calls.

use serde_json::Value;

use super::spec::{OpenApiMount, OperationInfo};

/// Error type for operation call failures.
#[derive(Debug, thiserror::Error)]
pub enum CallError {
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("Backend returned HTTP {status}: {body}")]
    BackendError { status: u16, body: String },
    #[error("Auth credential could not be resolved: {0}")]
    AuthResolution(String),
}

/// Forward a single MCP tool invocation to the corresponding REST endpoint.
///
/// - Path parameters are substituted into the URL template.
/// - Query parameters are appended to the URL.
/// - Remaining args (after removing path + query params) are serialised as
///   the JSON request body when `op.has_body` is `true`.
/// - Auth headers are injected from `auth` when present.
///
/// Returns the parsed JSON response body on HTTP 2xx, or a [`CallError`]
/// for transport failures and HTTP 4xx/5xx responses.
pub async fn call_operation(
    mount: &OpenApiMount,
    op: &OperationInfo,
    args: Value,
    client: &reqwest::Client,
) -> Result<Value, CallError> {
    let url = mount.resolve_url(op, &args);

    let mut builder = match op.method.as_str() {
        "GET" => client.get(&url),
        "POST" => client.post(&url),
        "PUT" => client.put(&url),
        "DELETE" => client.delete(&url),
        "PATCH" => client.patch(&url),
        "HEAD" => client.head(&url),
        other => {
            // For less common methods, fall back to a generic request.
            client.request(
                reqwest::Method::from_bytes(other.as_bytes()).unwrap_or(reqwest::Method::GET),
                &url,
            )
        }
    };

    // Auth header.
    if let Some(auth) = &mount.auth {
        let header_value = auth.header_value().ok_or_else(|| {
            CallError::AuthResolution(format!(
                "env-var '{}' referenced in auth config is not set",
                auth.value.strip_prefix('$').unwrap_or(&auth.value)
            ))
        })?;
        builder = builder.header(&auth.header, header_value);
    }

    // Request body: collect args that are not path/query params.
    if op.has_body {
        let skip: std::collections::HashSet<&str> = op
            .path_params
            .iter()
            .chain(op.query_params.iter())
            .map(String::as_str)
            .collect();

        let body: Value = if let Some(obj) = args.as_object() {
            let filtered: serde_json::Map<String, Value> = obj
                .iter()
                .filter(|(k, _)| !skip.contains(k.as_str()))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            Value::Object(filtered)
        } else {
            // Non-object args (unusual) are forwarded as-is.
            args
        };

        builder = builder
            .header("Content-Type", "application/json")
            .body(serde_json::to_string(&body).unwrap_or_default());
    }

    let response = builder.send().await?;
    let status = response.status();
    let body_text = response.text().await.unwrap_or_default();

    if status.is_success() {
        // Try to parse as JSON; fall back to a plain string envelope.
        let json: Value = serde_json::from_str(&body_text).unwrap_or(Value::String(body_text));
        Ok(json)
    } else {
        Err(CallError::BackendError {
            status: status.as_u16(),
            body: body_text,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Construct a minimal [`OpenApiMount`] backed by a mock HTTP server.
    ///
    /// We use `axum` + `tokio::net::TcpListener` to spin up a real in-process
    /// server so the tests exercise the full HTTP path (request building,
    /// header injection, response parsing) without mocking the `reqwest`
    /// internals.
    async fn start_mock_server(router: axum::Router) -> (String, tokio::task::JoinHandle<()>) {
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let base_url = format!("http://127.0.0.1:{}", addr.port());

        let handle = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });

        (base_url, handle)
    }

    fn simple_spec(base_url: &str) -> OpenApiMount {
        let spec = json!({
            "openapi": "3.0.0",
            "paths": {
                "/items": {
                    "get": {
                        "operationId": "listItems",
                        "summary": "List items",
                        "parameters": [
                            {"name": "filter", "in": "query", "schema": {"type": "string"}}
                        ]
                    },
                    "post": {
                        "operationId": "createItem",
                        "summary": "Create item",
                        "requestBody": {
                            "content": {
                                "application/json": {
                                    "schema": {
                                        "type": "object",
                                        "properties": {
                                            "name": {"type": "string"},
                                            "value": {"type": "integer"}
                                        },
                                        "required": ["name"]
                                    }
                                }
                            }
                        }
                    }
                },
                "/items/{id}": {
                    "get": {
                        "operationId": "getItem",
                        "summary": "Get item",
                        "parameters": [
                            {"name": "id", "in": "path", "required": true, "schema": {"type": "string"}}
                        ]
                    }
                }
            }
        });
        OpenApiMount::from_spec_json(spec).base_url(base_url)
    }

    #[tokio::test]
    async fn get_request_with_query_param() {
        use axum::{Json, Router, extract::Query, routing::get};
        use std::collections::HashMap;

        let router = Router::new().route(
            "/items",
            get(|Query(q): Query<HashMap<String, String>>| async move {
                Json(json!({"filter": q.get("filter").cloned().unwrap_or_default()}))
            }),
        );

        let (base_url, _srv) = start_mock_server(router).await;
        let mount = simple_spec(&base_url);
        let client = reqwest::Client::new();
        let op = mount.find_operation("listItems").unwrap();
        let result = call_operation(&mount, op, json!({"filter": "active"}), &client)
            .await
            .unwrap();

        assert_eq!(result["filter"], "active");
    }

    #[tokio::test]
    async fn post_request_with_body() {
        use axum::{Json, Router, routing::post};

        let router = Router::new().route(
            "/items",
            post(|Json(body): Json<Value>| async move { Json(json!({"echo": body})) }),
        );

        let (base_url, _srv) = start_mock_server(router).await;
        let mount = simple_spec(&base_url);
        let client = reqwest::Client::new();
        let op = mount.find_operation("createItem").unwrap();
        let result = call_operation(&mount, op, json!({"name": "foo", "value": 42}), &client)
            .await
            .unwrap();

        assert_eq!(result["echo"]["name"], "foo");
        assert_eq!(result["echo"]["value"], 42);
    }

    #[tokio::test]
    async fn get_request_with_path_param() {
        use axum::{Json, Router, extract::Path, routing::get};

        let router = Router::new().route(
            "/items/{id}",
            get(|Path(id): Path<String>| async move { Json(json!({"id": id})) }),
        );

        let (base_url, _srv) = start_mock_server(router).await;
        let mount = simple_spec(&base_url);
        let client = reqwest::Client::new();
        let op = mount.find_operation("getItem").unwrap();
        let result = call_operation(&mount, op, json!({"id": "abc-42"}), &client)
            .await
            .unwrap();

        assert_eq!(result["id"], "abc-42");
    }

    #[tokio::test]
    async fn auth_header_injected() {
        use axum::{Json, Router, extract::Request, routing::get};

        // Check Authorization header is forwarded.
        let router = Router::new().route(
            "/items",
            get(|req: Request| async move {
                let token = req
                    .headers()
                    .get("Authorization")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.strip_prefix("Bearer "))
                    .unwrap_or("")
                    .to_string();
                Json(json!({"token": token}))
            }),
        );

        let (base_url, _srv) = start_mock_server(router).await;
        let mount =
            simple_spec(&base_url).auth(super::super::auth::AuthConfig::bearer("my-secret-token"));
        let client = reqwest::Client::new();
        let op = mount.find_operation("listItems").unwrap();
        let result = call_operation(&mount, op, json!({}), &client)
            .await
            .unwrap();

        assert_eq!(result["token"], "my-secret-token");
    }

    #[tokio::test]
    async fn backend_error_returns_call_error() {
        use axum::{Router, http::StatusCode, routing::get};

        let router = Router::new().route(
            "/items",
            get(|| async { (StatusCode::NOT_FOUND, "not found") }),
        );

        let (base_url, _srv) = start_mock_server(router).await;
        let mount = simple_spec(&base_url);
        let client = reqwest::Client::new();
        let op = mount.find_operation("listItems").unwrap();
        let err = call_operation(&mount, op, json!({}), &client)
            .await
            .unwrap_err();

        match err {
            CallError::BackendError { status, .. } => assert_eq!(status, 404),
            other => panic!("unexpected error: {other}"),
        }
    }
}
