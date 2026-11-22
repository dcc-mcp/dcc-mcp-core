use dcc_mcp_catalog::{CatalogEntry, CoreBoundPolicy, evaluate};
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

    if let Some(required) = entry.min_core_version.as_deref() {
        let required_version =
            Version::parse(required).map_err(|_| InstallPlanError::InvalidMinCoreVersion {
                name: entry.name.clone(),
                required: required.into(),
            })?;
        if entry.core_requirement.is_none() {
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
        }
    }
    validate_core_requirement(entry)?;
    if entry.core_requirement.is_some()
        && entry
            .install
            .as_ref()
            .is_some_and(|install| install.install_type != "pip")
    {
        return Err(InstallPlanError::CoreRequirementRequiresPip {
            name: entry.name.clone(),
        });
    }
    Ok(())
}

fn validate_core_requirement(entry: &CatalogEntry) -> Result<(), InstallPlanError> {
    let Some(required) = entry.core_requirement.as_deref() else {
        return Ok(());
    };

    if !evaluate(required, &CoreBoundPolicy::default()).is_ok() {
        return Err(InstallPlanError::InvalidCoreRequirement {
            name: entry.name.clone(),
            required: required.into(),
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

    #[test]
    fn core_requirement_requires_pip_install_type() {
        let mut adapter = entry("blender");
        adapter.core_requirement = Some(">=0.20.0,<0.21.0".into());
        adapter.install = Some(dcc_mcp_catalog::CatalogInstall {
            install_type: "git".into(),
            url: Some("https://github.com/dcc-mcp/example".into()),
            ref_: Some("0123456789012345678901234567890123456789".into()),
            sha256: None,
            skill_roots: None,
            pip_package: None,
            pip_extras: None,
            python_path: None,
            entry_point: None,
            probe_tool: None,
            instructions_url: None,
            sop_version: None,
            sop_schema_digest: None,
            adobe: None,
        });
        assert_eq!(
            validate(&adapter),
            Err(InstallPlanError::CoreRequirementRequiresPip {
                name: adapter.name.clone(),
            })
        );
    }

    #[test]
    fn bounded_core_requirement_is_validated_without_using_cli_core_version() {
        let mut adapter = entry("blender");
        adapter.core_requirement = Some(">=999.0.0,<999.1.0".into());

        assert!(validate(&adapter).is_ok());
    }

    #[test]
    fn bounded_core_requirement_takes_precedence_over_legacy_minimum() {
        let mut adapter = entry("blender");
        adapter.min_core_version = Some("999.0.0".into());
        adapter.core_requirement = Some(">=0.20.0,<0.21.0".into());

        assert!(validate(&adapter).is_ok());
    }

    #[test]
    fn bounded_core_requirement_rejects_invalid_range() {
        let mut adapter = entry("blender");
        adapter.core_requirement = Some(">=0.20.0".into());
        assert_eq!(
            validate(&adapter),
            Err(InstallPlanError::InvalidCoreRequirement {
                name: adapter.name.clone(),
                required: ">=0.20.0".into(),
            })
        );
    }

    #[test]
    fn invalid_core_requirement_is_rejected_closed() {
        let mut adapter = entry("blender");
        for required in [">=0.20.0", ">=not-semver,<0.21.0", "dcc-mcp-core"] {
            adapter.core_requirement = Some(required.into());
            assert_eq!(
                validate(&adapter),
                Err(InstallPlanError::InvalidCoreRequirement {
                    name: adapter.name.clone(),
                    required: required.into(),
                })
            );
        }
    }
}
