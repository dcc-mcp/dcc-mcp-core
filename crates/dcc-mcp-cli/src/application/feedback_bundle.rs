use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use dcc_mcp_models::FindingV1;
use serde_json::Value;
use thiserror::Error;

use crate::application::control_plane::DccControlPlane;
use crate::application::doctor::{DoctorRequest, run_doctor};
use crate::application::feedback::{FeedbackRouteServiceError, read_finding};
use crate::application::install::InstallExecutionReport;
use crate::domain::feedback_bundle::{
    FeedbackBundle, FeedbackBundleError, FeedbackBundleInput, HostErrorTail, MAX_HOST_ERROR_BYTES,
    MAX_HOST_ERROR_LINES, assemble_public_bundle, project_public_host_event,
    validate_public_finding,
};

const HOST_ERROR_PREFIX: &str = "dcc_mcp_core.host_errors: ";
const MAX_INSTALL_REPORT_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone)]
pub struct FeedbackBundleRequest {
    pub finding_path: std::path::PathBuf,
    pub install_report_path: Option<std::path::PathBuf>,
    pub doctor_request: DoctorRequest,
    pub log_dir: Option<std::path::PathBuf>,
    pub dcc_pid: Option<u32>,
    pub host_error_lines: usize,
}

#[derive(Debug, Default)]
pub struct FeedbackBundleService;

impl FeedbackBundleService {
    pub async fn bundle(
        &self,
        request: FeedbackBundleRequest,
        control: &DccControlPlane,
    ) -> Result<FeedbackBundle, FeedbackBundleServiceError> {
        let finding = read_finding(&request.finding_path)?;
        validate_public_finding(&finding)?;
        let install_execution_report = request
            .install_report_path
            .as_deref()
            .map(|path| read_install_execution_report(path, &finding))
            .transpose()?;
        let doctor = run_doctor(request.doctor_request)
            .await
            .map_err(|error| FeedbackBundleServiceError::Doctor(error.to_string()))?;
        let issue_report = if let Some(request_id) = finding.evidence.request_id.as_deref() {
            control.issue_report(request_id).await.ok()
        } else {
            None
        };
        let host_errors = match resolve_dcc_pid(&finding, request.dcc_pid) {
            Some(dcc_pid) => match resolve_log_dir(request.log_dir) {
                Some(log_dir) => read_host_error_tail(
                    &log_dir,
                    &finding.dcc_type,
                    dcc_pid,
                    request.host_error_lines,
                ),
                None => HostErrorTail::not_available("host_error_log_directory_not_available"),
            },
            None => HostErrorTail::not_available("dcc_pid_not_available"),
        };
        assemble_public_bundle(FeedbackBundleInput {
            finding,
            doctor,
            issue_report,
            host_errors,
            install_execution_report,
        })
        .map_err(Into::into)
    }
}

#[derive(Debug, Error)]
pub enum FeedbackBundleServiceError {
    #[error(transparent)]
    Finding(#[from] FeedbackRouteServiceError),
    #[error("doctor snapshot failed: {0}")]
    Doctor(String),
    #[error(transparent)]
    Bundle(#[from] FeedbackBundleError),
    #[error("install execution report could not be read")]
    InstallReportRead,
    #[error("install execution report is not a regular file")]
    InstallReportNotRegular,
    #[error("install execution report exceeds the {MAX_INSTALL_REPORT_BYTES}-byte limit")]
    InstallReportTooLarge,
    #[error("install execution report is invalid")]
    InstallReportInvalid,
    #[error("install execution report is not terminal")]
    InstallReportNotTerminal,
    #[error("install execution report does not match the finding")]
    InstallReportMismatch,
}

fn read_install_execution_report(
    path: &Path,
    finding: &FindingV1,
) -> Result<Value, FeedbackBundleServiceError> {
    let metadata =
        fs::symlink_metadata(path).map_err(|_| FeedbackBundleServiceError::InstallReportRead)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(FeedbackBundleServiceError::InstallReportNotRegular);
    }
    if metadata.len() > MAX_INSTALL_REPORT_BYTES as u64 {
        return Err(FeedbackBundleServiceError::InstallReportTooLarge);
    }

    let file = File::open(path).map_err(|_| FeedbackBundleServiceError::InstallReportRead)?;
    let opened_metadata = file
        .metadata()
        .map_err(|_| FeedbackBundleServiceError::InstallReportRead)?;
    let current_metadata =
        fs::symlink_metadata(path).map_err(|_| FeedbackBundleServiceError::InstallReportRead)?;
    if !opened_metadata.is_file()
        || current_metadata.file_type().is_symlink()
        || !current_metadata.is_file()
    {
        return Err(FeedbackBundleServiceError::InstallReportNotRegular);
    }
    if opened_metadata.len() > MAX_INSTALL_REPORT_BYTES as u64
        || current_metadata.len() > MAX_INSTALL_REPORT_BYTES as u64
    {
        return Err(FeedbackBundleServiceError::InstallReportTooLarge);
    }

    let mut bytes = Vec::with_capacity(opened_metadata.len() as usize);
    file.take((MAX_INSTALL_REPORT_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| FeedbackBundleServiceError::InstallReportRead)?;
    if bytes.len() > MAX_INSTALL_REPORT_BYTES {
        return Err(FeedbackBundleServiceError::InstallReportTooLarge);
    }
    let report: InstallExecutionReport = serde_json::from_slice(&bytes)
        .map_err(|_| FeedbackBundleServiceError::InstallReportInvalid)?;
    if report.schema_version != 1 || !install_execution_report_contract_is_valid(&report) {
        return Err(FeedbackBundleServiceError::InstallReportInvalid);
    }
    if !matches!(
        report.status.as_str(),
        "ok" | "failed" | "partial" | "requires_restart"
    ) {
        return Err(FeedbackBundleServiceError::InstallReportNotTerminal);
    }
    if report.dcc_type != finding.dcc_type
        || report.core_version != finding.core_version
        || report.adapter_version != finding.adapter_version
    {
        return Err(FeedbackBundleServiceError::InstallReportMismatch);
    }
    serde_json::to_value(report).map_err(|_| FeedbackBundleServiceError::InstallReportInvalid)
}

fn install_execution_report_contract_is_valid(report: &InstallExecutionReport) -> bool {
    let present = |value: &str| value.chars().any(|character| !character.is_whitespace());
    present(&report.dcc_type)
        && present(&report.adapter_version)
        && present(&report.core_version)
        && present(&report.stage)
        && report.steps.iter().all(|step| {
            present(&step.id) && present(&step.status) && present(&step.rollback.status)
        })
        && present(&report.rollback.status)
        && report.next_steps.iter().all(|step| {
            present(&step.id)
                && present(&step.description)
                && present(&step.why)
                && !step.command.is_empty()
                && step.command.iter().all(|argument| present(argument))
        })
        && report.error.as_ref().is_none_or(|error| {
            present(&error.code)
                && present(&error.stage)
                && error.primary_code.as_deref().is_none_or(present)
        })
}

fn resolve_log_dir(explicit: Option<std::path::PathBuf>) -> Option<std::path::PathBuf> {
    explicit
        .or_else(|| std::env::var_os("DCC_MCP_LOG_DIR").map(std::path::PathBuf::from))
        .or_else(|| dirs::data_local_dir().map(|base| base.join("dcc-mcp").join("log")))
}

#[must_use]
pub fn resolve_dcc_pid(finding: &FindingV1, override_pid: Option<u32>) -> Option<u32> {
    override_pid.or_else(|| {
        finding
            .evidence
            .extra
            .get("dcc_pid")
            .and_then(Value::as_u64)
            .and_then(|value| u32::try_from(value).ok())
            .filter(|value| *value > 0)
    })
}

#[must_use]
pub fn read_host_error_tail(
    log_dir: &Path,
    dcc_type: &str,
    dcc_pid: u32,
    max_lines: usize,
) -> HostErrorTail {
    let max_lines = max_lines.clamp(1, MAX_HOST_ERROR_LINES);
    let file_name = format!(
        "dcc-mcp-{}.{dcc_pid}.host-errors.log",
        safe_dcc_name(dcc_type)
    );
    let path = log_dir.join(file_name);
    let Ok(metadata) = fs::symlink_metadata(&path) else {
        return HostErrorTail::not_available("host_error_log_not_found");
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return HostErrorTail::not_available("host_error_log_not_regular_file");
    }
    if !path_is_bounded_to_directory(&path, log_dir) {
        return HostErrorTail::not_available("host_error_log_outside_log_directory");
    }
    match read_bounded_tail(&path, metadata.len(), max_lines, dcc_type, dcc_pid) {
        Ok(tail) => tail,
        Err(_) => HostErrorTail::not_available("host_error_log_read_failed"),
    }
}

fn read_bounded_tail(
    path: &Path,
    file_len: u64,
    max_lines: usize,
    expected_dcc_type: &str,
    expected_dcc_pid: u32,
) -> std::io::Result<HostErrorTail> {
    let mut file = File::open(path)?;
    if !file.metadata()?.is_file() {
        return Ok(HostErrorTail::not_available(
            "host_error_log_not_regular_file",
        ));
    }
    let start = file_len.saturating_sub(MAX_HOST_ERROR_BYTES as u64);
    file.seek(SeekFrom::Start(start))?;
    let mut bytes = Vec::with_capacity((file_len - start) as usize);
    file.take(MAX_HOST_ERROR_BYTES as u64)
        .read_to_end(&mut bytes)?;
    if start > 0 {
        if let Some(newline) = bytes.iter().position(|byte| *byte == b'\n') {
            bytes.drain(..=newline);
        } else {
            bytes.clear();
        }
    }

    let text = String::from_utf8_lossy(&bytes);
    let lines = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect::<Vec<_>>();
    let truncated = start > 0 || lines.len() > max_lines;
    let mut skipped_invalid = 0;
    let mut records = Vec::new();
    for line in lines.iter().skip(lines.len().saturating_sub(max_lines)) {
        let Some(payload) = line
            .split_once(HOST_ERROR_PREFIX)
            .map(|(_, payload)| payload)
        else {
            skipped_invalid += 1;
            continue;
        };
        let Ok(event) = serde_json::from_str::<Value>(payload) else {
            skipped_invalid += 1;
            continue;
        };
        if !host_event_matches(&event, expected_dcc_type, expected_dcc_pid) {
            skipped_invalid += 1;
            continue;
        }
        let Some(projected) = project_public_host_event(&event) else {
            skipped_invalid += 1;
            continue;
        };
        records.push(projected);
    }
    if records.is_empty() && skipped_invalid > 0 {
        return Ok(HostErrorTail::not_available(
            "host_error_log_has_no_valid_records",
        ));
    }
    Ok(HostErrorTail::included_bounded(
        records,
        truncated,
        max_lines,
        skipped_invalid,
    ))
}

fn host_event_matches(event: &Value, dcc_type: &str, dcc_pid: u32) -> bool {
    event.get("event").and_then(Value::as_str) == Some("dcc_host_error")
        && event.get("dcc_type").and_then(Value::as_str) == Some(dcc_type)
        && event.get("dcc_pid").and_then(Value::as_u64) == Some(u64::from(dcc_pid))
}

fn path_is_bounded_to_directory(path: &Path, directory: &Path) -> bool {
    let Ok(path) = path.canonicalize() else {
        return false;
    };
    let Ok(directory) = directory.canonicalize() else {
        return false;
    };
    path.parent() == Some(directory.as_path())
}

fn safe_dcc_name(dcc_type: &str) -> String {
    let normalized = dcc_type
        .trim()
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '_' | '.' | '-') {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();
    let normalized = normalized.trim_matches(['.', '_', '-']);
    if normalized.is_empty() {
        "dcc".to_string()
    } else {
        normalized.to_string()
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use dcc_mcp_models::{
        FINDING_V1_SCHEMA_VERSION, FindingEvidenceV1, FindingPhase, FindingRedactionStatusV1,
        FindingReproV1, FindingSeverity, FindingV1,
    };
    use serde_json::{Value, json};

    use super::{
        MAX_INSTALL_REPORT_BYTES, read_host_error_tail, read_install_execution_report,
        resolve_dcc_pid,
    };

    fn finding(extra: Value) -> FindingV1 {
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
            intent: "Start".into(),
            observed: "Failed".into(),
            expected: "Ready".into(),
            repro: FindingReproV1 {
                argv: vec!["dcc-mcp-cli".into(), "status".into()],
                steps: Vec::new(),
            },
            evidence: FindingEvidenceV1 {
                error_kind: Some("startup_failed".into()),
                extra: extra
                    .as_object()
                    .cloned()
                    .unwrap_or_default()
                    .into_iter()
                    .collect(),
                ..FindingEvidenceV1::default()
            },
            redaction_status: FindingRedactionStatusV1::needs_review(false),
        }
    }

    #[test]
    fn pid_override_wins_and_finding_extra_is_the_fallback() {
        let value = finding(json!({"dcc_pid": 4321}));

        assert_eq!(resolve_dcc_pid(&value, Some(9876)), Some(9876));
        assert_eq!(resolve_dcc_pid(&value, None), Some(4321));
        assert_eq!(resolve_dcc_pid(&finding(json!({"dcc_pid": 0})), None), None);
        assert_eq!(
            resolve_dcc_pid(&finding(json!({"dcc_pid": "not-a-pid"})), None),
            None
        );
    }

    #[test]
    fn host_error_tail_is_bounded_and_projects_only_public_fields() {
        let temp = tempfile::tempdir().unwrap();
        let log = temp.path().join("dcc-mcp-godot.4321.host-errors.log");
        let lines = (0..4)
            .map(|index| {
                format!(
                    "2026-08-24T00:00:0{index}Z ERROR dcc_mcp_core.host_errors: {}",
                    json!({
                        "event": "dcc_host_error",
                        "dcc_type": "godot",
                        "dcc_pid": 4321,
                        "phase": "bootstrap",
                        "level": "error",
                        "message": format!("secret path C:\\studio\\{index}.godot"),
                        "traceback": "Bearer private-token",
                        "metadata": {"prompt": "private"},
                        "core_version": "0.20.11",
                        "sequence": index
                    })
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(log, lines).unwrap();

        let tail = read_host_error_tail(temp.path(), "godot", 4321, 2);
        let value = serde_json::to_value(tail.into_component()).unwrap();
        let encoded = serde_json::to_string(&value).unwrap();

        assert_eq!(value["status"], "included");
        assert_eq!(value["data"]["records"].as_array().unwrap().len(), 2);
        assert_eq!(value["data"]["truncated"], true);
        assert_eq!(value["data"]["max_lines"], 2);
        assert_eq!(value["data"]["max_bytes"], 256 * 1024);
        assert_eq!(value["data"]["records"][0]["event"], "dcc_host_error");
        for private in [
            "message",
            "traceback",
            "metadata",
            "studio",
            "private-token",
            "4321",
        ] {
            assert!(!encoded.contains(private), "leaked {private}");
        }
    }

    #[test]
    fn missing_host_error_file_is_reported_without_exposing_its_path() {
        let temp = tempfile::tempdir().unwrap();
        let tail = read_host_error_tail(temp.path(), "godot", 4321, 2);
        let value = serde_json::to_value(tail.into_component()).unwrap();

        assert_eq!(value["status"], "unavailable");
        assert_eq!(value["reason"], "host_error_log_not_found");
        assert!(value.get("data").is_none());
    }

    #[test]
    fn host_error_tail_rejects_records_from_another_process() {
        let temp = tempfile::tempdir().unwrap();
        let log = temp.path().join("dcc-mcp-godot.4321.host-errors.log");
        let record = |pid| {
            format!(
                "2026-08-24T00:00:00Z ERROR dcc_mcp_core.host_errors: {}",
                json!({
                    "event": "dcc_host_error",
                    "dcc_type": "godot",
                    "dcc_pid": pid,
                    "phase": "bootstrap"
                })
            )
        };
        fs::write(log, [record(9999), record(4321)].join("\n")).unwrap();

        let value = serde_json::to_value(
            read_host_error_tail(temp.path(), "godot", 4321, 10).into_component(),
        )
        .unwrap();

        assert_eq!(value["data"]["records"].as_array().unwrap().len(), 1);
        assert_eq!(value["data"]["skipped_invalid"], 1);
    }

    #[test]
    fn install_execution_report_is_terminal_bounded_and_bound_to_the_finding() {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/install-execution-report-v1-failed.json");
        let mut expected = finding(json!({}));
        expected.dcc_type = "maya".into();
        expected.adapter_version = "unknown".into();
        expected.core_version = "0.0.0-test".into();

        let report = read_install_execution_report(&fixture, &expected).unwrap();

        assert_eq!(report["schema_version"], 1);
        assert_eq!(report["status"], "failed");
        assert_eq!(report["dcc_type"], "maya");
    }

    #[test]
    fn install_execution_report_rejects_non_terminal_and_cross_finding_evidence() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("install-report.json");
        let mut report: Value = serde_json::from_str(include_str!(
            "../../../../tests/fixtures/install-execution-report-v1-failed.json"
        ))
        .unwrap();
        let mut expected = finding(json!({}));
        expected.dcc_type = "maya".into();
        expected.adapter_version = "unknown".into();
        expected.core_version = "0.0.0-test".into();

        report["status"] = json!("running");
        fs::write(&path, serde_json::to_vec(&report).unwrap()).unwrap();
        assert_eq!(
            read_install_execution_report(&path, &expected)
                .unwrap_err()
                .to_string(),
            "install execution report is not terminal"
        );

        report["status"] = json!("failed");
        report["dcc_type"] = json!("houdini");
        fs::write(&path, serde_json::to_vec(&report).unwrap()).unwrap();
        assert_eq!(
            read_install_execution_report(&path, &expected)
                .unwrap_err()
                .to_string(),
            "install execution report does not match the finding"
        );

        report["dcc_type"] = json!("maya");
        report["adapter_version"] = json!("different-version");
        fs::write(&path, serde_json::to_vec(&report).unwrap()).unwrap();
        assert_eq!(
            read_install_execution_report(&path, &expected)
                .unwrap_err()
                .to_string(),
            "install execution report does not match the finding"
        );
    }

    #[test]
    fn install_execution_report_rejects_unsafe_file_shapes_before_parsing() {
        let temp = tempfile::tempdir().unwrap();
        let mut expected = finding(json!({}));
        expected.dcc_type = "maya".into();
        expected.adapter_version = "unknown".into();
        expected.core_version = "0.0.0-test".into();

        assert_eq!(
            read_install_execution_report(temp.path(), &expected)
                .unwrap_err()
                .to_string(),
            "install execution report is not a regular file"
        );

        let oversized = temp.path().join("oversized.json");
        fs::write(&oversized, vec![b' '; MAX_INSTALL_REPORT_BYTES + 1]).unwrap();
        assert_eq!(
            read_install_execution_report(&oversized, &expected)
                .unwrap_err()
                .to_string(),
            format!("install execution report exceeds the {MAX_INSTALL_REPORT_BYTES}-byte limit")
        );
    }

    #[test]
    fn install_execution_report_drops_unknown_unreviewed_fields() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("install-report.json");
        let mut report: Value = serde_json::from_str(include_str!(
            "../../../../tests/fixtures/install-execution-report-v1-failed.json"
        ))
        .unwrap();
        report["raw_secret"] = json!("do-not-project");
        fs::write(&path, serde_json::to_vec(&report).unwrap()).unwrap();
        let mut expected = finding(json!({}));
        expected.dcc_type = "maya".into();
        expected.adapter_version = "unknown".into();
        expected.core_version = "0.0.0-test".into();

        let safe_report = read_install_execution_report(&path, &expected).unwrap();
        assert!(safe_report.get("raw_secret").is_none());
        assert!(
            !serde_json::to_string(&safe_report)
                .unwrap()
                .contains("do-not-project")
        );
    }

    #[test]
    fn install_execution_report_rejects_schema_invalid_nested_fields() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("install-report.json");
        let mut report: Value = serde_json::from_str(include_str!(
            "../../../../tests/fixtures/install-execution-report-v1-failed.json"
        ))
        .unwrap();
        report["steps"][0]["id"] = json!("");
        fs::write(&path, serde_json::to_vec(&report).unwrap()).unwrap();
        let mut expected = finding(json!({}));
        expected.dcc_type = "maya".into();
        expected.adapter_version = "unknown".into();
        expected.core_version = "0.0.0-test".into();

        assert_eq!(
            read_install_execution_report(&path, &expected)
                .unwrap_err()
                .to_string(),
            "install execution report is invalid"
        );
    }
}
