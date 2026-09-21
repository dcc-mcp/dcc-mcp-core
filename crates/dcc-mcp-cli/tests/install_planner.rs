//! Integration tests for the install planner in `domain::install`.
//!
//! These tests exercise `InstallPlanner::plan` through the crate public API so
//! the production module stays within the repository file-size budget.

use std::path::PathBuf;

use dcc_mcp_catalog::{CatalogEntry, CatalogInstall};
use dcc_mcp_cli::domain::install::{
    InstallPlanError, InstallPlanner, InstallRequest, InstallStepAction,
};

fn catalog_entry(name: &str, dcc: &[&str], install: Option<CatalogInstall>) -> CatalogEntry {
    CatalogEntry {
        name: name.into(),
        description: "Adapter".into(),
        dcc: dcc.iter().map(|value| value.to_string()).collect(),
        targets: vec![],
        url: Some("https://example.invalid/adapter".into()),
        issues_url: None,
        tags: vec!["official".into()],
        version: Some("0.3.0".into()),
        min_core_version: None,
        install,
        package: None,
        maintainer: None,
        category: None,
        policy: None,
        requires: None,
        icon: None,
        showcase: None,
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
            dcc_path: None,
            plugin_source: None,
            adobe_debug_root: None,
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
fn planner_keeps_custom_dcc_path_in_plan_and_next_steps() {
    let dcc_path = PathBuf::from(r"C:\Custom\Maya\maya.exe");
    let plan = InstallPlanner::plan(
        &[catalog_entry("dcc-mcp-maya", &["maya"], None)],
        InstallRequest {
            dcc_type: "maya".into(),
            version: None,
            catalog_path: None,
            python: None,
            dcc_path: Some(dcc_path.clone()),
            plugin_source: None,
            adobe_debug_root: None,
        },
    )
    .unwrap();

    assert_eq!(plan.dcc_path, Some(dcc_path.clone()));
    let path_step = plan
        .next_steps
        .iter()
        .find(|step| step.name == "resolve-dcc-path")
        .expect("resolve-dcc-path next step");
    assert!(
        path_step
            .description
            .contains(&dcc_path.display().to_string())
    );
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
            dcc_path: None,
            plugin_source: None,
            adobe_debug_root: None,
        },
    )
    .unwrap_err();

    assert_eq!(err, InstallPlanError::UnsupportedDcc("custom".into()));
}

#[test]
fn planner_generates_executable_steps_for_pip_install() {
    let install = CatalogInstall {
        install_type: "pip".into(),
        url: Some(
            "https://files.pythonhosted.org/packages/example/dcc_mcp_maya-0.3.0-py3-none-any.whl"
                .into(),
        ),
        ref_: None,
        sha256: Some("a".repeat(64)),
        skill_roots: None,
        pip_package: Some("dcc-mcp-maya".into()),
        pip_extras: Some(vec!["maya".into()]),
        python_path: Some("/usr/bin/mayapy".into()),
        entry_point: Some("dcc_mcp_maya.cli:main".into()),
        instructions_url: None,
        adobe: None,
        sop_version: None,
        sop_schema_digest: None,
    };
    let entries = vec![catalog_entry("dcc-mcp-maya", &["maya"], Some(install))];
    let plan = InstallPlanner::plan(
        &entries,
        InstallRequest {
            dcc_type: "maya".into(),
            version: None,
            catalog_path: None,
            python: None,
            dcc_path: None,
            plugin_source: None,
            adobe_debug_root: None,
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
        version,
        extras,
        python,
        artifact_url,
        sha256,
    }) = &plan.steps[0].action
    {
        assert_eq!(package, "dcc-mcp-maya");
        assert_eq!(version.as_deref(), Some("0.3.0"));
        assert_eq!(extras.as_deref(), Some(&["maya".into()][..]));
        assert_eq!(python.as_deref(), Some("/usr/bin/mayapy"));
        assert!(artifact_url.as_deref().unwrap().ends_with(".whl"));
        assert_eq!(
            sha256.as_deref(),
            Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
        );
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
            dcc_path: None,
            plugin_source: None,
            adobe_debug_root: None,
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
            dcc_path: None,
            plugin_source: None,
            adobe_debug_root: None,
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
        url: Some(
            "https://files.pythonhosted.org/packages/example/dcc_mcp_maya-0.3.0-py3-none-any.whl"
                .into(),
        ),
        ref_: None,
        sha256: Some("a".repeat(64)),
        skill_roots: None,
        pip_package: Some("dcc-mcp-maya".into()),
        pip_extras: None,
        python_path: None,
        entry_point: None,
        instructions_url: Some("https://example.com/custom-install.md".into()),
        adobe: None,
        sop_version: None,
        sop_schema_digest: None,
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
            dcc_path: None,
            plugin_source: None,
            adobe_debug_root: None,
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
        url: Some("https://github.com/dcc-mcp/dcc-mcp-maya-mgear".into()),
        ref_: Some("a".repeat(40)),
        sha256: None,
        skill_roots: None,
        pip_package: None,
        pip_extras: None,
        python_path: None,
        entry_point: None,
        instructions_url: None,
        adobe: None,
        sop_version: None,
        sop_schema_digest: None,
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
            dcc_path: None,
            plugin_source: None,
            adobe_debug_root: None,
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
fn planner_rejects_mutable_git_and_unverified_zip_installs() {
    let mut install = CatalogInstall {
        install_type: "git".into(),
        url: Some("https://github.com/dcc-mcp/example".into()),
        ref_: Some("main".into()),
        sha256: None,
        skill_roots: None,
        pip_package: None,
        pip_extras: None,
        python_path: None,
        entry_point: None,
        instructions_url: None,
        adobe: None,
        sop_version: None,
        sop_schema_digest: None,
    };
    let request = || InstallRequest {
        dcc_type: "maya".into(),
        version: None,
        catalog_path: None,
        python: None,
        dcc_path: None,
        plugin_source: None,
        adobe_debug_root: None,
    };

    let error = InstallPlanner::plan(
        &[catalog_entry(
            "git-adapter",
            &["maya"],
            Some(install.clone()),
        )],
        request(),
    )
    .unwrap_err();
    assert_eq!(error, InstallPlanError::UnpinnedGitReference);

    install.install_type = "zip".into();
    install.ref_ = None;
    let error = InstallPlanner::plan(
        &[catalog_entry(
            "zip-adapter",
            &["maya"],
            Some(install.clone()),
        )],
        request(),
    )
    .unwrap_err();
    assert_eq!(error, InstallPlanError::InvalidArchiveChecksum);

    install.sha256 = Some(format!("sha256:{}", "a".repeat(64)));
    assert!(
        InstallPlanner::plan(
            &[catalog_entry("zip-adapter", &["maya"], Some(install))],
            request(),
        )
        .is_ok()
    );
}

#[test]
fn planner_rejects_unpinned_pip_artifacts_and_version_overrides() {
    let mut install = CatalogInstall {
        install_type: "pip".into(),
        url: None,
        ref_: None,
        sha256: None,
        skill_roots: None,
        pip_package: Some("dcc-mcp-maya".into()),
        pip_extras: None,
        python_path: None,
        entry_point: None,
        instructions_url: None,
        adobe: None,
        sop_version: None,
        sop_schema_digest: None,
    };
    let entry = |install| catalog_entry("dcc-mcp-maya", &["maya"], Some(install));
    let request = |version: Option<&str>| InstallRequest {
        dcc_type: "maya".into(),
        version: version.map(str::to_string),
        catalog_path: None,
        python: None,
        dcc_path: None,
        plugin_source: None,
        adobe_debug_root: None,
    };

    let error = InstallPlanner::plan(&[entry(install.clone())], request(None)).unwrap_err();
    assert_eq!(error, InstallPlanError::InvalidPipArtifact);

    install.url = Some(
        "https://files.pythonhosted.org/packages/example/dcc_mcp_maya-0.3.0-py3-none-any.whl"
            .into(),
    );
    install.sha256 = Some("a".repeat(64));
    assert!(InstallPlanner::plan(&[entry(install.clone())], request(None)).is_ok());

    let error = InstallPlanner::plan(&[entry(install)], request(Some("0.2.0"))).unwrap_err();
    assert_eq!(
        error,
        InstallPlanError::PipVersionMismatch {
            requested: "0.2.0".into(),
            catalog: "0.3.0".into(),
        }
    );
}

#[test]
fn legacy_pip_plan_deserializes_without_integrity_but_cannot_hide_it() {
    let action: InstallStepAction = serde_json::from_value(serde_json::json!({
        "type": "PipInstall",
        "package": "dcc-mcp-maya",
        "version": "0.9.22"
    }))
    .unwrap();

    match action {
        InstallStepAction::PipInstall {
            artifact_url,
            sha256,
            ..
        } => {
            assert!(artifact_url.is_none());
            assert!(sha256.is_none());
        }
        other => panic!("expected PipInstall action, got {other:?}"),
    }
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
            dcc_path: None,
            plugin_source: None,
            adobe_debug_root: None,
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
        url: Some(
            "https://files.pythonhosted.org/packages/example/dcc_mcp_maya-0.3.0-py3-none-any.whl"
                .into(),
        ),
        ref_: None,
        sha256: Some("a".repeat(64)),
        skill_roots: None,
        pip_package: Some("dcc-mcp-maya".into()),
        pip_extras: None,
        python_path: Some("/catalog/mayapy".into()),
        entry_point: None,
        instructions_url: None,
        adobe: None,
        sop_version: None,
        sop_schema_digest: None,
    };
    let entries = vec![catalog_entry("dcc-mcp-maya", &["maya"], Some(install))];
    let plan = InstallPlanner::plan(
        &entries,
        InstallRequest {
            dcc_type: "maya".into(),
            version: None,
            catalog_path: None,
            python: Some("/custom/mayapy".into()),
            dcc_path: None,
            plugin_source: None,
            adobe_debug_root: None,
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
fn planner_builds_adobe_debug_link_from_operator_paths() {
    let install = CatalogInstall {
        install_type: "pip".into(),
        url: Some(
            "https://files.pythonhosted.org/packages/example/dcc_mcp_premiere-0.3.0-py3-none-any.whl"
                .into(),
        ),
        ref_: None,
        sha256: Some("a".repeat(64)),
        skill_roots: None,
        pip_package: Some("dcc-mcp-premiere".into()),
        pip_extras: None,
        python_path: None,
        entry_point: Some("dcc_mcp_premiere:PremiereMcpServer".into()),
        instructions_url: None,
        adobe: Some(dcc_mcp_catalog::CatalogAdobeInstall {
            product: "premierepro".into(),
            extension_type: "uxp".into(),
            plugin_id: Some("com.dccmcp.premiere".into()),
            source_subpath: Some("src/dcc_mcp_premiere/premiere_uxp".into()),
            manifest_path: Some("manifest.json".into()),
            target_subpath: None,
        }),
        sop_version: None,
        sop_schema_digest: None,
    };
    let plan = InstallPlanner::plan(
        &[catalog_entry(
            "dcc-mcp-premiere",
            &["premiere"],
            Some(install),
        )],
        InstallRequest {
            dcc_type: "premiere".into(),
            version: Some("0.3.0".into()),
            catalog_path: None,
            python: None,
            dcc_path: None,
            plugin_source: Some("F:/internal/dcc-mcp-premiere".into()),
            adobe_debug_root: Some("F:/internal/adobe-debug".into()),
        },
    )
    .unwrap();

    let action = plan.steps[1].action.as_ref().unwrap();
    assert!(matches!(
        action,
        InstallStepAction::AdobePluginLink {
            product,
            extension_type,
            source,
            dest,
            manifest,
        } if product == "premierepro"
            && extension_type == "uxp"
            && source.ends_with("src/dcc_mcp_premiere/premiere_uxp")
            && dest.ends_with("com.dccmcp.premiere")
            && manifest.as_ref().is_some_and(|path| path.ends_with("manifest.json"))
    ));
}

#[test]
fn planner_prefers_adapter_entry_over_skill_pack() {
    let install = CatalogInstall {
        install_type: "pip".into(),
        url: Some(
            "https://files.pythonhosted.org/packages/example/dcc_mcp_photoshop-0.3.0-py3-none-any.whl"
                .into(),
        ),
        ref_: None,
        sha256: Some("a".repeat(64)),
        skill_roots: None,
        pip_package: Some("dcc-mcp-photoshop".into()),
        pip_extras: None,
        python_path: None,
        entry_point: Some("dcc_mcp_photoshop.cli:main".into()),
        instructions_url: None,
        adobe: None,
        sop_version: None,
        sop_schema_digest: None,
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
            dcc_path: None,
            plugin_source: None,
            adobe_debug_root: None,
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
            dcc_path: None,
            plugin_source: None,
            adobe_debug_root: None,
        },
    )
    .unwrap();

    assert_eq!(plan.adapter.name, "dcc-mcp-3dsmax");
}
