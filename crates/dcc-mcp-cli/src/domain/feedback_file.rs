use dcc_mcp_models::{FindingV1, finding_fingerprint};
use serde::Serialize;
use thiserror::Error;

use crate::domain::feedback::FeedbackRoute;
use crate::domain::feedback_bundle::{FeedbackBundleError, validate_public_finding};

pub const FEEDBACK_FILE_SCHEMA_VERSION: &str = "dcc-mcp.feedback-file.v1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeedbackIssueDocument {
    pub title: String,
    pub body: String,
    pub comment_body: String,
    pub search_terms: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FeedbackIssueCandidate {
    pub number: u64,
    pub title: String,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum FeedbackFilingRecommendation {
    CommentExisting { issue_number: u64 },
    Create,
    ReviewRequired,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum FeedbackFileError {
    #[error("invalid finding: {0}")]
    InvalidFinding(String),
    #[error("finding is not public-safe")]
    FindingNeedsReview,
    #[error("finding fingerprint does not match the routed repository")]
    FingerprintRouteMismatch,
}

pub fn build_issue_document(
    finding: &FindingV1,
    route: &FeedbackRoute,
) -> Result<FeedbackIssueDocument, FeedbackFileError> {
    validate_public_finding(finding).map_err(|error| match error {
        FeedbackBundleError::FindingNeedsReview => FeedbackFileError::FindingNeedsReview,
        other => FeedbackFileError::InvalidFinding(other.to_string()),
    })?;
    let expected_fingerprint = finding_fingerprint(
        &route.repo,
        finding.phase,
        finding.tool_slug.as_deref(),
        finding.evidence.error_kind.as_deref(),
        &finding.host_version,
    )
    .map_err(|error| FeedbackFileError::InvalidFinding(error.to_string()))?;
    if finding.fingerprint != expected_fingerprint {
        return Err(FeedbackFileError::FingerprintRouteMismatch);
    }

    let subject = finding
        .tool_slug
        .as_deref()
        .or(finding.evidence.error_kind.as_deref())
        .expect("validated finding has a fingerprint subject");
    let title = format!(
        "agent report: {} {} {}",
        title_component(&finding.dcc_type, 48),
        finding.phase,
        title_component(subject, 96)
    );
    let mut rows = vec![
        ("Fingerprint", finding.fingerprint.as_str()),
        ("Repository", route.repo.as_str()),
        ("DCC", finding.dcc_type.as_str()),
        ("Adapter", finding.adapter.as_str()),
        ("Adapter version", finding.adapter_version.as_str()),
        ("Core version", finding.core_version.as_str()),
        ("Host version", finding.host_version.as_str()),
        ("OS", finding.os.as_str()),
    ];
    let phase = finding.phase.to_string();
    let severity = finding.severity.to_string();
    rows.push(("Phase", &phase));
    rows.push(("Severity", &severity));
    if let Some(tool_slug) = finding.tool_slug.as_deref() {
        rows.push(("Tool slug", tool_slug));
    }
    if let Some(error_kind) = finding.evidence.error_kind.as_deref() {
        rows.push(("Error kind", error_kind));
    }
    if let Some(run_id) = finding.evidence.run_id.as_deref() {
        rows.push(("Run id", run_id));
    }
    let table = rows
        .into_iter()
        .map(|(name, value)| format!("| {name} | <code>{}</code> |", html_escape(value)))
        .collect::<Vec<_>>()
        .join("\n");
    let repro = if finding.repro.argv.is_empty() {
        serde_json::to_string_pretty(&finding.repro.steps)
    } else {
        serde_json::to_string_pretty(&finding.repro.argv)
    }
    .expect("validated finding reproduction serializes");
    let marker = format!("<!-- dcc-mcp-finding:{} -->", finding.fingerprint);
    let body = format!(
        "{marker}\n\n## Agent report\n\n| Field | Value |\n|---|---|\n{table}\n\n## Intent\n\n<pre>{}</pre>\n\n## Observed\n\n<pre>{}</pre>\n\n## Expected\n\n<pre>{}</pre>\n\n## Reproduction\n\n<pre>{}</pre>\n\nThis report contains only reviewed Finding v1 fields; raw evidence and private correlation identifiers are excluded.\n",
        html_escape(&finding.intent),
        html_escape(&finding.observed),
        html_escape(&finding.expected),
        html_escape(&repro),
    );
    let comment_body = format!("## Agent re-observation\n\n{body}");
    let search_terms = vec![
        search_term(&finding.dcc_type),
        finding.phase.to_string(),
        search_term(subject),
    ];
    Ok(FeedbackIssueDocument {
        title,
        body,
        comment_body,
        search_terms,
    })
}

#[must_use]
pub fn recommend_filing(
    exact: &[FeedbackIssueCandidate],
    keyword: &[FeedbackIssueCandidate],
) -> FeedbackFilingRecommendation {
    match exact {
        [issue] => FeedbackFilingRecommendation::CommentExisting {
            issue_number: issue.number,
        },
        [] if keyword.is_empty() => FeedbackFilingRecommendation::Create,
        _ => FeedbackFilingRecommendation::ReviewRequired,
    }
}

fn title_component(value: &str, max_chars: usize) -> String {
    let normalized = value
        .trim()
        .chars()
        .map(|character| {
            if character.is_alphanumeric() || matches!(character, '-' | '_' | '.' | ':' | '/') {
                character
            } else if character.is_whitespace() {
                ' '
            } else {
                '_'
            }
        })
        .collect::<String>();
    normalized
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(max_chars)
        .collect()
}

fn search_term(value: &str) -> String {
    title_component(&value.replace([':', '/'], "_"), 96)
}

fn html_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use dcc_mcp_models::{
        FINDING_V1_SCHEMA_VERSION, FindingEvidenceV1, FindingPhase, FindingRedactionMode,
        FindingRedactionStatusV1, FindingReproV1, FindingSeverity, FindingV1, finding_fingerprint,
    };
    use serde_json::json;

    use crate::domain::feedback::{FeedbackRoute, FeedbackRouteRationale};

    use super::{
        FeedbackFileError, FeedbackFilingRecommendation, FeedbackIssueCandidate,
        build_issue_document, recommend_filing,
    };

    fn route() -> FeedbackRoute {
        FeedbackRoute {
            repo: "dcc-mcp/dcc-mcp-godot".to_string(),
            issues_url: "https://github.com/dcc-mcp/dcc-mcp-godot/issues".to_string(),
            rationale: FeedbackRouteRationale::AdapterPhase,
        }
    }

    fn finding(mode: FindingRedactionMode) -> FindingV1 {
        let route = route();
        let mut extra = BTreeMap::new();
        extra.insert("raw_payload".to_string(), json!("secret-extra"));
        let mut value = FindingV1 {
            schema_version: FINDING_V1_SCHEMA_VERSION,
            fingerprint: String::new(),
            dcc_type: "godot".to_string(),
            adapter: "dcc-mcp-godot".to_string(),
            adapter_version: "0.3.0".to_string(),
            core_version: "0.20.13".to_string(),
            host_version: "4.4.1".to_string(),
            os: "windows".to_string(),
            phase: FindingPhase::Startup,
            severity: FindingSeverity::Blocker,
            tool_slug: None,
            intent: "Start the Godot adapter".to_string(),
            observed: "The bridge did not start".to_string(),
            expected: "The bridge becomes ready".to_string(),
            repro: FindingReproV1 {
                argv: vec!["dcc-mcp-cli".to_string(), "status".to_string()],
                steps: Vec::new(),
            },
            evidence: FindingEvidenceV1 {
                request_id: Some("request-private".to_string()),
                job_id: Some("job-private".to_string()),
                instance_id: Some("instance-private".to_string()),
                error_kind: Some("startup_failed".to_string()),
                run_id: Some("eval-run-42".to_string()),
                extra,
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
        };
        value.fingerprint = finding_fingerprint(
            &route.repo,
            value.phase,
            value.tool_slug.as_deref(),
            value.evidence.error_kind.as_deref(),
            &value.host_version,
        )
        .unwrap();
        value
    }

    fn candidate(number: u64) -> FeedbackIssueCandidate {
        FeedbackIssueCandidate {
            number,
            title: format!("Issue {number}"),
            url: format!("https://github.com/dcc-mcp/dcc-mcp-godot/issues/{number}"),
        }
    }

    #[test]
    fn issue_document_projects_only_reviewed_schema_fields() {
        let finding = finding(FindingRedactionMode::PublicSafe);
        let document = build_issue_document(&finding, &route()).unwrap();

        assert!(document.title.starts_with("agent report: godot startup"));
        assert!(document.body.contains(&finding.fingerprint));
        assert!(document.body.contains("Start the Godot adapter"));
        assert!(document.body.contains("startup_failed"));
        assert!(document.body.contains("eval-run-42"));
        assert!(document.comment_body.contains("Agent re-observation"));
        assert_eq!(
            document.search_terms,
            vec!["godot", "startup", "startup_failed"]
        );
        for private in [
            "raw_payload",
            "secret-extra",
            "request-private",
            "job-private",
            "instance-private",
        ] {
            assert!(!document.body.contains(private), "leaked {private}");
            assert!(!document.comment_body.contains(private), "leaked {private}");
        }
    }

    #[test]
    fn issue_document_requires_public_safe_review() {
        let error = build_issue_document(&finding(FindingRedactionMode::NeedsReview), &route())
            .unwrap_err();

        assert_eq!(error, FeedbackFileError::FindingNeedsReview);
    }

    #[test]
    fn issue_document_binds_fingerprint_to_routed_owner() {
        let mut finding = finding(FindingRedactionMode::PublicSafe);
        finding.fingerprint = format!("sha256:{}", "a".repeat(64));

        let error = build_issue_document(&finding, &route()).unwrap_err();

        assert_eq!(error, FeedbackFileError::FingerprintRouteMismatch);
    }

    #[test]
    fn filing_recommendation_uses_only_one_exact_match_automatically() {
        assert_eq!(
            recommend_filing(&[candidate(42)], &[candidate(99)]),
            FeedbackFilingRecommendation::CommentExisting { issue_number: 42 }
        );
        assert_eq!(
            recommend_filing(&[], &[]),
            FeedbackFilingRecommendation::Create
        );
    }

    #[test]
    fn filing_recommendation_fails_closed_for_ambiguous_matches() {
        assert_eq!(
            recommend_filing(&[candidate(42), candidate(43)], &[]),
            FeedbackFilingRecommendation::ReviewRequired
        );
        assert_eq!(
            recommend_filing(&[], &[candidate(99)]),
            FeedbackFilingRecommendation::ReviewRequired
        );
    }

    #[test]
    fn search_terms_cannot_be_interpreted_as_github_qualifiers() {
        assert_eq!(
            super::search_term("gateway:error/retry"),
            "gateway_error_retry"
        );
    }
}
