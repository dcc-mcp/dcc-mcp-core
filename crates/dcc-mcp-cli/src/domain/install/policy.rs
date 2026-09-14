use dcc_mcp_catalog::CatalogEntry;
use semver::Version;

use super::InstallPlanError;

/// Check catalog revocation and core compatibility before advertising or planning an install.
pub(crate) fn validate(entry: &CatalogEntry) -> Result<(), InstallPlanError> {
    if let Some(policy) = &entry.policy {
        match policy.installation.as_str() {
            "available" => {}
            "not_available" => return Err(InstallPlanError::NotAvailable(entry.name.clone())),
            installation => {
                return Err(InstallPlanError::InvalidInstallationPolicy {
                    name: entry.name.clone(),
                    installation: installation.into(),
                });
            }
        }
    }

    let Some(required) = entry.min_core_version.as_deref() else {
        return Ok(());
    };
    let required_version =
        Version::parse(required).map_err(|_| InstallPlanError::InvalidMinCoreVersion {
            name: entry.name.clone(),
            required: required.into(),
        })?;
    let current = env!("CARGO_PKG_VERSION");
    let current_version =
        Version::parse(current).expect("workspace package version must be SemVer");
    if current_version < required_version {
        return Err(InstallPlanError::IncompatibleCoreVersion {
            name: entry.name.clone(),
            required: required.into(),
            current: current.into(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::install::{InstallPlanner, InstallRequest};
    use serde_json::json;

    fn entry(dcc: &str) -> CatalogEntry {
        serde_json::from_value(json!({
            "name": format!("dcc-mcp-{dcc}"),
            "description": "Adapter fixture",
            "dcc": [dcc],
            "tags": ["adapter"],
        }))
        .unwrap()
    }

    #[test]
    fn revoked_and_unknown_policies_prevent_even_informational_plans() {
        for dcc in ["maya", "photoshop", "studio-editor"] {
            for installation in ["not_available", "revoked", "AVAILABLE", ""] {
                let mut adapter = entry(dcc);
                adapter.policy = Some(dcc_mcp_catalog::CatalogPolicy {
                    installation: installation.into(),
                    reason: None,
                });
                let result = InstallPlanner::plan(
                    &[adapter],
                    InstallRequest {
                        dcc_type: dcc.into(),
                        version: None,
                        catalog_path: None,
                        python: None,
                        dcc_path: None,
                        plugin_source: None,
                        adobe_debug_root: None,
                    },
                );
                let expected = if installation == "not_available" {
                    InstallPlanError::NotAvailable(format!("dcc-mcp-{dcc}"))
                } else {
                    InstallPlanError::InvalidInstallationPolicy {
                        name: format!("dcc-mcp-{dcc}"),
                        installation: installation.into(),
                    }
                };
                assert_eq!(result.unwrap_err(), expected);
            }
        }
    }

    #[test]
    fn compatibility_requires_valid_semver_and_sufficient_core_version() {
        let mut adapter = entry("blender");
        for required in ["", "0.19", ">=0.19.0", "not-semver"] {
            adapter.min_core_version = Some(required.into());
            assert_eq!(
                validate(&adapter),
                Err(InstallPlanError::InvalidMinCoreVersion {
                    name: adapter.name.clone(),
                    required: required.into(),
                }),
            );
        }
        adapter.min_core_version = Some("999.0.0".into());
        assert_eq!(
            validate(&adapter),
            Err(InstallPlanError::IncompatibleCoreVersion {
                name: adapter.name.clone(),
                required: "999.0.0".into(),
                current: env!("CARGO_PKG_VERSION").into(),
            }),
        );
        for required in [None, Some("0.0.0"), Some(env!("CARGO_PKG_VERSION"))] {
            adapter.min_core_version = required.map(str::to_owned);
            assert!(validate(&adapter).is_ok());
            adapter.policy = Some(dcc_mcp_catalog::CatalogPolicy {
                installation: "available".into(),
                reason: None,
            });
            assert!(validate(&adapter).is_ok());
            adapter.policy = None;
        }
    }
}
