//! Finding v1 routing for the CLI filing path.
//!
//! The rule set is owned by `dcc-mcp-models::feedback_routing` so the gateway can
//! persist the same route at ingest time. This module only adapts the CLI's
//! `CatalogEntry` slice to the shared, catalog-agnostic target list.

use dcc_mcp_catalog::CatalogEntry;
use dcc_mcp_models::FindingV1;
use dcc_mcp_models::feedback_routing as shared;

pub use shared::{
    FeedbackRoute, FeedbackRouteError, FeedbackRouteRationale, FeedbackRouteTarget,
    is_core_error_kind,
};

/// Catalog package that owns gateway / CLI / protocol error kinds.
pub const CORE_PACKAGE: &str = shared::CORE_PACKAGE;

/// Resolve one Finding v1 to its owning GitHub issue tracker.
///
/// Behaviour is identical to `dcc_mcp_models::route_finding`; the only extra
/// step is projecting `CatalogEntry` values onto `FeedbackRouteTarget`.
pub fn route_finding(
    finding: &FindingV1,
    catalog: &[CatalogEntry],
) -> Result<FeedbackRoute, FeedbackRouteError> {
    let targets = catalog
        .iter()
        .map(|entry| FeedbackRouteTarget::new(&entry.name, entry.issues_url.as_deref()))
        .collect::<Vec<_>>();
    shared::route_finding(finding, &targets)
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
