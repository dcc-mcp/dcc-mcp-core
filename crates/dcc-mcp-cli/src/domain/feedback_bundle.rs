use dcc_mcp_models::{FindingRedactionMode, FindingV1};
use serde::Serialize;
use serde_json::{Map, Value};
use thiserror::Error;

pub const FEEDBACK_BUNDLE_SCHEMA_VERSION: &str = "dcc-mcp.feedback-bundle.v1";
pub const DEFAULT_HOST_ERROR_LINES: usize = 50;
pub const MAX_HOST_ERROR_LINES: usize = 200;
pub const MAX_HOST_ERROR_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone)]
pub struct FeedbackBundleInput {
    pub finding: FindingV1,
    pub doctor: Value,
    pub issue_report: Option<Value>,
    pub host_errors: HostErrorTail,
    pub install_execution_report: Option<Value>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FeedbackBundle {
    pub schema_version: &'static str,
    pub privacy_mode: &'static str,
    pub complete: bool,
    pub finding: FindingV1,
    pub version_matrix: VersionMatrix,
    pub components: FeedbackBundleComponents,
    pub redaction_status: BundleRedactionStatus,
}

#[derive(Debug, Clone, Serialize)]
pub struct VersionMatrix {
    pub dcc_type: String,
    pub adapter: String,
    pub adapter_version: String,
    pub core: String,
    pub host: String,
    pub os: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct FeedbackBundleComponents {
    pub issue_report: BundleComponent<Value>,
    pub doctor: BundleComponent<Value>,
    pub host_errors: BundleComponent<HostErrorTailData>,
    pub install_execution_report: BundleComponent<Value>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BundleComponent<T> {
    pub status: BundleComponentStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
}

impl<T> BundleComponent<T> {
    fn included(data: T) -> Self {
        Self {
            status: BundleComponentStatus::Included,
            reason: None,
            data: Some(data),
        }
    }

    fn not_applicable(reason: impl Into<String>) -> Self {
        Self {
            status: BundleComponentStatus::NotApplicable,
            reason: Some(reason.into()),
            data: None,
        }
    }

    fn unavailable(reason: impl Into<String>) -> Self {
        Self {
            status: BundleComponentStatus::Unavailable,
            reason: Some(reason.into()),
            data: None,
        }
    }

    fn is_resolved(&self) -> bool {
        self.status != BundleComponentStatus::Unavailable
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BundleComponentStatus {
    Included,
    NotApplicable,
    Unavailable,
}

#[derive(Debug, Clone)]
pub struct HostErrorTail {
    status: BundleComponentStatus,
    reason: Option<String>,
    records: Vec<Value>,
    truncated: bool,
    max_lines: usize,
    max_bytes: usize,
    skipped_invalid: usize,
}

impl HostErrorTail {
    #[must_use]
    pub fn included(records: Vec<Value>, truncated: bool) -> Self {
        Self {
            status: BundleComponentStatus::Included,
            reason: None,
            records,
            truncated,
            max_lines: DEFAULT_HOST_ERROR_LINES,
            max_bytes: MAX_HOST_ERROR_BYTES,
            skipped_invalid: 0,
        }
    }

    #[must_use]
    pub fn included_bounded(
        records: Vec<Value>,
        truncated: bool,
        max_lines: usize,
        skipped_invalid: usize,
    ) -> Self {
        Self {
            status: BundleComponentStatus::Included,
            reason: None,
            records,
            truncated,
            max_lines,
            max_bytes: MAX_HOST_ERROR_BYTES,
            skipped_invalid,
        }
    }

    #[must_use]
    pub fn not_available(reason: impl Into<String>) -> Self {
        Self {
            status: BundleComponentStatus::Unavailable,
            reason: Some(reason.into()),
            records: Vec::new(),
            truncated: false,
            max_lines: DEFAULT_HOST_ERROR_LINES,
            max_bytes: MAX_HOST_ERROR_BYTES,
            skipped_invalid: 0,
        }
    }

    pub(crate) fn into_component(self) -> BundleComponent<HostErrorTailData> {
        if self.status == BundleComponentStatus::Included {
            BundleComponent::included(HostErrorTailData {
                records: self.records,
                truncated: self.truncated,
                max_lines: self.max_lines,
                max_bytes: self.max_bytes,
                skipped_invalid: self.skipped_invalid,
            })
        } else {
            BundleComponent::unavailable(
                self.reason
                    .unwrap_or_else(|| "host_error_tail_not_available".to_string()),
            )
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct HostErrorTailData {
    pub records: Vec<Value>,
    pub truncated: bool,
    pub max_lines: usize,
    pub max_bytes: usize,
    pub skipped_invalid: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct BundleRedactionStatus {
    pub mode: &'static str,
    pub raw_payloads_excluded: bool,
    pub prompts_excluded: bool,
    pub scripts_excluded: bool,
    pub auth_material_excluded: bool,
    pub local_urls_excluded: bool,
    pub absolute_paths_excluded: bool,
    pub private_identifiers_excluded: bool,
    pub host_error_payload_fields_excluded: bool,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum FeedbackBundleError {
    #[error("invalid finding: {0}")]
    InvalidFinding(String),
    #[error("finding is not marked public-safe")]
    FindingNeedsReview,
    #[error("issue report is not public-safe")]
    UnsafeIssueReport,
    #[error("issue report request_id does not match the finding")]
    IssueReportRequestMismatch,
}

pub fn assemble_public_bundle(
    input: FeedbackBundleInput,
) -> Result<FeedbackBundle, FeedbackBundleError> {
    let mut finding = input.finding;
    validate_public_finding(&finding)?;

    let issue_report = issue_report_component(&finding, input.issue_report)?;
    let host_errors = input.host_errors.into_component();
    let install_execution_report = input
        .install_execution_report
        .map(|report| BundleComponent::included(project_public_install_execution_report(&report)))
        .unwrap_or_else(|| BundleComponent::unavailable("install_execution_report_not_provided"));
    let complete = issue_report.is_resolved()
        && host_errors.is_resolved()
        && install_execution_report.is_resolved();
    let version_matrix = VersionMatrix {
        dcc_type: finding.dcc_type.clone(),
        adapter: finding.adapter.clone(),
        adapter_version: finding.adapter_version.clone(),
        core: finding.core_version.clone(),
        host: finding.host_version.clone(),
        os: finding.os.clone(),
    };
    finding.evidence.extra.remove("dcc_pid");
    Ok(FeedbackBundle {
        schema_version: FEEDBACK_BUNDLE_SCHEMA_VERSION,
        privacy_mode: "public-safe",
        complete,
        finding,
        version_matrix,
        components: FeedbackBundleComponents {
            issue_report,
            doctor: BundleComponent::included(project_public_doctor(&input.doctor)),
            host_errors,
            install_execution_report,
        },
        redaction_status: BundleRedactionStatus {
            mode: "public-safe",
            raw_payloads_excluded: true,
            prompts_excluded: true,
            scripts_excluded: true,
            auth_material_excluded: true,
            local_urls_excluded: true,
            absolute_paths_excluded: true,
            private_identifiers_excluded: true,
            host_error_payload_fields_excluded: true,
        },
    })
}

#[must_use]
pub fn project_public_install_execution_report(value: &Value) -> Value {
    let Some(object) = value.as_object() else {
        return Value::Object(Map::new());
    };
    let mut projected = Map::new();
    for key in [
        "schema_version",
        "status",
        "dcc_type",
        "adapter_version",
        "core_version",
        "stage",
        "exit_code",
        "steps",
        "rollback",
        "next_steps",
        "receipt_path",
        "verify",
        "error",
    ] {
        if let Some(value) = object.get(key) {
            let value = if key == "next_steps" {
                project_public_install_next_steps(value)
            } else {
                sanitize_json(value, Some(key))
            };
            projected.insert(key.to_string(), value);
        }
    }
    Value::Object(projected)
}

fn project_public_install_next_steps(value: &Value) -> Value {
    let Some(steps) = value.as_array() else {
        return Value::Array(Vec::new());
    };
    Value::Array(
        steps
            .iter()
            .filter_map(|step| {
                let object = step.as_object()?;
                let mut projected = Map::new();
                for key in ["id", "description", "why"] {
                    if let Some(value) = object.get(key) {
                        projected.insert(key.to_string(), sanitize_json(value, Some(key)));
                    }
                }
                if let Some(command) = object.get("command") {
                    projected.insert("command".to_string(), sanitize_command_argv(command));
                }
                if let Some(file_edit) = object.get("file_edit") {
                    projected.insert(
                        "file_edit".to_string(),
                        project_public_install_file_edit(file_edit),
                    );
                }
                Some(Value::Object(projected))
            })
            .collect(),
    )
}

fn sanitize_command_argv(value: &Value) -> Value {
    let Some(arguments) = value.as_array() else {
        return Value::Array(Vec::new());
    };
    let mut redact_next = false;
    Value::Array(
        arguments
            .iter()
            .map(|argument| {
                let Some(argument) = argument.as_str() else {
                    return Value::String("[value-redacted]".to_string());
                };
                if redact_next {
                    redact_next = false;
                    return Value::String("[auth-redacted]".to_string());
                }
                if let Some(has_inline_value) = sensitive_command_option(argument) {
                    redact_next = !has_inline_value;
                    return Value::String("[auth-redacted]".to_string());
                }
                if contains_absolute_path(argument) || looks_like_relative_path(argument) {
                    return Value::String("[path-redacted]".to_string());
                }
                Value::String(sanitize_public_text(argument))
            })
            .collect(),
    )
}

fn sensitive_command_option(argument: &str) -> Option<bool> {
    if !argument.starts_with('-') {
        return None;
    }
    let (name, inline_value) = argument
        .split_once('=')
        .map_or((argument, false), |(name, _)| (name, true));
    sensitive_key(name.trim_start_matches('-')).then_some(inline_value)
}

fn looks_like_relative_path(argument: &str) -> bool {
    let lower = argument.to_ascii_lowercase();
    argument.contains('/')
        || argument.contains('\\')
        || lower.starts_with("./")
        || lower.starts_with("../")
        || [
            ".json", ".jsonl", ".yaml", ".yml", ".toml", ".log", ".txt", ".md",
        ]
        .iter()
        .any(|extension| lower.ends_with(extension))
}

fn project_public_install_file_edit(value: &Value) -> Value {
    let Some(object) = value.as_object() else {
        return Value::Object(Map::new());
    };
    let mut projected = Map::new();
    if object.contains_key("path") {
        projected.insert(
            "path".to_string(),
            Value::String("[path-redacted]".to_string()),
        );
    }
    if let Some(action) = object.get("action") {
        projected.insert("action".to_string(), sanitize_json(action, Some("action")));
    }
    Value::Object(projected)
}

pub fn validate_public_finding(finding: &FindingV1) -> Result<(), FeedbackBundleError> {
    finding
        .validate()
        .map_err(|error| FeedbackBundleError::InvalidFinding(error.to_string()))?;
    if !finding_is_public_safe(finding) {
        return Err(FeedbackBundleError::FindingNeedsReview);
    }
    Ok(())
}

fn finding_is_public_safe(finding: &FindingV1) -> bool {
    let status = &finding.redaction_status;
    status.mode == FindingRedactionMode::PublicSafe
        && status.raw_payloads_excluded
        && status.prompts_excluded
        && status.scripts_excluded
        && status.auth_material_excluded
        && status.local_urls_excluded
        && status.absolute_paths_excluded
        && status.private_identifiers_excluded
}

fn issue_report_component(
    finding: &FindingV1,
    report: Option<Value>,
) -> Result<BundleComponent<Value>, FeedbackBundleError> {
    let Some(request_id) = finding.evidence.request_id.as_deref() else {
        return Ok(BundleComponent::not_applicable("finding_has_no_request_id"));
    };
    let Some(report) = report else {
        return Ok(BundleComponent::unavailable("issue_report_not_available"));
    };
    if report.get("schema_version").and_then(Value::as_str) != Some("dcc-mcp.admin.issue-report.v1")
        || report.get("report_type").and_then(Value::as_str) != Some("github_issue_public_safe")
        || report.get("privacy_mode").and_then(Value::as_str) != Some("public-safe")
        || report.get("debug_bundle").is_some()
    {
        return Err(FeedbackBundleError::UnsafeIssueReport);
    }
    if report.get("request_id").and_then(Value::as_str) != Some(request_id) {
        return Err(FeedbackBundleError::IssueReportRequestMismatch);
    }
    Ok(BundleComponent::included(report))
}

#[must_use]
pub fn project_public_doctor(value: &Value) -> Value {
    sanitize_json(value, None)
}

#[must_use]
pub fn project_public_host_event(value: &Value) -> Option<Value> {
    let object = value.as_object()?;
    let mut projected = Map::new();
    for key in [
        "event",
        "dcc_type",
        "phase",
        "source",
        "stream",
        "level",
        "exception_type",
        "adapter_version",
        "min_core_version",
        "core_version",
        "python_version",
    ] {
        if let Some(value) = object
            .get(key)
            .filter(|value| value.is_string() || value.is_number())
        {
            projected.insert(key.to_string(), sanitize_json(value, Some(key)));
        }
    }
    projected.insert("payload_fields_excluded".to_string(), Value::Bool(true));
    Some(Value::Object(projected))
}

fn sanitize_json(value: &Value, key: Option<&str>) -> Value {
    if let Some(key) = key {
        if sensitive_key(key) {
            return Value::String("[auth-redacted]".to_string());
        }
        if path_key(key) {
            return Value::String("[path-redacted]".to_string());
        }
        if url_key(key) {
            return Value::String("[url-redacted]".to_string());
        }
        if endpoint_key(key) {
            return Value::String("[endpoint-redacted]".to_string());
        }
        if identifier_key(key) {
            return Value::String("[id-redacted]".to_string());
        }
    }
    match value {
        Value::String(text) => Value::String(sanitize_public_text(text)),
        Value::Array(items) => {
            Value::Array(items.iter().map(|item| sanitize_json(item, key)).collect())
        }
        Value::Object(object) => Value::Object(
            object
                .iter()
                .map(|(child_key, child)| {
                    (child_key.clone(), sanitize_json(child, Some(child_key)))
                })
                .collect(),
        ),
        scalar => scalar.clone(),
    }
}

fn sensitive_key(key: &str) -> bool {
    let lower = key.to_ascii_lowercase().replace('-', "_");
    [
        "token",
        "secret",
        "password",
        "credential",
        "authorization",
        "api_key",
        "access_key",
        "private_key",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

fn path_key(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    lower.contains("path") || lower.ends_with("_dir") || lower == "directory"
}

fn url_key(key: &str) -> bool {
    key.to_ascii_lowercase().contains("url")
}

fn endpoint_key(key: &str) -> bool {
    matches!(
        key.to_ascii_lowercase().as_str(),
        "host" | "hostname" | "port" | "addr" | "address" | "bind" | "endpoint"
    )
}

fn identifier_key(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    lower == "pid" || lower.ends_with("_pid") || lower.ends_with("_id")
}

fn sanitize_public_text(text: &str) -> String {
    let lower = text.to_ascii_lowercase();
    if contains_absolute_path(text) {
        return "[path-redacted]".to_string();
    }
    if contains_uri_scheme(text) {
        return "[url-redacted]".to_string();
    }
    if ["token=", "secret=", "password=", "bearer ", "api_key="]
        .iter()
        .any(|needle| lower.contains(needle))
    {
        return "[auth-redacted]".to_string();
    }
    text.to_string()
}

fn contains_uri_scheme(text: &str) -> bool {
    let bytes = text.as_bytes();
    (0..bytes.len()).any(|start| {
        if !bytes[start].is_ascii_alphabetic()
            || (start > 0 && uri_scheme_character(bytes[start - 1]))
        {
            return false;
        }
        let mut end = start + 1;
        while end < bytes.len() && uri_scheme_character(bytes[end]) {
            end += 1;
        }
        end - start >= 2
            && bytes.get(end) == Some(&b':')
            && bytes
                .get(end + 1)
                .is_some_and(|character| !character.is_ascii_whitespace())
    })
}

fn uri_scheme_character(character: u8) -> bool {
    character.is_ascii_alphanumeric() || matches!(character, b'+' | b'-' | b'.')
}

fn contains_absolute_path(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.windows(3).any(|window| {
        window[0].is_ascii_alphabetic()
            && window[1] == b':'
            && (window[2] == b'\\' || window[2] == b'/')
    }) || ["\\\\", "/Users/", "/home/", "/mnt/", "/studio/"]
        .iter()
        .any(|prefix| text.contains(prefix))
}

#[cfg(test)]
mod tests {
    use dcc_mcp_models::{
        FINDING_V1_SCHEMA_VERSION, FindingEvidenceV1, FindingPhase, FindingRedactionMode,
        FindingRedactionStatusV1, FindingReproV1, FindingSeverity, FindingV1,
    };
    use serde_json::json;

    use super::{
        FeedbackBundleInput, HostErrorTail, assemble_public_bundle, project_public_doctor,
        project_public_host_event,
    };

    fn finding(mode: FindingRedactionMode) -> FindingV1 {
        FindingV1 {
            schema_version: FINDING_V1_SCHEMA_VERSION,
            fingerprint: format!("sha256:{}", "a".repeat(64)),
            dcc_type: "godot".into(),
            adapter: "dcc-mcp-godot".into(),
            adapter_version: "0.3.0".into(),
            core_version: "0.20.11".into(),
            host_version: "4.4.1".into(),
            os: "windows".into(),
            phase: FindingPhase::Startup,
            severity: FindingSeverity::Blocker,
            tool_slug: None,
            intent: "Start the adapter".into(),
            observed: "The bridge did not start".into(),
            expected: "The bridge becomes ready".into(),
            repro: FindingReproV1 {
                argv: vec!["dcc-mcp-cli".into(), "status".into()],
                steps: Vec::new(),
            },
            evidence: FindingEvidenceV1 {
                error_kind: Some("startup_failed".into()),
                ..FindingEvidenceV1::default()
            },
            redaction_status: FindingRedactionStatusV1 {
                mode,
                redaction_markers_detected: false,
                raw_payloads_excluded: true,
                prompts_excluded: true,
                scripts_excluded: true,
                auth_material_excluded: true,
                local_urls_excluded: true,
                absolute_paths_excluded: true,
                private_identifiers_excluded: true,
            },
        }
    }

    #[test]
    fn public_bundle_rejects_findings_that_still_need_review() {
        let error = assemble_public_bundle(FeedbackBundleInput {
            finding: finding(FindingRedactionMode::NeedsReview),
            doctor: json!({"status": "ok"}),
            issue_report: None,
            host_errors: HostErrorTail::not_available("dcc_pid_not_available"),
            install_execution_report: None,
        })
        .unwrap_err();

        assert_eq!(error.to_string(), "finding is not marked public-safe");
    }

    #[test]
    fn doctor_projection_redacts_paths_urls_credentials_and_private_ids() {
        let projected = project_public_doctor(&json!({
            "status": "ok",
            "local": {
                "registry_dir": "C:\\Users\\artist\\AppData\\Local\\dcc-mcp",
                "instance_id": "private-instance",
                "inventory": {"ok": true, "total": 1}
            },
            "gateway": {
                "default_base_url": "http://127.0.0.1:9765",
                "status": {
                    "healthy": false,
                    "host": "127.0.0.1",
                    "port": 43210,
                    "pid": 1234
                }
            },
            "server_binary": {
                "path": "C:\\tools\\dcc-mcp-server.exe",
                "error": "token=secret-value"
            }
        }));
        let encoded = serde_json::to_string(&projected).unwrap();

        assert_eq!(projected["local"]["inventory"]["total"], 1);
        assert_eq!(projected["local"]["registry_dir"], "[path-redacted]");
        assert_eq!(projected["local"]["instance_id"], "[id-redacted]");
        assert_eq!(projected["gateway"]["default_base_url"], "[url-redacted]");
        assert_eq!(
            projected["gateway"]["status"]["host"],
            "[endpoint-redacted]"
        );
        assert_eq!(
            projected["gateway"]["status"]["port"],
            "[endpoint-redacted]"
        );
        assert_eq!(projected["gateway"]["status"]["pid"], "[id-redacted]");
        assert!(!encoded.contains("127.0.0.1"));
        assert!(!encoded.contains("43210"));
        assert!(!encoded.contains("artist"));
        assert!(!encoded.contains("secret-value"));
    }

    #[test]
    fn host_event_projection_omits_unreviewed_payload_fields() {
        let projected = project_public_host_event(&json!({
            "event": "dcc_host_error",
            "message": "failed to open C:\\studio\\secret.godot",
            "traceback": "Bearer private-token",
            "metadata": {"prompt": "private scene prompt"},
            "dcc_type": "godot",
            "dcc_pid": 4321,
            "phase": "bootstrap",
            "source": "adapter_bootstrap",
            "level": "error",
            "exception_type": "builtins.RuntimeError",
            "adapter_version": "0.3.0",
            "core_version": "0.20.11",
            "python_version": "3.12.4"
        }))
        .unwrap();
        let encoded = serde_json::to_string(&projected).unwrap();

        assert_eq!(projected["event"], "dcc_host_error");
        assert_eq!(projected["dcc_type"], "godot");
        assert_eq!(projected["payload_fields_excluded"], true);
        for private in [
            "message",
            "traceback",
            "metadata",
            "secret.godot",
            "private-token",
        ] {
            assert!(!encoded.contains(private), "leaked {private}");
        }
    }

    #[test]
    fn bundle_without_an_install_execution_report_is_explicitly_incomplete() {
        let bundle = assemble_public_bundle(FeedbackBundleInput {
            finding: finding(FindingRedactionMode::PublicSafe),
            doctor: json!({"status": "ok"}),
            issue_report: None,
            host_errors: HostErrorTail::included(vec![json!({"event": "dcc_host_error"})], false),
            install_execution_report: None,
        })
        .unwrap();
        let value = serde_json::to_value(bundle).unwrap();

        assert_eq!(value["schema_version"], "dcc-mcp.feedback-bundle.v1");
        assert_eq!(value["privacy_mode"], "public-safe");
        assert_eq!(value["complete"], false);
        assert_eq!(value["components"]["doctor"]["status"], "included");
        assert_eq!(
            value["components"]["issue_report"]["status"],
            "not_applicable"
        );
        assert_eq!(value["components"]["host_errors"]["status"], "included");
        assert_eq!(
            value["components"]["install_execution_report"]["reason"],
            "install_execution_report_not_provided"
        );
        assert_eq!(value["version_matrix"]["dcc_type"], "godot");
        assert_eq!(value["version_matrix"]["core"], "0.20.11");
    }

    #[test]
    fn bundle_includes_a_public_safe_terminal_install_execution_report() {
        let bundle = assemble_public_bundle(FeedbackBundleInput {
            finding: finding(FindingRedactionMode::PublicSafe),
            doctor: json!({"status": "ok"}),
            issue_report: None,
            host_errors: HostErrorTail::included(vec![json!({"event": "dcc_host_error"})], false),
            install_execution_report: Some(json!({
                "schema_version": 1,
                "status": "failed",
                "dcc_type": "godot",
                "adapter_version": "0.3.0",
                "core_version": "0.20.11",
                "stage": "install",
                "exit_code": 30,
                "steps": [{
                    "id": "install-path",
                    "status": "failed",
                    "rollback": {"attempted": false, "status": "not_available"}
                }],
                "rollback": {"attempted": false, "status": "not_attempted", "failure_count": 0},
                "next_steps": [{
                    "id": "inspect-runtime",
                    "description": "Inspect safe local runtime diagnostics.",
                    "why": "Diagnostics can identify a safe remediation before retrying installation.",
                    "command": ["dcc-mcp-cli", "doctor", "C:\\Users\\artist\\private.json"]
                }],
                "receipt_path": "C:\\Users\\artist\\receipt.json",
                "verify": {
                    "directly_usable": false,
                    "failure_stage": "install",
                    "failure_reason": "INSTALL_STEP_FAILED"
                },
                "error": {"code": "INSTALL_STEP_FAILED", "stage": "install", "exit_code": 30}
            })),
        })
        .unwrap();
        let value = serde_json::to_value(bundle).unwrap();
        let encoded = serde_json::to_string(&value).unwrap();

        assert_eq!(value["complete"], true);
        assert_eq!(
            value["components"]["install_execution_report"]["status"],
            "included"
        );
        assert_eq!(
            value["components"]["install_execution_report"]["data"]["receipt_path"],
            "[path-redacted]"
        );
        assert_eq!(
            value["components"]["install_execution_report"]["data"]["next_steps"][0]["command"][2],
            "[path-redacted]"
        );
        assert!(!encoded.contains("artist"));
        assert!(!encoded.contains("private.json"));
        assert!(!encoded.contains("receipt.json"));
    }

    #[test]
    fn bundle_redacts_structured_install_next_steps() {
        let bundle = assemble_public_bundle(FeedbackBundleInput {
            finding: finding(FindingRedactionMode::PublicSafe),
            doctor: json!({"status": "ok"}),
            issue_report: None,
            host_errors: HostErrorTail::included(vec![json!({"event": "dcc_host_error"})], false),
            install_execution_report: Some(json!({
                "schema_version": 1,
                "status": "failed",
                "dcc_type": "godot",
                "adapter_version": "0.3.0",
                "core_version": "0.20.11",
                "steps": [{"id": "register", "status": "failed"}],
                "next_steps": [
                    {
                        "id": "inspect-runtime",
                        "description": "Inspect safe runtime diagnostics.",
                        "why": "Diagnostics identify the next safe action.",
                        "command": [
                            "dcc-mcp-cli",
                            "doctor",
                            "--token",
                            "REVIEW_SECRET_7f0a",
                            "private/reports/install-result.json",
                            "mailto:private-contact@example.invalid",
                            "custom+report:private-location",
                            "--api-key=INLINE_REVIEW_SECRET_d820",
                            "contact(mailto:embedded-private@example.invalid)"
                        ]
                    },
                    {
                        "id": "update-registration",
                        "description": "Update the host registration file.",
                        "why": "Manual registration is still required.",
                        "file_edit": {
                            "path": "private/config/registration.json",
                            "action": "update",
                            "content": "REVIEW_FILE_EDIT_SECRET_4b91"
                        }
                    }
                ],
                "receipt_path": null,
                "verify": {
                    "directly_usable": false,
                    "failure_stage": "register",
                    "failure_reason": "MANUAL_REGISTRATION_REQUIRED"
                }
            })),
        })
        .unwrap();
        let value = serde_json::to_value(bundle).unwrap();
        let report = &value["components"]["install_execution_report"]["data"];
        let encoded = serde_json::to_string(report).unwrap();

        assert_eq!(report["next_steps"][0]["command"][0], "dcc-mcp-cli");
        assert_eq!(report["next_steps"][0]["command"][1], "doctor");
        assert_eq!(report["next_steps"][0]["command"][2], "[auth-redacted]");
        assert_eq!(report["next_steps"][0]["command"][3], "[auth-redacted]");
        assert_eq!(report["next_steps"][0]["command"][4], "[path-redacted]");
        assert_eq!(report["next_steps"][0]["command"][5], "[url-redacted]");
        assert_eq!(report["next_steps"][0]["command"][6], "[url-redacted]");
        assert_eq!(report["next_steps"][0]["command"][7], "[auth-redacted]");
        assert_eq!(report["next_steps"][0]["command"][8], "[url-redacted]");
        assert_eq!(
            report["next_steps"][1]["file_edit"]["path"],
            "[path-redacted]"
        );
        assert_eq!(report["next_steps"][1]["file_edit"]["action"], "update");
        assert!(
            report["next_steps"][1]["file_edit"]
                .get("content")
                .is_none()
        );
        for private in [
            "--token",
            "REVIEW_SECRET_7f0a",
            "private/reports/install-result.json",
            "mailto:private-contact@example.invalid",
            "custom+report:private-location",
            "--api-key=INLINE_REVIEW_SECRET_d820",
            "contact(mailto:embedded-private@example.invalid)",
            "private/config/registration.json",
            "REVIEW_FILE_EDIT_SECRET_4b91",
        ] {
            assert!(!encoded.contains(private), "bundle leaked {private}");
        }
    }

    #[test]
    fn issue_report_must_match_request_and_public_safe_mode() {
        let mut value = finding(FindingRedactionMode::PublicSafe);
        value.evidence.request_id = Some("request-42".into());
        let input = |issue_report| FeedbackBundleInput {
            finding: value.clone(),
            doctor: json!({"status": "ok"}),
            issue_report,
            host_errors: HostErrorTail::not_available("dcc_pid_not_available"),
            install_execution_report: None,
        };

        let raw = assemble_public_bundle(input(Some(json!({
            "schema_version": "dcc-mcp.admin.issue-report.v1",
            "report_type": "github_issue_debug_json",
            "privacy_mode": "raw-local-evidence",
            "request_id": "request-42",
            "debug_bundle": {}
        }))));
        assert_eq!(
            raw.unwrap_err(),
            super::FeedbackBundleError::UnsafeIssueReport
        );

        let mismatch = assemble_public_bundle(input(Some(json!({
            "schema_version": "dcc-mcp.admin.issue-report.v1",
            "report_type": "github_issue_public_safe",
            "privacy_mode": "public-safe",
            "request_id": "another-request"
        }))));
        assert_eq!(
            mismatch.unwrap_err(),
            super::FeedbackBundleError::IssueReportRequestMismatch
        );
    }
}
