//! Local/remote DCC control routing for `dcc-mcp-cli`.
//!
//! The CLI has one user-facing workflow: list/search/describe/load/call a DCC
//! instance. The built-in `local` profile uses the shared FileRegistry and the
//! instance's advertised MCP endpoint; remote profiles use gateway REST.

mod job_wait;

use std::path::PathBuf;
use std::time::Duration;

use serde_json::{Value, json};

use dcc_mcp_models::FeedbackReport;

use crate::application::client::{ClientError, DccMcpClient, TransportMode};
use crate::application::gateway_profile::GatewayTarget;
use crate::application::instance_selection::{
    InstanceSelectionError, instance_field, select_instances,
};
use crate::application::{local_control, local_registry};
use crate::domain::rest::{
    CallRequest, DescribeRequest, DirectCallRequest, Endpoint, FeedbackQueryRequest,
    LoadSkillRequest, ReloadSkillsRequest, SearchRequest, StatsRequest, StopInstanceRequest,
    WaitReadyRequest,
};
use crate::infra::http::{HttpError, HttpGateway};

pub(crate) use job_wait::JobWaitProgress;
use job_wait::*;

const RELOAD_SKILLS_TOOL: &str = "dcc_admin__reload_skills";

#[derive(Debug, Clone)]
pub struct DccControlPlane {
    target: GatewayTarget,
    endpoint: Endpoint,
    registry_dir: PathBuf,
    require_gateway: bool,
    auto_gateway_enabled: bool,
    transport: TransportMode,
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
            auto_gateway_enabled: true,
            transport: TransportMode::Auto,
        }
    }

    #[must_use]
    pub fn with_auto_gateway_enabled(mut self, enabled: bool) -> Self {
        self.auto_gateway_enabled = enabled;
        self
    }

    #[must_use]
    pub fn with_transport(mut self, transport: TransportMode) -> Self {
        self.transport = transport;
        self
    }

    fn uses_direct_local(&self) -> bool {
        self.target.is_local() && !self.require_gateway && self.transport != TransportMode::Rest
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

    /// File feedback through the gateway even when the instance inventory is empty.
    pub async fn feedback(&self, report: FeedbackReport) -> anyhow::Result<Value> {
        self.gateway_client()
            .feedback(report)
            .await
            .map_err(Into::into)
    }

    /// Query persisted feedback through the gateway-owned admin API.
    pub async fn feedback_entries(&self, request: FeedbackQueryRequest) -> anyhow::Result<Value> {
        self.gateway_client()
            .feedback_entries(request)
            .await
            .map_err(Into::into)
    }

    /// Fetch the stable public-safe issue report for one correlated request.
    pub async fn issue_report(&self, request_id: &str) -> anyhow::Result<Value> {
        self.gateway_client()
            .issue_report(request_id)
            .await
            .map_err(Into::into)
    }

    pub async fn search(&self, request: SearchRequest) -> anyhow::Result<Value> {
        if self.uses_direct_local() {
            return local_control::search_local(self.registry_dir.clone(), request).await;
        }
        if request.gateway_only.is_some() {
            // `--gateway-only` compares the local `tools/list` inventory
            // against the gateway capability index, and the gateway side
            // of that comparison is exactly what this call would have to
            // query. Filtering the gateway's own rows by it can only
            // return the full set, which is a plausible-looking wrong
            // answer for `--gateway-only=false`. Refuse instead.
            anyhow::bail!(
                "--gateway-only is a local-inventory filter and needs the direct local path; \\
                 drop --gateway / --require-gateway / --transport rest to compare against the gateway"
            );
        }
        self.gateway_client()
            .search(request)
            .await
            .map_err(Into::into)
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
        if self.uses_direct_local() && self.auto_gateway_enabled {
            let fallback_body = request.body.clone();
            match self.gateway_client().load_skill(request).await {
                Ok(value) => Ok(value),
                Err(ClientError::Http(HttpError::Request(error))) if error.is_connect() => {
                    local_control::load_skill_local(self.registry_dir.clone(), fallback_body).await
                }
                Err(error) => Err(error.into()),
            }
        } else if self.uses_direct_local() {
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
        if !direct_local
            && self.transport == TransportMode::Mcp
            && (dcc_type.is_some() || instance_id.is_some())
        {
            anyhow::bail!(
                "--transport mcp requires a gateway tool slug; direct backend calls need REST routing"
            );
        }
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
            )
            .with_transport(self.transport);
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

    #[allow(clippy::too_many_arguments)]
    pub async fn call_and_wait(
        &self,
        tool_slug: String,
        dcc_type: Option<String>,
        instance_id: Option<String>,
        arguments: Value,
        meta: Option<Value>,
        request_timeout: Duration,
        wait_timeout: Duration,
    ) -> anyhow::Result<Value> {
        self.call_and_wait_with_progress(
            tool_slug,
            dcc_type,
            instance_id,
            arguments,
            meta,
            request_timeout,
            wait_timeout,
            |_| {},
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn call_and_wait_with_progress<F>(
        &self,
        tool_slug: String,
        dcc_type: Option<String>,
        instance_id: Option<String>,
        arguments: Value,
        meta: Option<Value>,
        request_timeout: Duration,
        wait_timeout: Duration,
        mut on_progress: F,
    ) -> anyhow::Result<Value>
    where
        F: FnMut(&JobWaitProgress),
    {
        let poll_meta = job_poll_meta(meta.clone());
        let wait_declared_async = async_wait_requested(meta.as_ref());
        let mut result = self
            .call(
                tool_slug.clone(),
                dcc_type.clone(),
                instance_id.clone(),
                arguments,
                meta,
                request_timeout,
            )
            .await?;
        let started = tokio::time::Instant::now();
        let mut control_plane_disruptions = 0_u64;
        let mut last_poll_error: Option<String> = None;

        if adapter_wait_target(&result, 0).is_none() {
            let Some(initial) = job_wait_progress(&result, 0) else {
                // #2262: `--wait` must never look like it waited when the
                // launch result carries no job identity at all.
                attach_no_job_identity_wait(&mut result, wait_declared_async);
                return Ok(result);
            };
            on_progress(&initial);
            let job_id = initial.job_id.clone();
            let mut status = initial.status.clone();
            let mut last_progress = initial;
            let mut status_tool =
                job_status_tool(&tool_slug, dcc_type.as_deref(), instance_id.as_deref())?;

            while !is_terminal_job_status(&status) {
                if started.elapsed() >= wait_timeout {
                    return Ok(job_wait_timeout_result(
                        "core",
                        &job_id,
                        &status,
                        wait_timeout,
                        last_poll_error.as_deref(),
                        control_plane_disruptions,
                        result,
                    ));
                }
                tokio::time::sleep(JOB_POLL_INTERVAL).await;
                let poll_result = self
                    .call(
                        status_tool.clone(),
                        dcc_type.clone(),
                        instance_id.clone(),
                        json!({"job_id": job_id, "include_result": true}),
                        poll_meta.clone(),
                        request_timeout,
                    )
                    .await;
                let poll_result = match poll_result {
                    Err(error)
                        if status_tool != "jobs_get_status"
                            && job_status_tool_is_unknown(&error) =>
                    {
                        match self
                            .call(
                                "jobs_get_status".to_string(),
                                None,
                                None,
                                json!({"job_id": job_id, "include_result": true}),
                                poll_meta.clone(),
                                request_timeout,
                            )
                            .await
                        {
                            Ok(value) => {
                                status_tool = "jobs_get_status".to_string();
                                Ok(value)
                            }
                            Err(fallback_error) => Err(fallback_error),
                        }
                    }
                    other => other,
                };
                match poll_result {
                    Ok(value) => {
                        result = value;
                        last_poll_error = None;
                    }
                    Err(error)
                        if !self.uses_direct_local() && job_poll_error_is_retryable(&error) =>
                    {
                        control_plane_disruptions = control_plane_disruptions.saturating_add(1);
                        let outage_started = last_poll_error.is_none();
                        last_poll_error = Some(error.to_string());
                        if outage_started {
                            emit_reconnecting_progress(&mut on_progress, &last_progress, &status);
                        }
                        continue;
                    }
                    Err(error) if job_poll_owner_exited(&error) => {
                        return Ok(job_owner_exited_result(
                            "core", &job_id, &status, &error, result,
                        ));
                    }
                    Err(error) => return Err(error),
                }
                let Some(update) = job_wait_progress(&result, 0) else {
                    anyhow::bail!("jobs_get_status returned no job envelope for {job_id}");
                };
                if update.job_id != job_id {
                    return Ok(job_id_mismatch_result(
                        "core",
                        &job_id,
                        &update.job_id,
                        result,
                    ));
                }
                status = update.status.clone();
                on_progress(&update);
                last_progress = update;
            }
            annotate_wait_result_job_identity(&mut result, &job_id, &status_tool);
            if adapter_wait_target(&result, 0).is_none() {
                attach_wait_summary(&mut result, "core", &job_id, &status, true, None);
                attach_wait_recovery(&mut result, &job_id, control_plane_disruptions);
                return Ok(result);
            }
        }

        let mut adapter = adapter_wait_target(&result, 0)
            .expect("adapter target was checked before entering adapter wait");
        if is_terminal_job_status(&adapter.status) {
            on_progress(&adapter.progress());
            attach_adapter_terminal_result(&mut result, &adapter, None, 0);
            attach_wait_summary(
                &mut result,
                "adapter",
                &adapter.job_id,
                &adapter.status,
                true,
                None,
            );
            attach_wait_recovery(&mut result, &adapter.job_id, control_plane_disruptions);
            return Ok(result);
        }
        let Some(poll_tool) = adapter.poll_tool.clone() else {
            attach_wait_summary(
                &mut result,
                "adapter",
                &adapter.job_id,
                &adapter.status,
                false,
                Some(
                    adapter
                        .poll_contract_error
                        .as_deref()
                        .unwrap_or("poll_contract_missing"),
                ),
            );
            return Ok(result);
        };
        on_progress(&adapter.progress());
        let poll_arguments = adapter
            .poll_arguments
            .clone()
            .expect("validated adapter poll tool has arguments");
        let routed_poll_tool = adapter_job_status_tool(
            &tool_slug,
            dcc_type.as_deref(),
            instance_id.as_deref(),
            &poll_tool,
        )?;
        let mut last_progress = adapter.progress();
        let mut terminal_poll_result = None;

        while !is_terminal_job_status(&adapter.status) {
            if started.elapsed() >= wait_timeout {
                return Ok(job_wait_timeout_result(
                    "adapter",
                    &adapter.job_id,
                    &adapter.status,
                    wait_timeout,
                    last_poll_error.as_deref(),
                    control_plane_disruptions,
                    result,
                ));
            }
            tokio::time::sleep(JOB_POLL_INTERVAL).await;
            let poll_result = self
                .call(
                    routed_poll_tool.clone(),
                    dcc_type.clone(),
                    instance_id.clone(),
                    poll_arguments.clone(),
                    poll_meta.clone(),
                    request_timeout,
                )
                .await;
            let poll_result = match poll_result {
                Ok(value) => {
                    last_poll_error = None;
                    value
                }
                Err(error) if !self.uses_direct_local() && job_poll_error_is_retryable(&error) => {
                    control_plane_disruptions = control_plane_disruptions.saturating_add(1);
                    let outage_started = last_poll_error.is_none();
                    last_poll_error = Some(error.to_string());
                    if outage_started {
                        emit_reconnecting_progress(
                            &mut on_progress,
                            &last_progress,
                            &adapter.status,
                        );
                    }
                    continue;
                }
                Err(error) if job_poll_owner_exited(&error) => {
                    return Ok(job_owner_exited_result(
                        "adapter",
                        &adapter.job_id,
                        &adapter.status,
                        &error,
                        result,
                    ));
                }
                Err(error) => return Err(error),
            };
            let Some(update) = adapter_job_progress(&poll_result, 0) else {
                return Ok(invalid_job_poll_result(
                    "adapter",
                    &adapter.job_id,
                    "adapter status tool returned no canonical job envelope",
                    poll_result,
                    result,
                ));
            };
            if update.job_id != adapter.job_id {
                return Ok(job_id_mismatch_result(
                    "adapter",
                    &adapter.job_id,
                    &update.job_id,
                    poll_result,
                ));
            }
            adapter.status = update.status.clone();
            adapter.current = update.current;
            adapter.total = update.total;
            adapter.message.clone_from(&update.message);
            on_progress(&update);
            last_progress = update;
            terminal_poll_result = Some(poll_result);
        }

        attach_adapter_terminal_result(&mut result, &adapter, terminal_poll_result.as_ref(), 0);
        attach_wait_summary(
            &mut result,
            "adapter",
            &adapter.job_id,
            &adapter.status,
            true,
            None,
        );
        attach_wait_recovery(&mut result, &adapter.job_id, control_plane_disruptions);
        Ok(result)
    }

    pub async fn call_batch(&self, body: Value, timeout: Duration) -> anyhow::Result<Value> {
        if self.transport == TransportMode::Mcp {
            anyhow::bail!("--transport mcp does not support REST-only batch calls yet");
        }
        // Local mode owns and auto-starts the machine gateway, so batches use
        // its REST endpoint even though single calls can take the direct MCP path.
        let value =
            DccMcpClient::with_gateway(self.endpoint.clone(), HttpGateway::with_timeout(timeout))
                .with_transport(self.transport)
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
#[path = "control_plane_tests.rs"]
mod control_plane_tests;
