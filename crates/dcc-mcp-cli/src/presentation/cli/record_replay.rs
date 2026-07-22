use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Context;
use clap::Subcommand;
use serde_json::{Value, json};

use crate::application::call_attribution::attach_agent_session_id;
use crate::application::control_plane::DccControlPlane;
use crate::domain::rest::Endpoint;
use crate::infra::http::HttpGateway;

use super::image_artifacts::{default_image_artifact_root, materialize_call_images};

#[derive(Debug, Subcommand)]
pub(super) enum RecordReplayAction {
    /// Start capturing redacted gateway calls for --agent-session-id.
    Start {
        #[arg(long)]
        dcc_type: String,
        #[arg(long)]
        instance_id: Option<String>,
    },
    /// Stop one recording owned by --agent-session-id.
    Stop { recording_id: String },
    /// Review a recording without mutating it.
    Review { recording_id: String },
    /// Compile a stopped, reviewed recording into a local Skill package.
    Compile {
        recording_id: String,
        #[arg(long)]
        name: String,
        #[arg(long, default_value = "")]
        description: String,
        #[arg(long, default_value = "{}")]
        inputs_json: String,
        #[arg(long, default_value = "generated-skills")]
        output_dir: PathBuf,
        /// Confirm that the recording timeline was reviewed; does not grant replay.
        #[arg(long)]
        reviewed: bool,
    },
    /// Execute a generated WorkflowSpec through an explicitly selected current tool.
    Replay {
        #[arg(long)]
        workflow_file: PathBuf,
        /// Schema guards emitted by compile; defaults beside the workflow file.
        #[arg(long)]
        guard_file: Option<PathBuf>,
        #[arg(long)]
        tool_slug: String,
        #[arg(long, default_value = "{}")]
        inputs_json: String,
        /// Grant this replay attempt; recorded approvals are never reused.
        #[arg(long)]
        approve_replay: bool,
        #[arg(long, default_value = "30")]
        timeout_secs: u64,
    },
}

pub(super) struct RecordReplayResult {
    pub(super) value: Value,
    pub(super) failed: bool,
}

pub(super) async fn run_record_replay(
    action: RecordReplayAction,
    agent_session_id: Option<&str>,
    endpoint: &Endpoint,
    control: &DccControlPlane,
) -> anyhow::Result<RecordReplayResult> {
    let session_id = agent_session_id
        .filter(|value| !value.trim().is_empty())
        .context("record-replay requires --agent-session-id")?;
    let gateway = HttpGateway::with_timeout(Duration::from_secs(30));
    let headers = [("x-dcc-mcp-agent-session-id", session_id)];
    let mut failed = false;
    let value = match action {
        RecordReplayAction::Start {
            dcc_type,
            instance_id,
        } => {
            gateway
                .post_json_with_headers(
                    &endpoint.path("/v1/recordings/start"),
                    &json!({"dcc_type": dcc_type, "instance_id": instance_id}),
                    &headers,
                )
                .await?
        }
        RecordReplayAction::Stop { recording_id } => {
            gateway
                .post_json_with_headers(
                    &endpoint.path("/v1/recordings/stop"),
                    &json!({"recording_id": recording_id}),
                    &headers,
                )
                .await?
        }
        RecordReplayAction::Review { recording_id } => {
            validate_recording_id(&recording_id)?;
            gateway
                .post_json_with_headers(
                    &endpoint.path("/v1/recordings/review"),
                    &json!({"recording_id": recording_id}),
                    &headers,
                )
                .await?
        }
        RecordReplayAction::Compile {
            recording_id,
            name,
            description,
            inputs_json,
            output_dir,
            reviewed,
        } => {
            if !reviewed {
                anyhow::bail!("record-replay compile requires --reviewed");
            }
            let inputs = super::parse_json_object(&inputs_json, "--inputs-json")?;
            let response = gateway
                .post_json_with_headers(
                    &endpoint.path("/v1/recordings/compile"),
                    &json!({
                        "recording_id": recording_id,
                        "name": name,
                        "description": description,
                        "inputs": inputs,
                        "reviewed": true,
                    }),
                    &headers,
                )
                .await?;
            write_compiled_skill(&output_dir, &name, &response)?;
            response
        }
        RecordReplayAction::Replay {
            workflow_file,
            guard_file,
            tool_slug,
            inputs_json,
            approve_replay,
            timeout_secs,
        } => {
            if !approve_replay {
                anyhow::bail!("record-replay replay requires --approve-replay");
            }
            let guard_file = guard_file.unwrap_or_else(|| {
                workflow_file
                    .parent()
                    .unwrap_or_else(|| Path::new("."))
                    .join("replay.guard.json")
            });
            let guards: Value = serde_json::from_str(
                &std::fs::read_to_string(&guard_file)
                    .with_context(|| format!("read replay guard file {}", guard_file.display()))?,
            )
            .with_context(|| format!("parse replay guard file {}", guard_file.display()))?;
            gateway
                .post_json_with_headers(
                    &endpoint.path("/v1/recordings/replay/validate"),
                    &json!({"guards": guards}),
                    &headers,
                )
                .await?;
            let spec = std::fs::read_to_string(&workflow_file)
                .with_context(|| format!("read workflow file {}", workflow_file.display()))?;
            let inputs = super::parse_json_object(&inputs_json, "--inputs-json")?;
            let meta = attach_agent_session_id(None, Some(session_id))?;
            let mut result = control
                .call(
                    tool_slug,
                    None,
                    None,
                    json!({"spec": spec, "inputs": inputs}),
                    meta,
                    Duration::from_secs(timeout_secs.max(1)),
                )
                .await?;
            materialize_call_images(&mut result, &default_image_artifact_root());
            failed = !crate::application::local_control::call_result_succeeded(&result);
            result
        }
    };
    Ok(RecordReplayResult { value, failed })
}

fn validate_recording_id(recording_id: &str) -> anyhow::Result<()> {
    if recording_id.is_empty()
        || recording_id.len() > 64
        || !recording_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        anyhow::bail!("recording id must be a bounded ASCII identifier");
    }
    Ok(())
}

fn write_compiled_skill(root: &Path, name: &str, response: &Value) -> anyhow::Result<()> {
    let compiled = response
        .get("compiled")
        .context("compile response is missing compiled output")?;
    let skill_md = compiled
        .get("skill_md")
        .and_then(Value::as_str)
        .context("compile response is missing SKILL.md")?;
    let replay_contract = compiled
        .get("replay_contract_md")
        .and_then(Value::as_str)
        .context("compile response is missing replay contract")?;
    let workflow = compiled
        .get("workflow")
        .context("compile response is missing workflow")?;
    let guards = compiled
        .get("tool_guards")
        .context("compile response is missing replay schema guards")?;
    let package = root.join(name);
    let workflows = package.join("workflows");
    let references = package.join("references");
    std::fs::create_dir_all(&workflows)?;
    std::fs::create_dir_all(&references)?;
    std::fs::write(package.join("SKILL.md"), skill_md)?;
    std::fs::write(
        workflows.join("replay.workflow.yaml"),
        serde_json::to_string_pretty(workflow)?,
    )?;
    std::fs::write(
        workflows.join("replay.guard.json"),
        serde_json::to_string_pretty(guards)?,
    )?;
    std::fs::write(references.join("REPLAY_CONTRACT.md"), replay_contract)?;
    Ok(())
}
