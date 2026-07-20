use super::*;

fn unique_name(label: &str) -> String {
    format!(
        "dcc-mcp-updater-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    )
}

fn write_fixture(root: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let path = root.join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

fn installation_root(binary_name: &str, current_exe: &Path) -> PathBuf {
    let current_exe = canonical_existing(current_exe).unwrap();
    installation_staging_dir(binary_name, &current_exe).unwrap()
}

#[test]
fn stage_set_rejects_missing_and_hash_mismatch_without_marker() {
    let temp = tempfile::tempdir().unwrap();
    let current = write_fixture(temp.path(), "dcc-mcp-server.exe", b"old-server");
    let missing = temp.path().join("missing.exe");
    let target = UpdateTarget::CurrentExecutable;
    let binary_name = unique_name("invalid");
    let error = stage_update_set_for(
        &binary_name,
        &current,
        &[UpdateSetSource {
            downloaded: &missing,
            target: &target,
            expected_sha256: &"0".repeat(64),
        }],
    )
    .unwrap_err();
    assert!(error.to_string().contains("missing"));
    assert!(
        !installation_root(&binary_name, &current)
            .join(PENDING_SET_DIR)
            .exists()
    );

    let downloaded = write_fixture(temp.path(), "new.exe", b"new-server");
    let error = stage_update_set_for(
        &binary_name,
        &current,
        &[UpdateSetSource {
            downloaded: &downloaded,
            target: &target,
            expected_sha256: &"f".repeat(64),
        }],
    )
    .unwrap_err();
    assert!(matches!(error, UpdateError::ChecksumMismatch { .. }));
    assert!(
        !installation_root(&binary_name, &current)
            .join(PENDING_SET_DIR)
            .exists()
    );
}

#[test]
fn stage_set_rejects_removed_capture_helper() {
    let temp = tempfile::tempdir().unwrap();
    let current = write_fixture(temp.path(), "dcc-mcp-server.exe", b"old-server");
    let downloaded = write_fixture(temp.path(), "new.exe", b"new-server");
    let sha = sha256_file(&downloaded).unwrap();
    let current_target = UpdateTarget::CurrentExecutable;
    let removed_target = UpdateTarget::Sibling {
        file_name: REMOVED_CAPTURE_HELPER.into(),
    };
    let binary_name = unique_name("removed-helper");
    let error = stage_update_set_for(
        &binary_name,
        &current,
        &[
            UpdateSetSource {
                downloaded: &downloaded,
                target: &current_target,
                expected_sha256: &sha,
            },
            UpdateSetSource {
                downloaded: &downloaded,
                target: &removed_target,
                expected_sha256: &sha,
            },
        ],
    )
    .unwrap_err();
    assert!(error.to_string().contains("invalid sibling"));
}

#[test]
fn sibling_target_rejects_windows_ads_and_ambiguous_names() {
    for file_name in [
        "host.exe:stream",
        "host.exe.",
        "host.exe ",
        "host name.exe",
        "host\n.exe",
        "..\\host.exe",
    ] {
        assert!(
            validate_sibling_name(file_name).is_err(),
            "unsafe sibling target was accepted: {file_name:?}"
        );
    }
    validate_sibling_name("dcc-mcp-ui-control-host.exe").unwrap();
}

#[test]
fn update_set_applies_server_and_host_together() {
    let temp = tempfile::tempdir().unwrap();
    let current = write_fixture(temp.path(), "dcc-mcp-server.exe", b"old-server");
    let host = write_fixture(temp.path(), "dcc-mcp-ui-control-host.exe", b"old-host");
    let server_download = write_fixture(temp.path(), "server.download", b"new-server");
    let host_download = write_fixture(temp.path(), "host.download", b"new-host");
    let server_sha = sha256_file(&server_download).unwrap();
    let host_sha = sha256_file(&host_download).unwrap();
    let current_target = UpdateTarget::CurrentExecutable;
    let host_target = UpdateTarget::Sibling {
        file_name: "dcc-mcp-ui-control-host.exe".into(),
    };
    let binary_name = unique_name("apply");
    stage_update_set_for(
        &binary_name,
        &current,
        &[
            UpdateSetSource {
                downloaded: &server_download,
                target: &current_target,
                expected_sha256: &server_sha,
            },
            UpdateSetSource {
                downloaded: &host_download,
                target: &host_target,
                expected_sha256: &host_sha,
            },
        ],
    )
    .unwrap();
    assert!(apply_staged_update_set_for(&binary_name, &current).unwrap());
    assert_eq!(std::fs::read(&current).unwrap(), b"new-server");
    assert_eq!(std::fs::read(&host).unwrap(), b"new-host");
    assert!(
        !installation_root(&binary_name, &current)
            .join(PENDING_SET_DIR)
            .exists()
    );
}

#[test]
fn update_set_rolls_back_first_component_when_second_replace_fails() {
    let temp = tempfile::tempdir().unwrap();
    let current = write_fixture(temp.path(), "dcc-mcp-server.exe", b"old-server");
    let host = write_fixture(temp.path(), "dcc-mcp-ui-control-host.exe", b"old-host");
    let server_download = write_fixture(temp.path(), "server.download", b"new-server");
    let host_download = write_fixture(temp.path(), "host.download", b"new-host");
    let server_sha = sha256_file(&server_download).unwrap();
    let host_sha = sha256_file(&host_download).unwrap();
    let current_target = UpdateTarget::CurrentExecutable;
    let host_target = UpdateTarget::Sibling {
        file_name: "dcc-mcp-ui-control-host.exe".into(),
    };
    let binary_name = unique_name("rollback");
    stage_update_set_for(
        &binary_name,
        &current,
        &[
            UpdateSetSource {
                downloaded: &server_download,
                target: &current_target,
                expected_sha256: &server_sha,
            },
            UpdateSetSource {
                downloaded: &host_download,
                target: &host_target,
                expected_sha256: &host_sha,
            },
        ],
    )
    .unwrap();

    // A directory at the deterministic server backup path forces the
    // second replacement to fail after the sibling host was installed.
    // The transaction must restore the already-replaced host.
    let blocking_backup = current.with_file_name("dcc-mcp-server.exe.dcc-mcp-backup");
    std::fs::create_dir(&blocking_backup).unwrap();
    let error = apply_staged_update_set_for(&binary_name, &current).unwrap_err();
    assert!(matches!(error, UpdateError::Stage(_)));
    assert!(error.to_string().contains("not a regular file"));
    assert_eq!(std::fs::read(&current).unwrap(), b"old-server");
    assert_eq!(std::fs::read(&host).unwrap(), b"old-host");
    assert!(
        installation_root(&binary_name, &current)
            .join(PENDING_SET_DIR)
            .is_dir()
    );
    std::fs::remove_dir(&blocking_backup).unwrap();
    std::fs::remove_dir_all(installation_root(&binary_name, &current)).unwrap();
}

#[test]
fn rollback_preserves_original_before_first_rename() {
    let temp = tempfile::tempdir().unwrap();
    let target = write_fixture(temp.path(), "dcc-mcp-ui-control-host.exe", b"old-host");
    let prepared = write_fixture(
        temp.path(),
        "dcc-mcp-ui-control-host.exe.dcc-mcp-new",
        b"new-host",
    );
    let backup = temp
        .path()
        .join("dcc-mcp-ui-control-host.exe.dcc-mcp-backup");
    let original_sha256 = sha256_file(&target).unwrap();
    let expected_sha256 = sha256_file(&prepared).unwrap();
    let journal = UpdateJournal {
        state: JournalState::Applying,
        entries: vec![JournalEntry {
            target: target.clone(),
            prepared: prepared.clone(),
            backup,
            had_original: true,
            original_sha256: Some(original_sha256),
            expected_sha256,
        }],
    };

    rollback(&journal).unwrap();

    assert_eq!(std::fs::read(target).unwrap(), b"old-host");
    assert!(!prepared.exists());
}

#[test]
fn rollback_recovers_rename_before_backup_state_is_journaled() {
    let temp = tempfile::tempdir().unwrap();
    let target = write_fixture(temp.path(), "dcc-mcp-ui-control-host.exe", b"old-host");
    let prepared = write_fixture(
        temp.path(),
        "dcc-mcp-ui-control-host.exe.dcc-mcp-new",
        b"new-host",
    );
    let backup = temp
        .path()
        .join("dcc-mcp-ui-control-host.exe.dcc-mcp-backup");
    let original_sha256 = sha256_file(&target).unwrap();
    let expected_sha256 = sha256_file(&prepared).unwrap();
    std::fs::rename(&target, &backup).unwrap();
    let journal = UpdateJournal {
        state: JournalState::Applying,
        entries: vec![JournalEntry {
            target: target.clone(),
            prepared: prepared.clone(),
            backup: backup.clone(),
            had_original: true,
            original_sha256: Some(original_sha256),
            expected_sha256,
        }],
    };

    rollback(&journal).unwrap();

    assert_eq!(std::fs::read(target).unwrap(), b"old-host");
    assert_eq!(std::fs::read(backup).unwrap(), b"old-host");
    assert!(!prepared.exists());
}

#[test]
fn rollback_is_idempotent_after_restore_before_journal_removal() {
    let temp = tempfile::tempdir().unwrap();
    let target = write_fixture(temp.path(), "dcc-mcp-server.exe", b"old-server");
    let original_sha256 = sha256_file(&target).unwrap();
    let journal = UpdateJournal {
        state: JournalState::Applying,
        entries: vec![JournalEntry {
            target: target.clone(),
            prepared: temp.path().join("dcc-mcp-server.exe.dcc-mcp-new"),
            backup: temp.path().join("dcc-mcp-server.exe.dcc-mcp-backup"),
            had_original: true,
            original_sha256: Some(original_sha256),
            expected_sha256: sha256_bytes(b"new-server"),
        }],
    };

    rollback(&journal).unwrap();
    rollback(&journal).unwrap();

    assert_eq!(std::fs::read(target).unwrap(), b"old-server");
}

#[test]
fn rollback_recovers_each_install_and_restore_write_window() {
    for (label, target_bytes, prepared_bytes) in [
        (
            "installed-before-journal",
            Some(b"new-server".as_slice()),
            None,
        ),
        (
            "restore-copy-before-remove",
            Some(b"new-server".as_slice()),
            Some(b"old-server".as_slice()),
        ),
        (
            "restore-partial-copy",
            Some(b"new-server".as_slice()),
            Some(b"partial".as_slice()),
        ),
        (
            "restore-remove-before-rename",
            None,
            Some(b"old-server".as_slice()),
        ),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("dcc-mcp-server.exe");
        if let Some(bytes) = target_bytes {
            std::fs::write(&target, bytes).unwrap();
        }
        let prepared = temp.path().join("dcc-mcp-server.exe.dcc-mcp-new");
        if let Some(bytes) = prepared_bytes {
            std::fs::write(&prepared, bytes).unwrap();
        }
        let backup = write_fixture(
            temp.path(),
            "dcc-mcp-server.exe.dcc-mcp-backup",
            b"old-server",
        );
        let journal = UpdateJournal {
            state: JournalState::Applying,
            entries: vec![JournalEntry {
                target: target.clone(),
                prepared: prepared.clone(),
                backup: backup.clone(),
                had_original: true,
                original_sha256: Some(sha256_bytes(b"old-server")),
                expected_sha256: sha256_bytes(b"new-server"),
            }],
        };

        rollback(&journal).unwrap_or_else(|error| panic!("{label}: {error}"));

        assert_eq!(std::fs::read(target).unwrap(), b"old-server", "{label}");
        assert_eq!(std::fs::read(backup).unwrap(), b"old-server", "{label}");
        assert!(!prepared.exists(), "{label}");
    }
}

#[test]
fn apply_retries_a_partial_pre_journal_prepared_copy() {
    let temp = tempfile::tempdir().unwrap();
    let current = write_fixture(temp.path(), "dcc-mcp-server.exe", b"old-server");
    let downloaded = write_fixture(temp.path(), "server.download", b"new-server");
    let sha = sha256_file(&downloaded).unwrap();
    let target = UpdateTarget::CurrentExecutable;
    let binary_name = unique_name("partial-prepare");
    stage_update_set_for(
        &binary_name,
        &current,
        &[UpdateSetSource {
            downloaded: &downloaded,
            target: &target,
            expected_sha256: &sha,
        }],
    )
    .unwrap();
    std::fs::write(
        current.with_file_name("dcc-mcp-server.exe.dcc-mcp-new"),
        b"partial",
    )
    .unwrap();

    assert!(apply_staged_update_set_for(&binary_name, &current).unwrap());
    assert_eq!(std::fs::read(current).unwrap(), b"new-server");
}

#[test]
fn source_and_older_builds_reexec_but_newer_build_does_not() {
    let temp = tempfile::tempdir().unwrap();
    let current = write_fixture(temp.path(), "dcc-mcp-server.exe", b"old-server");
    let downloaded = write_fixture(temp.path(), "server.download", b"new-server");
    let sha = sha256_file(&downloaded).unwrap();
    let target = UpdateTarget::CurrentExecutable;
    let binary_name = unique_name("applied-generation");
    stage_update_set_for_build_with_timeout(
        &binary_name,
        &current,
        "1.0.0",
        &[UpdateSetSource {
            downloaded: &downloaded,
            target: &target,
            expected_sha256: &sha,
        }],
        UPDATE_LOCK_TIMEOUT,
    )
    .unwrap();

    assert!(
        apply_staged_update_set_for_build_with_timeout(
            &binary_name,
            &current,
            "1.0.0",
            UPDATE_LOCK_TIMEOUT,
        )
        .unwrap()
    );
    assert!(
        apply_staged_update_set_for_build_with_timeout(
            &binary_name,
            &current,
            "1.0.0",
            UPDATE_LOCK_TIMEOUT,
        )
        .unwrap()
    );
    assert!(
        apply_staged_update_set_for_build_with_timeout(
            &binary_name,
            &current,
            "0.9.0",
            UPDATE_LOCK_TIMEOUT,
        )
        .unwrap()
    );
    assert!(
        !apply_staged_update_set_for_build_with_timeout(
            &binary_name,
            &current,
            "2.0.0",
            UPDATE_LOCK_TIMEOUT,
        )
        .unwrap()
    );
}

#[test]
fn older_process_does_not_apply_a_later_generation_marker() {
    let temp = tempfile::tempdir().unwrap();
    let current = write_fixture(temp.path(), "dcc-mcp-server.exe", b"server-62");
    let server_63 = write_fixture(temp.path(), "server-63.download", b"server-63");
    let server_64 = write_fixture(temp.path(), "server-64.download", b"server-64");
    let sha_63 = sha256_file(&server_63).unwrap();
    let sha_64 = sha256_file(&server_64).unwrap();
    let target = UpdateTarget::CurrentExecutable;
    let binary_name = unique_name("older-with-new-marker");
    stage_update_set_for_build_with_timeout(
        &binary_name,
        &current,
        "0.19.62",
        &[UpdateSetSource {
            downloaded: &server_63,
            target: &target,
            expected_sha256: &sha_63,
        }],
        UPDATE_LOCK_TIMEOUT,
    )
    .unwrap();
    assert!(
        apply_staged_update_set_for_build_with_timeout(
            &binary_name,
            &current,
            "0.19.62",
            UPDATE_LOCK_TIMEOUT,
        )
        .unwrap()
    );
    stage_update_set_for_build_with_timeout(
        &binary_name,
        &current,
        "0.19.63",
        &[UpdateSetSource {
            downloaded: &server_64,
            target: &target,
            expected_sha256: &sha_64,
        }],
        UPDATE_LOCK_TIMEOUT,
    )
    .unwrap();

    assert!(
        apply_staged_update_set_for_build_with_timeout(
            &binary_name,
            &current,
            "0.19.62",
            UPDATE_LOCK_TIMEOUT,
        )
        .unwrap()
    );
    assert_eq!(std::fs::read(&current).unwrap(), b"server-63");
    assert!(
        apply_staged_update_set_for_build_with_timeout(
            &binary_name,
            &current,
            "0.19.63",
            UPDATE_LOCK_TIMEOUT,
        )
        .unwrap()
    );
    assert_eq!(std::fs::read(current).unwrap(), b"server-64");
}

#[test]
fn committed_recovery_preserves_generation_reexec_decision() {
    let temp = tempfile::tempdir().unwrap();
    let current = write_fixture(temp.path(), "dcc-mcp-server.exe", b"old-server");
    let downloaded = write_fixture(temp.path(), "server.download", b"new-server");
    let original_sha256 = sha256_file(&current).unwrap();
    let expected_sha256 = sha256_file(&downloaded).unwrap();
    let target = UpdateTarget::CurrentExecutable;
    let binary_name = unique_name("committed-generation");
    stage_update_set_for_build_with_timeout(
        &binary_name,
        &current,
        "1.0.0",
        &[UpdateSetSource {
            downloaded: &downloaded,
            target: &target,
            expected_sha256: &expected_sha256,
        }],
        UPDATE_LOCK_TIMEOUT,
    )
    .unwrap();
    let backup = current.with_file_name("dcc-mcp-server.exe.dcc-mcp-backup");
    std::fs::copy(&current, &backup).unwrap();
    std::fs::copy(&downloaded, &current).unwrap();
    let journal = UpdateJournal {
        state: JournalState::Committed,
        entries: vec![JournalEntry {
            target: current.clone(),
            prepared: current.with_file_name("dcc-mcp-server.exe.dcc-mcp-new"),
            backup,
            had_original: true,
            original_sha256: Some(original_sha256),
            expected_sha256,
        }],
    };
    let root = installation_root(&binary_name, &current);
    write_json_atomic(&root.join(PENDING_SET_DIR).join(JOURNAL_FILE), &journal).unwrap();

    assert!(
        apply_staged_update_set_for_build_with_timeout(
            &binary_name,
            &current,
            "1.0.0",
            UPDATE_LOCK_TIMEOUT,
        )
        .unwrap()
    );
    assert!(
        !apply_staged_update_set_for_build_with_timeout(
            &binary_name,
            &current,
            "2.0.0",
            UPDATE_LOCK_TIMEOUT,
        )
        .unwrap()
    );
}

#[test]
fn identical_server_generation_never_requests_reexec() {
    let temp = tempfile::tempdir().unwrap();
    let current = write_fixture(temp.path(), "dcc-mcp-server.exe", b"same-server");
    let downloaded = write_fixture(temp.path(), "server.download", b"same-server");
    let sha = sha256_file(&downloaded).unwrap();
    let target = UpdateTarget::CurrentExecutable;
    let binary_name = unique_name("identical-generation");
    stage_update_set_for_build_with_timeout(
        &binary_name,
        &current,
        "1.0.0",
        &[UpdateSetSource {
            downloaded: &downloaded,
            target: &target,
            expected_sha256: &sha,
        }],
        UPDATE_LOCK_TIMEOUT,
    )
    .unwrap();

    assert!(
        !apply_staged_update_set_for_build_with_timeout(
            &binary_name,
            &current,
            "1.0.0",
            UPDATE_LOCK_TIMEOUT,
        )
        .unwrap()
    );
    assert!(
        !apply_staged_update_set_for_build_with_timeout(
            &binary_name,
            &current,
            "1.0.0",
            UPDATE_LOCK_TIMEOUT,
        )
        .unwrap()
    );
}

#[test]
fn apply_recovers_previous_set_after_stage_swap_crash() {
    let temp = tempfile::tempdir().unwrap();
    let current = write_fixture(temp.path(), "dcc-mcp-server.exe", b"old-server");
    let downloaded = write_fixture(temp.path(), "server.download", b"new-server");
    let sha = sha256_file(&downloaded).unwrap();
    let target = UpdateTarget::CurrentExecutable;
    let binary_name = unique_name("apply-previous");
    stage_update_set_for(
        &binary_name,
        &current,
        &[UpdateSetSource {
            downloaded: &downloaded,
            target: &target,
            expected_sha256: &sha,
        }],
    )
    .unwrap();
    let root = installation_root(&binary_name, &current);
    std::fs::rename(root.join(PENDING_SET_DIR), root.join(PREVIOUS_SET_DIR)).unwrap();

    assert!(apply_staged_update_set_for(&binary_name, &current).unwrap());
    assert_eq!(std::fs::read(current).unwrap(), b"new-server");
    assert!(!root.join(PREVIOUS_SET_DIR).exists());
}

#[test]
fn stage_recovers_previous_set_and_replaces_it_with_new_pending() {
    let temp = tempfile::tempdir().unwrap();
    let current = write_fixture(temp.path(), "dcc-mcp-server.exe", b"old-server");
    let first = write_fixture(temp.path(), "first.download", b"first-server");
    let second = write_fixture(temp.path(), "second.download", b"second-server");
    let first_sha = sha256_file(&first).unwrap();
    let second_sha = sha256_file(&second).unwrap();
    let target = UpdateTarget::CurrentExecutable;
    let binary_name = unique_name("stage-previous");
    stage_update_set_for(
        &binary_name,
        &current,
        &[UpdateSetSource {
            downloaded: &first,
            target: &target,
            expected_sha256: &first_sha,
        }],
    )
    .unwrap();
    let root = installation_root(&binary_name, &current);
    std::fs::rename(root.join(PENDING_SET_DIR), root.join(PREVIOUS_SET_DIR)).unwrap();

    stage_update_set_for(
        &binary_name,
        &current,
        &[UpdateSetSource {
            downloaded: &second,
            target: &target,
            expected_sha256: &second_sha,
        }],
    )
    .unwrap();

    assert!(!root.join(PREVIOUS_SET_DIR).exists());
    assert!(apply_staged_update_set_for(&binary_name, &current).unwrap());
    assert_eq!(std::fs::read(current).unwrap(), b"second-server");
}

#[test]
fn successful_pending_set_discards_stale_previous_set() {
    let temp = tempfile::tempdir().unwrap();
    let current = write_fixture(temp.path(), "dcc-mcp-server.exe", b"old-server");
    let downloaded = write_fixture(temp.path(), "server.download", b"new-server");
    let sha = sha256_file(&downloaded).unwrap();
    let target = UpdateTarget::CurrentExecutable;
    let binary_name = unique_name("stale-previous");
    stage_update_set_for(
        &binary_name,
        &current,
        &[UpdateSetSource {
            downloaded: &downloaded,
            target: &target,
            expected_sha256: &sha,
        }],
    )
    .unwrap();
    let root = installation_root(&binary_name, &current);
    std::fs::create_dir(root.join(PREVIOUS_SET_DIR)).unwrap();

    assert!(apply_staged_update_set_for(&binary_name, &current).unwrap());
    assert!(!root.join(PREVIOUS_SET_DIR).exists());
}

#[test]
fn staged_sets_are_isolated_by_exact_server_installation() {
    let temp = tempfile::tempdir().unwrap();
    let first_dir = temp.path().join("first");
    let second_dir = temp.path().join("second");
    std::fs::create_dir_all(&first_dir).unwrap();
    std::fs::create_dir_all(&second_dir).unwrap();
    let first = write_fixture(&first_dir, "dcc-mcp-server.exe", b"old-first");
    let second = write_fixture(&second_dir, "dcc-mcp-server.exe", b"old-second");
    let downloaded = write_fixture(temp.path(), "server.download", b"new-server");
    let sha = sha256_file(&downloaded).unwrap();
    let target = UpdateTarget::CurrentExecutable;
    let binary_name = unique_name("bound");
    stage_update_set_for(
        &binary_name,
        &first,
        &[UpdateSetSource {
            downloaded: &downloaded,
            target: &target,
            expected_sha256: &sha,
        }],
    )
    .unwrap();
    stage_update_set_for(
        &binary_name,
        &second,
        &[UpdateSetSource {
            downloaded: &downloaded,
            target: &target,
            expected_sha256: &sha,
        }],
    )
    .unwrap();

    let first_root = installation_root(&binary_name, &first);
    let second_root = installation_root(&binary_name, &second);
    assert_ne!(first_root, second_root);
    assert!(first_root.join(PENDING_SET_DIR).is_dir());
    assert!(second_root.join(PENDING_SET_DIR).is_dir());
    assert!(apply_staged_update_set_for(&binary_name, &second).unwrap());
    assert!(apply_staged_update_set_for(&binary_name, &first).unwrap());
    assert_eq!(std::fs::read(&first).unwrap(), b"new-server");
    assert_eq!(std::fs::read(&second).unwrap(), b"new-server");
}

#[test]
fn installation_lock_times_out_without_blocking_another_installation() {
    let temp = tempfile::tempdir().unwrap();
    let first_dir = temp.path().join("first");
    let second_dir = temp.path().join("second");
    std::fs::create_dir_all(&first_dir).unwrap();
    std::fs::create_dir_all(&second_dir).unwrap();
    let first = write_fixture(&first_dir, "dcc-mcp-server.exe", b"old-first");
    let second = write_fixture(&second_dir, "dcc-mcp-server.exe", b"old-second");
    let downloaded = write_fixture(temp.path(), "server.download", b"new-server");
    let sha = sha256_file(&downloaded).unwrap();
    let target = UpdateTarget::CurrentExecutable;
    let source = UpdateSetSource {
        downloaded: &downloaded,
        target: &target,
        expected_sha256: &sha,
    };
    let binary_name = unique_name("file-lock");
    let first_root = installation_root(&binary_name, &first);
    std::fs::create_dir_all(&first_root).unwrap();
    let external_lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(first_root.join(TRANSACTION_LOCK_FILE))
        .unwrap();
    FileExt::lock(&external_lock).unwrap();

    stage_update_set_for_with_timeout(&binary_name, &second, &[source], Duration::from_millis(25))
        .unwrap();
    let error = stage_update_set_for_with_timeout(
        &binary_name,
        &first,
        &[source],
        Duration::from_millis(25),
    )
    .unwrap_err();
    assert!(error.to_string().contains("timed out waiting"));
    FileExt::unlock(&external_lock).unwrap();
}

#[test]
fn in_process_installation_lock_times_out_fail_closed() {
    let temp = tempfile::tempdir().unwrap();
    let current = write_fixture(temp.path(), "dcc-mcp-server.exe", b"old-server");
    let downloaded = write_fixture(temp.path(), "server.download", b"new-server");
    let sha = sha256_file(&downloaded).unwrap();
    let target = UpdateTarget::CurrentExecutable;
    let binary_name = unique_name("process-lock");
    let canonical = canonical_existing(&current).unwrap();
    let root = installation_staging_dir(&binary_name, &canonical).unwrap();
    let _held = acquire_installation_lock(&root, &canonical, Duration::from_millis(25)).unwrap();

    let error = stage_update_set_for_with_timeout(
        &binary_name,
        &current,
        &[UpdateSetSource {
            downloaded: &downloaded,
            target: &target,
            expected_sha256: &sha,
        }],
        Duration::from_millis(25),
    )
    .unwrap_err();
    assert!(error.to_string().contains("in-process update mutex"));
}

#[test]
fn committed_recovery_clears_marker_without_requesting_reexec() {
    let temp = tempfile::tempdir().unwrap();
    let current = write_fixture(temp.path(), "dcc-mcp-server.exe", b"new-server");
    let downloaded = write_fixture(temp.path(), "server.download", b"new-server");
    let sha = sha256_file(&downloaded).unwrap();
    let target = UpdateTarget::CurrentExecutable;
    let binary_name = unique_name("committed-recovery");
    stage_update_set_for(
        &binary_name,
        &current,
        &[UpdateSetSource {
            downloaded: &downloaded,
            target: &target,
            expected_sha256: &sha,
        }],
    )
    .unwrap();
    let prepared = write_fixture(temp.path(), "server.dcc-mcp-new", b"leftover");
    let backup = write_fixture(temp.path(), "server.dcc-mcp-backup", b"leftover");
    let journal = UpdateJournal {
        state: JournalState::Committed,
        entries: vec![JournalEntry {
            target: current.clone(),
            prepared: prepared.clone(),
            backup: backup.clone(),
            had_original: true,
            original_sha256: Some(sha256_file(&backup).unwrap()),
            expected_sha256: sha.clone(),
        }],
    };
    let pending = installation_root(&binary_name, &current).join(PENDING_SET_DIR);
    write_json_atomic(&pending.join(JOURNAL_FILE), &journal).unwrap();

    assert!(!apply_staged_update_set_for(&binary_name, &current).unwrap());
    assert!(!pending.exists());
    assert!(!prepared.exists());
    assert!(!backup.exists());
    assert!(!apply_staged_update_set_for(&binary_name, &current).unwrap());
}

#[test]
fn bootstrap_replaces_missing_or_stale_sibling_after_hash_check() {
    let temp = tempfile::tempdir().unwrap();
    let current = write_fixture(temp.path(), "dcc-mcp-server.exe", b"server");
    let host_download = write_fixture(temp.path(), "host.download", b"new-host");
    let host_sha = sha256_file(&host_download).unwrap();
    let binary_name = unique_name("bootstrap");
    install_verified_sibling(
        &binary_name,
        &current,
        &host_download,
        "dcc-mcp-ui-control-host.exe",
        &host_sha,
    )
    .unwrap();
    let host = temp.path().join("dcc-mcp-ui-control-host.exe");
    assert_eq!(std::fs::read(&host).unwrap(), b"new-host");
    std::fs::write(&host, b"stale-host").unwrap();
    install_verified_sibling(
        &binary_name,
        &current,
        &host_download,
        "dcc-mcp-ui-control-host.exe",
        &host_sha,
    )
    .unwrap();
    assert_eq!(std::fs::read(host).unwrap(), b"new-host");
}
