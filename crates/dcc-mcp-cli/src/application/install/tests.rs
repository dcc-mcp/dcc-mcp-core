use super::*;
use crate::domain::install::InstallStep;

#[test]
fn service_uses_bundled_catalog_when_default_path_is_missing() {
    let service = InstallService::new(PathBuf::from("__missing_dcc_mcp_catalog__.yml"));
    let plan = service
        .plan(InstallRequest {
            dcc_type: "maya".into(),
            version: None,
            catalog_path: None,
            python: Some("test-python".into()),
            dcc_path: None,
            plugin_source: None,
            adobe_debug_root: None,
        })
        .unwrap();

    assert_eq!(plan.adapter.name, "dcc-mcp-maya");
    assert!(matches!(
        plan.steps[0].action,
        Some(InstallStepAction::PipInstall { .. })
    ));
}

#[test]
fn bundled_catalog_prefers_an_adapter_over_its_skill_pack() {
    let service = InstallService::new(PathBuf::from("__missing_dcc_mcp_catalog__.yml"));
    let plan = service
        .plan(InstallRequest {
            dcc_type: "unreal".into(),
            version: None,
            catalog_path: None,
            python: Some("test-python".into()),
            dcc_path: None,
            plugin_source: None,
            adobe_debug_root: None,
        })
        .unwrap();

    assert_eq!(plan.adapter.name, "dcc-mcp-unreal");
    assert!(matches!(
        plan.steps[0].action,
        Some(InstallStepAction::PipInstall { .. })
    ));
}

#[test]
fn bundled_catalog_lists_adapter_dcc_types_and_collapses_aliases() {
    let service = InstallService::new(PathBuf::from("__missing_dcc_mcp_catalog__.yml"));
    let catalog = service.dcc_types(None).unwrap();

    assert!(catalog.custom_types_supported);
    // Registering a new adapter is a legal catalog change, so the catalog
    // size is not an invariant. Derive the bound from the entries this
    // test asserts below instead of pinning a literal: every cataloged
    // type must still be listed, and the reported total must track the
    // listed types.
    let expected_adapters = [
        ("c4d", "dcc-mcp-cinema4d"),
        ("comfyui", "dcc-mcp-comfyui"),
        ("freecad", "dcc-mcp-freecad"),
        ("gimp", "dcc-mcp-gimp"),
        ("godot", "dcc-mcp-godot"),
        ("krita", "dcc-mcp-krita"),
        ("kdenlive", "dcc-mcp-kdenlive"),
        ("liquigen", "dcc-mcp-liquigen"),
        ("mari", "dcc-mcp-mari"),
        ("material-maker", "dcc-mcp-material-maker"),
        ("obs", "dcc-mcp-obs"),
        ("openscad", "dcc-mcp-openscad"),
        ("powerpoint", "dcc-mcp-PowerPoint"),
        ("renderdoc", "dcc-mcp-renderdoc"),
        ("sketchup", "dcc-mcp-sketchup"),
        ("shogun", "dcc-mcp-shogun"),
        ("tiled", "dcc-mcp-tiled"),
        ("touchdesigner", "dcc-mcp-touchdesigner"),
        ("unity", "dcc-mcp-unity"),
        ("wwise", "dcc-mcp-wwise"),
    ];
    assert!(
        catalog.dcc_types.len() >= expected_adapters.len(),
        "the bundled catalog must still list every DCC type asserted below"
    );
    assert_eq!(
        catalog.total,
        catalog.dcc_types.len(),
        "the reported total must track the number of listed DCC types"
    );
    for alias in ["after effects", "after-effects", "comfy-ui"] {
        assert!(
            catalog
                .dcc_types
                .iter()
                .all(|entry| entry.dcc_type != alias),
            "alias {alias} must collapse into its first canonical catalog spelling"
        );
    }
    assert_eq!(
        catalog
            .dcc_types
            .iter()
            .filter(|entry| entry.dcc_type == "3dsmax")
            .count(),
        1
    );
    for (expected_type, expected_adapter) in expected_adapters {
        let entry = catalog
            .dcc_types
            .iter()
            .find(|entry| entry.dcc_type == expected_type)
            .unwrap_or_else(|| panic!("missing cataloged DCC type {expected_type}"));
        assert!(
            entry
                .adapters
                .iter()
                .any(|adapter| adapter.name == expected_adapter)
        );
    }
}

#[test]
fn install_plan_reports_disabled_auto_install_policy_with_custom_prompt() {
    let service = InstallService::with_auto_install_policy(
        PathBuf::from("__missing_dcc_mcp_catalog__.yml"),
        AutoInstallPolicy::disabled(
            "Auto install unavailable; contact PipelineTD to deploy {adapter} for {dcc_type}.",
        ),
    );
    let plan = service
        .plan(InstallRequest {
            dcc_type: "maya".into(),
            version: None,
            catalog_path: None,
            python: Some("test-python".into()),
            dcc_path: None,
            plugin_source: None,
            adobe_debug_root: None,
        })
        .unwrap();

    assert!(!plan.install_policy.auto_install_enabled);
    assert_eq!(
        plan.install_policy.prompt.as_deref(),
        Some("Auto install unavailable; contact PipelineTD to deploy dcc-mcp-maya for maya.")
    );
}

#[test]
fn execute_reports_preflight_failure_when_auto_install_is_disabled() {
    let service = InstallService::with_auto_install_policy(
        PathBuf::from("__missing_dcc_mcp_catalog__.yml"),
        AutoInstallPolicy::disabled("Automatic install disabled; ask PipelineTD for {adapter}."),
    );

    let report = service.execute(
        InstallRequest {
            dcc_type: "maya".into(),
            version: None,
            catalog_path: None,
            python: Some("/__nonexistent__/python".into()),
            dcc_path: None,
            plugin_source: None,
            adobe_debug_root: None,
        },
        true,
    );

    assert_eq!(report.status, "failed");
    assert_eq!(report.stage, "preflight");
    assert_eq!(report.exit_code, 10);
    assert_eq!(report.error.as_ref().unwrap().code, "AUTO_INSTALL_DISABLED");
    assert!(report.steps.iter().all(|step| step.status == "not_run"));
    assert!(!report.verify.directly_usable);
    assert_eq!(report.verify.failure_stage.as_deref(), Some("preflight"));
    assert_eq!(
        report.verify.failure_reason.as_deref(),
        Some("AUTO_INSTALL_DISABLED")
    );
}

#[test]
fn pip_install_missing_python_reports_error() {
    let result = execute_pip_install(
        "nonexistent-package",
        Some("1.0.0"),
        None,
        Some("/__nonexistent__/python"),
        Some(
            "https://files.pythonhosted.org/packages/example/nonexistent_package-1.0.0-py3-none-any.whl",
        ),
        Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
    );
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        matches!(&err, InstallError::StepFailed { step, .. } if step.contains("pip-")),
        "expected StepFailed, got {err}"
    );
}

#[test]
fn git_clone_nonexistent_url_fails() {
    let dest = PathBuf::from("/__nonexistent__/test-repo");
    let result = execute_git_clone("https://__nonexistent__.invalid/repo.git", None, &dest);
    assert!(result.is_err());
}

#[test]
fn git_commit_integrity_fails_before_network_or_filesystem_io() {
    let root = tempfile::tempdir().unwrap();
    let git_dest = root.path().join("git-install");
    let git_error = execute_git_clone(
        "https://__nonexistent__.invalid/repo.git",
        Some("main"),
        &git_dest,
    )
    .unwrap_err();
    assert!(git_error.to_string().contains("40-character commit"));
    assert!(!git_dest.exists());
}

#[test]
fn git_failure_cleans_staged_checkout_without_leaving_destination() {
    let root = tempfile::tempdir().unwrap();
    let dest = root.path().join("git-install");
    let result = execute_git_clone(
        "https://__nonexistent__.invalid/repo.git",
        Some(&"a".repeat(40)),
        &dest,
    );

    assert!(matches!(
        &result,
        Err(InstallError::StepFailed { step, .. }) if step == "git-fetch"
    ));
    assert!(!dest.exists());
    assert!(std::fs::read_dir(root.path()).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".dcc-mcp-install-git-")
    }));
}

#[test]
fn path_copy_missing_source_fails() {
    let result = execute_path_copy(
        &PathBuf::from("/__nonexistent__/source"),
        &PathBuf::from("/__nonexistent__/dest"),
    );
    assert!(result.is_err());
}

#[test]
fn rollback_remove_path_does_not_error_on_nonexistent() {
    let rb = StepRollback::RemovePath(PathBuf::from("/__nonexistent__/path"));
    assert!(execute_rollback(&rb).is_ok());
}

#[test]
fn register_dcc_is_explicitly_deferred() {
    let result = execute_register_dcc("maya", Some("dcc_mcp_maya.cli:main"), None);
    assert!(matches!(result, StepExecution::Deferred));
}

#[test]
fn pip_install_uses_verified_direct_artifact_reference() {
    let spec = verified_pip_artifact_spec(
        "dcc-mcp-maya",
        Some("0.9.22"),
        Some(&["maya".to_string()]),
        Some(
            "https://files.pythonhosted.org/packages/example/dcc_mcp_maya-0.9.22-py3-none-any.whl",
        ),
        Some("A".repeat(64).as_str()),
    )
    .unwrap();
    assert_eq!(
        pip_install_args(&spec),
        vec![
            "-m".to_string(),
            "pip".to_string(),
            "install".to_string(),
            "--upgrade".to_string(),
            format!(
                "dcc-mcp-maya[maya] @ https://files.pythonhosted.org/packages/example/dcc_mcp_maya-0.9.22-py3-none-any.whl#sha256={}",
                "a".repeat(64)
            ),
        ]
    );

    let error = verified_pip_artifact_spec(
        "dcc-mcp-unity",
        Some("0.11.2"),
        None,
        Some("https://pypi.org/project/dcc-mcp-unity"),
        None,
    );
    assert!(error.unwrap_err().to_string().contains("pip-integrity"));
    assert_eq!(
        pip_uninstall_args("dcc-mcp-maya"),
        vec![
            "-m".to_string(),
            "pip".to_string(),
            "uninstall".to_string(),
            "-y".to_string(),
            "dcc-mcp-maya".to_string(),
        ]
    );
    assert_eq!(
        pip_show_args("dcc-mcp-maya"),
        vec![
            "-m".to_string(),
            "pip".to_string(),
            "show".to_string(),
            "dcc-mcp-maya".to_string(),
        ]
    );
    assert_eq!(
        pip_show_version(b"Name: dcc-mcp-maya\nVersion: 0.9.21\n"),
        Some("0.9.21".into())
    );
    assert_eq!(pip_show_version(b"Name: dcc-mcp-maya\n"), None);
    assert_eq!(pip_show_version(b"Version: 0.9.21 beta\n"), None);
}

#[test]
fn pip_rollback_restores_previous_version_and_uninstalls_new_packages() {
    let artifact = PipArtifact {
        version: "0.9.21".into(),
        url: "https://files.pythonhosted.org/example.whl".into(),
        sha256: "a".repeat(64),
        path: PathBuf::from("/tmp/dcc_mcp_maya-0.9.21-py3-none-any.whl"),
        metadata_path: PathBuf::from("/tmp/dcc_mcp_maya-0.9.21.dist-info/direct_url.json"),
        metadata: "{}".into(),
    };
    let expected_args = pip_restore_args_for_artifact("dcc-mcp-maya", &artifact);
    let expected_path = artifact.path.clone();
    let rollback = pip_rollback("dcc-mcp-maya", "python", Some(artifact));
    let StepRollback::PipCommand {
        args,
        artifact: restored,
        ..
    } = rollback
    else {
        panic!("expected pip rollback command");
    };
    assert_eq!(args, expected_args);
    assert_eq!(restored.path, expected_path);

    let StepRollback::Command { args, .. } = pip_rollback("dcc-mcp-maya", "python", None) else {
        panic!("expected uninstall rollback command");
    };
    assert_eq!(args, pip_uninstall_args("dcc-mcp-maya"));
}

fn verify_plan(action: InstallStepAction) -> InstallPlan {
    InstallPlan {
        retry_request: None,
        catalog: None,
        dcc_type: "maya".into(),
        version: None,
        dcc_path: None,
        adapter: dcc_mcp_catalog::CatalogEntry {
            name: "dcc-mcp-maya".into(),
            description: "Maya adapter".into(),
            dcc: vec!["maya".into()],
            targets: vec![],
            url: None,
            issues_url: None,
            tags: vec![],
            version: None,
            min_core_version: None,
            core_requirement: None,
            install: None,
            package: None,
            maintainer: None,
            category: None,
            policy: None,
            requires: None,
            icon: None,
            showcase: None,
        },
        probe_tool: None,
        steps: vec![
            InstallStep {
                name: "install".into(),
                description: "Install adapter".into(),
                action: Some(action),
            },
            InstallStep {
                name: "verify".into(),
                description: "Verify adapter".into(),
                action: Some(InstallStepAction::Verify),
            },
        ],
        next_steps: vec![],
        install_policy: InstallPolicy::enabled(),
        python_source: None,
    }
}

#[test]
fn verify_accepts_non_empty_installed_path() {
    let tmp = tempfile::TempDir::new().unwrap();
    let source = tmp.path().join("source");
    let dest = tmp.path().join("dest");
    fs::create_dir_all(&dest).unwrap();
    fs::write(dest.join("adapter.txt"), "installed").unwrap();

    let result = execute_verify(&verify_plan(InstallStepAction::PathCopy { source, dest }));
    assert!(result.is_ok());
    assert!(result.unwrap().is_none());
}

#[test]
fn verify_rejects_missing_installed_path() {
    let tmp = tempfile::TempDir::new().unwrap();
    let source = tmp.path().join("source");
    let dest = tmp.path().join("missing");

    let err = execute_verify(&verify_plan(InstallStepAction::PathCopy {
        source,
        dest: dest.clone(),
    }))
    .unwrap_err();
    assert!(
        matches!(&err, InstallError::StepFailed { step, message }
            if step == "verify" && message.contains(&dest.display().to_string())),
        "expected verify StepFailed, got {err}"
    );
}

#[test]
fn verify_rejects_empty_installed_directory() {
    let tmp = tempfile::TempDir::new().unwrap();
    let source = tmp.path().join("source");
    let dest = tmp.path().join("empty");
    fs::create_dir_all(&dest).unwrap();

    let err = execute_verify(&verify_plan(InstallStepAction::PathCopy {
        source,
        dest: dest.clone(),
    }))
    .unwrap_err();
    assert!(
        matches!(&err, InstallError::StepFailed { step, message }
            if step == "verify" && message.contains("installed directory is empty")),
        "expected verify StepFailed, got {err}"
    );
}

#[test]
fn adapter_import_verification_requires_the_target_interpreter() {
    let plan = verify_plan(InstallStepAction::Verify);
    let error = verify_target_adapter_import(&plan, "dcc-mcp-maya", None).unwrap_err();
    assert!(
        matches!(error, InstallError::StepFailed { ref step, ref message }
            if step == "verify" && message.contains("target Python interpreter")),
        "expected missing interpreter verification error, got {error}"
    );
}

#[test]
fn uninstall_state_is_absent_when_receipted_destination_is_missing() {
    let root = tempfile::tempdir().unwrap();
    let plan = verify_plan(InstallStepAction::PathCopy {
        source: root.path().join("source"),
        dest: root.path().join("missing"),
    });
    assert!(install_state_is_absent(&plan));
}

#[test]
fn target_core_requirement_prefers_bounded_contract_and_derives_legacy_floor() {
    let mut plan = verify_plan(InstallStepAction::Verify);
    plan.adapter.min_core_version = Some("0.20.14".into());
    assert_eq!(
        target_core_requirement(&plan).as_deref(),
        Some(">=0.20.14,<0.21.0")
    );

    plan.adapter.core_requirement = Some(">=0.20.20,<0.21.0".into());
    assert_eq!(
        target_core_requirement(&plan).as_deref(),
        Some(">=0.20.20,<0.21.0")
    );
}

#[test]
fn execution_report_success_keeps_live_dcc_readiness_unproven() {
    let service = InstallService::new(PathBuf::from("__unused__.yml"));
    let plan = verify_plan(InstallStepAction::PathCopy {
        source: PathBuf::from("source"),
        dest: PathBuf::from("dest"),
    });

    let report = service.execute_plan_with(
        &plan,
        true,
        |_action, _plan| Ok(StepExecution::Completed(None)),
        |_rollback| Ok(()),
    );

    assert_eq!(report.status, "ok");
    assert_eq!(report.stage, "complete");
    assert_eq!(report.exit_code, 0);
    assert!(report.error.is_none());
    assert_eq!(report.steps[0].id, "install-path");
    assert_eq!(report.steps[0].status, "ok");
    assert_eq!(report.steps[0].rollback.status, "not_available");
    assert_eq!(report.steps[1].id, "verify");
    assert_eq!(report.steps[1].status, "ok");
    assert!(!report.verify.directly_usable);
    assert_eq!(
        report.verify.failure_stage.as_deref(),
        Some("host-readiness")
    );
    assert_eq!(
        report.verify.failure_reason.as_deref(),
        Some("LIVE_DCC_VERIFICATION_REQUIRED")
    );
    assert_eq!(report.receipt_path, None);

    let mut actual = serde_json::to_value(report).unwrap();
    actual["core_version"] = serde_json::json!("0.0.0-test");
    let expected: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../../tests/fixtures/install-execution-report-v1-success.json"
    ))
    .unwrap();
    assert_eq!(actual, expected);
}

#[test]
fn execution_report_mid_failure_rolls_back_completed_steps_in_reverse() {
    let service = InstallService::new(PathBuf::from("__unused__.yml"));
    let mut plan = verify_plan(InstallStepAction::PathCopy {
        source: PathBuf::from("source"),
        dest: PathBuf::from("dest"),
    });
    plan.steps.insert(
        1,
        InstallStep {
            name: "register-dcc".into(),
            description: "Register adapter".into(),
            action: Some(InstallStepAction::RegisterDcc {
                dcc_type: "maya".into(),
                entry_point: None,
                probe_tool: None,
                dcc_path: None,
            }),
        },
    );
    let mut calls = 0;
    let mut rollback_order = Vec::new();

    let report = service.execute_plan_with(
        &plan,
        true,
        |_action, _plan| {
            calls += 1;
            if calls == 1 {
                Ok(StepExecution::Completed(Some(StepRollback::RemovePath(
                    PathBuf::from("secret-a"),
                ))))
            } else {
                Err(InstallError::StepFailed {
                    step: "secret-stage".into(),
                    message: "token=super-secret".into(),
                })
            }
        },
        |rollback| {
            if let StepRollback::RemovePath(path) = rollback {
                rollback_order.push(path.clone());
            }
            Ok(())
        },
    );

    assert_eq!(rollback_order, vec![PathBuf::from("secret-a")]);
    assert_eq!(report.status, "failed");
    assert_eq!(report.stage, "install");
    assert_eq!(report.exit_code, 30);
    assert_eq!(report.error.as_ref().unwrap().code, "INSTALL_STEP_FAILED");
    assert_eq!(report.steps[0].status, "ok");
    assert!(report.steps[0].rollback.attempted);
    assert_eq!(report.steps[0].rollback.status, "ok");
    assert_eq!(report.steps[1].status, "failed");
    assert_eq!(report.steps[2].status, "not_run");
    assert!(report.rollback.attempted);
    assert_eq!(report.rollback.status, "ok");
    assert_eq!(report.rollback.failure_count, 0);
    assert_eq!(
        report.verify.failure_reason.as_deref(),
        Some("INSTALL_STEP_FAILED")
    );

    let public = serde_json::to_string(&report).unwrap();
    assert!(!public.contains("secret-a"));
    assert!(!public.contains("secret-stage"));
    assert!(!public.contains("super-secret"));
}

#[test]
fn execution_report_preserves_core_compatibility_failure_details() {
    let service = InstallService::new(PathBuf::from("__unused__.yml"));
    let plan = verify_plan(InstallStepAction::PathCopy {
        source: PathBuf::from("source"),
        dest: PathBuf::from("dest"),
    });

    let report = service.execute_plan_with(
        &plan,
        true,
        |action, _plan| {
            if matches!(action, InstallStepAction::Verify) {
                return Err(InstallError::CoreRequirementFailed {
                    requirement: ">=0.20.0,<0.21.0".into(),
                    running: "0.21.0".into(),
                    verdict: "core_newer_than_declared".into(),
                    declaration_valid: true,
                    reason: "ok".into(),
                });
            }
            Ok(StepExecution::Completed(None))
        },
        |_rollback| Ok(()),
    );

    assert_eq!(report.status, "failed");
    assert_eq!(report.stage, "verify");
    assert_eq!(
        report.error.as_ref().unwrap().code,
        "ADAPTER_CORE_INCOMPATIBLE"
    );
    assert_eq!(
        report.verify.observed_core_compatibility,
        Some(serde_json::json!({
            "requirement": ">=0.20.0,<0.21.0",
            "running": "0.21.0",
            "status": "core_newer_than_declared",
            "declaration_valid": true,
        }))
    );
}

#[test]
fn execution_report_rollback_failure_is_partial_and_stable() {
    let service = InstallService::new(PathBuf::from("__unused__.yml"));
    let mut plan = verify_plan(InstallStepAction::PathCopy {
        source: PathBuf::from("source"),
        dest: PathBuf::from("dest"),
    });
    plan.steps.insert(
        1,
        InstallStep {
            name: "register-dcc".into(),
            description: "Register adapter".into(),
            action: Some(InstallStepAction::RegisterDcc {
                dcc_type: "maya".into(),
                entry_point: None,
                probe_tool: None,
                dcc_path: None,
            }),
        },
    );
    let mut calls = 0;

    let report = service.execute_plan_with(
        &plan,
        true,
        |_action, _plan| {
            calls += 1;
            if calls == 1 {
                Ok(StepExecution::Completed(Some(StepRollback::RemovePath(
                    PathBuf::from("secret-path"),
                ))))
            } else {
                Err(InstallError::StepFailed {
                    step: "install".into(),
                    message: "primary secret".into(),
                })
            }
        },
        |_rollback| {
            Err(InstallError::RollbackFailed {
                step: "secret-program".into(),
                message: "rollback secret".into(),
            })
        },
    );

    assert_eq!(report.status, "partial");
    assert_eq!(report.stage, "rollback");
    assert_eq!(report.exit_code, 30);
    let error = report.error.as_ref().unwrap();
    assert_eq!(error.code, "ROLLBACK_FAILED");
    assert_eq!(error.primary_code.as_deref(), Some("INSTALL_STEP_FAILED"));
    assert_eq!(report.steps[0].rollback.status, "failed");
    assert_eq!(report.rollback.status, "failed");
    assert_eq!(report.rollback.failure_count, 1);
    assert_eq!(
        report.verify.failure_reason.as_deref(),
        Some("ROLLBACK_FAILED")
    );

    let public = serde_json::to_string(&report).unwrap();
    for secret in [
        "secret-path",
        "secret-program",
        "primary secret",
        "rollback secret",
    ] {
        assert!(!public.contains(secret));
    }

    let mut actual = serde_json::to_value(report).unwrap();
    actual["core_version"] = serde_json::json!("0.0.0-test");
    let expected: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../../tests/fixtures/install-execution-report-v1-rollback-failed.json"
    ))
    .unwrap();
    assert_eq!(actual, expected);
}

#[test]
fn failed_execution_report_matches_shared_cross_language_fixture() {
    let service = InstallService::new(PathBuf::from("__unused__.yml"));
    let mut plan = verify_plan(InstallStepAction::PathCopy {
        source: PathBuf::from("source"),
        dest: PathBuf::from("dest"),
    });
    plan.steps.insert(
        1,
        InstallStep {
            name: "register-dcc".into(),
            description: "Register adapter".into(),
            action: Some(InstallStepAction::RegisterDcc {
                dcc_type: "maya".into(),
                entry_point: None,
                probe_tool: None,
                dcc_path: None,
            }),
        },
    );
    let mut calls = 0;
    let report = service.execute_plan_with(
        &plan,
        true,
        |_action, _plan| {
            calls += 1;
            if calls == 1 {
                Ok(StepExecution::Completed(Some(StepRollback::RemovePath(
                    PathBuf::from("private"),
                ))))
            } else {
                Err(InstallError::StepFailed {
                    step: "private".into(),
                    message: "private".into(),
                })
            }
        },
        |_rollback| Ok(()),
    );
    let mut actual = serde_json::to_value(report).unwrap();
    actual["core_version"] = serde_json::json!("0.0.0-test");
    let expected: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../../tests/fixtures/install-execution-report-v1-failed.json"
    ))
    .unwrap();

    assert_eq!(actual, expected);
}

#[test]
fn upgrade_requires_restart_report_is_machine_readable_and_does_not_claim_success() {
    let service = InstallService::new(PathBuf::from("__missing_dcc_mcp_catalog__.yml"));
    let plan = service
        .plan(InstallRequest {
            dcc_type: "maya".into(),
            version: None,
            catalog_path: None,
            python: Some("test-python".into()),
            dcc_path: None,
            plugin_source: None,
            adobe_debug_root: None,
        })
        .unwrap();
    let mut report = execution_report_for_plan(&plan);
    report.status = "running".into();
    let report = upgrade_requires_restart_report(
        report,
        plan.retry_request.as_ref(),
        "sidecar_stop_failed",
        "a live sidecar still holds the adapter payload",
    );

    assert_eq!(report.status, "requires_restart");
    assert_eq!(report.stage, "preflight");
    assert_eq!(report.exit_code, 50);
    assert!(!report.verify.directly_usable);
    assert_eq!(
        report.error.as_ref().unwrap().code,
        "UPGRADE_REQUIRES_RESTART"
    );
    assert_eq!(
        report.error.as_ref().unwrap().primary_code.as_deref(),
        Some("sidecar_stop_failed")
    );
    assert_eq!(report.next_steps[0].id, "retry-after-restart");
    assert!(
        report.next_steps[0]
            .command
            .contains(&"install".to_string())
    );
    assert!(
        report.next_steps[0]
            .command
            .contains(&"--python".to_string())
    );
    assert!(
        report.next_steps[0]
            .command
            .contains(&"test-python".to_string())
    );
}

#[test]
fn failed_sidecar_preflight_never_fails_open_to_pip() {
    // A Python interpreter that cannot launch makes discover_pip_artifact fail
    // during the upgrade preflight. The error must abort before any step runs.
    let service = InstallService::new(PathBuf::from("__missing_dcc_mcp_catalog__.yml"));
    let report = service.execute(
        InstallRequest {
            dcc_type: "maya".into(),
            version: None,
            catalog_path: None,
            python: Some("/__nonexistent__/python".into()),
            dcc_path: None,
            plugin_source: None,
            adobe_debug_root: None,
        },
        true,
    );

    assert_eq!(report.status, "failed");
    assert_eq!(report.stage, "preflight");
    assert_eq!(report.exit_code, 10);
    assert_eq!(
        report.error.as_ref().unwrap().code,
        "SIDECAR_PREFLIGHT_FAILED"
    );
    assert!(report.steps.iter().all(|step| step.status == "not_run"));
    assert!(!report.verify.directly_usable);
}
