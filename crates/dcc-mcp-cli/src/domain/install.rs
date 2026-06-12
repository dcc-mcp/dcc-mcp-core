use std::path::PathBuf;

use dcc_mcp_catalog::{CatalogEntry, CatalogInstall};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallRequest {
    pub dcc_type: String,
    pub version: Option<String>,
    pub catalog_path: Option<PathBuf>,
    pub python: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InstallPlan {
    pub dcc_type: String,
    pub version: Option<String>,
    pub adapter: CatalogEntry,
    pub steps: Vec<InstallStep>,
    #[serde(default)]
    pub next_steps: Vec<InstallNextStep>,
    #[serde(default = "InstallPolicy::enabled")]
    pub install_policy: InstallPolicy,
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

/// A machine-readable post-install action that gets a live DCC under CLI control.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InstallNextStep {
    pub name: String,
    pub description: String,
    /// Optional document URL for agent-facing instructions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Command arguments to run exactly as a process argv vector. `None` means manual host action.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<Vec<String>>,
    pub requires_live_instance: bool,
}

/// Environment or studio policy controlling whether the CLI may execute installs.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InstallPolicy {
    pub auto_install_enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
}

impl InstallPolicy {
    pub fn enabled() -> Self {
        Self {
            auto_install_enabled: true,
            prompt: None,
        }
    }

    pub fn disabled(prompt: impl Into<String>) -> Self {
        Self {
            auto_install_enabled: false,
            prompt: Some(prompt.into()),
        }
    }
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
    /// Verify local install artefacts. Live DCC readiness is a next step.
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
        let dcc_key = normalized_dcc_key(&request.dcc_type);
        let adapter = select_adapter(entries, &dcc_key)
            .cloned()
            .ok_or_else(|| InstallPlanError::UnsupportedDcc(request.dcc_type.clone()))?;

        let dcc_type = request.dcc_type.clone();
        let version = request.version.clone();

        let steps = match &adapter.install {
            Some(install) => {
                Self::build_executable_steps(&adapter, install, &dcc_type, request.python.clone())
            }
            None => Self::build_info_steps(),
        };

        let next_steps = Self::build_next_steps(&adapter, &dcc_type);

        Ok(InstallPlan {
            next_steps,
            dcc_type,
            version,
            adapter,
            steps,
            install_policy: InstallPolicy::enabled(),
        })
    }

    /// Build executable steps from a catalog entry's `install` metadata.
    fn build_executable_steps(
        entry: &CatalogEntry,
        install: &CatalogInstall,
        dcc_type: &str,
        python_override: Option<String>,
    ) -> Vec<InstallStep> {
        let adapter_dir = default_adapter_dir().join(&entry.name);

        let mut steps = Vec::new();
        let install_action = match install.install_type.as_str() {
            "pip" => {
                let python = python_override.or_else(|| install.python_path.clone());
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
                    .map(|u| {
                        u.strip_prefix("file://")
                            .map(PathBuf::from)
                            .unwrap_or(PathBuf::from(&u))
                    })
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
                    description: format!(
                        "Unsupported install type '{other}': manual installation required."
                    ),
                    action: None,
                }];
            }
        };

        steps.push(InstallStep {
            name: format!("install-{}", install.install_type),
            description: format!("Install {} adapter via {}", dcc_type, install.install_type),
            action: Some(install_action),
        });

        // Register step
        steps.push(InstallStep {
            name: "register-dcc".into(),
            description: format!(
                "Start or enable the {} host plugin so its sidecar self-registers",
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
            description: "Verify installed package or file artefacts before the DCC plugin starts."
                .into(),
            action: Some(InstallStepAction::Verify),
        });

        steps
    }

    /// Build informational-only steps when no install metadata exists.
    fn build_info_steps() -> Vec<InstallStep> {
        vec![
            InstallStep {
                name: "resolve-adapter".into(),
                description: "Resolve the official adapter package from the DCC-MCP catalog."
                    .into(),
                action: None,
            },
            InstallStep {
                name: "install-runtime".into(),
                description:
                    "Install the cross-platform dcc-mcp-cli and companion runtime binaries.".into(),
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
                    "Follow the emitted next_steps for live instance discovery, readiness, and CLI smoke checks."
                        .into(),
                action: None,
            },
        ]
    }

    fn build_next_steps(entry: &CatalogEntry, dcc_type: &str) -> Vec<InstallNextStep> {
        let mut steps = Vec::new();

        if let Some(url) = install_instructions_url(entry) {
            steps.push(InstallNextStep {
                name: "read-install-instructions".into(),
                description: format!(
                    "Read the adapter-maintained install.md for {dcc_type}; treat it as the authoritative host-specific setup runbook before executing local install steps."
                ),
                url: Some(url),
                command: None,
                requires_live_instance: false,
            });
        }

        steps.extend([
            InstallNextStep {
                name: "start-dcc-plugin".into(),
                description: format!(
                    "Start or enable the {dcc_type} host plugin. Package install alone does not create a live registry row; the plugin sidecar must start, stay alive, and self-register."
                ),
                url: None,
                command: None,
                requires_live_instance: false,
            },
            InstallNextStep {
                name: "inspect-runtime".into(),
                description:
                    "Inspect CLI, server binary, server version, gateway profile, and default registry diagnostics."
                        .into(),
                url: None,
                command: Some(command(["dcc-mcp-cli", "doctor"])),
                requires_live_instance: false,
            },
            InstallNextStep {
                name: "confirm-local-instance".into(),
                description: format!(
                    "Confirm the {dcc_type} plugin published a direct local MCP/server instance in the shared registry."
                ),
                url: None,
                command: Some(command(["dcc-mcp-cli", "list"])),
                requires_live_instance: false,
            },
            InstallNextStep {
                name: "wait-ready".into(),
                description: format!(
                    "Wait until the {dcc_type} adapter reports readiness before issuing tool calls."
                ),
                url: None,
                command: Some(command_with_dcc(
                    ["dcc-mcp-cli", "wait-ready", "--dcc-type"],
                    dcc_type,
                    [],
                )),
                requires_live_instance: true,
            },
            InstallNextStep {
                name: "discover-tools".into(),
                description: format!(
                    "Search available {dcc_type} tools through the CLI direct-control route."
                ),
                url: None,
                command: Some(command_with_dcc(
                    ["dcc-mcp-cli", "search", "--dcc-type"],
                    dcc_type,
                    ["--query", "diagnostics"],
                )),
                requires_live_instance: true,
            },
            InstallNextStep {
                name: "search-community-skills".into(),
                description: format!(
                    "Find optional community skill packages that target {dcc_type}."
                ),
                url: None,
                command: Some(command_with_dcc(
                    ["dcc-mcp-cli", "marketplace", "search", "--dcc"],
                    dcc_type,
                    ["--query", "skills"],
                )),
                requires_live_instance: false,
            },
            InstallNextStep {
                name: "inspect-community-skill".into(),
                description:
                    "Inspect the selected marketplace skill package before installing it, replacing <package-name> with the chosen package."
                        .into(),
                url: None,
                command: Some(command(["dcc-mcp-cli", "marketplace", "inspect", "<package-name>"])),
                requires_live_instance: false,
            },
            InstallNextStep {
                name: "install-community-skill".into(),
                description:
                    "Install a selected marketplace skill package for this DCC, replacing <package-name> with the chosen package."
                        .into(),
                url: None,
                command: Some(command_with_dcc(
                    ["dcc-mcp-cli", "marketplace", "install", "<package-name>", "--dcc"],
                    dcc_type,
                    [],
                )),
                requires_live_instance: false,
            },
            InstallNextStep {
                name: "reload-skills".into(),
                description: format!(
                    "Reload skills in the live {dcc_type} adapter after installing community skill packages."
                ),
                url: None,
                command: Some(command_with_dcc(
                    ["dcc-mcp-cli", "reload-skills", "--dcc-type"],
                    dcc_type,
                    [],
                )),
                requires_live_instance: true,
            },
        ]);

        steps
    }
}

fn command<const N: usize>(args: [&str; N]) -> Vec<String> {
    args.into_iter().map(str::to_string).collect()
}

fn command_with_dcc<const P: usize, const S: usize>(
    prefix: [&str; P],
    dcc_type: &str,
    suffix: [&str; S],
) -> Vec<String> {
    prefix
        .into_iter()
        .chain(std::iter::once(dcc_type))
        .chain(suffix)
        .map(str::to_string)
        .collect()
}

fn install_instructions_url(entry: &CatalogEntry) -> Option<String> {
    entry
        .install
        .as_ref()
        .and_then(|install| non_empty(install.instructions_url.as_deref()))
        .map(str::to_string)
        .or_else(|| entry.url.as_deref().and_then(github_install_md_raw_url))
}

fn github_install_md_raw_url(url: &str) -> Option<String> {
    let repo = url
        .trim_end_matches('/')
        .strip_suffix(".git")
        .unwrap_or_else(|| url.trim_end_matches('/'));
    let path = repo.strip_prefix("https://github.com/")?;
    let mut parts = path.split('/');
    let owner = parts.next()?;
    let name = parts.next()?;
    if owner.is_empty() || name.is_empty() || parts.next().is_some() {
        return None;
    }
    Some(format!(
        "https://raw.githubusercontent.com/{owner}/{name}/main/install.md"
    ))
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

fn select_adapter<'a>(entries: &'a [CatalogEntry], dcc_key: &str) -> Option<&'a CatalogEntry> {
    entries
        .iter()
        .filter(|entry| {
            entry
                .dcc
                .iter()
                .any(|dcc| normalized_dcc_key(dcc) == dcc_key)
        })
        .max_by_key(|entry| adapter_rank(entry, dcc_key))
}

fn adapter_rank(entry: &CatalogEntry, dcc_key: &str) -> (bool, bool, bool, bool) {
    let has_adapter_tag = entry
        .tags
        .iter()
        .any(|tag| tag.eq_ignore_ascii_case("adapter"));
    let has_skill_tag = entry
        .tags
        .iter()
        .any(|tag| tag.eq_ignore_ascii_case("skills"));
    let official_adapter_name = entry
        .name
        .eq_ignore_ascii_case(&format!("dcc-mcp-{dcc_key}"));

    (
        has_adapter_tag,
        entry.install.is_some(),
        official_adapter_name,
        !has_skill_tag,
    )
}

fn normalized_dcc_key(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
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

    fn argv(args: &[&str]) -> Vec<String> {
        args.iter().map(|arg| (*arg).to_string()).collect()
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
                python: None,
            },
        )
        .unwrap();

        assert_eq!(plan.adapter.name, "dcc-mcp-maya");
        assert_eq!(plan.steps.len(), 4);
        // Without install metadata, steps have no action
        assert!(plan.steps.iter().all(|s| s.action.is_none()));
        assert_eq!(plan.next_steps[0].name, "start-dcc-plugin");
        assert!(plan.next_steps[0].command.is_none());
    }

    #[test]
    fn planner_rejects_unknown_dcc() {
        let err = InstallPlanner::plan(
            &[],
            InstallRequest {
                dcc_type: "custom".into(),
                version: None,
                catalog_path: None,
                python: None,
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
            python_path: Some("/usr/bin/mayapy".into()),
            entry_point: Some("dcc_mcp_maya.cli:main".into()),
            instructions_url: None,
        };
        let entries = vec![catalog_entry("dcc-mcp-maya", &["maya"], Some(install))];
        let plan = InstallPlanner::plan(
            &entries,
            InstallRequest {
                dcc_type: "maya".into(),
                version: None,
                catalog_path: None,
                python: None,
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
        assert!(
            plan.steps[2]
                .description
                .contains("package or file artefacts")
        );
        assert!(matches!(
            plan.steps[2].action,
            Some(InstallStepAction::Verify)
        ));
    }

    #[test]
    fn planner_emits_cli_next_steps_for_live_control_and_skill_install() {
        let entries = vec![catalog_entry("dcc-mcp-blender", &["blender"], None)];
        let plan = InstallPlanner::plan(
            &entries,
            InstallRequest {
                dcc_type: "blender".into(),
                version: None,
                catalog_path: None,
                python: None,
            },
        )
        .unwrap();

        let wait_ready = plan
            .next_steps
            .iter()
            .find(|step| step.name == "wait-ready")
            .expect("wait-ready next step");
        assert_eq!(
            wait_ready.command.as_ref().unwrap(),
            &argv(&["dcc-mcp-cli", "wait-ready", "--dcc-type", "blender"])
        );
        assert!(wait_ready.requires_live_instance);

        let search_skills = plan
            .next_steps
            .iter()
            .find(|step| step.name == "search-community-skills")
            .expect("search-community-skills next step");
        assert_eq!(
            search_skills.command.as_ref().unwrap(),
            &argv(&[
                "dcc-mcp-cli",
                "marketplace",
                "search",
                "--dcc",
                "blender",
                "--query",
                "skills",
            ])
        );
        assert!(!search_skills.requires_live_instance);

        let inspect_skill = plan
            .next_steps
            .iter()
            .find(|step| step.name == "inspect-community-skill")
            .expect("inspect-community-skill next step");
        assert_eq!(
            inspect_skill.command.as_ref().unwrap(),
            &argv(&["dcc-mcp-cli", "marketplace", "inspect", "<package-name>"])
        );
        assert!(!inspect_skill.requires_live_instance);

        let install_skill = plan
            .next_steps
            .iter()
            .find(|step| step.name == "install-community-skill")
            .expect("install-community-skill next step");
        assert_eq!(
            install_skill.command.as_ref().unwrap(),
            &argv(&[
                "dcc-mcp-cli",
                "marketplace",
                "install",
                "<package-name>",
                "--dcc",
                "blender",
            ])
        );
        assert!(!install_skill.requires_live_instance);

        let reload = plan
            .next_steps
            .iter()
            .find(|step| step.name == "reload-skills")
            .expect("reload-skills next step");
        assert_eq!(
            reload.command.as_ref().unwrap(),
            &argv(&["dcc-mcp-cli", "reload-skills", "--dcc-type", "blender"])
        );
        assert!(reload.requires_live_instance);
    }

    #[test]
    fn planner_derives_agent_install_instructions_from_adapter_repo_url() {
        let mut entry = catalog_entry("dcc-mcp-maya", &["maya"], None);
        entry.url = Some("https://github.com/dcc-mcp/dcc-mcp-maya".into());
        let plan = InstallPlanner::plan(
            &[entry],
            InstallRequest {
                dcc_type: "maya".into(),
                version: None,
                catalog_path: None,
                python: None,
            },
        )
        .unwrap();

        assert_eq!(plan.next_steps[0].name, "read-install-instructions");
        assert_eq!(
            plan.next_steps[0].url.as_deref(),
            Some("https://raw.githubusercontent.com/dcc-mcp/dcc-mcp-maya/main/install.md")
        );
        assert!(plan.next_steps[0].command.is_none());
    }

    #[test]
    fn planner_prefers_catalog_install_instructions_url() {
        let install = CatalogInstall {
            install_type: "pip".into(),
            url: None,
            ref_: None,
            sha256: None,
            pip_package: Some("dcc-mcp-maya".into()),
            pip_extras: None,
            python_path: None,
            entry_point: None,
            instructions_url: Some("https://example.com/custom-install.md".into()),
        };
        let mut entry = catalog_entry("dcc-mcp-maya", &["maya"], Some(install));
        entry.url = Some("https://github.com/dcc-mcp/dcc-mcp-maya".into());
        let plan = InstallPlanner::plan(
            &[entry],
            InstallRequest {
                dcc_type: "maya".into(),
                version: None,
                catalog_path: None,
                python: None,
            },
        )
        .unwrap();

        assert_eq!(
            plan.next_steps[0].url.as_deref(),
            Some("https://example.com/custom-install.md")
        );
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
            python_path: None,
            entry_point: None,
            instructions_url: None,
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
                python: None,
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
                python: None,
            },
        )
        .unwrap();

        assert_eq!(plan.steps.len(), 4);
        assert!(plan.steps.iter().all(|s| s.action.is_none()));
    }

    #[test]
    fn planner_prefers_requested_python_for_pip_install() {
        let install = CatalogInstall {
            install_type: "pip".into(),
            url: None,
            ref_: None,
            sha256: None,
            pip_package: Some("dcc-mcp-maya".into()),
            pip_extras: None,
            python_path: Some("/catalog/mayapy".into()),
            entry_point: None,
            instructions_url: None,
        };
        let entries = vec![catalog_entry("dcc-mcp-maya", &["maya"], Some(install))];
        let plan = InstallPlanner::plan(
            &entries,
            InstallRequest {
                dcc_type: "maya".into(),
                version: None,
                catalog_path: None,
                python: Some("/custom/mayapy".into()),
            },
        )
        .unwrap();

        match &plan.steps[0].action {
            Some(InstallStepAction::PipInstall { python, .. }) => {
                assert_eq!(python.as_deref(), Some("/custom/mayapy"));
            }
            other => panic!("expected PipInstall action, got {other:?}"),
        }
    }

    #[test]
    fn planner_prefers_adapter_entry_over_skill_pack() {
        let install = CatalogInstall {
            install_type: "pip".into(),
            url: None,
            ref_: None,
            sha256: None,
            pip_package: Some("dcc-mcp-photoshop".into()),
            pip_extras: None,
            python_path: None,
            entry_point: Some("dcc_mcp_photoshop.cli:main".into()),
            instructions_url: None,
        };
        let mut skill_pack = catalog_entry("dcc-mcp-photoshop-skills", &["photoshop"], None);
        skill_pack.tags = vec!["skills".into(), "official".into()];
        let mut adapter = catalog_entry("dcc-mcp-photoshop", &["photoshop"], Some(install));
        adapter.tags = vec!["adapter".into(), "official".into()];

        let plan = InstallPlanner::plan(
            &[skill_pack, adapter],
            InstallRequest {
                dcc_type: "photoshop".into(),
                version: None,
                catalog_path: None,
                python: None,
            },
        )
        .unwrap();

        assert_eq!(plan.adapter.name, "dcc-mcp-photoshop");
        assert!(matches!(
            plan.steps[0].action,
            Some(InstallStepAction::PipInstall { .. })
        ));
    }

    #[test]
    fn planner_accepts_normalized_dcc_aliases() {
        let entries = vec![catalog_entry("dcc-mcp-3dsmax", &["3dsmax"], None)];
        let plan = InstallPlanner::plan(
            &entries,
            InstallRequest {
                dcc_type: "3ds Max".into(),
                version: None,
                catalog_path: None,
                python: None,
            },
        )
        .unwrap();

        assert_eq!(plan.adapter.name, "dcc-mcp-3dsmax");
    }
}
