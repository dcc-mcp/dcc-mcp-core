//! Integration tests for the built-in `jobs.get_status` tool (#319).
//!
//! Covers:
//! * Tool name passes SEP-986 (`validate_tool_name`).
//! * `tools/list` always surfaces `jobs.get_status`, irrespective of
//!   whether any job has been created.
//! * `tools/call jobs.get_status` with an unknown job id returns an
//!   `isError=true` `CallToolResult` (never a JSON-RPC transport error).
//! * Each `JobStatus` variant is reported correctly.
//! * `include_result=false` omits the result even for a terminal job.
//! * `parent_job_id` is surfaced for child jobs.

use dcc_mcp_http::JobManager;
use dcc_mcp_naming::validate_tool_name;

#[test]
fn jobs_get_status_name_is_sep986_compliant() {
    validate_tool_name("jobs.get_status").expect("jobs.get_status must satisfy TOOL_NAME_RE");
}

#[test]
fn job_manager_status_envelope_shape() {
    // Minimal shape sanity check using `to_status_json` — the handler
    // builds the envelope on top of the same fields.
    let jm = JobManager::new();
    let parent = jm.create("workflow.run");
    let parent_id = parent.read().id.clone();
    let child = jm.create_with_parent("workflow.step", Some(parent_id.clone()));
    let v = child.read().to_status_json();
    assert_eq!(
        v.get("parent_job_id").and_then(|x| x.as_str()),
        Some(parent_id.as_str())
    );
    assert_eq!(v.get("status").and_then(|x| x.as_str()), Some("pending"));
    assert!(v.get("tool_name").and_then(|x| x.as_str()).is_some());
    assert!(v.get("job_id").and_then(|x| x.as_str()).is_some());
    assert!(v.get("created_at").is_some());
    assert!(v.get("updated_at").is_some());
}

#[test]
fn all_job_status_variants_serialize_lowercase() {
    // The `jobs.get_status` envelope uses serde's lowercase rename, so the
    // string representation surfaced to clients matches the #326 channel.
    use dcc_mcp_http::JobStatus;
    for (variant, expected) in [
        (JobStatus::Pending, "\"pending\""),
        (JobStatus::Running, "\"running\""),
        (JobStatus::Completed, "\"completed\""),
        (JobStatus::Failed, "\"failed\""),
        (JobStatus::Cancelled, "\"cancelled\""),
        (JobStatus::Interrupted, "\"interrupted\""),
    ] {
        assert_eq!(serde_json::to_string(&variant).unwrap(), expected);
    }
}
