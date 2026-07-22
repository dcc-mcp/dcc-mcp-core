//! Local/remote DCC control routing for `dcc-mcp-cli`.
//!
//! The CLI has one user-facing workflow: list/search/describe/load/call a DCC
//! instance. The built-in `local` profile uses the shared FileRegistry and the
//! instance's advertised MCP endpoint; remote profiles use gateway REST.

use std::path::PathBuf;
use std::time::Duration;

use serde_json::{Value, json};

use crate::application::client::DccMcpClient;
use crate::application::gateway_profile::GatewayTarget;
use crate::application::instance_selection::{
    InstanceSelectionError, instance_field, select_instances,
};
use crate::application::{local_control, local_registry};
use crate::domain::rest::{
    CallRequest, DescribeRequest, DirectCallRequest, Endpoint, LoadSkillRequest,
    ReloadSkillsRequest, SearchRequest, StatsRequest, StopInstanceRequest, WaitReadyRequest,
};
use crate::infra::http::HttpGateway;

const RELOAD_SKILLS_TOOL: &str = "dcc_admin__reload_skills";

#[derive(Debug, Clone)]
pub struct DccControlPlane {
    target: GatewayTarget,
    endpoint: Endpoint,
    registry_dir: PathBuf,
    require_gateway: bool,
}

impl DccControlPlane {
    #[must_use]
    pub fn new(
        target: GatewayTarget,
        endpoint: Endpoint,
        registry_dir: PathBuf,
        require_gateway: bool,
    ) -> Self {
        Self {
            target,
            endpoint,
            registry_dir,
            require_gateway,
        }
    }

    fn uses_direct_local(&self) -> bool {
        self.target.is_local() && !self.require_gateway
    }

    pub async fn list_instances(&self) -> anyhow::Result<Value> {
        if self.uses_direct_local() {
            local_registry::list_local_instances(self.registry_dir.clone())
        } else {
            self.gateway_client()
                .list_instances()
                .await
                .map_err(Into::into)
        }
    }

    pub async fn stats(&self, request: StatsRequest) -> anyhow::Result<Value> {
        let value = DccMcpClient::new(self.endpoint.clone())
            .stats(request)
            .await
            .map_err(anyhow::Error::from)?;
        Ok(attach_stats_coverage(value, self.uses_direct_local()))
    }

    pub async fn search(&self, request: SearchRequest) -> anyhow::Result<Value> {
        if self.uses_direct_local() {
            local_control::search_local(self.registry_dir.clone(), request).await
        } else {
            self.gateway_client()
                .search(request)
                .await
                .map_err(Into::into)
        }
    }

    pub async fn describe(&self, tool_slug: String) -> anyhow::Result<Value> {
        if self.uses_direct_local() {
            local_control::describe_local(self.registry_dir.clone(), tool_slug).await
        } else {
            self.gateway_client()
                .describe(DescribeRequest { tool_slug })
                .await
                .map_err(Into::into)
        }
    }

    pub async fn load_skill(&self, request: LoadSkillRequest) -> anyhow::Result<Value> {
        if self.uses_direct_local() {
            local_control::load_skill_local(self.registry_dir.clone(), request.body).await
        } else {
            self.gateway_client()
                .load_skill(request)
                .await
                .map_err(Into::into)
        }
    }

    pub async fn call(
        &self,
        tool_slug: String,
        dcc_type: Option<String>,
        instance_id: Option<String>,
        arguments: Value,
        meta: Option<Value>,
        timeout: Duration,
    ) -> anyhow::Result<Value> {
        let direct_local = self.uses_direct_local();
        let value = if direct_local {
            local_control::call_local(
                self.registry_dir.clone(),
                tool_slug,
                dcc_type,
                instance_id,
                arguments,
                meta,
                timeout,
            )
            .await?
        } else {
            let client = DccMcpClient::with_gateway(
                self.endpoint.clone(),
                HttpGateway::with_timeout(timeout),
            );
            match (dcc_type, instance_id) {
                (Some(dcc_type), Some(instance_id)) => client
                    .direct_call(DirectCallRequest {
                        dcc_type,
                        instance_id,
                        backend_tool: tool_slug,
                        arguments,
                        meta,
                    })
                    .await
                    .map_err(anyhow::Error::from)?,
                (None, None) => client
                    .call(CallRequest {
                        tool_slug,
                        arguments,
                        meta,
                    })
                    .await
                    .map_err(anyhow::Error::from)?,
                _ => anyhow::bail!(
                    "call requires both --dcc-type and --instance-id for direct backend-tool calls"
                ),
            }
        };
        Ok(attach_call_route(value, direct_local))
    }

    pub async fn call_batch(&self, body: Value, timeout: Duration) -> anyhow::Result<Value> {
        // Local mode owns and auto-starts the machine gateway, so batches use
        // its REST endpoint even though single calls can take the direct MCP path.
        let value =
            DccMcpClient::with_gateway(self.endpoint.clone(), HttpGateway::with_timeout(timeout))
                .call_batch(body)
                .await
                .map_err(anyhow::Error::from)?;
        Ok(attach_call_route(value, false))
    }

    pub async fn wait_ready(&self, request: WaitReadyRequest) -> anyhow::Result<Value> {
        if self.uses_direct_local() {
            local_control::wait_ready_local(self.registry_dir.clone(), request).await
        } else {
            self.gateway_client()
                .wait_ready(request)
                .await
                .map_err(Into::into)
        }
    }

    pub async fn reload_skills(&self, request: ReloadSkillsRequest) -> anyhow::Result<Value> {
        if self.uses_direct_local() {
            local_control::reload_skills_local(self.registry_dir.clone(), request).await
        } else {
            self.reload_skills_remote(request).await
        }
    }

    pub async fn stop_instance(&self, request: StopInstanceRequest) -> anyhow::Result<Value> {
        if self.uses_direct_local() {
            local_control::stop_instance_local(self.registry_dir.clone(), request).await
        } else {
            self.gateway_client()
                .stop_instance(request)
                .await
                .map_err(Into::into)
        }
    }

    async fn reload_skills_remote(&self, request: ReloadSkillsRequest) -> anyhow::Result<Value> {
        let client = self.gateway_client();
        let inventory = client.list_instances().await?;
        let targets = select_remote_instances(
            &inventory,
            request.dcc_type.as_deref(),
            request.instance_id.as_deref(),
        )?;
        let mut results = Vec::new();

        for instance in targets {
            let dcc_type = instance_field(&instance, "dcc_type")
                .or_else(|| instance_field(&instance, "dcc"))
                .ok_or_else(|| anyhow::anyhow!("gateway instance row is missing dcc_type"))?
                .to_string();
            let instance_id = instance_field(&instance, "instance_id")
                .ok_or_else(|| anyhow::anyhow!("gateway instance row is missing instance_id"))?
                .to_string();
            let result = client
                .direct_call(DirectCallRequest {
                    dcc_type: dcc_type.clone(),
                    instance_id: instance_id.clone(),
                    backend_tool: RELOAD_SKILLS_TOOL.to_string(),
                    arguments: json!({}),
                    meta: None,
                })
                .await?;
            results.push(json!({
                "dcc_type": dcc_type,
                "instance_id": instance_id,
                "instance_short": instance.get("instance_short").cloned().unwrap_or(Value::Null),
                "backend_tool": RELOAD_SKILLS_TOOL,
                "result": result,
                "source": "gateway",
            }));
        }

        let reloaded = results.iter().all(local_control::reload_result_succeeded);

        Ok(json!({
            "ok": reloaded,
            "reloaded": reloaded,
            "count": results.len(),
            "results": results,
            "source": "gateway",
        }))
    }

    fn gateway_client(&self) -> DccMcpClient {
        DccMcpClient::new(self.endpoint.clone())
    }
}

fn attach_call_route(mut value: Value, direct_local: bool) -> Value {
    if let Some(object) = value.as_object_mut() {
        object.insert(
            "control_route".to_string(),
            json!(if direct_local {
                "local_mcp_direct"
            } else {
                "gateway"
            }),
        );
        object.insert("gateway_stats_recorded".to_string(), json!(!direct_local));
        if direct_local {
            object.insert(
                "gateway_stats_hint".to_string(),
                json!(
                    "Use --require-gateway and _meta.agent_context.session_id for attributable gateway stats."
                ),
            );
        }
    }
    value
}

fn attach_stats_coverage(mut value: Value, direct_local: bool) -> Value {
    if let Some(object) = value.as_object_mut() {
        object.insert(
            "stats_coverage".to_string(),
            json!({
                "source": "gateway_admin_sqlite",
                "configured_call_route": if direct_local { "local_mcp_direct" } else { "gateway" },
                "configured_route_recorded": !direct_local,
                "excluded_control_routes": ["local_mcp_direct"],
                "session_id_meta_path": "_meta.agent_context.session_id",
                "hint": "Use --require-gateway for every task call when gateway stats are required evidence.",
            }),
        );
    }
    value
}

fn select_remote_instances(
    inventory: &Value,
    dcc_type: Option<&str>,
    instance_hint: Option<&str>,
) -> anyhow::Result<Vec<Value>> {
    let matches = select_instances(inventory, dcc_type, instance_hint)?;
    if matches.is_empty() {
        anyhow::bail!("no remote DCC instance matched the request");
    }
    if instance_hint
        .map(str::trim)
        .is_some_and(|value| !value.is_empty())
        && matches.len() > 1
    {
        return Err(InstanceSelectionError::Ambiguous {
            candidates: matches,
        }
        .into());
    }
    Ok(matches)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use axum::extract::Query;
    use axum::routing::{get, post};
    use axum::{Json, Router};
    use tempfile::tempdir;

    use super::*;

    #[tokio::test]
    async fn required_gateway_routes_a_local_call_and_reports_stats_coverage() {
        async fn call(Json(body): Json<Value>) -> Json<Value> {
            Json(json!({"success": true, "request": body}))
        }

        async fn stats(Query(query): Query<HashMap<String, String>>) -> Json<Value> {
            Json(json!({"total_calls": 1, "query": query}))
        }

        let app = Router::new()
            .route("/v1/call", post(call))
            .route("/v1/debug/stats", get(stats));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let registry = tempdir().unwrap();
        let control = DccControlPlane::new(
            GatewayTarget::Local,
            Endpoint::new(format!("http://{addr}")),
            registry.path().to_path_buf(),
            true,
        );

        let result = control
            .call(
                "maya.abc12345.inspect".to_string(),
                None,
                None,
                json!({"detail": true}),
                Some(json!({"agent_context": {"session_id": "task-42"}})),
                Duration::from_secs(2),
            )
            .await
            .unwrap();

        assert_eq!(result["control_route"], "gateway");
        assert_eq!(result["gateway_stats_recorded"], true);
        assert_eq!(
            result["request"]["meta"]["agent_context"]["session_id"],
            "task-42"
        );

        let stats = control
            .stats(StatsRequest {
                range: "24h".to_string(),
                session_id: Some("task-42".to_string()),
                ..StatsRequest::default()
            })
            .await
            .unwrap();
        assert_eq!(stats["stats_coverage"]["configured_call_route"], "gateway");
        assert_eq!(stats["stats_coverage"]["configured_route_recorded"], true);
        assert_eq!(stats["query"]["session_id"], "task-42");

        server.abort();
    }

    #[test]
    fn direct_local_results_disclose_that_gateway_stats_exclude_them() {
        let call = attach_call_route(json!({"success": true}), true);
        assert_eq!(call["control_route"], "local_mcp_direct");
        assert_eq!(call["gateway_stats_recorded"], false);
        assert!(
            call["gateway_stats_hint"]
                .as_str()
                .unwrap()
                .contains("--require-gateway")
        );

        let stats = attach_stats_coverage(json!({"total_calls": 0}), true);
        assert_eq!(
            stats["stats_coverage"]["configured_call_route"],
            "local_mcp_direct"
        );
        assert_eq!(stats["stats_coverage"]["configured_route_recorded"], false);
        assert_eq!(
            stats["stats_coverage"]["excluded_control_routes"][0],
            "local_mcp_direct"
        );
    }
}
