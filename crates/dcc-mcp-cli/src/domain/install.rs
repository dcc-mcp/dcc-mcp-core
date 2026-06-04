use std::path::PathBuf;

use dcc_mcp_catalog::CatalogEntry;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallRequest {
    pub dcc_type: String,
    pub version: Option<String>,
    pub catalog_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InstallPlan {
    pub dcc_type: String,
    pub version: Option<String>,
    pub adapter: CatalogEntry,
    pub steps: Vec<InstallStep>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InstallStep {
    pub name: String,
    pub description: String,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum InstallPlanError {
    #[error("no catalog entry targets dcc type '{0}'")]
    UnsupportedDcc(String),
}

pub struct InstallPlanner;

impl InstallPlanner {
    pub fn plan(
        entries: &[CatalogEntry],
        request: InstallRequest,
    ) -> Result<InstallPlan, InstallPlanError> {
        let adapter = entries
            .iter()
            .find(|entry| {
                entry
                    .dcc
                    .iter()
                    .any(|dcc| dcc.eq_ignore_ascii_case(&request.dcc_type))
            })
            .cloned()
            .ok_or_else(|| InstallPlanError::UnsupportedDcc(request.dcc_type.clone()))?;

        Ok(InstallPlan {
            dcc_type: request.dcc_type,
            version: request.version,
            adapter,
            steps: vec![
                InstallStep {
                    name: "resolve-adapter".into(),
                    description: "Resolve the official adapter package from the DCC-MCP catalog."
                        .into(),
                },
                InstallStep {
                    name: "install-runtime".into(),
                    description: "Install the cross-platform dcc-mcp-cli and companion runtime binaries."
                        .into(),
                },
                InstallStep {
                    name: "install-dcc-adapter".into(),
                    description: "Install the DCC-specific adapter into the user's local DCC plugin location."
                        .into(),
                },
                InstallStep {
                    name: "verify".into(),
                    description: "Run health, instance discovery, search, describe, and call smoke checks."
                        .into(),
                },
            ],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn catalog_entry(name: &str, dcc: &[&str]) -> CatalogEntry {
        CatalogEntry {
            name: name.into(),
            description: "Adapter".into(),
            dcc: dcc.iter().map(|value| value.to_string()).collect(),
            url: Some("https://example.invalid/adapter".into()),
            tags: vec!["official".into()],
            version: None,
            min_core_version: None,
            install: None,
            maintainer: None,
        }
    }

    #[test]
    fn planner_selects_matching_dcc_case_insensitively() {
        let entries = vec![catalog_entry("dcc-mcp-maya", &["maya"])];
        let plan = InstallPlanner::plan(
            &entries,
            InstallRequest {
                dcc_type: "MAYA".into(),
                version: Some("2026".into()),
                catalog_path: None,
            },
        )
        .unwrap();

        assert_eq!(plan.adapter.name, "dcc-mcp-maya");
        assert_eq!(plan.steps.len(), 4);
    }

    #[test]
    fn planner_rejects_unknown_dcc() {
        let err = InstallPlanner::plan(
            &[],
            InstallRequest {
                dcc_type: "custom".into(),
                version: None,
                catalog_path: None,
            },
        )
        .unwrap_err();

        assert_eq!(err, InstallPlanError::UnsupportedDcc("custom".into()));
    }
}
