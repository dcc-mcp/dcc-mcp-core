use super::*;
use crate::domain::install::{InstallPolicy, InstallStep};

fn path_plan(dest: &Path) -> InstallPlan {
    InstallPlan {
        retry_request: None,
        dcc_type: "fixture".into(),
        version: Some("1.2.3".into()),
        dcc_path: None,
        adapter: serde_json::from_value(serde_json::json!({
            "name": "dcc-mcp-fixture",
            "description": "receipt fixture",
            "version": "1.2.3"
        }))
        .unwrap(),
        probe_tool: None,
        catalog: None,
        steps: vec![InstallStep {
            name: "install-path".into(),
            description: "copy fixture".into(),
            action: Some(InstallStepAction::PathCopy {
                source: PathBuf::from("source"),
                dest: dest.to_path_buf(),
            }),
        }],
        next_steps: Vec::new(),
        install_policy: InstallPolicy::enabled(),
        python_source: None,
    }
}

fn symlink_creation_unavailable(error: &std::io::Error) -> bool {
    error.kind() == std::io::ErrorKind::PermissionDenied || error.raw_os_error() == Some(1314)
}

#[test]
fn receipt_filename_is_bounded_to_an_adapter_identity() {
    assert_eq!(safe_filename("DCC/Maya"), "dcc_maya");
    assert_eq!(safe_filename(""), "adapter");
}

#[test]
fn overlong_legacy_receipt_names_are_treated_as_missing() {
    let mut plan = path_plan(Path::new("destination"));
    plan.adapter.name = "a".repeat(226);
    assert!(legacy_receipt_path_for_plan(&plan).is_none());
    assert!(legacy_non_pip_receipt_path_for_plan(&plan).is_none());
}

#[test]
fn directory_digest_detects_content_and_empty_directory_changes() {
    let root = tempfile::tempdir().unwrap();
    let target = root.path().join("payload");
    fs::create_dir_all(target.join("empty")).unwrap();
    fs::write(target.join("payload.txt"), "one").unwrap();

    let first = path_digest(&target, false).unwrap().unwrap();
    fs::write(target.join("payload.txt"), "two").unwrap();
    let after_content_change = path_digest(&target, false).unwrap().unwrap();
    assert_ne!(after_content_change, first);

    fs::create_dir_all(target.join("another-empty")).unwrap();
    assert_ne!(
        path_digest(&target, false).unwrap().unwrap(),
        after_content_change
    );
}

#[test]
fn receipt_ownership_rejects_changed_or_unexpected_paths() {
    let root = tempfile::tempdir().unwrap();
    let destination = root.path().join("adapter");
    fs::create_dir_all(&destination).unwrap();
    fs::write(destination.join("payload.txt"), "payload").unwrap();
    let plan = path_plan(&destination);
    let mut receipt = receipt_for_plan(&plan).unwrap();
    receipt.owned_paths =
        owned_paths_for_plan(&plan, &[], &[plan.steps[0].action.clone().unwrap()]).unwrap();

    assert!(receipt_ownership_matches(&plan, &receipt).unwrap());
    receipt.owned_paths.push(receipt.owned_paths[0].clone());
    assert!(!receipt_ownership_matches(&plan, &receipt).unwrap());
    receipt.owned_paths.pop();
    fs::write(destination.join("payload.txt"), "changed").unwrap();
    assert!(!receipt_ownership_matches(&plan, &receipt).unwrap());

    receipt.owned_paths[0].path = root.path().join("other").display().to_string();
    assert!(!receipt_ownership_matches(&plan, &receipt).unwrap());
}

#[test]
fn receipt_identity_covers_every_mutating_action() {
    let first = PathBuf::from("first");
    let second = PathBuf::from("second");
    let mut plan = path_plan(&first);
    plan.steps.push(InstallStep {
        name: "install-second-path".into(),
        description: "copy second fixture".into(),
        action: Some(InstallStepAction::PathCopy {
            source: PathBuf::from("source-b"),
            dest: second,
        }),
    });
    let receipt = receipt_for_plan(&plan).unwrap();
    assert_eq!(receipt.actions.len(), 2);

    if let Some(InstallStep {
        action: Some(InstallStepAction::PathCopy { source, .. }),
        ..
    }) = plan.steps.get_mut(1)
    {
        *source = PathBuf::from("source-c");
    }
    assert!(!same_identity(&receipt, &receipt_for_plan(&plan).unwrap()));
}

#[test]
fn adobe_link_digest_binds_the_root_link_without_following_it() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    let other = root.path().join("other");
    let link = root.path().join("link");
    fs::create_dir(&source).unwrap();
    fs::create_dir(&other).unwrap();

    #[cfg(unix)]
    let create_link =
        |target: &Path, destination: &Path| std::os::unix::fs::symlink(target, destination);
    #[cfg(windows)]
    let create_link =
        |target: &Path, destination: &Path| std::os::windows::fs::symlink_dir(target, destination);
    if let Err(error) = create_link(&source, &link) {
        if symlink_creation_unavailable(&error) {
            return;
        }
        panic!("failed to create symlink fixture: {error}");
    }

    let first = path_digest(&link, true).unwrap().unwrap();
    assert!(path_digest(&link, false).is_err());
    #[cfg(unix)]
    fs::remove_file(&link).unwrap();
    #[cfg(windows)]
    fs::remove_dir(&link).unwrap();
    if let Err(error) = create_link(&other, &link) {
        if symlink_creation_unavailable(&error) {
            return;
        }
        panic!("failed to create second symlink fixture: {error}");
    }
    assert_ne!(path_digest(&link, true).unwrap().unwrap(), first);
}

#[cfg(unix)]
#[test]
fn receipt_containment_resolves_symlinks_before_parent_components() {
    let root = tempfile::tempdir().unwrap();
    let destination = root.path().join("destination");
    let target = destination.join("child");
    let link = root.path().join("link");
    fs::create_dir_all(&target).unwrap();

    #[cfg(unix)]
    let create_link =
        |source: &Path, destination: &Path| std::os::unix::fs::symlink(source, destination);
    #[cfg(windows)]
    let create_link =
        |source: &Path, destination: &Path| std::os::windows::fs::symlink_dir(source, destination);
    if let Err(error) = create_link(&target, &link) {
        if symlink_creation_unavailable(&error) {
            return;
        }
        panic!("failed to create containment symlink fixture: {error}");
    }

    let plan = path_plan(&destination);
    let receipt_path = link.join("..").join("receipt.json");
    assert!(validate_receipt_location(&plan, &receipt_path).is_err());

    #[cfg(unix)]
    fs::remove_file(&link).unwrap();
    #[cfg(windows)]
    fs::remove_dir(&link).unwrap();
}

#[test]
fn legacy_pip_receipts_are_normalized() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("legacy.json");
    let legacy = serde_json::json!({
        "schema_version": 1,
        "dcc_type": "maya",
        "adapter_name": "dcc-mcp-maya",
        "adapter_version": "1.2.3",
        "python": "python",
        "package": "dcc-mcp-maya",
        "artifact_url": "https://example.invalid/dcc_mcp_maya-1.2.3-py3-none-any.whl",
        "sha256": "a".repeat(64),
        "extras": []
    });
    fs::write(&path, serde_json::to_vec(&legacy).unwrap()).unwrap();

    let receipt = read_receipt(&path).unwrap().unwrap();
    assert_eq!(receipt.install_type, "pip");
    assert_eq!(receipt.source, receipt.artifact_url);
    assert!(receipt.owned_paths.is_empty());
}

#[test]
fn receipt_commit_and_match_use_the_configured_directory() {
    if std::env::var_os("DCC_MCP_RECEIPT_TEST_CHILD").is_some() {
        let destination = PathBuf::from(std::env::var_os("DCC_MCP_RECEIPT_DEST").unwrap());
        let receipt_dir = PathBuf::from(std::env::var_os("DCC_MCP_INSTALL_RECEIPT_DIR").unwrap());
        let plan = path_plan(&destination);
        let action = plan.steps[0].action.clone().unwrap();
        assert!(commit_install_receipt(&plan, &[], &[action]).unwrap());
        assert_eq!(
            receipt_path_for_plan(&plan).parent(),
            Some(receipt_dir.as_path())
        );
        assert!(receipt_path_for_plan(&plan).is_file());
        assert!(receipt_matches_install(&plan).unwrap());
        let before_drift = read_receipt(&receipt_path_for_plan(&plan))
            .unwrap()
            .unwrap();
        fs::write(destination.join("payload.txt"), "drifted").unwrap();
        assert!(commit_install_receipt(&plan, &[], &[]).is_err());
        assert_eq!(
            read_receipt(&receipt_path_for_plan(&plan))
                .unwrap()
                .unwrap(),
            before_drift
        );
        return;
    }

    let root = tempfile::tempdir().unwrap();
    let destination = root.path().join("adapter");
    fs::create_dir_all(&destination).unwrap();
    fs::write(destination.join("payload.txt"), "payload").unwrap();
    let receipt_dir = root.path().join("receipts");
    let output = std::process::Command::new(std::env::current_exe().unwrap())
            .arg("application::install::receipt::tests::receipt_commit_and_match_use_the_configured_directory")
            .arg("--exact")
            .arg("--nocapture")
            .env("DCC_MCP_RECEIPT_TEST_CHILD", "1")
            .env("DCC_MCP_INSTALL_RECEIPT_DIR", &receipt_dir)
            .env("DCC_MCP_RECEIPT_DEST", &destination)
            .output()
            .unwrap();
    assert!(
        output.status.success(),
        "receipt child test failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn uninstall_ownership_allows_missing_receipted_destinations() {
    let root = tempfile::tempdir().unwrap();
    let destination = root.path().join("adapter");
    fs::create_dir_all(&destination).unwrap();
    fs::write(destination.join("payload.txt"), "payload").unwrap();
    let plan = path_plan(&destination);
    let action = plan.steps[0].action.clone().unwrap();
    let mut receipt = receipt_for_plan(&plan).unwrap();
    receipt.owned_paths = owned_paths_for_plan(&plan, &[], &[action]).unwrap();
    fs::remove_dir_all(&destination).unwrap();
    assert!(receipt_uninstall_ownership_is_safe(&plan, &receipt).unwrap());
}

#[test]
fn uninstall_ownership_rejects_drifted_receipted_destinations() {
    let root = tempfile::tempdir().unwrap();
    let destination = root.path().join("adapter");
    fs::create_dir_all(&destination).unwrap();
    fs::write(destination.join("payload.txt"), "payload").unwrap();
    let plan = path_plan(&destination);
    let action = plan.steps[0].action.clone().unwrap();
    let mut receipt = receipt_for_plan(&plan).unwrap();
    receipt.owned_paths = owned_paths_for_plan(&plan, &[], &[action]).unwrap();
    fs::write(destination.join("payload.txt"), "user data").unwrap();
    assert!(!receipt_uninstall_ownership_is_safe(&plan, &receipt).unwrap());
}

#[test]
fn uninstall_ownership_rejects_non_pip_receipts_without_owned_paths() {
    let root = tempfile::tempdir().unwrap();
    let destination = root.path().join("adapter");
    fs::create_dir_all(&destination).unwrap();
    fs::write(destination.join("payload.txt"), "payload").unwrap();
    let plan = path_plan(&destination);
    let receipt = receipt_for_plan(&plan).unwrap();

    assert!(!receipt_uninstall_ownership_is_safe(&plan, &receipt).unwrap());
}
