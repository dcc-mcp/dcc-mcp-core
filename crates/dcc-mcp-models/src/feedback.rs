//! Gateway-level agent feedback contract.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Maximum lengths for bounded, in-band feedback fields.
const MAX_TOOL_NAME_CHARS: usize = 256;
const MAX_TEXT_CHARS: usize = 4_096;
const MAX_ID_CHARS: usize = 256;

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
}
