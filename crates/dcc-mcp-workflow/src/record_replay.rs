//! Safe demonstration recording contracts and compilation into reusable workflows.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::policy::StepPolicy;
use crate::{Step, StepId, StepKind, WorkflowSpec};

/// Versioned, redacted recording captured under one trusted caller namespace.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RecordingManifest {
    /// Manifest schema version.
    pub version: u32,
    /// Opaque recording identifier.
    pub recording_id: String,
    /// Server-derived caller/session namespace.
    pub session_namespace: String,
    /// Exact DCC target identity recorded without titles or coordinates.
    pub target: RecordingTarget,
    /// Ordered semantic events.
    pub events: Vec<RecordedEvent>,
}

/// Stable target correlation retained by a recording.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordingTarget {
    /// Canonical or custom DCC type.
    pub dcc_type: String,
    /// Instance identity at demonstration time. Replay re-resolves it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instance_id: Option<String>,
}

/// One bounded, redacted demonstration event.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RecordedEvent {
    /// Successful structured tool mutation.
    ToolCall {
        /// Monotonic sequence within the recording.
        sequence: u64,
        /// Stable backend tool name, never a transient gateway slug.
        tool: String,
        /// DCC type used for current-instance discovery during replay.
        dcc_type: String,
        /// Parameterized, redacted arguments.
        #[serde(default)]
        arguments_template: Value,
        /// SHA-256 fingerprint of the demonstrated input schema.
        schema_fingerprint: String,
        /// Whether the demonstrated call succeeded.
        success: bool,
    },
    /// One exact-window semantic UI action.
    UiSemanticAction {
        /// Monotonic sequence within the recording.
        sequence: u64,
        /// Fresh-snapshot query used to resolve a current control id.
        query: Value,
        /// Semantic action name such as `click` or `set_text`.
        action: String,
        /// Optional redacted action value/template.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        value: Option<Value>,
        /// Semantic postcondition evaluated after a new observation.
        postcondition: Value,
    },
    /// A bounded visual/accessibility assertion.
    Assertion {
        /// Monotonic sequence within the recording.
        sequence: u64,
        /// Declarative recognizer contract.
        recognizer: Value,
    },
    /// A demonstration-time approval marker. It is never compiled as authority.
    Approval {
        /// Monotonic sequence within the recording.
        sequence: u64,
    },
}

impl RecordedEvent {
    fn sequence(&self) -> u64 {
        match self {
            Self::ToolCall { sequence, .. }
            | Self::UiSemanticAction { sequence, .. }
            | Self::Assertion { sequence, .. }
            | Self::Approval { sequence } => *sequence,
        }
    }
}

/// Operator-reviewed names and input schema used during compilation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompileOptions {
    /// Generated workflow and Skill name.
    pub name: String,
    /// Human-readable purpose.
    #[serde(default)]
    pub description: String,
    /// JSON-schema-shaped workflow input declaration.
    #[serde(default)]
    pub inputs: Value,
}

/// Reviewable output of a recording compilation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompiledRecording {
    /// Existing workflow runtime contract.
    pub workflow: WorkflowSpec,
    /// Minimal local Skill entrypoint.
    pub skill_md: String,
    /// Safety and drift contract shipped beside the workflow.
    pub replay_contract_md: String,
    /// Current tool schemas that must still match immediately before replay.
    pub tool_guards: Vec<ReplayToolGuard>,
    /// Non-fatal review findings, including dropped approvals.
    pub warnings: Vec<String>,
}

/// One fail-closed schema guard resolved again immediately before replay.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplayToolGuard {
    /// Canonical or custom DCC type.
    pub dcc_type: String,
    /// Stable backend callable id rather than a transient instance slug.
    pub tool: String,
    /// SHA-256 of the canonical input schema accepted during compilation.
    pub schema_fingerprint: String,
}

/// A recording cannot be compiled safely without operator correction.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RecordingCompileError {
    /// The manifest version is not supported.
    #[error("unsupported recording manifest version {0}")]
    UnsupportedVersion(u32),
    /// Required identity text is empty or malformed.
    #[error("invalid recording field: {0}")]
    InvalidField(String),
    /// Event order is ambiguous.
    #[error("recording event sequence must be strictly increasing")]
    InvalidSequence,
    /// No executable steps remain after safe filtering.
    #[error("recording contains no executable events")]
    NoExecutableEvents,
    /// An event contains a forbidden secret/authority field.
    #[error("recording contains forbidden field at {0}")]
    ForbiddenField(String),
    /// The generated workflow violates its runtime contract.
    #[error("generated workflow is invalid: {0}")]
    InvalidWorkflow(String),
}

/// Compute a stable SHA-256 over a JSON schema after recursively sorting object keys.
#[must_use]
pub fn schema_fingerprint(schema: &Value) -> String {
    let canonical = canonical_json(schema);
    let bytes = serde_json::to_vec(&canonical).unwrap_or_default();
    const HEX: &[u8; 16] = b"0123456789abcdef";
    Sha256::digest(bytes)
        .iter()
        .flat_map(|byte| {
            [
                HEX[usize::from(byte >> 4)] as char,
                HEX[usize::from(byte & 0x0f)] as char,
            ]
        })
        .collect()
}

/// Compile a redacted demonstration into a reviewable workflow and local Skill.
pub fn compile_recording(
    manifest: &RecordingManifest,
    options: &CompileOptions,
) -> Result<CompiledRecording, RecordingCompileError> {
    validate_manifest(manifest, options)?;

    let mut steps = Vec::new();
    let mut tool_guards = Vec::new();
    let mut warnings = Vec::new();
    for event in &manifest.events {
        match event {
            RecordedEvent::ToolCall {
                sequence,
                tool,
                dcc_type,
                arguments_template,
                schema_fingerprint,
                success,
                ..
            } if *success => {
                steps.push(step(
                    format!("recorded_{sequence}"),
                    StepKind::ToolRemote {
                        dcc: dcc_type.clone(),
                        tool: tool.clone(),
                        args: arguments_template.clone(),
                    },
                ));
                let guard = ReplayToolGuard {
                    dcc_type: dcc_type.clone(),
                    tool: tool.clone(),
                    schema_fingerprint: schema_fingerprint.clone(),
                };
                if !tool_guards.contains(&guard) {
                    tool_guards.push(guard);
                }
            }
            RecordedEvent::ToolCall { sequence, .. } => warnings.push(format!(
                "Dropped failed exploratory tool call at sequence {sequence}."
            )),
            RecordedEvent::UiSemanticAction {
                sequence,
                query,
                action,
                value,
                postcondition,
            } => {
                let session_id = "{{env.workflow_id}}";
                steps.push(step(
                    format!("ui_{sequence}_snapshot_before"),
                    StepKind::ToolRemote {
                        dcc: manifest.target.dcc_type.clone(),
                        tool: "ui_control__snapshot".to_owned(),
                        args: serde_json::json!({"session_id": session_id}),
                    },
                ));
                steps.push(step(
                    format!("ui_{sequence}_find"),
                    StepKind::ToolRemote {
                        dcc: manifest.target.dcc_type.clone(),
                        tool: "ui_control__find".to_owned(),
                        args: serde_json::json!({
                            "session_id": session_id,
                            "query": query,
                        }),
                    },
                ));
                let mut action_args = serde_json::json!({
                    "session_id": session_id,
                    "action": action,
                    "control_id": format!("{{{{steps.ui_{sequence}_find.matches.0.id}}}}"),
                });
                if let Some(value) = value {
                    action_args["value"] = value.clone();
                }
                steps.push(step(
                    format!("ui_{sequence}_act"),
                    StepKind::ToolRemote {
                        dcc: manifest.target.dcc_type.clone(),
                        tool: "ui_control__act".to_owned(),
                        args: action_args,
                    },
                ));
                steps.push(step(
                    format!("ui_{sequence}_verify"),
                    StepKind::ToolRemote {
                        dcc: manifest.target.dcc_type.clone(),
                        tool: "ui_control__wait_for".to_owned(),
                        args: serde_json::json!({
                            "session_id": session_id,
                            "condition": postcondition,
                        }),
                    },
                ));
            }
            RecordedEvent::Assertion {
                sequence,
                recognizer,
            } => steps.push(step(
                format!("assert_{sequence}"),
                StepKind::ToolRemote {
                    dcc: manifest.target.dcc_type.clone(),
                    tool: "ui_control__wait_for".to_owned(),
                    args: serde_json::json!({
                        "session_id": "{{env.workflow_id}}",
                        "condition": {"kind": "recognizer", "recognizer": recognizer},
                    }),
                },
            )),
            RecordedEvent::Approval { sequence } => warnings.push(format!(
                "Dropped demonstration-time approval at sequence {sequence}; replay authority must be granted again."
            )),
        }
    }
    if steps.is_empty() {
        return Err(RecordingCompileError::NoExecutableEvents);
    }

    let workflow = WorkflowSpec {
        name: options.name.clone(),
        description: options.description.clone(),
        inputs: options.inputs.clone(),
        steps,
    };
    workflow
        .validate()
        .map_err(|error| RecordingCompileError::InvalidWorkflow(error.to_string()))?;
    let workflow_yaml = serde_yaml_ng::to_string(&workflow)
        .map_err(|error| RecordingCompileError::InvalidWorkflow(error.to_string()))?;
    let skill_md = format!(
        "---\nname: {}\ndescription: {}\nmetadata:\n  dcc-mcp:\n    workflows: workflows/replay.workflow.yaml\n---\n\n# {}\n\nRun the reviewed workflow in `workflows/replay.workflow.yaml`. Re-discover current tools and stop on schema, target, approval, or observation drift.\n\n```yaml\n{}\n```\n",
        options.name,
        yaml_scalar(&options.description),
        options.name,
        workflow_yaml.trim_end(),
    );
    let replay_contract_md = format!(
        "# Replay contract\n\nRecording `{}` was compiled from trusted session `{}`. The demonstrated instance id is intentionally not retained as replay authority. Every replay requires a current grant, current tool schemas, fresh UI observations, and post-step verification.\n",
        manifest.recording_id, manifest.session_namespace,
    );
    Ok(CompiledRecording {
        workflow,
        skill_md,
        replay_contract_md,
        tool_guards,
        warnings,
    })
}

fn step(id: String, kind: StepKind) -> Step {
    Step {
        id: StepId(id),
        kind,
        policy: StepPolicy::default(),
    }
}

fn validate_manifest(
    manifest: &RecordingManifest,
    options: &CompileOptions,
) -> Result<(), RecordingCompileError> {
    if manifest.version != 1 {
        return Err(RecordingCompileError::UnsupportedVersion(manifest.version));
    }
    for (name, value) in [
        ("recording_id", manifest.recording_id.as_str()),
        ("session_namespace", manifest.session_namespace.as_str()),
        ("target.dcc_type", manifest.target.dcc_type.as_str()),
        ("options.name", options.name.as_str()),
    ] {
        if value.trim().is_empty() {
            return Err(RecordingCompileError::InvalidField(name.to_owned()));
        }
    }
    let mut previous = None;
    for event in &manifest.events {
        let sequence = event.sequence();
        if previous.is_some_and(|value| sequence <= value) {
            return Err(RecordingCompileError::InvalidSequence);
        }
        previous = Some(sequence);
        match event {
            RecordedEvent::ToolCall {
                tool,
                dcc_type,
                arguments_template,
                schema_fingerprint,
                ..
            } => {
                if tool.trim().is_empty() || dcc_type.trim().is_empty() {
                    return Err(RecordingCompileError::InvalidField(format!(
                        "events[{sequence}].tool"
                    )));
                }
                if schema_fingerprint.len() != 64
                    || !schema_fingerprint
                        .bytes()
                        .all(|byte| byte.is_ascii_hexdigit())
                {
                    return Err(RecordingCompileError::InvalidField(format!(
                        "events[{sequence}].schema_fingerprint"
                    )));
                }
                reject_forbidden_fields(
                    arguments_template,
                    &format!("events[{sequence}].arguments"),
                )?;
            }
            RecordedEvent::UiSemanticAction {
                query,
                value,
                postcondition,
                ..
            } => {
                reject_forbidden_fields(query, &format!("events[{sequence}].query"))?;
                if let Some(value) = value {
                    reject_forbidden_fields(value, &format!("events[{sequence}].value"))?;
                }
                reject_forbidden_fields(
                    postcondition,
                    &format!("events[{sequence}].postcondition"),
                )?;
            }
            RecordedEvent::Assertion { recognizer, .. } => {
                reject_forbidden_fields(recognizer, &format!("events[{sequence}].recognizer"))?;
            }
            RecordedEvent::Approval { .. } => {}
        }
    }
    Ok(())
}

fn reject_forbidden_fields(value: &Value, path: &str) -> Result<(), RecordingCompileError> {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                let normalized = key.to_ascii_lowercase();
                if [
                    "password",
                    "passwd",
                    "secret",
                    "token",
                    "credential",
                    "authorization",
                    "cookie",
                    "approval",
                    "confirmation",
                    "grant",
                ]
                .iter()
                .any(|forbidden| normalized.contains(forbidden))
                {
                    return Err(RecordingCompileError::ForbiddenField(format!(
                        "{path}.{key}"
                    )));
                }
                reject_forbidden_fields(child, &format!("{path}.{key}"))?;
            }
        }
        Value::Array(items) => {
            for (index, child) in items.iter().enumerate() {
                reject_forbidden_fields(child, &format!("{path}[{index}]"))?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn yaml_scalar(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"Recorded workflow\"".to_owned())
}

fn canonical_json(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut keys = map.keys().collect::<Vec<_>>();
            keys.sort_unstable();
            Value::Object(
                keys.into_iter()
                    .map(|key| (key.clone(), canonical_json(&map[key])))
                    .collect(),
            )
        }
        Value::Array(items) => Value::Array(items.iter().map(canonical_json).collect()),
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::StepKind;

    fn manifest(events: Vec<RecordedEvent>) -> RecordingManifest {
        RecordingManifest {
            version: 1,
            recording_id: "rec-1".to_owned(),
            session_namespace: "trusted-session".to_owned(),
            target: RecordingTarget {
                dcc_type: "maya".to_owned(),
                instance_id: Some("instance-old".to_owned()),
            },
            events,
        }
    }

    fn options() -> CompileOptions {
        CompileOptions {
            name: "recorded_build".to_owned(),
            description: "Build a reviewed scene.".to_owned(),
            inputs: json!({"output": {"type": "string"}}),
        }
    }

    #[test]
    fn compiles_structured_calls_and_drops_recorded_approval() {
        let compiled = compile_recording(
            &manifest(vec![
                RecordedEvent::ToolCall {
                    sequence: 1,
                    tool: "scene__build".to_owned(),
                    dcc_type: "maya".to_owned(),
                    arguments_template: json!({"output": "{{inputs.output}}"}),
                    schema_fingerprint: "a".repeat(64),
                    success: true,
                },
                RecordedEvent::Approval { sequence: 2 },
            ]),
            &options(),
        )
        .unwrap();

        assert_eq!(compiled.workflow.steps.len(), 1);
        assert!(matches!(
            &compiled.workflow.steps[0].kind,
            StepKind::ToolRemote { dcc, tool, .. } if dcc == "maya" && tool == "scene__build"
        ));
        assert!(
            compiled
                .warnings
                .iter()
                .any(|warning| warning.contains("approval"))
        );
        assert!(
            !compiled
                .skill_md
                .to_ascii_lowercase()
                .contains("instance-old")
        );
        assert!(compiled.workflow.validate().is_ok());
        assert_eq!(compiled.tool_guards.len(), 1);
        assert_eq!(compiled.tool_guards[0].schema_fingerprint, "a".repeat(64));
    }

    #[test]
    fn refuses_secret_or_reusable_authority_fields() {
        let error = compile_recording(
            &manifest(vec![RecordedEvent::ToolCall {
                sequence: 1,
                tool: "scene__publish".to_owned(),
                dcc_type: "houdini".to_owned(),
                arguments_template: json!({"api_token": "do-not-record"}),
                schema_fingerprint: "b".repeat(64),
                success: true,
            }]),
            &options(),
        )
        .unwrap_err();

        assert!(matches!(error, RecordingCompileError::ForbiddenField(_)));
    }

    #[test]
    fn schema_fingerprint_ignores_object_key_order() {
        assert_eq!(
            schema_fingerprint(&json!({"type": "object", "properties": {"b": {}, "a": {}}})),
            schema_fingerprint(&json!({"properties": {"a": {}, "b": {}}, "type": "object"}))
        );
    }
}
