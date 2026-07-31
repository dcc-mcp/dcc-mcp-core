//! Local/remote DCC control routing for `dcc-mcp-cli`.
//!
//! The CLI has one user-facing workflow: list/search/describe/load/call a DCC
//! instance. The built-in `local` profile uses the shared FileRegistry and the
//! instance's advertised MCP endpoint; remote profiles use gateway REST.

use std::path::PathBuf;
use std::time::Duration;

use serde_json::{Value, json};

use crate::application::client::{ClientError, DccMcpClient};
use crate::application::gateway_profile::GatewayTarget;
use crate::application::instance_selection::{
    InstanceSelectionError, instance_field, select_instances,
};
use crate::application::{local_control, local_registry};
use crate::domain::rest::{
    CallRequest, DescribeRequest, DirectCallRequest, Endpoint, LoadSkillRequest,
    ReloadSkillsRequest, SearchRequest, StatsRequest, StopInstanceRequest, WaitReadyRequest,
};
use crate::infra::http::{HttpError, HttpGateway};

const RELOAD_SKILLS_TOOL: &str = "dcc_admin__reload_skills";
const JOB_POLL_INTERVAL: Duration = Duration::from_secs(1);
const TERMINAL_JOB_STATUSES: &[&str] = &["completed", "failed", "cancelled", "interrupted"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct JobWaitProgress {
    pub(crate) job_id: String,
    pub(crate) status: String,
    pub(crate) current: Option<u64>,
    pub(crate) total: Option<u64>,
    pub(crate) message: Option<String>,
}

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
        let status_tool = job_status_tool(&tool_slug, dcc_type.as_deref(), instance_id.as_deref())?;
        let poll_meta = job_poll_meta(meta.clone());
        let mut result = self
            .call(
                tool_slug,
                dcc_type.clone(),
                instance_id.clone(),
                arguments,
                meta,
                request_timeout,
            )
            .await?;
        let Some(initial) = job_wait_progress(&result, 0) else {
            return Ok(result);
        };
        on_progress(&initial);
        let job_id = initial.job_id.clone();
        let mut status = initial.status.clone();
        let mut last_progress = initial;
        let mut control_plane_disruptions = 0_u64;
        let mut last_poll_error: Option<String> = None;
        if is_terminal_job_status(&status) {
            return Ok(result);
        }

        let started = tokio::time::Instant::now();
        loop {
            if started.elapsed() >= wait_timeout {
                return Ok(json!({
                    "success": false,
                    "error": format!("timed out waiting for job {job_id} after {}s", wait_timeout.as_secs()),
                    "job_id": job_id,
                    "status": status,
                    "wait_timed_out": true,
                    "tracking_status": last_poll_error.as_ref().map(|_| "control_plane_unavailable"),
                    "control_plane_disruptions": control_plane_disruptions,
                    "last_poll_error": last_poll_error,
                    "job_not_resubmitted": true,
                    "recommended_next_action": "Continue querying the same job ID later; restore the gateway first only if it is unavailable. Do not submit the operation again.",
                    "last_result": result,
                }));
            }
            tokio::time::sleep(JOB_POLL_INTERVAL).await;
            match self
                .call(
                    status_tool.clone(),
                    dcc_type.clone(),
                    instance_id.clone(),
                    json!({"job_id": job_id, "include_result": true}),
                    poll_meta.clone(),
                    request_timeout,
                )
                .await
            {
                Ok(value) => {
                    result = value;
                    last_poll_error = None;
                }
                Err(error) if !self.uses_direct_local() && job_poll_error_is_retryable(&error) => {
                    control_plane_disruptions = control_plane_disruptions.saturating_add(1);
                    let outage_started = last_poll_error.is_none();
                    last_poll_error = Some(error.to_string());
                    if outage_started {
                        let mut reconnecting = last_progress.clone();
                        reconnecting.status = "control_plane_reconnecting".to_string();
                        reconnecting.message = Some(format!(
                            "last_job_status={status}; gateway unavailable, retrying the same job"
                        ));
                        on_progress(&reconnecting);
                    }
                    continue;
                }
                Err(error) if job_poll_owner_exited(&error) => {
                    return Ok(json!({
                        "success": false,
                        "error": "job tracking owner exited; the job was not resubmitted",
                        "job_id": job_id,
                        "status": status,
                        "tracking_status": "owner_exited",
                        "control_plane_error": job_poll_error_value(&error),
                        "job_not_resubmitted": true,
                        "recommended_next_action": "Use the isolated worker's typed status tool if one was returned; otherwise restore the owning adapter before querying this job again.",
                        "last_result": result,
                    }));
                }
                Err(error) => return Err(error),
            }
            let Some(update) = job_wait_progress(&result, 0) else {
                anyhow::bail!("jobs_get_status returned no job envelope for {job_id}");
            };
            if update.job_id != job_id {
                anyhow::bail!(
                    "jobs_get_status returned job {} while waiting for {job_id}",
                    update.job_id
                );
            }
            status = update.status.clone();
            on_progress(&update);
            last_progress = update;
            if is_terminal_job_status(&status) {
                attach_wait_recovery(&mut result, &job_id, control_plane_disruptions);
                return Ok(result);
            }
        }
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

fn job_poll_http_error(error: &anyhow::Error) -> Option<&HttpError> {
    match error.downcast_ref::<ClientError>()? {
        ClientError::Http(error) => Some(error),
        ClientError::Protocol(_) => None,
    }
}

fn job_poll_error_is_retryable(error: &anyhow::Error) -> bool {
    match job_poll_http_error(error) {
        Some(HttpError::Request(error)) => {
            error.is_connect()
                || error.is_timeout()
                || error.is_request()
                || error.is_body()
                || error.is_decode()
        }
        Some(HttpError::Status { status, .. }) => matches!(
            *status,
            reqwest::StatusCode::NOT_FOUND
                | reqwest::StatusCode::TOO_MANY_REQUESTS
                | reqwest::StatusCode::BAD_GATEWAY
                | reqwest::StatusCode::SERVICE_UNAVAILABLE
                | reqwest::StatusCode::GATEWAY_TIMEOUT
        ),
        None => false,
    }
}

fn job_poll_owner_exited(error: &anyhow::Error) -> bool {
    matches!(
        job_poll_http_error(error),
        Some(HttpError::Status { status, .. }) if *status == reqwest::StatusCode::GONE
    )
}

fn job_poll_error_value(error: &anyhow::Error) -> Value {
    match job_poll_http_error(error) {
        Some(HttpError::Status { status, body }) => json!({
            "http_status": status.as_u16(),
            "body": serde_json::from_str::<Value>(body).unwrap_or_else(|_| json!(body)),
        }),
        _ => json!({"message": error.to_string()}),
    }
}

fn attach_wait_recovery(result: &mut Value, job_id: &str, disruptions: u64) {
    if disruptions == 0 {
        return;
    }
    if let Some(object) = result.as_object_mut() {
        object.insert(
            "wait_recovery".to_string(),
            json!({
                "job_id": job_id,
                "control_plane_disruptions": disruptions,
                "resumed": true,
                "job_resubmitted": false,
            }),
        );
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

fn job_status_tool(
    tool_slug: &str,
    dcc_type: Option<&str>,
    instance_id: Option<&str>,
) -> anyhow::Result<String> {
    if dcc_type.is_some() && instance_id.is_some() {
        return Ok("jobs_get_status".to_string());
    }
    let mut parts = tool_slug.splitn(3, '.');
    let dcc = parts.next().unwrap_or_default();
    let instance = parts.next().unwrap_or_default();
    if dcc.is_empty() || instance.is_empty() || parts.next().is_none() {
        anyhow::bail!("--wait requires a canonical DCC tool slug or direct instance selection");
    }
    Ok(format!("{dcc}.{instance}.jobs_get_status"))
}

fn job_wait_progress(value: &Value, depth: u8) -> Option<JobWaitProgress> {
    if depth > 4 {
        return None;
    }
    if let (Some(job_id), Some(status)) = (
        value.get("job_id").and_then(Value::as_str),
        value.get("status").and_then(Value::as_str),
    ) {
        let progress = value.get("progress");
        return Some(JobWaitProgress {
            job_id: job_id.to_string(),
            status: status.to_string(),
            current: progress
                .and_then(|progress| progress.get("current"))
                .and_then(Value::as_u64),
            total: progress
                .and_then(|progress| progress.get("total"))
                .and_then(Value::as_u64),
            message: progress
                .and_then(|progress| progress.get("message"))
                .and_then(Value::as_str)
                .map(str::to_string),
        });
    }
    [
        "output",
        "result",
        "structuredContent",
        "structured_content",
    ]
    .iter()
    .filter_map(|key| value.get(*key))
    .find_map(|nested| job_wait_progress(nested, depth + 1))
}

fn is_terminal_job_status(status: &str) -> bool {
    TERMINAL_JOB_STATUSES.contains(&status)
}

fn job_poll_meta(mut meta: Option<Value>) -> Option<Value> {
    let Some(Value::Object(root)) = meta.as_mut() else {
        return meta;
    };
    root.remove("progressToken");
    if let Some(Value::Object(dcc)) = root.get_mut("dcc") {
        dcc.remove("async");
        dcc.remove("wait_for_terminal");
        if dcc.is_empty() {
            root.remove("dcc");
        }
    }
    meta.filter(|value| value.as_object().is_some_and(|object| !object.is_empty()))
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
    use std::sync::{Arc, Mutex};

    use axum::extract::{Query, State};
    use axum::http::StatusCode;
    use axum::response::{IntoResponse, Response};
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

    #[tokio::test]
    async fn wait_for_async_call_returns_terminal_result_without_requeueing_polls() {
        async fn call(
            State(requests): State<Arc<Mutex<Vec<Value>>>>,
            Json(body): Json<Value>,
        ) -> Json<Value> {
            let slug = body["tool_slug"].as_str().unwrap_or_default().to_string();
            let poll = {
                let mut requests = requests.lock().unwrap();
                let poll = requests
                    .iter()
                    .filter(|request| {
                        request["tool_slug"]
                            .as_str()
                            .is_some_and(|tool| tool.ends_with(".jobs_get_status"))
                    })
                    .count();
                requests.push(body);
                poll
            };
            if slug.ends_with(".jobs_get_status") {
                let status = if poll == 0 { "running" } else { "completed" };
                let current = if poll == 0 { 45 } else { 90 };
                return Json(json!({
                    "structuredContent": {
                        "job_id": "job-42",
                        "status": status,
                        "progress": {
                            "current": current,
                            "total": 90,
                            "message": format!("frame {current}")
                        },
                        "result": (status == "completed").then(|| json!({
                            "success": true,
                            "message": "done"
                        }))
                    }
                }));
            }
            Json(json!({
                "slug": slug,
                "output": {"job_id": "job-42", "status": "pending"}
            }))
        }

        let requests = Arc::new(Mutex::new(Vec::new()));
        let app = Router::new()
            .route("/v1/call", post(call))
            .with_state(requests.clone());
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

        let mut progress = Vec::new();
        let result = control
            .call_and_wait_with_progress(
                "unity.abc12345.run_tests".to_string(),
                None,
                None,
                json!({}),
                Some(json!({
                    "agent_context": {"session_id": "task-42"},
                    "lease_owner": "workflow-42",
                    "dcc": {"async": true, "wait_for_terminal": true},
                    "progressToken": "progress-9"
                })),
                Duration::from_secs(2),
                Duration::from_secs(5),
                |update| progress.push(update.clone()),
            )
            .await
            .unwrap();

        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 3);
        let poll_meta = &requests[1]["meta"];
        assert_eq!(poll_meta["agent_context"]["session_id"], "task-42");
        assert_eq!(poll_meta["lease_owner"], "workflow-42");
        assert!(poll_meta["dcc"].get("async").is_none());
        assert!(poll_meta["dcc"].get("wait_for_terminal").is_none());
        assert!(poll_meta.get("progressToken").is_none());
        assert_eq!(result["structuredContent"]["status"], "completed");
        assert_eq!(result["structuredContent"]["result"]["message"], "done");
        assert_eq!(
            progress
                .iter()
                .map(|update| (update.status.as_str(), update.current, update.total))
                .collect::<Vec<_>>(),
            vec![
                ("pending", None, None),
                ("running", Some(45), Some(90)),
                ("completed", Some(90), Some(90)),
            ]
        );
        server.abort();
    }

    #[tokio::test]
    async fn wait_for_async_call_resumes_after_gateway_unavailable() {
        async fn call(State(polls): State<Arc<Mutex<u32>>>, Json(body): Json<Value>) -> Response {
            if body["tool_slug"]
                .as_str()
                .is_some_and(|slug| slug.ends_with(".jobs_get_status"))
            {
                let attempt = {
                    let mut polls = polls.lock().unwrap();
                    let attempt = *polls;
                    *polls += 1;
                    attempt
                };
                if attempt < 2 {
                    return (
                        StatusCode::SERVICE_UNAVAILABLE,
                        Json(json!({
                            "error": {
                                "kind": "instance-offline",
                                "previous_status": "unreachable",
                                "retryable": true
                            }
                        })),
                    )
                        .into_response();
                }
                return Json(json!({
                    "structuredContent": {
                        "job_id": "job-houdini-42",
                        "status": "completed",
                        "result": {"success": true}
                    }
                }))
                .into_response();
            }
            Json(json!({
                "output": {"job_id": "job-houdini-42", "status": "running"}
            }))
            .into_response()
        }

        let polls = Arc::new(Mutex::new(0));
        let app = Router::new()
            .route("/v1/call", post(call))
            .with_state(polls.clone());
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

        let mut progress = Vec::new();
        let result = control
            .call_and_wait_with_progress(
                "houdini.04fccb17.render".to_string(),
                None,
                None,
                json!({}),
                None,
                Duration::from_secs(2),
                Duration::from_secs(5),
                |update| progress.push(update.clone()),
            )
            .await
            .unwrap();

        assert_eq!(result["structuredContent"]["status"], "completed");
        assert_eq!(result["wait_recovery"]["control_plane_disruptions"], 2);
        assert_eq!(result["wait_recovery"]["resumed"], true);
        assert_eq!(result["wait_recovery"]["job_resubmitted"], false);
        assert_eq!(
            progress
                .iter()
                .filter(|update| update.status == "control_plane_reconnecting")
                .count(),
            1,
            "one outage should emit one reconnecting transition"
        );
        assert_eq!(*polls.lock().unwrap(), 3);
        server.abort();
    }

    #[tokio::test]
    async fn wait_for_async_call_reports_exited_owner_without_resubmitting() {
        async fn call(Json(body): Json<Value>) -> Response {
            if body["tool_slug"]
                .as_str()
                .is_some_and(|slug| slug.ends_with(".jobs_get_status"))
            {
                return (
                    StatusCode::GONE,
                    Json(json!({
                        "error": {
                            "kind": "instance-offline",
                            "previous_status": "exited",
                            "retryable": false,
                            "recommended_next_action": "Refresh instances and search for a replacement."
                        }
                    })),
                )
                    .into_response();
            }
            Json(json!({
                "output": {"job_id": "job-maya-42", "status": "running"}
            }))
            .into_response()
        }

        let app = Router::new().route("/v1/call", post(call));
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
            .call_and_wait(
                "maya.abcdef01.render".to_string(),
                None,
                None,
                json!({}),
                None,
                Duration::from_secs(2),
                Duration::from_secs(5),
            )
            .await
            .unwrap();

        assert_eq!(result["tracking_status"], "owner_exited");
        assert_eq!(result["job_id"], "job-maya-42");
        assert_eq!(result["job_not_resubmitted"], true);
        assert_eq!(
            result["control_plane_error"]["body"]["error"]["previous_status"],
            "exited"
        );
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
