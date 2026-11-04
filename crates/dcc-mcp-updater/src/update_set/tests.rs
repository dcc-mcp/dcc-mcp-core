use super::committed_cleanup::backup_path_for_transaction;
#[cfg(windows)]
use super::committed_cleanup::is_deferred_image_lock;
use super::io_diagnostics::{
    IoOp, record_io_failure, redact_path, redact_path_with_lookup, redact_path_with_prefixes,
    redact_path_with_sources, sanitize,
};
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

/// Removes one test's isolated staging subtree on every exit path, including
/// assertion panics. Staging writes under the user's real data dir (`APPDATA`
/// on Windows), which a `tempfile::tempdir()` does not cover; `unique_name`
/// keeps that subtree isolated from real user data and from other tests.
#[cfg(windows)]
struct StagingRootGuard(PathBuf);

#[cfg(windows)]
impl StagingRootGuard {
    fn new(binary_name: &str) -> Self {
        Self(crate::staging_dir(binary_name).expect("test staging dir must be resolvable"))
    }
}

#[cfg(windows)]
impl Drop for StagingRootGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
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
fn single_binary_update_rejects_tampered_staged_bytes() {
    let temp = tempfile::tempdir().unwrap();
    let current = write_fixture(temp.path(), "dcc-mcp-cli.exe", b"old-cli");
    let downloaded = write_fixture(temp.path(), "cli.download", b"new-cli");
    let expected_sha256 = sha256_file(&downloaded).unwrap();
    let binary_name = unique_name("tampered-single");

    stage_verified_binary_update_for(&binary_name, &current, &downloaded, &expected_sha256)
        .unwrap();
    let staged = installation_root(&binary_name, &current)
        .join(PENDING_SET_DIR)
        .join("component-0.bin");
    std::fs::write(staged, b"tampered-cli").unwrap();

    let error = apply_staged_binary_update_for(&binary_name, &current).unwrap_err();

    assert!(matches!(error, UpdateError::RejectedStagedUpdate { .. }));
    assert_eq!(std::fs::read(&current).unwrap(), b"old-cli");
    assert!(
        !installation_root(&binary_name, &current)
            .join(PENDING_SET_DIR)
            .exists()
    );
    assert!(!apply_staged_binary_update_for(&binary_name, &current).unwrap());
}

#[test]
fn single_binary_update_rejects_staged_sibling_components() {
    let temp = tempfile::tempdir().unwrap();
    let current = write_fixture(temp.path(), "dcc-mcp-server.exe", b"old-server");
    let sibling = write_fixture(temp.path(), "dcc-mcp-host.exe", b"old-host");
    let server_download = write_fixture(temp.path(), "server.download", b"new-server");
    let host_download = write_fixture(temp.path(), "host.download", b"new-host");
    let server_sha = sha256_file(&server_download).unwrap();
    let host_sha = sha256_file(&host_download).unwrap();
    let current_target = UpdateTarget::CurrentExecutable;
    let sibling_target = UpdateTarget::Sibling {
        file_name: "dcc-mcp-host.exe".into(),
    };
    let binary_name = unique_name("foreign-set");
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
                target: &sibling_target,
                expected_sha256: &host_sha,
            },
        ],
    )
    .unwrap();

    let error = apply_staged_binary_update_for(&binary_name, &current).unwrap_err();

    assert!(matches!(error, UpdateError::RejectedStagedUpdate { .. }));
    assert!(error.to_string().contains("single binary"));
    assert_eq!(std::fs::read(&current).unwrap(), b"old-server");
    assert_eq!(std::fs::read(sibling).unwrap(), b"old-host");
    assert!(
        !installation_root(&binary_name, &current)
            .join(PENDING_SET_DIR)
            .exists()
    );
}

#[test]
fn clearing_a_staged_binary_update_preserves_the_current_executable() {
    let temp = tempfile::tempdir().unwrap();
    let current = write_fixture(temp.path(), "dcc-mcp-cli.exe", b"old-cli");
    let downloaded = write_fixture(temp.path(), "cli.download", b"new-cli");
    let expected_sha256 = sha256_file(&downloaded).unwrap();
    let binary_name = unique_name("clear-single");
    stage_verified_binary_update_for(&binary_name, &current, &downloaded, &expected_sha256)
        .unwrap();

    clear_staged_binary_update_for(&binary_name, &current).unwrap();

    assert!(!apply_staged_binary_update_for(&binary_name, &current).unwrap());
    assert_eq!(std::fs::read(current).unwrap(), b"old-cli");
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
    validate_sibling_name("dcc-mcp-cli.exe").unwrap();
}

#[test]
fn update_set_applies_server_and_cli_together() {
    let temp = tempfile::tempdir().unwrap();
    let current = write_fixture(temp.path(), "dcc-mcp-server.exe", b"old-server");
    let host = write_fixture(temp.path(), "dcc-mcp-cli.exe", b"old-cli");
    let server_download = write_fixture(temp.path(), "server.download", b"new-server");
    let host_download = write_fixture(temp.path(), "cli.download", b"new-cli");
    let server_sha = sha256_file(&server_download).unwrap();
    let host_sha = sha256_file(&host_download).unwrap();
    let current_target = UpdateTarget::CurrentExecutable;
    let host_target = UpdateTarget::Sibling {
        file_name: "dcc-mcp-cli.exe".into(),
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
    assert_eq!(std::fs::read(&host).unwrap(), b"new-cli");
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
    let host = write_fixture(temp.path(), "dcc-mcp-cli.exe", b"old-cli");
    let server_download = write_fixture(temp.path(), "server.download", b"new-server");
    let host_download = write_fixture(temp.path(), "cli.download", b"new-cli");
    let server_sha = sha256_file(&server_download).unwrap();
    let host_sha = sha256_file(&host_download).unwrap();
    let current_target = UpdateTarget::CurrentExecutable;
    let host_target = UpdateTarget::Sibling {
        file_name: "dcc-mcp-cli.exe".into(),
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
    // second replacement to fail after the sibling CLI was installed.
    // The transaction must restore the already-replaced CLI.
    let blocking_backup = current.with_file_name("dcc-mcp-server.exe.dcc-mcp-backup");
    std::fs::create_dir(&blocking_backup).unwrap();
    let error = apply_staged_update_set_for(&binary_name, &current).unwrap_err();
    assert!(matches!(error, UpdateError::Stage(_)));
    assert!(error.to_string().contains("not a regular file"));
    assert_eq!(std::fs::read(&current).unwrap(), b"old-server");
    assert_eq!(std::fs::read(&host).unwrap(), b"old-cli");
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
    let target = write_fixture(temp.path(), "dcc-mcp-cli.exe", b"old-cli");
    let prepared = write_fixture(temp.path(), "dcc-mcp-cli.exe.dcc-mcp-new", b"new-cli");
    let backup = temp.path().join("dcc-mcp-cli.exe.dcc-mcp-backup");
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

    assert_eq!(std::fs::read(target).unwrap(), b"old-cli");
    assert!(!prepared.exists());
}

#[test]
fn rollback_recovers_rename_before_backup_state_is_journaled() {
    let temp = tempfile::tempdir().unwrap();
    let target = write_fixture(temp.path(), "dcc-mcp-cli.exe", b"old-cli");
    let prepared = write_fixture(temp.path(), "dcc-mcp-cli.exe.dcc-mcp-new", b"new-cli");
    let backup = temp.path().join("dcc-mcp-cli.exe.dcc-mcp-backup");
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

    assert_eq!(std::fs::read(target).unwrap(), b"old-cli");
    assert_eq!(std::fs::read(backup).unwrap(), b"old-cli");
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
        "dcc-mcp-cli.exe",
        &host_sha,
    )
    .unwrap();
    let host = temp.path().join("dcc-mcp-cli.exe");
    assert_eq!(std::fs::read(&host).unwrap(), b"new-host");
    std::fs::write(&host, b"stale-host").unwrap();
    install_verified_sibling(
        &binary_name,
        &current,
        &host_download,
        "dcc-mcp-cli.exe",
        &host_sha,
    )
    .unwrap();
    assert_eq!(std::fs::read(host).unwrap(), b"new-host");
}

// ── Path-safe I/O diagnostics (regression for #2864) ──────────────────────
//
// These tests only exercise the diagnostic layer: they must NOT assume a
// particular activation fix. The activation bug itself is still open; what is
// verified here is that a failing staged-update filesystem operation carries
// a diagnosable, path-free context and preserves the original OS error and
// rollback semantics.

fn io_op_fixture() -> IoOp {
    IoOp {
        stage: "apply",
        operation: "rename_target_to_backup",
        source: INSTALLED_COMPONENT,
        target: "backup",
    }
}

#[test]
fn record_io_failure_preserves_raw_os_error_and_path_free_context() {
    let temp = tempfile::tempdir().unwrap();
    let secret_dir = temp.path().join("Users").join("private-user").join("bin");
    let source = secret_dir.join("dcc-mcp-cli.exe.dcc-mcp-new");
    let target = secret_dir.join("dcc-mcp-cli.exe");
    let code = if cfg!(windows) { 5 } else { 13 };
    let io_error = std::io::Error::from_raw_os_error(code);

    let error =
        record_io_failure(io_op_fixture(), &source, &target, Err::<(), _>(io_error)).unwrap_err();

    // The original OS error stays reachable through the error source chain.
    let source_io: &std::io::Error = match &error {
        UpdateError::IoWithContext { error, .. } => error,
        other => panic!("expected IoWithContext, got {other:?}"),
    };
    assert_eq!(source_io.raw_os_error(), Some(code));

    // The public error names the operation and raw OS code but contains no
    // absolute path (no user directory / credential material).
    let message = error.to_string();
    assert!(message.contains("apply"), "missing stage: {message}");
    assert!(
        message.contains("rename_target_to_backup"),
        "missing operation: {message}"
    );
    assert!(
        message.contains("installed-component") && message.contains("backup"),
        "missing path categories: {message}"
    );
    assert!(
        message.contains(&format!("os error {code}")),
        "missing raw OS error: {message}"
    );
    assert!(
        !message.contains("private-user") && !message.contains(&*temp.path().to_string_lossy()),
        "absolute path leaked into public error: {message}"
    );
}

#[test]
fn diagnostic_helpers_redact_and_bound_sensitive_text() {
    // A path under the user profile must have that prefix redacted so no
    // user directory leaks into the controlled local diagnostic channel.
    if let Some(home) = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")) {
        let home = PathBuf::from(home);
        let path = home
            .join("AppData")
            .join("Roaming")
            .join("update")
            .join("dcc-mcp-cli.exe");
        let redacted = redact_path(&path);
        assert!(
            !redacted.contains(&*home.to_string_lossy()),
            "home prefix was not redacted: {redacted}"
        );
        assert!(
            redacted.ends_with("dcc-mcp-cli.exe"),
            "redaction dropped the diagnostically useful file name: {redacted}"
        );
    }
    // Control characters are stripped and output is bounded.
    let noisy = format!("ok\n\u{1b}[31m{}", "x".repeat(1024));
    let clean = sanitize(&noisy, 32);
    assert_eq!(clean.chars().count(), 32);
    assert!(!clean.contains('\n'));
    assert!(!clean.contains('\u{1b}'));
}

#[test]
fn redact_path_is_case_insensitive_and_handles_device_prefixes() {
    // The opt-in diagnostic channel is used for byte-level forensics on an
    // affected host, so a surviving profile directory is a privacy leak.
    // Windows paths are case-insensitive and may use extended-length / UNC
    // device prefixes; every spelling must still be redacted.
    let home = r"C:\Users\hallong";
    let prefixes = vec![(home.to_string(), "<home>")];
    for case in [
        r"C:\Users\hallong\secret\cli.exe",
        r"C:\USERS\HALLONG\secret\cli.exe",
        r"C:/USERS/HALLONG/secret/cli.exe",
        r"\\?\C:\USERS\HALLONG\secret\cli.exe",
        r"//?/C:/USERS/HALLONG/secret/cli.exe",
    ] {
        let redacted = redact_path_with_prefixes(Path::new(case), &prefixes);
        assert!(
            !redacted.to_ascii_lowercase().contains("hallong"),
            "profile path survived redaction for {case}: {redacted}"
        );
        assert!(
            redacted.ends_with("cli.exe"),
            "diagnostic file name lost for {case}: {redacted}"
        );
    }

    // The environment value itself may be written with a device prefix.
    let long_prefix = vec![(r"\\?\C:\Users\hallong".to_string(), "<home>")];
    let redacted =
        redact_path_with_prefixes(Path::new(r"C:\USERS\HALLONG\secret\cli.exe"), &long_prefix);
    assert!(
        !redacted.to_ascii_lowercase().contains("hallong"),
        "device-prefixed env prefix was not redacted: {redacted}"
    );
    assert!(redacted.ends_with("cli.exe"), "file name lost: {redacted}");

    // A UNC profile path may appear on the failing path in extended-length
    // UNC form; both spellings of the prefix must be recognized.
    let unc_prefix = vec![(r"\\server\share\hallong".to_string(), "<appdata>")];
    for case in [
        r"\\server\share\hallong\secret\cli.exe",
        r"\\?\UNC\server\share\hallong\secret\cli.exe",
        r"//?/UNC/server/share/hallong/secret/cli.exe",
        r"\\?\UNC\SERVER\SHARE\HALLONG\secret\cli.exe",
    ] {
        let redacted = redact_path_with_prefixes(Path::new(case), &unc_prefix);
        assert!(
            !redacted.to_ascii_lowercase().contains("hallong"),
            "UNC profile path survived redaction for {case}: {redacted}"
        );
        assert!(
            redacted.ends_with("cli.exe"),
            "diagnostic file name lost for {case}: {redacted}"
        );
    }
}

#[test]
fn redact_path_collects_xdg_data_home_outside_home() {
    // On Linux `dirs_data_dir()` prefers `XDG_DATA_HOME` as the staging root.
    // When it is configured outside `HOME`, that prefix must still be collected
    // or the full staging path leaks into the opt-in diagnostic channel.
    let path = Path::new("/var/cache/dcc-mcp-private/update/example/cli.exe");
    let redacted = redact_path_with_lookup(path, |key| match key {
        "HOME" => Some("/home/alice".to_string()),
        "XDG_DATA_HOME" => Some("/var/cache/dcc-mcp-private".to_string()),
        _ => None,
    });
    assert!(
        !redacted.contains("/var/cache") && !redacted.contains("dcc-mcp-private"),
        "external XDG staging root leaked: {redacted}"
    );
    assert!(
        redacted.contains("<xdg-data-home>"),
        "XDG prefix label missing: {redacted}"
    );
    assert!(
        redacted.ends_with("update/example/cli.exe"),
        "diagnostic path tail lost: {redacted}"
    );

    // The `HOME/.local/share` fallback remains covered by the `HOME` prefix
    // when `XDG_DATA_HOME` is unset.
    let fallback = Path::new("/home/alice/.local/share/update/example/cli.exe");
    let redacted_fallback = redact_path_with_lookup(fallback, |key| match key {
        "HOME" => Some("/home/alice".to_string()),
        _ => None,
    });
    assert!(
        !redacted_fallback.contains("/home/alice"),
        "HOME fallback prefix leaked: {redacted_fallback}"
    );
    assert!(
        redacted_fallback.ends_with("update/example/cli.exe"),
        "HOME fallback path tail lost: {redacted_fallback}"
    );
}

#[test]
fn redact_path_collects_canonical_spelling_of_symlinked_prefix() {
    // `canonical_existing()` resolves `current_exe` with
    // `std::fs::canonicalize()`. If an env prefix is a symlink/junction to the
    // real install directory, the failing path carries the *resolved* spelling,
    // which the raw env value no longer matches. Without collecting the
    // canonical spelling too, the opt-in debug channel leaks the resolved
    // install directory.
    let raw_prefix = r"C:\Users\hallong\AppData\Local\dcc-mcp";
    let canonical_prefix = r"\\?\D:\RealInstall\dcc-mcp";
    let failing_path = Path::new(r"\\?\D:\RealInstall\dcc-mcp\dcc-mcp-cli.exe");

    // Control: redacting with only the raw prefix cannot match the resolved
    // path, so the install directory survives (this is the bug being fixed).
    let raw_only =
        redact_path_with_prefixes(failing_path, &[(raw_prefix.to_string(), "<localappdata>")]);
    assert!(
        raw_only.to_ascii_lowercase().contains("realinstall"),
        "control no longer reproduces the leak: {raw_only}"
    );

    // With the canonicalizer injected, the resolved spelling is collected and
    // redacted under the same label, while the file name is preserved.
    let redacted = redact_path_with_sources(
        failing_path,
        |key| match key {
            "LOCALAPPDATA" => Some(raw_prefix.to_string()),
            _ => None,
        },
        |prefix| {
            assert_eq!(prefix, Path::new(raw_prefix));
            Some(PathBuf::from(canonical_prefix))
        },
    );
    assert!(
        !redacted.to_ascii_lowercase().contains("realinstall"),
        "resolved install directory leaked: {redacted}"
    );
    assert!(
        redacted.contains("<localappdata>"),
        "canonical prefix was not labeled: {redacted}"
    );
    assert!(
        redacted.ends_with("dcc-mcp-cli.exe"),
        "diagnostic file name lost: {redacted}"
    );
}

#[test]
fn redact_path_with_lookup_uses_real_canonical_spelling() {
    // The production seam wires `std::fs::canonicalize()` into prefix
    // collection. Exercise that real call path (no injected canonicalizer) with
    // a genuine on-disk directory whose raw spelling canonicalizes to a
    // different string, modelling a symlink/junction that resolves elsewhere.
    let temp = tempfile::tempdir().unwrap();
    let install = temp.path().join("install");
    std::fs::create_dir(&install).unwrap();
    // A `.` component is normalized away by canonicalize on every platform,
    // so the resolved spelling is guaranteed to differ from the raw prefix.
    let raw = temp.path().join(".").join("install");
    let raw_str = raw.to_string_lossy().into_owned();
    let resolved = std::fs::canonicalize(&raw).unwrap();
    let resolved_str = resolved.to_string_lossy().into_owned();
    assert_ne!(
        resolved_str, raw_str,
        "canonicalize did not change the spelling"
    );
    let failing = resolved.join("dcc-mcp-cli.exe");

    let redacted = redact_path_with_lookup(&failing, |key| match key {
        "LOCALAPPDATA" => Some(raw_str.clone()),
        _ => None,
    });
    assert!(
        !redacted.contains(&resolved_str),
        "real canonicalized install directory leaked: {redacted}"
    );
    assert!(
        redacted.contains("<localappdata>"),
        "canonical prefix not labeled by production seam: {redacted}"
    );
    assert!(
        redacted.ends_with("dcc-mcp-cli.exe"),
        "diagnostic file name lost: {redacted}"
    );
}

#[test]
fn redact_path_redacts_cross_drive_descendant_junction() {
    // A junction *below* a protected prefix (not the prefix itself) can
    // resolve `current_exe` onto another drive. The resolved path keeps the
    // profile identity (`Users\hallong`) but matches neither the raw env
    // prefix nor the prefix's own canonical spelling, which is the case
    // prefix canonicalization alone cannot cover.
    let raw_prefix = r"C:\Users\hallong\AppData\Local";
    let canonical_prefix = r"\\?\C:\Users\hallong\AppData\Local";
    let failing = Path::new(r"\\?\D:\Users\hallong\RealInstall\dcc-mcp\dcc-mcp-cli.exe");

    // Control: with no protected prefix collected the raw path survives, so
    // the synthetic fixture really does reproduce the identity leak.
    let unredacted = redact_path_with_prefixes(failing, &[]);
    assert!(
        unredacted.to_ascii_lowercase().contains("hallong"),
        "control no longer reproduces the leak: {unredacted}"
    );

    let assert_redacted = |redacted: String| {
        assert!(
            !redacted.to_ascii_lowercase().contains("hallong"),
            "profile identity leaked after descendant junction: {redacted}"
        );
        assert!(
            redacted.contains("<localappdata>"),
            "derived path was not labeled: {redacted}"
        );
        assert!(
            redacted.contains("RealInstall"),
            "diagnostic tail of the resolved target was dropped: {redacted}"
        );
        assert!(
            redacted.ends_with("dcc-mcp-cli.exe"),
            "diagnostic file name lost: {redacted}"
        );
    };

    // Injected source: only the prefix itself is canonicalized; the junction
    // is a descendant, so its resolved spelling still carries the identity.
    assert_redacted(redact_path_with_sources(
        failing,
        |key| match key {
            "LOCALAPPDATA" => Some(raw_prefix.to_string()),
            _ => None,
        },
        |prefix| {
            assert_eq!(prefix, Path::new(raw_prefix));
            Some(PathBuf::from(canonical_prefix))
        },
    ));

    // Production seam: the same path stays redacted with the real
    // `std::fs::canonicalize()` wiring (no injected canonicalizer).
    assert_redacted(redact_path_with_lookup(failing, |key| match key {
        "LOCALAPPDATA" => Some(raw_prefix.to_string()),
        _ => None,
    }));
}

#[test]
fn redact_path_keeps_similar_but_unrelated_paths_visible() {
    // Identity redaction is a bounded fallback, not a substring filter: paths
    // outside the protected profile/data locations must stay available for
    // byte-level diagnostics on the affected host.
    let prefixes = vec![(
        r"C:\Users\hallong\AppData\Local".to_string(),
        "<localappdata>",
    )];
    for case in [
        // Same cross-drive shape, but a different profile identity.
        r"\\?\D:\Users\serviceacct\RealInstall\dcc-mcp\dcc-mcp-cli.exe",
        // The identity only as a substring of a different component.
        r"\\?\D:\Users\hallong-backup\RealInstall\dcc-mcp-cli.exe",
        // A genuinely unrelated install tree that shares only the file name.
        r"D:\tools\dcc-mcp\dcc-mcp-cli.exe",
    ] {
        let redacted = redact_path_with_prefixes(Path::new(case), &prefixes);
        assert_eq!(
            redacted, case,
            "similar but unrelated path was wrongly redacted: {redacted}"
        );
    }
}

#[test]
fn sibling_target_failure_is_not_labeled_as_current_exe() {
    // A staged update set may target a sibling component rather than the
    // current executable. A failing rename on such a journal entry must not
    // claim the failed file was `current-exe`, or operators will inspect the
    // wrong file on the affected host. Force the rename to fail in a
    // platform-independent way by giving the backup a missing parent.
    let temp = tempfile::tempdir().unwrap();
    let _current = write_fixture(temp.path(), "dcc-mcp-server.exe", b"server");
    let sibling = write_fixture(temp.path(), "dcc-mcp-cli.exe", b"old-cli");
    let prepared = write_fixture(temp.path(), "dcc-mcp-cli.exe.dcc-mcp-new", b"new-cli");
    let original_sha256 = sha256_file(&sibling).unwrap();
    let expected_sha256 = sha256_file(&prepared).unwrap();
    let journal = UpdateJournal {
        state: JournalState::Applying,
        entries: vec![JournalEntry {
            target: sibling.clone(),
            prepared,
            backup: temp
                .path()
                .join("absent")
                .join("dcc-mcp-cli.exe.dcc-mcp-backup"),
            had_original: true,
            original_sha256: Some(original_sha256),
            expected_sha256,
        }],
    };

    let error = apply_entries(&journal).unwrap_err();
    let message = error.to_string();
    assert!(
        matches!(error, UpdateError::IoWithContext { .. }),
        "expected a diagnosable I/O failure, got {error:?}"
    );
    assert!(
        message.contains("rename_target_to_backup"),
        "failing operation not named: {message}"
    );
    assert!(
        message.contains("installed-component"),
        "journal entry lost its installed-component category: {message}"
    );
    assert!(
        !message.contains("current-exe"),
        "sibling target was mislabeled as the current executable: {message}"
    );
    assert_eq!(std::fs::read(&sibling).unwrap(), b"old-cli");
}

#[cfg(windows)]
fn deny_delete(path: &Path) -> std::fs::File {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::{FILE_SHARE_READ, FILE_SHARE_WRITE};
    // Deliberately omit FILE_SHARE_DELETE so a rename/delete of this file
    // fails in a real Windows temp directory. This models the "another
    // program is using this file" family behind #2864 without touching
    // ACLs, privileges, services, or any production binary.
    std::fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .open(path)
        .unwrap()
}

#[cfg(windows)]
#[test]
fn windows_locked_target_reports_diagnosable_io_error_and_rolls_back() {
    let temp = tempfile::tempdir().unwrap();
    let current = write_fixture(temp.path(), "dcc-mcp-cli.exe", b"old-cli");
    let downloaded = write_fixture(temp.path(), "cli.download", b"new-cli");
    let expected_sha256 = sha256_file(&downloaded).unwrap();
    let binary_name = unique_name("win-locked-target");
    // Remove this test's isolated staging subtree on every exit path,
    // including assertion panics, so no pending dir is left in APPDATA.
    let _staging_guard = StagingRootGuard::new(&binary_name);

    stage_verified_binary_update_for(&binary_name, &current, &downloaded, &expected_sha256)
        .unwrap();

    // Hold a handle without FILE_SHARE_DELETE so the swap's first rename
    // fails with a real Windows error (ERROR_SHARING_VIOLATION, os error 32,
    // on this host) in a real temp directory.
    let _locked = deny_delete(&current);
    let error = apply_staged_binary_update_for(&binary_name, &current).unwrap_err();

    let source_io = match &error {
        UpdateError::IoWithContext { error, .. } => error,
        other => panic!("expected IoWithContext, got {other:?}"),
    };
    // A real, non-zero Windows OS error from the failing rename is preserved.
    assert!(
        source_io.raw_os_error().is_some_and(|code| code != 0),
        "failing rename must keep its raw OS error: {error:?}"
    );

    let message = error.to_string();
    assert!(
        message.contains("rename_target_to_backup"),
        "failing operation not named: {message}"
    );
    assert!(
        message.contains("os error"),
        "raw OS error not surfaced: {message}"
    );
    assert!(
        !message.contains(&*temp.path().to_string_lossy()),
        "absolute temp path leaked: {message}"
    );
    // Rollback semantics: the original executable is intact.
    assert_eq!(std::fs::read(&current).unwrap(), b"old-cli");
}

#[cfg(windows)]
#[test]
fn windows_access_denied_error_5_is_preserved_by_the_diagnostic_layer() {
    // remove_file on a directory yields a real ERROR_ACCESS_DENIED
    // (os error 5, PermissionDenied) on Windows, with no ACL or privilege
    // changes. Feed that genuine error through the same wrapping the swap
    // uses and verify the exact code survives and stays path-free.
    let temp = tempfile::tempdir().unwrap();
    let directory = temp.path().join("occupied-directory");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join("child.bin"), b"x").unwrap();
    let raw_error = std::fs::remove_file(&directory).unwrap_err();
    assert_eq!(raw_error.raw_os_error(), Some(5));

    let error = record_io_failure(
        io_op_fixture(),
        &directory,
        &directory,
        Err::<(), _>(raw_error),
    )
    .unwrap_err();

    let message = error.to_string();
    assert!(
        message.contains("os error 5"),
        "exact os error 5 not surfaced: {message}"
    );
    assert!(
        message.contains("rename_target_to_backup"),
        "operation not named: {message}"
    );
    assert!(
        !message.contains(&*temp.path().to_string_lossy()),
        "absolute path leaked: {message}"
    );
}

/// Count the committed sets retained as deferred-cleanup retry markers.
#[cfg(windows)]
fn retained_committed_sets(root: &Path) -> usize {
    std::fs::read_dir(root)
        .unwrap()
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(COMMITTED_SET_PREFIX)
        })
        .count()
}

#[test]
fn deferred_canonical_backup_routes_next_transaction_to_a_unique_backup() {
    let temp = tempfile::tempdir().unwrap();
    let target = write_fixture(temp.path(), "dcc-mcp-cli.exe", b"old-cli");
    let canonical = target.with_file_name("dcc-mcp-cli.exe.dcc-mcp-backup");
    assert_eq!(
        backup_path_for_transaction(&target, "dcc-mcp-cli.exe").unwrap(),
        canonical
    );

    // A regular file at the canonical path is a deferred committed backup.
    std::fs::write(&canonical, b"old-cli").unwrap();
    let unique = backup_path_for_transaction(&target, "dcc-mcp-cli.exe").unwrap();
    assert_ne!(unique, canonical);
    assert!(unique.to_string_lossy().contains(".dcc-mcp-backup-"));
    assert!(!unique.exists());

    // Other path shapes stay on the canonical path so the transaction's
    // reserved-path validation still rejects them where it always did.
    std::fs::remove_file(&canonical).unwrap();
    std::fs::create_dir(&canonical).unwrap();
    assert_eq!(
        backup_path_for_transaction(&target, "dcc-mcp-cli.exe").unwrap(),
        canonical
    );
}

#[cfg(windows)]
#[test]
fn deferred_cleanup_recognizes_windows_in_use_errors_only() {
    let with_context = |code| UpdateError::IoWithContext {
        stage: "cleanup",
        operation: "remove_committed_reserved_file",
        source: "committed-reserved-file",
        target: "removed-reserved-file",
        error: std::io::Error::from_raw_os_error(code),
    };
    // ERROR_ACCESS_DENIED (mapped image) and ERROR_SHARING_VIOLATION (open
    // handle without FILE_SHARE_DELETE) clear once the holder exits.
    assert!(is_deferred_image_lock(&with_context(5)));
    assert!(is_deferred_image_lock(&with_context(32)));
    assert!(is_deferred_image_lock(&UpdateError::Io(
        std::io::Error::from_raw_os_error(5)
    )));
    // Unrelated failures must stay fatal, not silently defer cleanup.
    assert!(!is_deferred_image_lock(&with_context(2)));
    assert!(!is_deferred_image_lock(&with_context(13)));
    assert!(!is_deferred_image_lock(&UpdateError::Stage(
        "unrelated".into()
    )));
}

/// Reproduces the #2864 activation failure: the swap landed, but deleting the
/// renamed old executable image is still blocked by a live Windows process.
/// The cleanup used to propagate that error, so `apply_staged_update` returned
/// `Err` before the reexec decision and every later launch retried the same
/// failure forever. The fix must let activation proceed and defer cleanup.
#[cfg(windows)]
#[test]
fn windows_locked_committed_backup_still_activates_and_defers_cleanup() {
    let temp = tempfile::tempdir().unwrap();
    let current = write_fixture(temp.path(), "dcc-mcp-server.exe", b"old-server");
    let downloaded = write_fixture(temp.path(), "server.download", b"new-server");
    let original_sha256 = sha256_file(&current).unwrap();
    let expected_sha256 = sha256_file(&downloaded).unwrap();
    let target = UpdateTarget::CurrentExecutable;
    let binary_name = unique_name("win-deferred-cleanup");
    let _staging_guard = StagingRootGuard::new(&binary_name);
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

    // Reconstruct the exact state left by a landed-but-uncleaned commit:
    // new bytes at the target, old bytes at the backup, a Committed journal.
    let backup = current.with_file_name("dcc-mcp-server.exe.dcc-mcp-backup");
    std::fs::copy(&current, &backup).unwrap();
    std::fs::copy(&downloaded, &current).unwrap();
    let journal = UpdateJournal {
        state: JournalState::Committed,
        entries: vec![JournalEntry {
            target: current.clone(),
            prepared: current.with_file_name("dcc-mcp-server.exe.dcc-mcp-new"),
            backup: backup.clone(),
            had_original: true,
            original_sha256: Some(original_sha256),
            expected_sha256,
        }],
    };
    let root = installation_root(&binary_name, &current);
    write_json_atomic(&root.join(PENDING_SET_DIR).join(JOURNAL_FILE), &journal).unwrap();

    // A real Windows in-use error, produced without touching ACLs, services,
    // privileges, or any production binary.
    let locked = deny_delete(&backup);

    assert!(
        apply_staged_update_set_for_build_with_timeout(
            &binary_name,
            &current,
            "1.0.0",
            UPDATE_LOCK_TIMEOUT,
        )
        .unwrap(),
        "a deferred backup cleanup must not abort activation"
    );
    assert_eq!(std::fs::read(&current).unwrap(), b"new-server");
    assert!(backup.exists(), "deferred backup must be left in place");
    assert_eq!(
        retained_committed_sets(&root),
        1,
        "one committed set must be kept as a retry marker"
    );

    // Releasing the image lets a later launch finish cleanup: no permanent
    // error loop and no repeated reexec.
    drop(locked);
    assert!(
        !apply_staged_update_set_for_build_with_timeout(
            &binary_name,
            &current,
            "2.0.0",
            UPDATE_LOCK_TIMEOUT,
        )
        .unwrap()
    );
    assert!(!backup.exists());
    assert_eq!(retained_committed_sets(&root), 0);
}

/// A deferred canonical backup from an earlier generation must not collide
/// with the next transaction: Windows rename refuses an existing destination,
/// and the stale-backup digest check would reject a different generation.
#[cfg(windows)]
#[test]
fn windows_deferred_canonical_backup_does_not_block_the_next_staged_update() {
    let temp = tempfile::tempdir().unwrap();
    let current = write_fixture(temp.path(), "dcc-mcp-server.exe", b"gen-1-server");
    let gen2 = write_fixture(temp.path(), "gen2.download", b"gen-2-server");
    let binary_name = unique_name("win-deferred-next-update");
    let _staging_guard = StagingRootGuard::new(&binary_name);
    let root = installation_root(&binary_name, &current);
    std::fs::create_dir_all(&root).unwrap();

    let canonical_backup = current.with_file_name("dcc-mcp-server.exe.dcc-mcp-backup");
    std::fs::write(&canonical_backup, b"gen-0-server").unwrap();
    let old_journal = UpdateJournal {
        state: JournalState::Committed,
        entries: vec![JournalEntry {
            target: current.clone(),
            prepared: current.with_file_name("dcc-mcp-server.exe.dcc-mcp-new"),
            backup: canonical_backup.clone(),
            had_original: true,
            original_sha256: Some(sha256_file(&canonical_backup).unwrap()),
            expected_sha256: sha256_file(&current).unwrap(),
        }],
    };
    let committed_set = root.join(format!("{COMMITTED_SET_PREFIX}previous-generation"));
    std::fs::create_dir(&committed_set).unwrap();
    write_json_atomic(&committed_set.join(JOURNAL_FILE), &old_journal).unwrap();
    let locked = deny_delete(&canonical_backup);

    let gen2_sha = sha256_file(&gen2).unwrap();
    let target = UpdateTarget::CurrentExecutable;
    stage_update_set_for_build_with_timeout(
        &binary_name,
        &current,
        "2.0.0",
        &[UpdateSetSource {
            downloaded: &gen2,
            target: &target,
            expected_sha256: &gen2_sha,
        }],
        UPDATE_LOCK_TIMEOUT,
    )
    .unwrap();
    assert!(
        apply_staged_update_set_for_build_with_timeout(
            &binary_name,
            &current,
            "2.0.0",
            UPDATE_LOCK_TIMEOUT,
        )
        .unwrap(),
        "the deferred canonical backup must not block the next activation"
    );
    assert_eq!(std::fs::read(&current).unwrap(), b"gen-2-server");
    assert!(canonical_backup.exists());
    assert!(committed_set.exists());
    // The new transaction's own unique backup was cleaned up.
    assert!(
        std::fs::read_dir(temp.path())
            .unwrap()
            .filter_map(|entry| entry.ok())
            .all(|entry| !entry
                .file_name()
                .to_string_lossy()
                .contains("dcc-mcp-backup-")),
        "the unique transactional backup must be cleaned up"
    );

    drop(locked);
    assert!(
        !apply_staged_update_set_for_build_with_timeout(
            &binary_name,
            &current,
            "3.0.0",
            UPDATE_LOCK_TIMEOUT,
        )
        .unwrap()
    );
    assert!(!canonical_backup.exists());
    assert!(!committed_set.exists());
}
