//! Local/remote DCC control routing for `dcc-mcp-cli`.
//!
//! The CLI has one user-facing workflow: list/search/describe/load/call a DCC
//! instance. The built-in `local` profile uses the shared FileRegistry and the
//! instance's advertised MCP endpoint; remote profiles use gateway REST.

use std::path::PathBuf;

use serde_json::{Value, json};

use crate::application::client::DccMcpClient;
use crate::application::gateway_profile::GatewayTarget;
use crate::application::instance_selection::{
    InstanceSelectionError, instance_field, select_instances,
};
use crate::application::{local_control, local_registry};
use crate::domain::rest::{
    CallRequest, DescribeRequest, DirectCallRequest, Endpoint, LoadSkillRequest,
    ReloadSkillsRequest, SearchRequest, StopInstanceRequest, WaitReadyRequest,
};

const RELOAD_SKILLS_TOOL: &str = "dcc_admin__reload_skills";

#[derive(Debug, Clone)]
pub struct DccControlPlane {
    target: GatewayTarget,
    endpoint: Endpoint,
    registry_dir: PathBuf,
}

impl DccControlPlane {
    #[must_use]
    pub fn new(target: GatewayTarget, endpoint: Endpoint, registry_dir: PathBuf) -> Self {
        Self {
            target,
            endpoint,
            registry_dir,
        }
    }

    pub async fn list_instances(&self) -> anyhow::Result<Value> {
        if self.target.is_local() {
            local_registry::list_local_instances(self.registry_dir.clone())
        } else {
            self.gateway_client()
                .list_instances()
                .await
                .map_err(Into::into)
        }
    }

    pub async fn search(&self, request: SearchRequest) -> anyhow::Result<Value> {
        if self.target.is_local() {
            local_control::search_local(self.registry_dir.clone(), request).await
        } else {
            self.gateway_client()
                .search(request)
                .await
                .map_err(Into::into)
        }
    }

    pub async fn describe(&self, tool_slug: String) -> anyhow::Result<Value> {
        if self.target.is_local() {
            local_control::describe_local(self.registry_dir.clone(), tool_slug).await
        } else {
            self.gateway_client()
                .describe(DescribeRequest { tool_slug })
                .await
                .map_err(Into::into)
        }
    }

    pub async fn load_skill(&self, request: LoadSkillRequest) -> anyhow::Result<Value> {
        if self.target.is_local() {
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
    ) -> anyhow::Result<Value> {
        if self.target.is_local() {
            return local_control::call_local(
                self.registry_dir.clone(),
                tool_slug,
                dcc_type,
                instance_id,
                arguments,
                meta,
            )
            .await;
        }

        let client = self.gateway_client();
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
                .map_err(Into::into),
            (None, None) => client
                .call(CallRequest {
                    tool_slug,
                    arguments,
                    meta,
                })
                .await
                .map_err(Into::into),
            _ => anyhow::bail!(
                "call requires both --dcc-type and --instance-id for direct backend-tool calls"
            ),
        }
    }

    pub async fn wait_ready(&self, request: WaitReadyRequest) -> anyhow::Result<Value> {
        if self.target.is_local() {
            local_control::wait_ready_local(self.registry_dir.clone(), request).await
        } else {
            self.gateway_client()
                .wait_ready(request)
                .await
                .map_err(Into::into)
        }
    }

    pub async fn reload_skills(&self, request: ReloadSkillsRequest) -> anyhow::Result<Value> {
        if self.target.is_local() {
            local_control::reload_skills_local(self.registry_dir.clone(), request).await
        } else {
            self.reload_skills_remote(request).await
        }
    }

    pub async fn stop_instance(&self, request: StopInstanceRequest) -> anyhow::Result<Value> {
        if self.target.is_local() {
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

        Ok(json!({
            "ok": true,
            "reloaded": true,
            "count": results.len(),
            "results": results,
            "source": "gateway",
        }))
    }

    fn gateway_client(&self) -> DccMcpClient {
        DccMcpClient::new(self.endpoint.clone())
    }
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
