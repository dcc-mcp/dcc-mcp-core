use dcc_mcp_catalog::CatalogEntry;
use dcc_mcp_models::{FindingPhase, FindingV1};
use serde::{Deserialize, Serialize};
use thiserror::Error;

const CORE_PACKAGE: &str = "dcc-mcp-core";
const SKILL_ROUTING_EVIDENCE_KEY: &str = "routing";

/// Machine-readable destination for one validated feedback finding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FeedbackRoute {
    pub repo: String,
    pub issues_url: String,
    pub rationale: FeedbackRouteRationale,
}

/// Stable reason code explaining why a route was selected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FeedbackRouteRationale {
    AdapterPhase,
    CoreErrorKind,
    SkillMetadata,
}

/// A finding cannot be routed safely and deterministically.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum FeedbackRouteError {
    #[error("invalid Finding v1 payload: {0}")]
    InvalidFinding(String),
    #[error("catalog package '{package}' was not found")]
    CatalogPackageNotFound { package: String },
    #[error("catalog package '{package}' is duplicated")]
    AmbiguousCatalogPackage { package: String },
    #[error("catalog package '{package}' does not declare issues_url")]
    MissingIssuesUrl { package: String },
    #[error("issue tracker URL is not a canonical public GitHub issues URL: {issues_url}")]
    InvalidIssuesUrl { issues_url: String },
    #[error("skill findings require evidence.routing captured from skill metadata")]
    MissingSkillRouting,
    #[error("skill routing evidence is invalid: {0}")]
    InvalidSkillRouting(String),
    #[error("routing repo '{repo}' does not match issue tracker repo '{issues_repo}'")]
    RepositoryMismatch { repo: String, issues_repo: String },
    #[error("phase=other requires a gateway, CLI, or protocol error_kind")]
    AmbiguousOtherPhase,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SkillRoutingEvidence {
    source: String,
    skill_name: String,
    repo: String,
    issues_url: String,
}

/// Resolve one Finding v1 to its owning GitHub issue tracker.
///
/// Shared gateway, CLI, and protocol error kinds override the phase fallback.
/// Adapter lifecycle phases use an exact catalog package match. Skill findings
/// must carry routing evidence copied from the owning Skill's metadata; this
/// prevents an adapter fallback from silently misrouting standalone packages.
pub fn route_finding(
    finding: &FindingV1,
    catalog: &[CatalogEntry],
) -> Result<FeedbackRoute, FeedbackRouteError> {
    finding
        .validate()
        .map_err(|error| FeedbackRouteError::InvalidFinding(error.to_string()))?;

    if finding
        .evidence
        .error_kind
        .as_deref()
        .is_some_and(is_core_error_kind)
    {
        return route_catalog_package(catalog, CORE_PACKAGE, FeedbackRouteRationale::CoreErrorKind);
    }

    match finding.phase {
        FindingPhase::Install | FindingPhase::Startup | FindingPhase::Dispatch => {
            route_catalog_package(
                catalog,
                &finding.adapter,
                FeedbackRouteRationale::AdapterPhase,
            )
        }
        FindingPhase::Skill => route_skill_metadata(finding),
        FindingPhase::Other => Err(FeedbackRouteError::AmbiguousOtherPhase),
    }
}

fn route_catalog_package(
    catalog: &[CatalogEntry],
    package: &str,
    rationale: FeedbackRouteRationale,
) -> Result<FeedbackRoute, FeedbackRouteError> {
    let matches = catalog
        .iter()
        .filter(|entry| entry.name.eq_ignore_ascii_case(package))
        .collect::<Vec<_>>();
    let entry = match matches.as_slice() {
        [] => {
            return Err(FeedbackRouteError::CatalogPackageNotFound {
                package: package.to_string(),
            });
        }
        [entry] => *entry,
        _ => {
            return Err(FeedbackRouteError::AmbiguousCatalogPackage {
                package: package.to_string(),
            });
        }
    };
    let issues_url =
        entry
            .issues_url
            .as_deref()
            .ok_or_else(|| FeedbackRouteError::MissingIssuesUrl {
                package: entry.name.clone(),
            })?;
    let (repo, issues_url) = canonical_github_issues(issues_url)?;
    Ok(FeedbackRoute {
        repo,
        issues_url,
        rationale,
    })
}

fn route_skill_metadata(finding: &FindingV1) -> Result<FeedbackRoute, FeedbackRouteError> {
    let value = finding
        .evidence
        .extra
        .get(SKILL_ROUTING_EVIDENCE_KEY)
        .ok_or(FeedbackRouteError::MissingSkillRouting)?;
    let routing: SkillRoutingEvidence = serde_json::from_value(value.clone())
        .map_err(|error| FeedbackRouteError::InvalidSkillRouting(error.to_string()))?;
    if routing.source != "skill_metadata" {
        return Err(FeedbackRouteError::InvalidSkillRouting(
            "source must be skill_metadata".to_string(),
        ));
    }
    if routing.skill_name.trim().is_empty() {
        return Err(FeedbackRouteError::InvalidSkillRouting(
            "skill_name must not be empty".to_string(),
        ));
    }
    let (issues_repo, issues_url) = canonical_github_issues(&routing.issues_url)?;
    let repo = normalize_repo(&routing.repo).ok_or_else(|| {
        FeedbackRouteError::InvalidSkillRouting(
            "repo must be a canonical GitHub repository URL or owner/repository slug".to_string(),
        )
    })?;
    if !repo.eq_ignore_ascii_case(&issues_repo) {
        return Err(FeedbackRouteError::RepositoryMismatch { repo, issues_repo });
    }
    Ok(FeedbackRoute {
        repo: issues_repo,
        issues_url,
        rationale: FeedbackRouteRationale::SkillMetadata,
    })
}

fn is_core_error_kind(value: &str) -> bool {
    let normalized = value.trim().to_ascii_lowercase().replace(['-', '.'], "_");
    if matches!(
        normalized.as_str(),
        "unknown_slug" | "instance_offline" | "ambiguous"
    ) {
        return true;
    }
    ["gateway", "cli", "protocol", "mcp_protocol", "jsonrpc"]
        .iter()
        .any(|namespace| {
            normalized == *namespace
                || normalized
                    .strip_prefix(namespace)
                    .is_some_and(|suffix| suffix.starts_with('_'))
        })
}

fn canonical_github_issues(value: &str) -> Result<(String, String), FeedbackRouteError> {
    let canonical = value.trim().trim_end_matches('/');
    let Some(path) = canonical.strip_prefix("https://github.com/") else {
        return Err(FeedbackRouteError::InvalidIssuesUrl {
            issues_url: value.to_string(),
        });
    };
    if canonical.contains(['?', '#']) {
        return Err(FeedbackRouteError::InvalidIssuesUrl {
            issues_url: value.to_string(),
        });
    }
    let parts = path.split('/').collect::<Vec<_>>();
    if parts.len() != 3
        || parts[0].is_empty()
        || parts[1].is_empty()
        || parts[2] != "issues"
        || !parts[..2].iter().all(|part| valid_repo_component(part))
    {
        return Err(FeedbackRouteError::InvalidIssuesUrl {
            issues_url: value.to_string(),
        });
    }
    Ok((format!("{}/{}", parts[0], parts[1]), canonical.to_string()))
}

fn normalize_repo(value: &str) -> Option<String> {
    let mut candidate = value.trim().trim_end_matches('/');
    if let Some(path) = candidate.strip_prefix("https://github.com/") {
        candidate = path;
    }
    candidate = candidate.strip_suffix(".git").unwrap_or(candidate);
    let parts = candidate.split('/').collect::<Vec<_>>();
    (parts.len() == 2 && parts.iter().all(|part| valid_repo_component(part)))
        .then(|| format!("{}/{}", parts[0], parts[1]))
}

fn valid_repo_component(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 100
        && value != "."
        && value != ".."
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use dcc_mcp_catalog::CatalogEntry;
    use dcc_mcp_models::{
        FINDING_V1_SCHEMA_VERSION, FindingEvidenceV1, FindingPhase, FindingRedactionStatusV1,
        FindingReproV1, FindingSeverity, FindingV1,
    };
    use serde_json::json;

    use super::*;

    fn catalog_entry(name: &str, issues_url: &str, tags: &[&str]) -> CatalogEntry {
        CatalogEntry {
            name: name.to_string(),
            description: format!("{name} package"),
            dcc: vec![],
            targets: vec![],
            url: Some(format!("https://github.com/dcc-mcp/{name}")),
            issues_url: Some(issues_url.to_string()),
            tags: tags.iter().map(|tag| (*tag).to_string()).collect(),
            version: None,
            min_core_version: None,
            install: None,
            package: None,
            maintainer: Some("dcc-mcp".to_string()),
            category: None,
            policy: None,
            requires: None,
            icon: None,
            showcase: None,
        }
    }

    fn finding(phase: FindingPhase, error_kind: &str) -> FindingV1 {
        FindingV1 {
            schema_version: FINDING_V1_SCHEMA_VERSION,
            fingerprint: format!("sha256:{}", "a".repeat(64)),
            dcc_type: "godot".to_string(),
            adapter: "dcc-mcp-godot".to_string(),
            adapter_version: "0.1.9".to_string(),
            core_version: "0.20.11".to_string(),
            host_version: "4.4.1".to_string(),
            os: "windows".to_string(),
            phase,
            severity: FindingSeverity::Blocker,
            tool_slug: Some("godot.12345678.scene_export".to_string()),
            intent: "Export the project".to_string(),
            observed: "The operation failed".to_string(),
            expected: "The operation succeeds".to_string(),
            repro: FindingReproV1 {
                argv: vec![],
                steps: vec!["Run the operation".to_string()],
            },
            evidence: FindingEvidenceV1 {
                request_id: None,
                job_id: None,
                instance_id: None,
                error_kind: Some(error_kind.to_string()),
                run_id: None,
                extra: BTreeMap::new(),
            },
            redaction_status: FindingRedactionStatusV1::needs_review(false),
        }
    }

    fn catalog() -> Vec<CatalogEntry> {
        vec![
            catalog_entry(
                "dcc-mcp-core",
                "https://github.com/dcc-mcp/dcc-mcp-core/issues",
                &["core"],
            ),
            catalog_entry(
                "dcc-mcp-godot",
                "https://github.com/dcc-mcp/dcc-mcp-godot/issues",
                &["adapter"],
            ),
        ]
    }

    #[test]
    fn adapter_phase_routes_to_exact_catalog_entry() {
        let route = route_finding(
            &finding(FindingPhase::Install, "install_failed"),
            &catalog(),
        )
        .unwrap();

        assert_eq!(route.repo, "dcc-mcp/dcc-mcp-godot");
        assert_eq!(
            route.issues_url,
            "https://github.com/dcc-mcp/dcc-mcp-godot/issues"
        );
        assert_eq!(route.rationale, FeedbackRouteRationale::AdapterPhase);
    }

    #[test]
    fn shared_error_kind_routes_to_core_before_phase_fallback() {
        let route = route_finding(
            &finding(FindingPhase::Dispatch, "gateway_protocol_error"),
            &catalog(),
        )
        .unwrap();

        assert_eq!(route.repo, "dcc-mcp/dcc-mcp-core");
        assert_eq!(route.rationale, FeedbackRouteRationale::CoreErrorKind);
    }

    #[test]
    fn stable_gateway_routing_error_kind_routes_to_core() {
        let route =
            route_finding(&finding(FindingPhase::Dispatch, "unknown-slug"), &catalog()).unwrap();

        assert_eq!(route.repo, "dcc-mcp/dcc-mcp-core");
        assert_eq!(route.rationale, FeedbackRouteRationale::CoreErrorKind);
    }

    #[test]
    fn skill_phase_uses_validated_skill_metadata_route() {
        let mut value = finding(FindingPhase::Skill, "skill_contract_violation");
        value.evidence.extra.insert(
            "routing".to_string(),
            json!({
                "source": "skill_metadata",
                "skill_name": "godot-export",
                "repo": "https://github.com/dcc-mcp/dcc-mcp-godot",
                "issues_url": "https://github.com/dcc-mcp/dcc-mcp-godot/issues"
            }),
        );

        let route = route_finding(&value, &catalog()).unwrap();

        assert_eq!(route.repo, "dcc-mcp/dcc-mcp-godot");
        assert_eq!(route.rationale, FeedbackRouteRationale::SkillMetadata);
    }

    #[test]
    fn skill_phase_without_metadata_fails_closed() {
        let error = route_finding(
            &finding(FindingPhase::Skill, "skill_contract_violation"),
            &catalog(),
        )
        .unwrap_err();

        assert_eq!(error, FeedbackRouteError::MissingSkillRouting);
    }

    #[test]
    fn mismatched_skill_repo_and_issue_url_fail_closed() {
        let mut value = finding(FindingPhase::Skill, "skill_contract_violation");
        value.evidence.extra.insert(
            "routing".to_string(),
            json!({
                "source": "skill_metadata",
                "skill_name": "godot-export",
                "repo": "https://github.com/dcc-mcp/dcc-mcp-godot",
                "issues_url": "https://github.com/dcc-mcp/dcc-mcp-core/issues"
            }),
        );

        assert!(matches!(
            route_finding(&value, &catalog()),
            Err(FeedbackRouteError::RepositoryMismatch { .. })
        ));
    }
}
