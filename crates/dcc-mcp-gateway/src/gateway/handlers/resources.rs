use super::*;

pub(super) async fn handle_resources_list(gs: &GatewayState, id: Value) -> Value {
    let result = aggregator::aggregate_resources_list(gs).await;
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

pub(super) async fn handle_resources_read(
    gs: &GatewayState,
    id: Value,
    req: &super::mcp_impl::JsonRpcRequest,
) -> Value {
    let uri = req
        .params
        .as_ref()
        .and_then(|params| params.get("uri"))
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_owned();

    if let Some((id8, backend_uri)) = crate::gateway::namespace::decode_resource_uri(&uri) {
        let owning = aggregator::find_instance_by_prefix(gs, &id8).await;
        return match owning {
            Some(entry) => {
                let url = format!("http://{}:{}/mcp", entry.host, entry.port);
                match crate::gateway::backend_client::read_resource(
                    &gs.http_client,
                    &url,
                    &backend_uri,
                    gs.backend_timeout,
                )
                .await
                {
                    Ok(mut result) => {
                        rewrite_content_uris(&mut result, &backend_uri, &uri);
                        json!({"jsonrpc": "2.0", "id": id, "result": result})
                    }
                    Err(e) => json!({
                        "jsonrpc": "2.0", "id": id,
                        "error": {"code": -32002, "message": format!("Backend resources/read failed: {e}")}
                    }),
                }
            }
            None => json!({
                "jsonrpc": "2.0", "id": id,
                "error": {"code": -32002, "message": format!("Resource not found: {uri} (no live instance matches prefix '{id8}')")}
            }),
        };
    }

    let parts: Vec<&str> = uri.trim_start_matches("dcc://").splitn(2, '/').collect();
    let registry = gs.registry.read().await;
    let found = gs.live_instances(&registry).into_iter().find(|entry| {
        parts.len() == 2
            && entry.dcc_type == parts[0]
            && entry.instance_id.to_string().starts_with(parts[1])
    });

    match found {
        Some(entry) => {
            let detail = entry_to_json(&entry, gs.stale_timeout);
            json!({
                "jsonrpc": "2.0", "id": id,
                "result": {
                    "contents": [{
                        "uri":      uri,
                        "mimeType": "application/json",
                        "text":     serde_json::to_string_pretty(&detail).unwrap_or_default()
                    }]
                }
            })
        }
        None => json!({
            "jsonrpc": "2.0", "id": id,
            "error": {"code": -32002, "message": format!("Resource not found: {uri}")}
        }),
    }
}

fn rewrite_content_uris(result: &mut Value, backend_uri: &str, client_uri: &str) {
    let Some(contents) = result.get_mut("contents").and_then(Value::as_array_mut) else {
        return;
    };
    for entry in contents {
        if let Some(obj) = entry.as_object_mut()
            && obj
                .get("uri")
                .and_then(Value::as_str)
                .is_some_and(|u| u == backend_uri)
        {
            obj.insert("uri".to_string(), Value::String(client_uri.to_string()));
        }
    }
}

pub(super) async fn handle_resource_subscription(
    gs: &GatewayState,
    id: Value,
    req: &super::mcp_impl::JsonRpcRequest,
    session_id: &str,
    subscribe: bool,
) -> Value {
    let uri = req
        .params
        .as_ref()
        .and_then(|params| params.get("uri"))
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_owned();

    {
        let mut subscriptions = gs.resource_subscriptions.write().await;
        if subscribe {
            subscriptions
                .entry(session_id.to_owned())
                .or_default()
                .insert(uri.clone());
        } else if let Some(set) = subscriptions.get_mut(session_id) {
            set.remove(&uri);
        }
    }

    if let Some((id8, backend_uri)) = crate::gateway::namespace::decode_resource_uri(&uri) {
        let owning = aggregator::find_instance_by_prefix(gs, &id8).await;
        return match owning {
            Some(entry) => {
                let backend_url = format!("http://{}:{}/mcp", entry.host, entry.port);
                if subscribe {
                    gs.subscriber.bind_resource_subscription(
                        &backend_url,
                        &backend_uri,
                        session_id,
                        &uri,
                    );
                    gs.subscriber.ensure_subscribed(&backend_url);
                } else {
                    gs.subscriber.unbind_resource_subscription(
                        &backend_url,
                        &backend_uri,
                        session_id,
                        &uri,
                    );
                }

                let Some(backend_session_id) = gs
                    .subscriber
                    .wait_for_backend_session_id(&backend_url, std::time::Duration::from_secs(3))
                    .await
                else {
                    if subscribe {
                        gs.subscriber.unbind_resource_subscription(
                            &backend_url,
                            &backend_uri,
                            session_id,
                            &uri,
                        );
                    }
                    return json!({
                        "jsonrpc": "2.0", "id": id,
                        "error": {"code": -32002, "message": format!("Backend {backend_url} SSE subscriber not yet ready; retry")}
                    });
                };

                match crate::gateway::backend_client::subscribe_resource(
                    &gs.http_client,
                    &backend_url,
                    &backend_uri,
                    subscribe,
                    &backend_session_id,
                    gs.backend_timeout,
                )
                .await
                {
                    Ok(_) => json!({"jsonrpc": "2.0", "id": id, "result": {}}),
                    Err(e) => {
                        if subscribe {
                            gs.subscriber.unbind_resource_subscription(
                                &backend_url,
                                &backend_uri,
                                session_id,
                                &uri,
                            );
                        }
                        json!({
                            "jsonrpc": "2.0", "id": id,
                            "error": {"code": -32002, "message": format!("Backend resources/{}: {e}", if subscribe { "subscribe" } else { "unsubscribe" })}
                        })
                    }
                }
            }
            None => json!({
                "jsonrpc": "2.0", "id": id,
                "error": {"code": -32002, "message": format!("Resource not found: {uri} (no live instance matches prefix '{id8}')")}
            }),
        };
    }

    json!({"jsonrpc":"2.0","id":id,"result":{}})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rewrites_matching_content_uris_only() {
        let mut result = json!({
            "contents": [
                {"uri": "file://scene.ma", "text": "a"},
                {"uri": "file://other.ma", "text": "b"},
                {"text": "c"}
            ]
        });

        rewrite_content_uris(&mut result, "file://scene.ma", "file://abcd1234/scene.ma");

        assert_eq!(result["contents"][0]["uri"], "file://abcd1234/scene.ma");
        assert_eq!(result["contents"][1]["uri"], "file://other.ma");
        assert!(result["contents"][2].get("uri").is_none());
    }
}
