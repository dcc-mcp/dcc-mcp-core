use super::*;

#[test]
fn actual_planner_never_reports_manual_registration_as_ok() {
    let service = InstallService::new(PathBuf::from("__missing_dcc_mcp_catalog__.yml"));
    let plan = service
        .plan(InstallRequest {
            dcc_type: "maya".into(),
            version: None,
            catalog_path: None,
            python: Some("test-python".into()),
            dcc_path: None,
        })
        .unwrap();
    let register_index = plan
        .steps
        .iter()
        .position(|step| matches!(step.action, Some(InstallStepAction::RegisterDcc { .. })))
        .expect("the real planner must include register-dcc");

    let report = service.execute_plan_with(
        &plan,
        true,
        |action, _plan| match action {
            InstallStepAction::RegisterDcc { .. } => execute_action(action),
            _ => Ok(StepExecution::Completed(None)),
        },
        |_rollback| Ok(()),
    );

    assert_eq!(report.steps[register_index].id, "register-dcc");
    assert_eq!(report.steps[register_index].status, "deferred");
    assert_ne!(report.steps[register_index].status, "ok");
    assert_eq!(report.status, "partial");
    assert_eq!(report.stage, "complete");
    assert_eq!(report.exit_code, 0);
    assert!(report.error.is_none());
    assert!(!report.verify.directly_usable);

    let mut actual = serde_json::to_value(&report).unwrap();
    actual["adapter_version"] = serde_json::json!("0.0.0-test");
    actual["core_version"] = serde_json::json!("0.0.0-test");
    let expected: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../../tests/fixtures/install-execution-report-v1-deferred.json"
    ))
    .unwrap();
    assert_eq!(actual, expected);
}
