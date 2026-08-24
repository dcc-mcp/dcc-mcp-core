//! Typed job-wait contracts shared by Core and adapter-owned polling.

use std::time::Duration;

use serde_json::{Value, json};

use dcc_mcp_models::{LinkedAdapterJob, linked_adapter_job_from_result};

use crate::application::client::ClientError;
use crate::infra::http::HttpError;

pub(super) const JOB_POLL_INTERVAL: Duration = Duration::from_secs(1);
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
pub(super) struct AdapterWaitTarget {
    pub(super) job_id: String,
    pub(super) status: String,
    pub(super) current: Option<u64>,
    pub(super) total: Option<u64>,
    pub(super) message: Option<String>,
    pub(super) poll_tool: Option<String>,
    pub(super) poll_arguments: Option<Value>,
    pub(super) poll_contract_error: Option<String>,
}

impl AdapterWaitTarget {
    pub(super) fn progress(&self) -> JobWaitProgress {
        JobWaitProgress {
            job_id: self.job_id.clone(),
            status: self.status.clone(),
            current: self.current,
            total: self.total,
            message: self.message.clone(),
        }
    }
}

fn job_poll_http_error(error: &anyhow::Error) -> Option<&HttpError> {
    match error.downcast_ref::<ClientError>()? {
        ClientError::Http(error) => Some(error),
        ClientError::Protocol(_) => None,
    }
}

pub(super) fn job_poll_error_is_retryable(error: &anyhow::Error) -> bool {
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
        Some(HttpError::MissingRequestId { .. } | HttpError::RequestIdMismatch { .. }) => false,
        None => false,
    }
}

pub(super) fn job_status_tool_is_unknown(error: &anyhow::Error) -> bool {
    matches!(
        job_poll_http_error(error),
        Some(HttpError::Status { status, body })
            if *status == reqwest::StatusCode::NOT_FOUND
                && body.contains("unknown-slug")
                && body.contains("jobs_get_status")
    )
}

pub(super) fn job_poll_owner_exited(error: &anyhow::Error) -> bool {
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

pub(super) fn emit_reconnecting_progress<F>(
    on_progress: &mut F,
    last_progress: &JobWaitProgress,
    status: &str,
) where
    F: FnMut(&JobWaitProgress),
{
    let mut reconnecting = last_progress.clone();
    reconnecting.status = "control_plane_reconnecting".to_string();
    reconnecting.message = Some(format!(
        "last_job_status={status}; gateway unavailable, retrying the same job"
    ));
    on_progress(&reconnecting);
}

pub(super) fn job_wait_timeout_result(
    owner: &str,
    job_id: &str,
    status: &str,
    wait_timeout: Duration,
    last_poll_error: Option<&str>,
    control_plane_disruptions: u64,
    last_result: Value,
) -> Value {
    json!({
        "success": false,
        "error": format!("timed out waiting for {owner} job {job_id} after {}s", wait_timeout.as_secs()),
        "job_id": job_id,
        "job_id_owner": owner,
        "status": status,
        "wait_timed_out": true,
        "tracking_status": last_poll_error.map(|_| "control_plane_unavailable"),
        "control_plane_disruptions": control_plane_disruptions,
        "last_poll_error": last_poll_error,
        "job_not_resubmitted": true,
        "recommended_next_action": "Continue querying the same job ID later; restore the gateway first only if it is unavailable. Do not submit the operation again.",
        "last_result": last_result,
    })
}

pub(super) fn job_owner_exited_result(
    owner: &str,
    job_id: &str,
    status: &str,
    error: &anyhow::Error,
    last_result: Value,
) -> Value {
    json!({
        "success": false,
        "error": "job tracking owner exited; the job was not resubmitted",
        "job_id": job_id,
        "job_id_owner": owner,
        "status": status,
        "tracking_status": "owner_exited",
        "control_plane_error": job_poll_error_value(error),
        "job_not_resubmitted": true,
        "recommended_next_action": "Use the isolated worker's typed status tool if one was returned; otherwise restore the owning adapter before querying this job again.",
        "last_result": last_result,
    })
}

pub(super) fn job_id_mismatch_result(
    owner: &str,
    expected_job_id: &str,
    actual_job_id: &str,
    poll_result: Value,
) -> Value {
    json!({
        "success": false,
        "error": format!("{owner} status tool returned job {actual_job_id} while waiting for {expected_job_id}"),
        "job_id": expected_job_id,
        "job_id_owner": owner,
        "returned_job_id": actual_job_id,
        "tracking_status": "job_id_mismatch",
        "job_not_resubmitted": true,
        "recommended_next_action": "Fix the status tool so it returns the queried job. Do not submit the original operation again.",
        "last_result": poll_result,
    })
}

pub(super) fn invalid_job_poll_result(
    owner: &str,
    job_id: &str,
    reason: &str,
    poll_result: Value,
    launch_result: Value,
) -> Value {
    json!({
        "success": false,
        "error": reason,
        "job_id": job_id,
        "job_id_owner": owner,
        "tracking_status": "invalid_poll_response",
        "job_not_resubmitted": true,
        "recommended_next_action": "Fix the typed status response to return the queried job_id and a canonical status. Do not submit the original operation again.",
        "poll_result": poll_result,
        "launch_result": launch_result,
    })
}

pub(super) fn adapter_wait_target(value: &Value, depth: u8) -> Option<AdapterWaitTarget> {
    if depth > 6 {
        return None;
    }
    if let Some(descriptor) = value.get("adapter_job").and_then(Value::as_object) {
        let job_id = descriptor.get("job_id")?.as_str()?.trim();
        if !job_id.is_empty() && descriptor.get("owner").and_then(Value::as_str) == Some("adapter")
        {
            let observed = adapter_job_progress_for_id(value, job_id, 0);
            let status = descriptor
                .get("status")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|status| !status.is_empty())
                .map(str::to_string)
                .or_else(|| observed.as_ref().map(|progress| progress.status.clone()))
                .unwrap_or_else(|| "unknown".to_string());
            let progress = descriptor.get("progress");
            let current = progress
                .and_then(|progress| progress.get("current"))
                .and_then(Value::as_u64)
                .or_else(|| observed.as_ref().and_then(|progress| progress.current));
            let total = progress
                .and_then(|progress| progress.get("total"))
                .and_then(Value::as_u64)
                .or_else(|| observed.as_ref().and_then(|progress| progress.total));
            let message = progress
                .and_then(|progress| progress.get("message"))
                .and_then(Value::as_str)
                .map(str::to_string)
                .or_else(|| {
                    observed
                        .as_ref()
                        .and_then(|progress| progress.message.clone())
                });
            let contract_registered = descriptor
                .get("poll_contract")
                .and_then(|contract| contract.get("registered"))
                .and_then(Value::as_bool)
                == Some(true);
            let (mut poll_tool, mut poll_arguments, mut poll_contract_error) =
                validate_adapter_poll(descriptor.get("poll"), job_id);
            let declared_contract_error = descriptor
                .get("poll_contract")
                .and_then(|contract| contract.get("reason"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|reason| !reason.is_empty())
                .map(str::to_string);
            if !contract_registered {
                poll_tool = None;
                poll_arguments = None;
                poll_contract_error = declared_contract_error.or_else(|| {
                    Some(
                        if descriptor.get("poll").is_some() {
                            "poll_contract_unverified"
                        } else {
                            "poll_contract_missing"
                        }
                        .to_string(),
                    )
                });
            } else if poll_tool.is_none()
                && let Some(reason) = descriptor
                    .get("poll_contract")
                    .and_then(|contract| contract.get("reason"))
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|reason| !reason.is_empty())
            {
                poll_contract_error = Some(reason.to_string());
            }
            return Some(AdapterWaitTarget {
                job_id: job_id.to_string(),
                status,
                current,
                total,
                message,
                poll_tool,
                poll_arguments,
                poll_contract_error,
            });
        }
    }
    [
        "output",
        "result",
        "structuredContent",
        "structured_content",
    ]
    .iter()
    .filter_map(|key| value.get(*key))
    .find_map(|nested| adapter_wait_target(nested, depth + 1))
}

fn validate_adapter_poll(
    poll: Option<&Value>,
    job_id: &str,
) -> (Option<String>, Option<Value>, Option<String>) {
    let Some(poll) = poll.and_then(Value::as_object) else {
        return (None, None, Some("poll_contract_missing".to_string()));
    };
    if poll.get("owner").and_then(Value::as_str) != Some("adapter") {
        return (None, None, Some("poll_owner_invalid".to_string()));
    }
    let Some(tool) = poll
        .get("tool")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|tool| !tool.is_empty())
    else {
        return (None, None, Some("poll_tool_missing".to_string()));
    };
    let Some(arguments) = poll.get("arguments").filter(|value| value.is_object()) else {
        return (None, None, Some("poll_arguments_missing".to_string()));
    };
    if arguments.get("job_id").and_then(Value::as_str) != Some(job_id) {
        return (None, None, Some("poll_job_id_mismatch".to_string()));
    }
    (Some(tool.to_string()), Some(arguments.clone()), None)
}

pub(super) fn adapter_job_progress(value: &Value, depth: u8) -> Option<JobWaitProgress> {
    if depth > 6 {
        return None;
    }
    for (job_path, status_path, progress_path) in [
        (
            "/adapter_job/job_id",
            "/adapter_job/status",
            "/adapter_job/progress",
        ),
        (
            "/context/adapter_job_id",
            "/context/status",
            "/context/progress",
        ),
        ("/context/job_id", "/context/status", "/context/progress"),
        ("/job_id", "/status", "/progress"),
    ] {
        let Some(job_id) = value.pointer(job_path).and_then(Value::as_str) else {
            continue;
        };
        let Some(status) = value.pointer(status_path).and_then(Value::as_str) else {
            continue;
        };
        if !is_known_job_status(status) {
            continue;
        }
        let progress = value.pointer(progress_path);
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
    .find_map(|nested| adapter_job_progress(nested, depth + 1))
}

fn adapter_job_progress_for_id(
    value: &Value,
    expected_job_id: &str,
    depth: u8,
) -> Option<JobWaitProgress> {
    if depth > 6 {
        return None;
    }
    if let Some(progress) = adapter_job_progress(value, depth)
        && progress.job_id == expected_job_id
    {
        return Some(progress);
    }
    [
        "output",
        "result",
        "structuredContent",
        "structured_content",
        "context",
    ]
    .iter()
    .filter_map(|key| value.get(*key))
    .find_map(|nested| adapter_job_progress_for_id(nested, expected_job_id, depth + 1))
}

pub(super) fn adapter_job_status_tool(
    launch_tool_slug: &str,
    dcc_type: Option<&str>,
    instance_id: Option<&str>,
    poll_tool: &str,
) -> anyhow::Result<String> {
    if dcc_type.is_some() && instance_id.is_some() {
        if poll_tool.contains('.') {
            anyhow::bail!("adapter poll tool must be a backend tool for direct instance calls");
        }
        return Ok(poll_tool.to_string());
    }
    let mut parts = launch_tool_slug.splitn(3, '.');
    let dcc = parts.next().unwrap_or_default();
    let instance = parts.next().unwrap_or_default();
    if dcc.is_empty() || instance.is_empty() || parts.next().is_none() {
        anyhow::bail!("--wait requires a canonical DCC tool slug or direct instance selection");
    }
    let prefix = format!("{dcc}.{instance}.");
    if poll_tool.starts_with(&prefix) {
        return Ok(poll_tool.to_string());
    }
    if poll_tool.contains('.') {
        anyhow::bail!("adapter poll tool must stay on the launch instance route");
    }
    Ok(format!("{prefix}{poll_tool}"))
}

pub(super) fn attach_adapter_terminal_result(
    value: &mut Value,
    adapter: &AdapterWaitTarget,
    terminal_result: Option<&Value>,
    depth: u8,
) -> bool {
    if depth > 6 {
        return false;
    }
    if let Some(descriptor) = value.get_mut("adapter_job").and_then(Value::as_object_mut)
        && descriptor.get("job_id").and_then(Value::as_str) == Some(adapter.job_id.as_str())
    {
        descriptor.insert("status".to_string(), Value::String(adapter.status.clone()));
        descriptor.insert("terminal".to_string(), Value::Bool(true));
        if let Some(result) = terminal_result {
            descriptor.insert("terminal_result".to_string(), result.clone());
        }
        return true;
    }
    [
        "output",
        "result",
        "structuredContent",
        "structured_content",
    ]
    .iter()
    .any(|key| {
        value.get_mut(*key).is_some_and(|nested| {
            attach_adapter_terminal_result(nested, adapter, terminal_result, depth + 1)
        })
    })
}

pub(super) fn attach_wait_summary(
    value: &mut Value,
    owner: &str,
    job_id: &str,
    status: &str,
    terminal: bool,
    tracking_status: Option<&str>,
) {
    let Some(object) = value.as_object_mut() else {
        return;
    };
    let completed = terminal && status == "completed";
    object.insert(
        "wait".to_string(),
        json!({
            "terminal": terminal,
            "completed": completed,
            "owner": owner,
            "job_id": job_id,
            "status": status,
            "tracking_status": tracking_status,
            "job_resubmitted": false,
        }),
    );
    if !completed {
        object.insert("success".to_string(), Value::Bool(false));
        object.entry("error".to_string()).or_insert_with(|| {
            Value::String(if terminal {
                format!("{owner} job {job_id} reached terminal status {status}")
            } else {
                format!("--wait could not establish a terminal state for {owner} job {job_id}")
            })
        });
    }
}

pub(super) fn is_known_job_status(status: &str) -> bool {
    matches!(
        status,
        "pending" | "running" | "completed" | "failed" | "cancelled" | "interrupted"
    )
}

pub(super) fn attach_wait_recovery(result: &mut Value, job_id: &str, disruptions: u64) {
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

pub(super) fn annotate_wait_result_job_identity(
    result: &mut Value,
    core_job_id: &str,
    status_tool: &str,
) {
    let adapter_job = find_terminal_adapter_job(result, core_job_id, 0);
    annotate_core_job_envelope(result, core_job_id, status_tool, adapter_job.as_ref(), 0);
}

fn find_terminal_adapter_job(
    value: &Value,
    core_job_id: &str,
    depth: u8,
) -> Option<LinkedAdapterJob> {
    if depth > 4 {
        return None;
    }
    if value.get("job_id").and_then(Value::as_str) == Some(core_job_id)
        && value
            .get("status")
            .and_then(Value::as_str)
            .is_some_and(is_terminal_job_status)
        && let Some(result) = value.get("result")
    {
        return linked_adapter_job_from_result(result, core_job_id);
    }
    [
        "output",
        "result",
        "structuredContent",
        "structured_content",
    ]
    .iter()
    .filter_map(|key| value.get(*key))
    .find_map(|nested| find_terminal_adapter_job(nested, core_job_id, depth + 1))
}

fn annotate_core_job_envelope(
    value: &mut Value,
    core_job_id: &str,
    status_tool: &str,
    adapter_job: Option<&LinkedAdapterJob>,
    depth: u8,
) -> bool {
    if depth > 4 {
        return false;
    }
    if value.get("job_id").and_then(Value::as_str) == Some(core_job_id)
        && value.get("status").and_then(Value::as_str).is_some()
    {
        let Some(object) = value.as_object_mut() else {
            return false;
        };
        object
            .entry("core_job_id")
            .or_insert_with(|| Value::String(core_job_id.to_string()));
        object
            .entry("job_id_owner")
            .or_insert_with(|| Value::String("core".to_string()));
        object.entry("core_poll").or_insert_with(|| {
            json!({
                "owner": "core",
                "tool": status_tool,
                "arguments": {"job_id": core_job_id, "include_result": true},
            })
        });
        if let Some(adapter_job) = adapter_job {
            object
                .entry("adapter_job_id")
                .or_insert_with(|| Value::String(adapter_job.job_id.clone()));
            object.entry("adapter_job").or_insert_with(|| {
                json!({
                    "job_id": adapter_job.job_id,
                    "owner": "adapter",
                    "identity_source": adapter_job.source,
                    "core_job_id": core_job_id,
                    "cancellation": {
                        "owner": "adapter",
                        "inherits_core_cancellation": false,
                    },
                    "hint": "Discover the adapter's typed status tool and pass adapter_job_id; do not pass this id to jobs_get_status.",
                })
            });
        }
        return true;
    }
    [
        "output",
        "result",
        "structuredContent",
        "structured_content",
    ]
    .iter()
    .any(|key| {
        value.get_mut(*key).is_some_and(|nested| {
            annotate_core_job_envelope(nested, core_job_id, status_tool, adapter_job, depth + 1)
        })
    })
}

pub(super) fn job_status_tool(
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

pub(super) fn job_wait_progress(value: &Value, depth: u8) -> Option<JobWaitProgress> {
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

pub(super) fn is_terminal_job_status(status: &str) -> bool {
    TERMINAL_JOB_STATUSES.contains(&status)
}

pub(super) fn job_poll_meta(mut meta: Option<Value>) -> Option<Value> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adapter_wait_rejects_unverified_poll_descriptors() {
        let result = json!({
            "adapter_job": {
                "job_id": "render-job-42",
                "owner": "adapter",
                "status": "running",
                "poll": {
                    "owner": "adapter",
                    "tool": "render__get_job",
                    "arguments": {"job_id": "render-job-42"},
                },
            },
        });

        let target = adapter_wait_target(&result, 0).expect("adapter job target");

        assert_eq!(target.poll_tool, None);
        assert_eq!(target.poll_arguments, None);
        assert_eq!(
            target.poll_contract_error.as_deref(),
            Some("poll_contract_unverified")
        );
    }

    #[test]
    fn terminal_adapter_failure_is_not_a_successful_wait() {
        let mut result = json!({"success": true});

        attach_wait_summary(
            &mut result,
            "adapter",
            "render-job-42",
            "failed",
            true,
            None,
        );

        assert_eq!(result["success"], false);
        assert_eq!(result["wait"]["terminal"], true);
        assert_eq!(result["wait"]["completed"], false);
        assert!(
            result["error"]
                .as_str()
                .is_some_and(|error| error.contains("terminal status failed"))
        );
    }
}
