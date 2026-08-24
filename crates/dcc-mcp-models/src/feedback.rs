//! Gateway-level agent feedback contract.

use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use thiserror::Error;

/// Maximum lengths for bounded, in-band feedback fields.
const MAX_TOOL_NAME_CHARS: usize = 256;
const MAX_TEXT_CHARS: usize = 4_096;
const MAX_ID_CHARS: usize = 256;
const MAX_EVIDENCE_BYTES: usize = 32 * 1024;
const MAX_REPRO_ITEMS: usize = 64;

/// Finding schema version shared by Rust, Python, evals, and issue bodies.
pub const FINDING_V1_SCHEMA_VERSION: u8 = 1;

/// Canonical JSON Schema shipped in the Python package and embedded for Rust users.
pub const FINDING_V1_JSON_SCHEMA: &str =
    include_str!("../../../python/dcc_mcp_core/schemas/feedback-finding-v1.schema.json");

/// Phase in which a finding was observed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FindingPhase {
    Install,
    Startup,
    Dispatch,
    Skill,
    Other,
}

impl fmt::Display for FindingPhase {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Install => "install",
            Self::Startup => "startup",
            Self::Dispatch => "dispatch",
            Self::Skill => "skill",
            Self::Other => "other",
        })
    }
}

/// Severity vocabulary used by the versioned finding contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FindingSeverity {
    Blocker,
    Degraded,
    WorkaroundFound,
    Suggestion,
}

impl fmt::Display for FindingSeverity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Blocker => "blocker",
            Self::Degraded => "degraded",
            Self::WorkaroundFound => "workaround_found",
            Self::Suggestion => "suggestion",
        })
    }
}

/// Exactly one executable argv or ordered human-readable step list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct FindingReproV1 {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub argv: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub steps: Vec<String>,
}

/// Bounded correlation metadata; future evidence-bundle references may use `extra`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct FindingEvidenceV1 {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub job_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instance_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// Review state of potentially shareable finding evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FindingRedactionMode {
    NeedsReview,
    PublicSafe,
    RawLocalEvidence,
}

/// Explicit redaction status carried with every finding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FindingRedactionStatusV1 {
    pub mode: FindingRedactionMode,
    pub redaction_markers_detected: bool,
    #[serde(default)]
    pub raw_payloads_excluded: bool,
    #[serde(default)]
    pub prompts_excluded: bool,
    #[serde(default)]
    pub scripts_excluded: bool,
    #[serde(default)]
    pub auth_material_excluded: bool,
    #[serde(default)]
    pub local_urls_excluded: bool,
    #[serde(default)]
    pub absolute_paths_excluded: bool,
    #[serde(default)]
    pub private_identifiers_excluded: bool,
}

impl FindingRedactionStatusV1 {
    /// Conservative default for unsanitized agent-authored observations.
    pub fn needs_review(redaction_markers_detected: bool) -> Self {
        Self {
            mode: FindingRedactionMode::NeedsReview,
            redaction_markers_detected,
            raw_payloads_excluded: false,
            prompts_excluded: false,
            scripts_excluded: false,
            auth_material_excluded: false,
            local_urls_excluded: false,
            absolute_paths_excluded: false,
            private_identifiers_excluded: false,
        }
    }
}

/// Versioned finding shared across gateway feedback, evals, and issue bodies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FindingV1 {
    pub schema_version: u8,
    pub fingerprint: String,
    pub dcc_type: String,
    pub adapter: String,
    pub adapter_version: String,
    pub core_version: String,
    pub host_version: String,
    pub os: String,
    pub phase: FindingPhase,
    pub severity: FindingSeverity,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_slug: Option<String>,
    pub intent: String,
    pub observed: String,
    pub expected: String,
    pub repro: FindingReproV1,
    pub evidence: FindingEvidenceV1,
    pub redaction_status: FindingRedactionStatusV1,
}

impl FindingV1 {
    /// Validate schema identity, bounded fields, reproduction shape, and evidence size.
    pub fn validate(&self) -> Result<(), FeedbackValidationError> {
        if self.schema_version != FINDING_V1_SCHEMA_VERSION {
            return Err(FeedbackValidationError::UnsupportedSchemaVersion {
                received: self.schema_version,
            });
        }
        if !valid_fingerprint(&self.fingerprint) {
            return Err(FeedbackValidationError::InvalidFingerprint);
        }
        for (field, value) in [
            ("dcc_type", self.dcc_type.as_str()),
            ("adapter", self.adapter.as_str()),
            ("adapter_version", self.adapter_version.as_str()),
            ("core_version", self.core_version.as_str()),
            ("host_version", self.host_version.as_str()),
            ("os", self.os.as_str()),
        ] {
            validate_required(field, value, MAX_ID_CHARS)?;
        }
        validate_optional("tool_slug", self.tool_slug.as_deref(), MAX_TOOL_NAME_CHARS)?;
        validate_required("intent", &self.intent, MAX_TEXT_CHARS)?;
        validate_required("observed", &self.observed, MAX_TEXT_CHARS)?;
        validate_required("expected", &self.expected, MAX_TEXT_CHARS)?;
        self.repro.validate()?;
        self.evidence.validate()?;
        if self.tool_slug.is_none() && self.evidence.error_kind.is_none() {
            return Err(FeedbackValidationError::MissingFingerprintSubject);
        }
        Ok(())
    }
}

impl FindingReproV1 {
    fn validate(&self) -> Result<(), FeedbackValidationError> {
        if self.argv.is_empty() == self.steps.is_empty()
            || self.argv.len() > MAX_REPRO_ITEMS
            || self.steps.len() > MAX_REPRO_ITEMS
        {
            return Err(FeedbackValidationError::InvalidRepro);
        }
        for value in self.argv.iter().chain(&self.steps) {
            validate_required("repro item", value, MAX_TEXT_CHARS)?;
        }
        Ok(())
    }
}

impl FindingEvidenceV1 {
    fn validate(&self) -> Result<(), FeedbackValidationError> {
        for (field, value) in [
            ("request_id", self.request_id.as_deref()),
            ("job_id", self.job_id.as_deref()),
            ("instance_id", self.instance_id.as_deref()),
            ("error_kind", self.error_kind.as_deref()),
            ("run_id", self.run_id.as_deref()),
        ] {
            validate_optional(field, value, MAX_ID_CHARS)?;
        }
        let bytes =
            serde_json::to_vec(self).map_err(|_| FeedbackValidationError::EvidenceTooLarge)?;
        if bytes.len() > MAX_EVIDENCE_BYTES {
            return Err(FeedbackValidationError::EvidenceTooLarge);
        }
        Ok(())
    }
}

/// Return the stable SHA-256 fingerprint for one routed finding context.
pub fn finding_fingerprint(
    owning_repo: &str,
    phase: FindingPhase,
    tool_slug: Option<&str>,
    error_kind: Option<&str>,
    host_version: &str,
) -> Result<String, FeedbackValidationError> {
    let owner = normalize_owner(owning_repo);
    if owner.is_empty() {
        return Err(FeedbackValidationError::Empty {
            field: "owning_repo",
        });
    }
    let subject = tool_slug
        .filter(|value| !value.trim().is_empty())
        .or_else(|| error_kind.filter(|value| !value.trim().is_empty()))
        .ok_or(FeedbackValidationError::MissingFingerprintSubject)?
        .trim()
        .to_ascii_lowercase();
    let host_major = host_version
        .split(|character: char| !character.is_ascii_digit())
        .find(|part| !part.is_empty())
        .unwrap_or("unknown");
    let basis = format!("finding-v1\n{owner}\n{phase}\n{subject}\n{host_major}");
    let digest = Sha256::digest(basis.as_bytes());
    let encoded = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    Ok(format!("sha256:{encoded}"))
}

fn normalize_owner(value: &str) -> String {
    let mut owner = value.trim().replace('\\', "/").to_ascii_lowercase();
    for prefix in ["https://github.com/", "http://github.com/", "github.com/"] {
        if let Some(stripped) = owner.strip_prefix(prefix) {
            owner = stripped.to_string();
            break;
        }
    }
    owner = owner.trim_end_matches('/').to_string();
    if let Some(stripped) = owner.strip_suffix(".git") {
        owner = stripped.to_string();
    }
    owner.trim_end_matches('/').to_string()
}

fn valid_fingerprint(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|digest| {
        digest.len() == 64
            && digest
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    })
}

/// Severity of an agent feedback report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FeedbackSeverity {
    Blocked,
    WorkaroundFound,
    Suggestion,
}

impl fmt::Display for FeedbackSeverity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Blocked => "blocked",
            Self::WorkaroundFound => "workaround_found",
            Self::Suggestion => "suggestion",
        })
    }
}

impl FromStr for FeedbackSeverity {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().replace('-', "_").as_str() {
            "blocked" => Ok(Self::Blocked),
            "workaround_found" => Ok(Self::WorkaroundFound),
            "suggestion" => Ok(Self::Suggestion),
            _ => Err("severity must be one of: blocked, workaround_found, suggestion".to_string()),
        }
    }
}

/// Feedback that can be filed against the gateway even when no DCC is live.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeedbackReport {
    pub tool_name: String,
    pub intent: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attempt: Option<String>,
    pub blocker: String,
    pub severity: FeedbackSeverity,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dcc_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instance_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub job_id: Option<String>,
}

impl FeedbackReport {
    /// Validate required values and keep the bounded gateway event ring safe.
    pub fn validate(&self) -> Result<(), FeedbackValidationError> {
        validate_required("tool_name", &self.tool_name, MAX_TOOL_NAME_CHARS)?;
        validate_required("intent", &self.intent, MAX_TEXT_CHARS)?;
        validate_optional("attempt", self.attempt.as_deref(), MAX_TEXT_CHARS)?;
        validate_required("blocker", &self.blocker, MAX_TEXT_CHARS)?;
        validate_optional("dcc_type", self.dcc_type.as_deref(), MAX_ID_CHARS)?;
        validate_optional("instance_id", self.instance_id.as_deref(), MAX_ID_CHARS)?;
        validate_optional("request_id", self.request_id.as_deref(), MAX_ID_CHARS)?;
        validate_optional("job_id", self.job_id.as_deref(), MAX_ID_CHARS)?;
        Ok(())
    }
}

/// Invalid gateway feedback input.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum FeedbackValidationError {
    #[error("{field} must not be empty")]
    Empty { field: &'static str },
    #[error("{field} exceeds the {max_chars}-character limit")]
    TooLong {
        field: &'static str,
        max_chars: usize,
    },
    #[error("finding schema version {received} is unsupported")]
    UnsupportedSchemaVersion { received: u8 },
    #[error("fingerprint must be sha256 followed by 64 lowercase hex characters")]
    InvalidFingerprint,
    #[error("repro must contain exactly one non-empty argv or steps list with at most 64 items")]
    InvalidRepro,
    #[error("tool_slug or evidence.error_kind is required for fingerprinting")]
    MissingFingerprintSubject,
    #[error("evidence exceeds the 32768-byte limit")]
    EvidenceTooLarge,
}

fn validate_required(
    field: &'static str,
    value: &str,
    max_chars: usize,
) -> Result<(), FeedbackValidationError> {
    if value.trim().is_empty() {
        return Err(FeedbackValidationError::Empty { field });
    }
    validate_length(field, value, max_chars)
}

fn validate_optional(
    field: &'static str,
    value: Option<&str>,
    max_chars: usize,
) -> Result<(), FeedbackValidationError> {
    match value {
        Some(value) if value.trim().is_empty() => Err(FeedbackValidationError::Empty { field }),
        Some(value) => validate_length(field, value, max_chars),
        None => Ok(()),
    }
}

fn validate_length(
    field: &'static str,
    value: &str,
    max_chars: usize,
) -> Result<(), FeedbackValidationError> {
    if value.chars().count() > max_chars {
        return Err(FeedbackValidationError::TooLong { field, max_chars });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn report() -> FeedbackReport {
        FeedbackReport {
            tool_name: "houdini.ui_control__act".to_string(),
            intent: "Open the render menu".to_string(),
            attempt: Some("Invoked the menu action".to_string()),
            blocker: "The owning process exited".to_string(),
            severity: FeedbackSeverity::Blocked,
            dcc_type: Some("houdini".to_string()),
            instance_id: Some("abc12345".to_string()),
            request_id: Some("request-42".to_string()),
            job_id: None,
        }
    }

    #[test]
    fn report_accepts_dead_instance_correlation() {
        assert_eq!(report().validate(), Ok(()));
    }

    #[test]
    fn report_rejects_empty_required_fields() {
        let mut value = report();
        value.blocker = "  ".to_string();
        assert_eq!(
            value.validate(),
            Err(FeedbackValidationError::Empty { field: "blocker" })
        );
    }

    #[test]
    fn report_rejects_empty_optional_correlation() {
        let mut value = report();
        value.request_id = Some("  ".to_string());
        assert_eq!(
            value.validate(),
            Err(FeedbackValidationError::Empty {
                field: "request_id"
            })
        );
    }

    #[test]
    fn severity_parses_cli_spellings() {
        assert_eq!(
            "workaround-found".parse::<FeedbackSeverity>().unwrap(),
            FeedbackSeverity::WorkaroundFound
        );
        assert_eq!(
            serde_json::to_value(FeedbackSeverity::Suggestion).unwrap(),
            serde_json::json!("suggestion")
        );
    }

    fn finding() -> FindingV1 {
        FindingV1 {
            schema_version: FINDING_V1_SCHEMA_VERSION,
            fingerprint: finding_fingerprint(
                "https://github.com/dcc-mcp/dcc-mcp-houdini",
                FindingPhase::Dispatch,
                Some("houdini.ui_control__act"),
                None,
                "20.5.332",
            )
            .unwrap(),
            dcc_type: "houdini".to_string(),
            adapter: "dcc-mcp-houdini".to_string(),
            adapter_version: "0.31.5".to_string(),
            core_version: "0.20.11".to_string(),
            host_version: "20.5.332".to_string(),
            os: "windows".to_string(),
            phase: FindingPhase::Dispatch,
            severity: FindingSeverity::Blocker,
            tool_slug: Some("houdini.ui_control__act".to_string()),
            intent: "Open the render menu".to_string(),
            observed: "The owning process exited".to_string(),
            expected: "The menu opens on the bound Houdini instance".to_string(),
            repro: FindingReproV1 {
                argv: vec![],
                steps: vec!["Call the semantic menu action".to_string()],
            },
            evidence: FindingEvidenceV1 {
                request_id: Some("request-42".to_string()),
                job_id: None,
                instance_id: Some("houdini-instance-1".to_string()),
                error_kind: Some("instance_exited".to_string()),
                run_id: Some("eval-run-42".to_string()),
                extra: BTreeMap::new(),
            },
            redaction_status: FindingRedactionStatusV1::needs_review(false),
        }
    }

    #[test]
    fn finding_v1_round_trips_the_versioned_contract() {
        let value = finding();
        assert_eq!(value.validate(), Ok(()));
        let encoded = serde_json::to_value(&value).unwrap();
        assert_eq!(encoded["schema_version"], FINDING_V1_SCHEMA_VERSION);
        assert_eq!(encoded["phase"], "dispatch");
        assert_eq!(encoded["severity"], "blocker");
        assert_eq!(
            encoded["repro"]["steps"][0],
            "Call the semantic menu action"
        );
        assert_eq!(encoded["redaction_status"]["mode"], "needs-review");
        assert_eq!(encoded["redaction_status"]["raw_payloads_excluded"], false);
        assert_eq!(serde_json::from_value::<FindingV1>(encoded).unwrap(), value);
    }

    #[test]
    fn finding_fingerprint_is_stable_and_owner_sensitive() {
        let first = finding_fingerprint(
            "https://github.com/dcc-mcp/dcc-mcp-photoshop.git",
            FindingPhase::Skill,
            Some("photoshop_layers__merge"),
            None,
            "26.4.1",
        )
        .unwrap();
        let equivalent = finding_fingerprint(
            "dcc-mcp/dcc-mcp-photoshop/",
            FindingPhase::Skill,
            Some("PHOTOSHOP_LAYERS__MERGE"),
            None,
            "26.4",
        )
        .unwrap();
        let other_owner = finding_fingerprint(
            "studio/custom-photoshop-adapter",
            FindingPhase::Skill,
            Some("photoshop_layers__merge"),
            None,
            "26.4.1",
        )
        .unwrap();

        assert_eq!(first, equivalent);
        assert_eq!(
            first,
            "sha256:0f7545056e6e3ee9387804a6276669a21c723f8a422b59477e3587be6a8997ac"
        );
        assert!(first.starts_with("sha256:"));
        assert_eq!(first.len(), 71);
        assert_ne!(first, other_owner);
    }

    #[test]
    fn finding_v1_requires_exactly_one_bounded_repro_shape() {
        let mut value = finding();
        value.repro.argv = vec!["dcc-mcp-cli".to_string()];
        assert_eq!(value.validate(), Err(FeedbackValidationError::InvalidRepro));
    }

    #[test]
    fn finding_v1_schema_is_the_shared_machine_contract() {
        let schema: serde_json::Value = serde_json::from_str(FINDING_V1_JSON_SCHEMA).unwrap();
        assert_eq!(schema["properties"]["schema_version"]["const"], 1);
        assert_eq!(
            schema["properties"]["severity"]["enum"],
            serde_json::json!(["blocker", "degraded", "workaround_found", "suggestion"])
        );
        assert!(
            schema["required"]
                .as_array()
                .unwrap()
                .iter()
                .any(|field| field == "fingerprint")
        );
    }
}
