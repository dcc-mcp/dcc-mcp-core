use super::*;

struct PreparedHook;

impl Drop for PreparedHook {
    fn drop(&mut self) {
        PREPARED_WRITE_HOOK.with(|hook| *hook.borrow_mut() = None);
    }
}

#[test]
fn forward_expected_collision_keeps_unknown_file_and_recovery_journal() {
    forward_matching_collision(b"foreground");
}

#[test]
fn forward_original_collision_keeps_unknown_file_and_recovery_journal() {
    forward_matching_collision(b"old");
}

fn forward_matching_collision(bytes: &[u8]) {
    let fixture = Fixture::new();
    std::fs::write(fixture.install.join("dcc-cua"), b"old").unwrap();
    let protected = std::rc::Rc::new(std::cell::RefCell::new(None));
    let saved = protected.clone();
    let bytes = bytes.to_vec();
    let collision = bytes.clone();
    PREPARED_WRITE_HOOK.with(|hook| {
        *hook.borrow_mut() = Some(Box::new(move |path, stage| {
            if stage == "forward" && saved.borrow().is_none() {
                std::fs::write(path, &collision).unwrap();
                *saved.borrow_mut() = Some(path.to_path_buf());
            }
        }));
    });
    let _hook = PreparedHook;
    let result = checked(
        &fixture,
        &sha256_bytes(b"cli"),
        &sha256_bytes(b"old"),
        |_| Ok(()),
    );
    assert!(result.is_err());
    assert!(
        protected.borrow().as_ref().unwrap().exists(),
        "forward create_new collision was deleted during recovery"
    );
    assert_eq!(
        std::fs::read(protected.borrow().as_ref().unwrap()).unwrap(),
        bytes
    );
    assert_eq!(
        std::fs::read(fixture.install.join("dcc-cua")).unwrap(),
        b"old"
    );
    assert!(
        fixture.journal().exists(),
        "ambiguous ownership must retain recovery evidence"
    );
    assert!(result.unwrap_err().to_string().contains("recovery"));
}

#[test]
fn recovery_write_collision_keeps_unknown_file_and_recovery_journal() {
    let fixture = Fixture::new();
    let target = fixture.install.join("dcc-cua");
    let prepared = fixture.install.join("dcc-cua.dcc-mcp-new");
    let backup = fixture.install.join("dcc-cua.dcc-mcp-backup");
    std::fs::write(&target, b"foreground").unwrap();
    std::fs::write(&backup, b"old").unwrap();
    let entry = JournalEntry {
        target: target.clone(),
        prepared: prepared.clone(),
        backup,
        had_original: true,
        original_sha256: Some(sha256_bytes(b"old")),
        expected_sha256: sha256_bytes(b"foreground"),
    };
    let journal = PackageJournal {
        created_prepared: BTreeSet::new(),
        bound_executable: std::fs::canonicalize(&fixture.current).unwrap(),
        transaction_id: None,
        created_directories: vec![],
        #[cfg(unix)]
        original_modes: vec![original_mode(&entry).unwrap()],
        update: UpdateJournal {
            state: JournalState::Applying,
            entries: vec![entry],
        },
    };
    write_json_atomic(&fixture.journal(), &journal).unwrap();
    PREPARED_WRITE_HOOK.with(|hook| {
        *hook.borrow_mut() = Some(Box::new(|path, stage| {
            if stage == "recovery" {
                std::fs::write(path, b"unknown recovery collision").unwrap();
            }
        }));
    });
    let _hook = PreparedHook;
    let result = recover_package(&fixture.journal(), &journal.bound_executable);
    assert!(
        prepared.exists(),
        "recovery writer consumed an unknown collision"
    );
    assert_eq!(
        std::fs::read(&prepared).unwrap(),
        b"unknown recovery collision"
    );
    assert!(result.is_err());
    assert_eq!(std::fs::read(&target).unwrap(), b"foreground");
    assert!(fixture.journal().exists());
}

#[test]
fn legacy_or_partial_prepared_files_are_preserved_for_inspection() {
    for (bytes, owned) in [
        (b"old".as_slice(), false),
        (b"foreground".as_slice(), false),
        (b"partial".as_slice(), true),
    ] {
        let fixture = Fixture::new();
        let target = fixture.install.join("dcc-cua");
        let prepared = fixture.install.join("dcc-cua.dcc-mcp-new");
        let backup = fixture.install.join("dcc-cua.dcc-mcp-backup");
        std::fs::write(&target, b"foreground").unwrap();
        std::fs::write(&prepared, bytes).unwrap();
        std::fs::write(&backup, b"old").unwrap();
        let entry = JournalEntry {
            target: target.clone(),
            prepared: prepared.clone(),
            backup,
            had_original: true,
            original_sha256: Some(sha256_bytes(b"old")),
            expected_sha256: sha256_bytes(b"foreground"),
        };
        let journal = PackageJournal {
            bound_executable: std::fs::canonicalize(&fixture.current).unwrap(),
            transaction_id: None,
            created_prepared: if owned {
                BTreeSet::from([prepared.clone()])
            } else {
                BTreeSet::new()
            },
            created_directories: vec![],
            #[cfg(unix)]
            original_modes: vec![original_mode(&entry).unwrap()],
            update: UpdateJournal {
                state: JournalState::Applying,
                entries: vec![entry],
            },
        };
        let mut legacy = serde_json::to_value(&journal).unwrap();
        if !owned {
            legacy.as_object_mut().unwrap().remove("created_prepared");
        }
        write_json_atomic(&fixture.journal(), &legacy).unwrap();
        let before = std::fs::read(fixture.journal()).unwrap();
        assert!(recover_package(&fixture.journal(), &journal.bound_executable).is_err());
        assert_eq!(std::fs::read(&prepared).unwrap(), bytes);
        assert_eq!(std::fs::read(&target).unwrap(), b"foreground");
        assert_eq!(std::fs::read(fixture.journal()).unwrap(), before);
    }
}

fn checked(
    fixture: &Fixture,
    core: &str,
    old: &str,
    validate: impl Fn(&Path) -> Result<(), UpdateError>,
) -> Result<(), UpdateError> {
    let foreground = PackageFile {
        path: "dcc-cua",
        sha256: old,
    };
    install_verified_package_checked(
        &fixture.name,
        &fixture.current,
        CheckedPackage {
            identity: PackageIdentity {
                name: "dcc-cua",
                version: "1.0.0",
                verified_legacy_foreground: Some(foreground),
            },
            preconditions: PackagePreconditions {
                executable_sha256: core,
                foreground,
            },
        },
        &fixture.source,
        &fixture.directories,
        &fixture.plan(),
        validate,
    )
}

#[test]
fn fixed_bytes_preserve_legacy_sidecars_on_success_and_rollback() {
    for fail in [false, true] {
        let fixture = Fixture::new();
        std::fs::write(fixture.install.join("dcc-cua"), b"old").unwrap();
        for suffix in ["new", "backup"] {
            std::fs::write(
                fixture.install.join(format!("dcc-cua.dcc-mcp-{suffix}")),
                b"historical",
            )
            .unwrap();
        }
        let result = checked(
            &fixture,
            &sha256_bytes(b"cli"),
            &sha256_bytes(b"old"),
            |root| {
                if root == std::fs::canonicalize(&fixture.source).unwrap() {
                    assert!(!fixture.journal().exists());
                    return Ok(());
                }
                let journal: PackageJournal = read_json(&fixture.journal()).unwrap();
                assert!(journal.transaction_id.is_some());
                assert!(
                    journal
                        .update
                        .entries
                        .iter()
                        .all(|entry| entry.backup != root.join("dcc-cua.dcc-mcp-backup"))
                );
                if fail {
                    Err(UpdateError::Stage("metadata validation failed".into()))
                } else {
                    Ok(())
                }
            },
        );
        assert_eq!(result.is_err(), fail);
        assert_eq!(
            std::fs::read(fixture.install.join("dcc-cua")).unwrap(),
            if fail {
                b"old".as_slice()
            } else {
                b"foreground"
            }
        );
        for suffix in ["new", "backup"] {
            assert_eq!(
                std::fs::read(fixture.install.join(format!("dcc-cua.dcc-mcp-{suffix}"))).unwrap(),
                b"historical"
            );
        }
        assert!(!fixture.journal().exists());
        let sidecars: Vec<_> = std::fs::read_dir(&fixture.install)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|name| name.contains(".dcc-mcp-"))
            .collect();
        assert_eq!(sidecars.len(), 2);
    }
}

#[test]
fn fixed_byte_guards_apply_even_with_a_valid_ownership_receipt() {
    let mut fixture = Fixture::new();
    fixture.run(|_| Ok(())).unwrap();
    let old = sha256_bytes(b"foreground");
    fixture.replace_source("dcc-cua", b"new foreground");
    for (core, foreground) in [
        (sha256_bytes(b"changed cli"), old.clone()),
        (sha256_bytes(b"cli"), sha256_bytes(b"wrong old")),
    ] {
        assert!(
            checked(&fixture, &core, &foreground, |_| panic!(
                "must fail before runtime validation"
            ))
            .is_err()
        );
        assert_eq!(
            std::fs::read(fixture.install.join("dcc-cua")).unwrap(),
            b"foreground"
        );
        assert!(!fixture.journal().exists());
    }
    checked(&fixture, &sha256_bytes(b"cli"), &old, |_| Ok(())).unwrap();
    assert_eq!(
        std::fs::read(fixture.install.join("dcc-cua")).unwrap(),
        b"new foreground"
    );
}

#[test]
fn pending_journal_is_not_recovered_or_removed_by_fixed_byte_install() {
    let fixture = Fixture::new();
    std::fs::write(fixture.install.join("dcc-cua"), b"old").unwrap();
    let journal = fixture.journal();
    std::fs::write(
        &journal,
        b"pending transaction, including an unknown schema",
    )
    .unwrap();
    let error = checked(
        &fixture,
        &sha256_bytes(b"cli"),
        &sha256_bytes(b"old"),
        |_| panic!("pending transaction must short circuit"),
    )
    .unwrap_err();
    assert!(error.to_string().contains("recovery required"));
    assert_eq!(
        std::fs::read(&journal).unwrap(),
        b"pending transaction, including an unknown schema"
    );
    assert_eq!(
        std::fs::read(fixture.install.join("dcc-cua")).unwrap(),
        b"old"
    );
}

#[test]
fn checks_expected_bytes_after_acquiring_shared_installation_lock() {
    let fixture = Fixture::new();
    std::fs::write(fixture.install.join("dcc-cua"), b"old").unwrap();
    let current = std::fs::canonicalize(&fixture.current).unwrap();
    let root = installation_staging_dir(&fixture.name, &current).unwrap();
    let lock = acquire_installation_lock(&root, &current, UPDATE_LOCK_TIMEOUT).unwrap();
    std::thread::scope(|scope| {
        let worker = scope.spawn(|| {
            checked(
                &fixture,
                &sha256_bytes(b"cli"),
                &sha256_bytes(b"old"),
                |_| panic!("changed bytes must be rejected"),
            )
        });
        std::fs::write(fixture.install.join("dcc-cua"), b"changed while waiting").unwrap();
        drop(lock);
        assert!(worker.join().unwrap().is_err());
    });
    assert_eq!(
        std::fs::read(fixture.install.join("dcc-cua")).unwrap(),
        b"changed while waiting"
    );
    assert!(!fixture.journal().exists());
}

#[test]
fn nonce_journal_cannot_claim_a_legacy_backup_or_escape_its_target() {
    let fixture = Fixture::new();
    std::fs::write(fixture.install.join("dcc-cua"), b"old").unwrap();
    let core = sha256_bytes(b"cli");
    let old = sha256_bytes(b"old");
    checked(&fixture, &core, &old, |_| {
        if !fixture.journal().exists() {
            return Ok(());
        }
        let journal: PackageJournal = read_json(&fixture.journal()).unwrap();
        let mut value = serde_json::to_value(&journal).unwrap();
        value["transaction_id"] = serde_json::json!("../escape");
        let malformed: PackageJournal = serde_json::from_value(value).unwrap();
        assert!(validate_package_journal(&malformed, &journal.bound_executable).is_err());
        let mut altered: PackageJournal =
            serde_json::from_slice(&serde_json::to_vec(&journal).unwrap()).unwrap();
        altered.update.entries[0].backup = fixture.install.join("dcc-cua.dcc-mcp-backup");
        assert!(validate_package_journal(&altered, &journal.bound_executable).is_err());
        Ok(())
    })
    .unwrap();
}

#[test]
fn source_runtime_validation_fails_before_installation_writes() {
    let fixture = Fixture::new();
    std::fs::write(fixture.install.join("dcc-cua"), b"old").unwrap();
    let error = checked(
        &fixture,
        &sha256_bytes(b"cli"),
        &sha256_bytes(b"old"),
        |root| {
            assert_eq!(root, std::fs::canonicalize(&fixture.source).unwrap());
            assert!(!fixture.journal().exists());
            Err(UpdateError::Stage("source metadata rejected".into()))
        },
    )
    .unwrap_err();
    assert!(error.to_string().contains("source metadata rejected"));
    assert_eq!(
        std::fs::read(fixture.install.join("dcc-cua")).unwrap(),
        b"old"
    );
    assert!(!fixture.install.join("assets").exists());
    assert!(!fixture.journal().exists());
}

#[test]
fn unknown_prepared_collision_is_neither_overwritten_nor_removed_on_failure() {
    let fixture = Fixture::new();
    std::fs::write(fixture.install.join("dcc-cua"), b"old").unwrap();
    let protected = std::cell::RefCell::new(None);
    let error = checked(
        &fixture,
        &sha256_bytes(b"cli"),
        &sha256_bytes(b"old"),
        |_| {
            if !fixture.journal().exists() {
                return Ok(());
            }
            let journal: PackageJournal = read_json(&fixture.journal()).unwrap();
            let prepared = journal.update.entries[0].prepared.clone();
            std::fs::write(&prepared, b"unrecognized collision").unwrap();
            assert!(create_package_prepared(&prepared).is_err());
            assert_eq!(std::fs::read(&prepared).unwrap(), b"unrecognized collision");
            *protected.borrow_mut() = Some(prepared);
            Err(UpdateError::Stage("post validation failed".into()))
        },
    )
    .unwrap_err();
    assert!(error.to_string().contains("post validation failed"));
    assert!(error.to_string().contains("recovery failed"));
    assert_eq!(
        std::fs::read(protected.into_inner().unwrap()).unwrap(),
        b"unrecognized collision"
    );
    assert!(fixture.journal().exists());
}
