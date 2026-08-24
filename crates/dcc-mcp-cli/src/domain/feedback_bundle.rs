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
        complete: false,
        finding,
        version_matrix,
        components: FeedbackBundleComponents {
            issue_report,
            doctor: BundleComponent::included(project_public_doctor(&input.doctor)),
            host_errors: input.host_errors.into_component(),
            install_execution_report: BundleComponent::unavailable(
                "install_execution_report_contract_not_available",
            ),
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
    let lower = key.to_ascii_lowercase();
    [
        "token",
        "secret",
        "password",
        "credential",
        "authorization",
        "api_key",
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
    if lower.contains("http://") || lower.contains("https://") {
        return "[url-redacted]".to_string();
    }
    if contains_absolute_path(text) {
        return "[path-redacted]".to_string();
    }
    if ["token=", "secret=", "password=", "bearer ", "api_key="]
        .iter()
        .any(|needle| lower.contains(needle))
    {
        return "[auth-redacted]".to_string();
    }
    text.to_string()
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
    fn bundle_is_incomplete_until_install_execution_report_contract_lands() {
        let bundle = assemble_public_bundle(FeedbackBundleInput {
            finding: finding(FindingRedactionMode::PublicSafe),
            doctor: json!({"status": "ok"}),
            issue_report: None,
            host_errors: HostErrorTail::included(vec![json!({"event": "dcc_host_error"})], false),
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
            "install_execution_report_contract_not_available"
        );
        assert_eq!(value["version_matrix"]["dcc_type"], "godot");
        assert_eq!(value["version_matrix"]["core"], "0.20.11");
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
