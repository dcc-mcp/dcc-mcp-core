//! Shared routing for Finding v1 feedback reports.
//!
//! Routing answers one question: which public GitHub issue tracker owns this
//! finding? The logic lives in `dcc-mcp-models` so the gateway can persist a
//! route at ingest time while the CLI keeps using the exact same rules when it
//! files an issue.
//!
//! The rule set is deliberately decoupled from any concrete catalog type:
//! callers pass [`FeedbackRouteTarget`] slices, so `dcc-mcp-models` never has
//! to depend on `dcc-mcp-catalog`.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::feedback::{FindingPhase, FindingV1};

/// Catalog package that owns gateway / CLI / protocol error kinds.
pub const CORE_PACKAGE: &str = "dcc-mcp-core";

/// Evidence key carrying routing metadata copied from an owning Skill.
pub const SKILL_ROUTING_EVIDENCE_KEY: &str = "routing";

/// One catalog-shaped routing candidate: a package name and its issues URL.
///
/// Build these from `CatalogEntry` (or any other package source) with
/// [`FeedbackRouteTarget::new`]; keeping this struct local avoids a
/// `dcc-mcp-models` → `dcc-mcp-catalog` dependency.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FeedbackRouteTarget<'a> {
    pub name: &'a str,
    pub issues_url: Option<&'a str>,
}

impl<'a> FeedbackRouteTarget<'a> {
    #[must_use]
    pub fn new(name: &'a str, issues_url: Option<&'a str>) -> Self {
        Self { name, issues_url }
    }
}

/// Machine-readable destination for one validated feedback finding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeedbackRoute {
    pub repo: String,
    pub issues_url: String,
    pub rationale: FeedbackRouteRationale,
}

/// Stable reason code explaining why a route was selected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FeedbackRouteRationale {
    AdapterPhase,
    CoreErrorKind,
    SkillMetadata,
}

impl FeedbackRouteRationale {
    /// Stable snake_case label used when the rationale is persisted.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AdapterPhase => "adapter_phase",
            Self::CoreErrorKind => "core_error_kind",
            Self::SkillMetadata => "skill_metadata",
        }
    }
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
    targets: &[FeedbackRouteTarget<'_>],
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
        return route_catalog_package(targets, CORE_PACKAGE, FeedbackRouteRationale::CoreErrorKind);
    }

    match finding.phase {
        FindingPhase::Install | FindingPhase::Startup | FindingPhase::Dispatch => {
            route_catalog_package(
                targets,
                &finding.adapter,
                FeedbackRouteRationale::AdapterPhase,
            )
        }
        FindingPhase::Skill => route_skill_metadata(finding),
        FindingPhase::Other => Err(FeedbackRouteError::AmbiguousOtherPhase),
    }
}

fn route_catalog_package(
    targets: &[FeedbackRouteTarget<'_>],
    package: &str,
    rationale: FeedbackRouteRationale,
) -> Result<FeedbackRoute, FeedbackRouteError> {
    let matches = targets
        .iter()
        .filter(|target| target.name.eq_ignore_ascii_case(package))
        .copied()
        .collect::<Vec<_>>();
    let target = match matches.as_slice() {
        [] => {
            return Err(FeedbackRouteError::CatalogPackageNotFound {
                package: package.to_string(),
            });
        }
        [target] => *target,
        _ => {
            return Err(FeedbackRouteError::AmbiguousCatalogPackage {
                package: package.to_string(),
            });
        }
    };
    let issues_url = target
        .issues_url
        .ok_or_else(|| FeedbackRouteError::MissingIssuesUrl {
            package: target.name.to_string(),
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

/// True when an error kind belongs to the shared core (gateway / CLI / protocol).
pub fn is_core_error_kind(value: &str) -> bool {
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
    use serde_json::json;

    use super::*;
    use crate::feedback::{
        FINDING_V1_SCHEMA_VERSION, FindingEvidenceV1, FindingRedactionStatusV1, FindingReproV1,
        FindingSeverity,
    };

    fn targets(
        entries: &[(&'static str, Option<&'static str>)],
    ) -> Vec<FeedbackRouteTarget<'static>> {
        entries
            .iter()
            .map(|(name, issues_url)| FeedbackRouteTarget::new(name, *issues_url))
            .collect()
    }

    fn finding(phase: FindingPhase, error_kind: Option<&str>) -> FindingV1 {
        FindingV1 {
            schema_version: FINDING_V1_SCHEMA_VERSION,
            fingerprint: format!("sha256:{}", "a".repeat(64)),
            dcc_type: "godot".to_string(),
            adapter: "dcc-mcp-godot".to_string(),
            adapter_version: "0.1.0".to_string(),
            core_version: "0.20.11".to_string(),
            host_version: "4.3".to_string(),
            os: "windows".to_string(),
            phase,
            severity: FindingSeverity::Degraded,
            tool_slug: Some("godot_scene__save".to_string()),
            intent: "Save the scene".to_string(),
            observed: "Nothing happened".to_string(),
            expected: "The scene is saved".to_string(),
            repro: FindingReproV1 {
                argv: Vec::new(),
                steps: vec!["Open a scene".to_string()],
            },
            evidence: FindingEvidenceV1 {
                request_id: None,
                job_id: None,
                instance_id: None,
                error_kind: error_kind.map(|kind| kind.to_string()),
                run_id: None,
                extra: Default::default(),
            },
            redaction_status: FindingRedactionStatusV1::needs_review(false),
        }
    }

    fn catalog() -> Vec<(&'static str, Option<&'static str>)> {
        vec![
            (
                "dcc-mcp-core",
                Some("https://github.com/dcc-mcp/dcc-mcp-core/issues"),
            ),
            (
                "dcc-mcp-godot",
                Some("https://github.com/dcc-mcp/dcc-mcp-godot/issues"),
            ),
        ]
    }

    #[test]
    fn dispatch_phase_routes_to_the_adapter_package() {
        let targets = targets(&catalog());
        let route = route_finding(&finding(FindingPhase::Dispatch, None), &targets)
            .expect("dispatch routes to the adapter package");
        assert_eq!(route.repo, "dcc-mcp/dcc-mcp-godot");
        assert_eq!(
            route.issues_url,
            "https://github.com/dcc-mcp/dcc-mcp-godot/issues"
        );
        assert_eq!(route.rationale, FeedbackRouteRationale::AdapterPhase);
    }

    #[test]
    fn adapter_match_is_case_insensitive() {
        let finding = FindingV1 {
            adapter: "DCC-MCP-GoDot".to_string(),
            ..finding(FindingPhase::Startup, None)
        };
        let targets = targets(&catalog());
        let route = route_finding(&finding, &targets).expect("case-insensitive adapter match");
        assert_eq!(route.repo, "dcc-mcp/dcc-mcp-godot");
    }

    #[test]
    fn core_error_kind_overrides_the_adapter_phase() {
        let targets = targets(&catalog());
        let route = route_finding(
            &finding(FindingPhase::Dispatch, Some("gateway_timeout")),
            &targets,
        )
        .expect("core error kind routes to core");
        assert_eq!(route.repo, "dcc-mcp/dcc-mcp-core");
        assert_eq!(route.rationale, FeedbackRouteRationale::CoreErrorKind);
    }

    #[test]
    fn duplicated_package_names_are_rejected() {
        let targets = targets(&[
            ("dup", Some("https://github.com/dcc-mcp/a/issues")),
            ("DUP", Some("https://github.com/dcc-mcp/b/issues")),
        ]);
        let finding = FindingV1 {
            adapter: "dup".to_string(),
            ..finding(FindingPhase::Install, None)
        };
        let error = route_finding(&finding, &targets).expect_err("ambiguous package");
        assert_eq!(
            error,
            FeedbackRouteError::AmbiguousCatalogPackage {
                package: "dup".to_string()
            }
        );
    }

    #[test]
    fn package_without_issues_url_is_rejected() {
        let targets = targets(&[("dcc-mcp-godot", None)]);
        let error =
            route_finding(&finding(FindingPhase::Dispatch, None), &targets).expect_err("no url");
        assert_eq!(
            error,
            FeedbackRouteError::MissingIssuesUrl {
                package: "dcc-mcp-godot".to_string()
            }
        );
    }

    #[test]
    fn non_github_issues_url_is_rejected() {
        let targets = targets(&[("dcc-mcp-godot", Some("https://gitlab.com/dcc-mcp/x/issues"))]);
        let error =
            route_finding(&finding(FindingPhase::Dispatch, None), &targets).expect_err("invalid");
        assert!(matches!(error, FeedbackRouteError::InvalidIssuesUrl { .. }));
    }

    #[test]
    fn skill_phase_requires_routing_evidence() {
        let targets = targets(&catalog());
        let error =
            route_finding(&finding(FindingPhase::Skill, None), &targets).expect_err("no evidence");
        assert_eq!(error, FeedbackRouteError::MissingSkillRouting);
    }

    #[test]
    fn skill_phase_routes_from_skill_metadata() {
        let mut finding = finding(FindingPhase::Skill, None);
        finding.evidence.extra.insert(
            SKILL_ROUTING_EVIDENCE_KEY.to_string(),
            json!({
                "source": "skill_metadata",
                "skill_name": "godot-scene-save",
                "repo": "dcc-mcp/dcc-mcp-godot-skills",
                "issues_url": "https://github.com/dcc-mcp/dcc-mcp-godot-skills/issues"
            }),
        );
        let targets = targets(&catalog());
        let route = route_finding(&finding, &targets).expect("skill metadata route");
        assert_eq!(route.repo, "dcc-mcp/dcc-mcp-godot-skills");
        assert_eq!(route.rationale, FeedbackRouteRationale::SkillMetadata);
    }

    #[test]
    fn skill_phase_rejects_mismatched_repository() {
        let mut finding = finding(FindingPhase::Skill, None);
        finding.evidence.extra.insert(
            SKILL_ROUTING_EVIDENCE_KEY.to_string(),
            json!({
                "source": "skill_metadata",
                "skill_name": "godot-scene-save",
                "repo": "dcc-mcp/other",
                "issues_url": "https://github.com/dcc-mcp/dcc-mcp-godot-skills/issues"
            }),
        );
        let targets = targets(&catalog());
        let error = route_finding(&finding, &targets).expect_err("mismatch");
        assert!(matches!(
            error,
            FeedbackRouteError::RepositoryMismatch { .. }
        ));
    }

    #[test]
    fn other_phase_without_core_error_kind_is_ambiguous() {
        let targets = targets(&catalog());
        let error =
            route_finding(&finding(FindingPhase::Other, None), &targets).expect_err("ambiguous");
        assert_eq!(error, FeedbackRouteError::AmbiguousOtherPhase);
    }

    #[test]
    fn rationale_labels_are_stable() {
        assert_eq!(
            FeedbackRouteRationale::AdapterPhase.as_str(),
            "adapter_phase"
        );
        assert_eq!(
            FeedbackRouteRationale::CoreErrorKind.as_str(),
            "core_error_kind"
        );
        assert_eq!(
            FeedbackRouteRationale::SkillMetadata.as_str(),
            "skill_metadata"
        );
    }
}
