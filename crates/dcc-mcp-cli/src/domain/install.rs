use std::path::PathBuf;

use dcc_mcp_catalog::{CatalogEntry, CatalogInstall};
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

/// A single install step with a human-readable description and the action to execute.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InstallStep {
    pub name: String,
    pub description: String,
    /// The executable action for this step. `None` for informational/display-only steps.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<InstallStepAction>,
}

/// The concrete action to perform during install execution.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub enum InstallStepAction {
    /// Install a Python package via pip (optionally with a specific interpreter).
    PipInstall {
        /// Pip package name (e.g. "dcc-mcp-maya").
        package: String,
        /// Optional pip extras (e.g. ["maya"]).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        extras: Option<Vec<String>>,
        /// Optional Python/mayapy interpreter path override.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        python: Option<String>,
    },
    /// Clone a git repository.
    GitClone {
        /// Git remote URL.
        url: String,
        /// Git ref, tag, or branch to check out.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ref_: Option<String>,
        /// Target directory for the clone.
        dest: PathBuf,
    },
    /// Download and extract a ZIP archive.
    ZipExtract {
        /// Archive download URL.
        url: String,
        /// Optional SHA-256 hash for integrity verification.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sha256: Option<String>,
        /// Target extract directory.
        dest: PathBuf,
    },
    /// Copy files from a local path.
    PathCopy {
        /// Source directory or file.
        source: PathBuf,
        /// Target directory.
        dest: PathBuf,
    },
    /// Register the DCC adapter with the gateway.
    RegisterDcc {
        /// DCC type name (e.g. "maya").
        dcc_type: String,
        /// Optional Python entry point for the adapter.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        entry_point: Option<String>,
    },
    /// Run post-install verification (health, instance discovery, smoke tests).
    Verify,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum InstallPlanError {
    #[error("no catalog entry targets dcc type '{0}'")]
    UnsupportedDcc(String),
    #[error("no install metadata found in catalog entry for '{0}'")]
    MissingInstallMetadata(String),
}

// ── defaults ─────────────────────────────────────────────────────────────────────

/// Default install paths relative to user home or DCC-MCP data root.
pub fn default_adapter_dir() -> PathBuf {
    dirs_data_dir().join("adapters")
}

fn dirs_data_dir() -> PathBuf {
    dirs::data_dir()
        .map(|p| p.join("dcc-mcp"))
        .unwrap_or_else(|| PathBuf::from("~/.local/share/dcc-mcp"))
}

// ── planner ──────────────────────────────────────────────────────────────────────

pub struct InstallPlanner;

impl InstallPlanner {
    /// Generate an install plan from catalog entries and user request.
    ///
    /// If the matching catalog entry has an `install` field, the generated steps
    /// will include executable actions.  Otherwise only informational steps are
    /// produced (for display-only plan output).
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

        let dcc_type = request.dcc_type.clone();
        let version = request.version.clone();

        let steps = match &adapter.install {
            Some(install) => Self::build_executable_steps(&adapter, install, &dcc_type),
            None => Self::build_info_steps(),
        };

        Ok(InstallPlan {
            dcc_type,
            version,
            adapter,
            steps,
        })
    }

    /// Build executable steps from a catalog entry's `install` metadata.
    fn build_executable_steps(
        entry: &CatalogEntry,
        install: &CatalogInstall,
        dcc_type: &str,
    ) -> Vec<InstallStep> {
        let adapter_dir = default_adapter_dir().join(&entry.name);

        let mut steps = Vec::new();
        let install_action = match install.install_type.as_str() {
            "pip" => {
                let python = install.mayapy_path.clone();
                InstallStepAction::PipInstall {
                    package: install
                        .pip_package
                        .clone()
                        .unwrap_or_else(|| entry.name.clone()),
                    extras: install.pip_extras.clone(),
                    python,
                }
            }
            "git" => InstallStepAction::GitClone {
                url: install
                    .url
                    .clone()
                    .unwrap_or_else(|| format!("https://github.com/dcc-mcp/{}", entry.name)),
                ref_: install.ref_.clone(),
                dest: adapter_dir.clone(),
            },
            "zip" => InstallStepAction::ZipExtract {
                url: install.url.clone().unwrap_or_default(),
                sha256: install.sha256.clone(),
                dest: adapter_dir.clone(),
            },
            "path" => {
                let source = install
                    .url
                    .clone()
                    .map(|u| u.strip_prefix("file://").map(PathBuf::from).unwrap_or(PathBuf::from(&u)))
                    .unwrap_or_else(|| PathBuf::from("."));
                InstallStepAction::PathCopy {
                    source,
                    dest: adapter_dir.clone(),
                }
            }
            other => {
                // Unknown install type — produce an info step instead.
                return vec![InstallStep {
                    name: format!("install-{}", other),
                    description: format!("Unsupported install type '{other}': manual installation required."),
                    action: None,
                }];
            }
        };

        steps.push(InstallStep {
            name: format!("install-{}", install.install_type),
            description: format!(
                "Install {} adapter via {}",
                dcc_type,
                install.install_type
            ),
            action: Some(install_action),
        });

        // Register step
        steps.push(InstallStep {
            name: "register-dcc".into(),
            description: format!(
                "Register {} adapter with the DCC-MCP gateway",
                dcc_type
            ),
            action: Some(InstallStepAction::RegisterDcc {
                dcc_type: dcc_type.to_string(),
                entry_point: install.entry_point.clone(),
            }),
        });

        // Verify step
        steps.push(InstallStep {
            name: "verify".into(),
            description: "Run health and smoke checks to verify the installation.".into(),
            action: Some(InstallStepAction::Verify),
        });

        steps
    }

    /// Build informational-only steps when no install metadata exists.
    fn build_info_steps() -> Vec<InstallStep> {
        vec![
            InstallStep {
                name: "resolve-adapter".into(),
                description:
                    "Resolve the official adapter package from the DCC-MCP catalog.".into(),
                action: None,
            },
            InstallStep {
                name: "install-runtime".into(),
                description:
                    "Install the cross-platform dcc-mcp-cli and companion runtime binaries."
                        .into(),
                action: None,
            },
            InstallStep {
                name: "install-dcc-adapter".into(),
                description:
                    "Install the DCC-specific adapter into the user's local DCC plugin location."
                        .into(),
                action: None,
            },
            InstallStep {
                name: "verify".into(),
                description:
                    "Run health, instance discovery, search, describe, and call smoke checks."
                        .into(),
                action: None,
            },
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn catalog_entry(name: &str, dcc: &[&str], install: Option<CatalogInstall>) -> CatalogEntry {
        CatalogEntry {
            name: name.into(),
            description: "Adapter".into(),
            dcc: dcc.iter().map(|value| value.to_string()).collect(),
            url: Some("https://example.invalid/adapter".into()),
            tags: vec!["official".into()],
            version: None,
            min_core_version: None,
            install,
            maintainer: None,
            icon: None,
        }
    }

    #[test]
    fn planner_selects_matching_dcc_case_insensitively() {
        let entries = vec![catalog_entry("dcc-mcp-maya", &["maya"], None)];
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
        // Without install metadata, steps have no action
        assert!(plan.steps.iter().all(|s| s.action.is_none()));
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

    #[test]
    fn planner_generates_executable_steps_for_pip_install() {
        let install = CatalogInstall {
            install_type: "pip".into(),
            url: Some("https://pypi.org/project/dcc-mcp-maya".into()),
            ref_: None,
            sha256: None,
            pip_package: Some("dcc-mcp-maya".into()),
            pip_extras: Some(vec!["maya".into()]),
            mayapy_path: Some("/usr/bin/mayapy".into()),
            entry_point: Some("dcc_mcp_maya.cli:main".into()),
        };
        let entries = vec![catalog_entry("dcc-mcp-maya", &["maya"], Some(install))];
        let plan = InstallPlanner::plan(
            &entries,
            InstallRequest {
                dcc_type: "maya".into(),
                version: None,
                catalog_path: None,
            },
        )
        .unwrap();

        assert_eq!(plan.steps.len(), 3);
        assert_eq!(plan.steps[0].name, "install-pip");
        assert!(matches!(
            plan.steps[0].action,
            Some(InstallStepAction::PipInstall { .. })
        ));
        if let Some(InstallStepAction::PipInstall {
            package,
            extras,
            python,
        }) = &plan.steps[0].action
        {
            assert_eq!(package, "dcc-mcp-maya");
            assert_eq!(extras.as_deref(), Some(&["maya".into()][..]));
            assert_eq!(python.as_deref(), Some("/usr/bin/mayapy"));
        } else {
            panic!("expected PipInstall action");
        }

        assert_eq!(plan.steps[1].name, "register-dcc");
        assert!(matches!(
            plan.steps[1].action,
            Some(InstallStepAction::RegisterDcc { .. })
        ));

        assert_eq!(plan.steps[2].name, "verify");
        assert!(matches!(
            plan.steps[2].action,
            Some(InstallStepAction::Verify)
        ));
    }

    #[test]
    fn planner_generates_executable_steps_for_git_install() {
        let install = CatalogInstall {
            install_type: "git".into(),
            url: Some("https://github.com/loonghao/dcc-mcp-maya-mgear".into()),
            ref_: Some("main".into()),
            sha256: None,
            pip_package: None,
            pip_extras: None,
            mayapy_path: None,
            entry_point: None,
        };
        let entries = vec![catalog_entry(
            "dcc-mcp-maya-mgear",
            &["maya"],
            Some(install),
        )];
        let plan = InstallPlanner::plan(
            &entries,
            InstallRequest {
                dcc_type: "maya".into(),
                version: None,
                catalog_path: None,
            },
        )
        .unwrap();

        assert_eq!(plan.steps.len(), 3);
        assert_eq!(plan.steps[0].name, "install-git");
        assert!(matches!(
            plan.steps[0].action,
            Some(InstallStepAction::GitClone { .. })
        ));
    }

    #[test]
    fn planner_missing_install_metadata_uses_info_steps() {
        let entries = vec![catalog_entry("dcc-mcp-blender", &["blender"], None)];
        let plan = InstallPlanner::plan(
            &entries,
            InstallRequest {
                dcc_type: "blender".into(),
                version: None,
                catalog_path: None,
            },
        )
        .unwrap();

        assert_eq!(plan.steps.len(), 4);
        assert!(plan.steps.iter().all(|s| s.action.is_none()));
    }
}
