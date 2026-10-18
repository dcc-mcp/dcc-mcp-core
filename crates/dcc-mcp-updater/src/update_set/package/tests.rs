use super::*;

struct Fixture {
    root: tempfile::TempDir,
    source: PathBuf,
    install: PathBuf,
    current: PathBuf,
    name: String,
    directories: Vec<String>,
    files: Vec<(String, String)>,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source");
        let install = root.path().join("install");
        std::fs::create_dir(&source).unwrap();
        std::fs::create_dir(&install).unwrap();
        // Journals bind canonical executable paths, including Windows' extended prefix.
        let install = std::fs::canonicalize(install).unwrap();
        let current = install.join("dcc-mcp-cli");
        std::fs::write(&current, b"cli").unwrap();
        let name = format!("package-test-{}", unique_nonce().unwrap());
        let mut fixture = Self {
            root,
            source,
            install,
            current,
            name,
            directories: vec!["assets".into(), "skills/cua-cli".into()],
            files: Vec::new(),
        };
        for (path, bytes) in [
            ("dcc-cua", b"foreground".as_slice()),
            ("dcc-cua-background", b"background"),
            ("assets/theme.json", b"theme"),
            ("skills/cua-cli/SKILL.md", b"skill"),
        ] {
            fixture.add(path, bytes);
        }
        fixture
    }

    fn add(&mut self, path: &str, bytes: &[u8]) {
        let target = self.source.join(path);
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(&target, bytes).unwrap();
        self.files
            .push((path.into(), sha256_file(&target).unwrap()));
    }

    fn plan(&self) -> Vec<PackageFile<'_>> {
        self.files
            .iter()
            .map(|(path, sha256)| PackageFile { path, sha256 })
            .collect()
    }

    fn replace_source(&mut self, path: &str, bytes: &[u8]) {
        std::fs::write(self.source.join(path), bytes).unwrap();
        self.files
            .iter_mut()
            .find(|(name, _)| name == path)
            .unwrap()
            .1 = sha256_bytes(bytes);
    }

    fn run(
        &self,
        validate: impl FnOnce(&Path) -> Result<(), UpdateError>,
    ) -> Result<(), UpdateError> {
        self.run_with_legacy(None, validate)
    }

    fn run_with_legacy(
        &self,
        legacy: Option<PackageFile<'_>>,
        validate: impl FnOnce(&Path) -> Result<(), UpdateError>,
    ) -> Result<(), UpdateError> {
        install_verified_package(
            &self.name,
            &self.current,
            PackageIdentity {
                name: "dcc-cua",
                version: "1.0.0",
                verified_legacy_foreground: legacy,
            },
            &self.source,
            &self.directories,
            &self.plan(),
            validate,
        )
    }

    fn journal(&self) -> PathBuf {
        let root =
            installation_staging_dir(&self.name, &std::fs::canonicalize(&self.current).unwrap())
                .unwrap();
        std::fs::create_dir_all(&root).unwrap();
        root.join("package-dcc-cua.json")
    }
}

#[test]
fn installs_all_declared_files_and_repairs_incomplete_same_version_layouts() {
    let fixture = Fixture::new();
    std::fs::copy(
        fixture.source.join("dcc-cua"),
        fixture.install.join("dcc-cua"),
    )
    .unwrap();
    std::fs::write(fixture.install.join("unrelated.txt"), b"keep").unwrap();
    fixture.run(|_| Ok(())).unwrap();
    for (path, hash) in &fixture.files {
        require_file_hash(&fixture.install.join(path), hash, "test").unwrap();
    }
    std::fs::remove_file(fixture.install.join("dcc-cua-background")).unwrap();
    std::fs::remove_file(fixture.install.join("assets/theme.json")).unwrap();
    fixture.run(|_| Ok(())).unwrap();
    for (path, hash) in &fixture.files {
        require_file_hash(&fixture.install.join(path), hash, "test").unwrap();
    }
    assert_eq!(
        std::fs::read(fixture.install.join("unrelated.txt")).unwrap(),
        b"keep"
    );
    assert_eq!(std::fs::read(&fixture.current).unwrap(), b"cli");
    assert!(!fixture.journal().exists());
}

#[test]
fn checks_every_source_hash_before_writing_any_package_file() {
    let fixture = Fixture::new();
    std::fs::write(fixture.install.join("dcc-cua"), b"old").unwrap();
    std::fs::write(fixture.source.join("skills/cua-cli/SKILL.md"), b"corrupt").unwrap();
    assert!(fixture.run(|_| Ok(())).is_err());
    assert_eq!(
        std::fs::read(fixture.install.join("dcc-cua")).unwrap(),
        b"old"
    );
    assert!(!fixture.install.join("assets").exists());
    assert!(!fixture.install.join("dcc-cua-background").exists());
    assert!(!fixture.journal().exists());
}

#[test]
fn failed_runtime_validation_restores_originals_and_removes_only_new_content() {
    let fixture = Fixture::new();
    std::fs::write(fixture.install.join("dcc-cua"), b"foreground").unwrap();
    std::fs::create_dir(fixture.install.join("assets")).unwrap();
    std::fs::write(fixture.install.join("assets/local.json"), b"unrelated").unwrap();
    assert!(
        fixture
            .run(|root| {
                assert!(root.join("dcc-cua-background").is_file());
                Err(UpdateError::Stage("runtime validation failed".into()))
            })
            .is_err()
    );
    assert_eq!(
        std::fs::read(fixture.install.join("dcc-cua")).unwrap(),
        b"foreground"
    );
    assert_eq!(
        std::fs::read(fixture.install.join("assets/local.json")).unwrap(),
        b"unrelated"
    );
    assert!(!fixture.install.join("assets/theme.json").exists());
    assert!(!fixture.install.join("skills").exists());
    assert!(!fixture.install.join("dcc-cua-background").exists());
    assert!(!fixture.install.join(receipt_name("dcc-cua")).exists());
    assert!(!fixture.journal().exists());
}

#[test]
fn destination_conflict_is_rejected_before_the_first_replacement() {
    let fixture = Fixture::new();
    std::fs::write(fixture.install.join("dcc-cua"), b"foreground").unwrap();
    std::fs::write(fixture.install.join("skills"), b"user file").unwrap();
    assert!(fixture.run(|_| Ok(())).is_err());
    assert_eq!(
        std::fs::read(fixture.install.join("dcc-cua")).unwrap(),
        b"foreground"
    );
    assert!(!fixture.install.join("assets").exists());
    assert!(!fixture.journal().exists());
}

#[test]
fn rejects_unsafe_names_case_collisions_and_file_directory_overlaps() {
    let digest = "a".repeat(64);
    for path in [
        "",
        "../outside",
        "/absolute",
        "a/./b",
        "a//b",
        "a\\b",
        "C:drive",
        "nul.txt",
        "COM1",
        "assets/x.dcc-mcp-new",
        "assets/x.DCC-MCP-NEW",
        "trailing.",
        "file ",
    ] {
        assert!(
            validate_package_paths(
                &[],
                &[PackageFile {
                    path,
                    sha256: &digest
                }]
            )
            .is_err(),
            "{path}"
        );
    }
    for (first, second) in [
        ("file", "FILE"),
        ("assets", "assets/file"),
        ("Assets/a", "assets/b"),
    ] {
        assert!(
            validate_package_paths(
                &[],
                &[
                    PackageFile {
                        path: first,
                        sha256: &digest
                    },
                    PackageFile {
                        path: second,
                        sha256: &digest
                    }
                ]
            )
            .is_err()
        );
    }
    assert!(
        validate_package_paths(
            &["ASSETS".into()],
            &[PackageFile {
                path: "assets",
                sha256: &digest
            }]
        )
        .is_err()
    );
}

#[test]
fn package_cannot_replace_the_cli_and_preserves_unrelated_legacy_sidecars() {
    let mut fixture = Fixture::new();
    fixture.add("dcc-mcp-cli", b"not a companion");
    assert!(fixture.run(|_| Ok(())).is_err());
    assert_eq!(std::fs::read(&fixture.current).unwrap(), b"cli");
    fixture.files.pop();
    std::fs::write(fixture.install.join("dcc-cua.dcc-mcp-new"), b"unrecognized").unwrap();
    fixture.run(|_| Ok(())).unwrap();
    assert_eq!(
        std::fs::read(fixture.install.join("dcc-cua.dcc-mcp-new")).unwrap(),
        b"unrecognized"
    );
}

#[cfg(unix)]
#[test]
fn refuses_source_and_destination_symlinks_without_touching_their_targets() {
    use std::os::unix::fs::symlink;
    let fixture = Fixture::new();
    let outside = fixture.root.path().join("outside");
    std::fs::create_dir(&outside).unwrap();
    std::fs::write(outside.join("theme.json"), b"outside").unwrap();
    symlink(&outside, fixture.install.join("assets")).unwrap();
    assert!(fixture.run(|_| Ok(())).is_err());
    assert_eq!(
        std::fs::read(outside.join("theme.json")).unwrap(),
        b"outside"
    );
    std::fs::remove_file(fixture.install.join("assets")).unwrap();
    std::fs::remove_file(fixture.source.join("assets/theme.json")).unwrap();
    symlink(
        outside.join("theme.json"),
        fixture.source.join("assets/theme.json"),
    )
    .unwrap();
    assert!(fixture.run(|_| Ok(())).is_err());
    assert!(!fixture.install.join("dcc-cua").exists());
}

#[test]
fn crash_recovery_rolls_back_a_partly_applied_package_before_retry() {
    let fixture = Fixture::new();
    let target = fixture.install.join("dcc-cua");
    let prepared = fixture.install.join("dcc-cua.dcc-mcp-new");
    let backup = fixture.install.join("dcc-cua.dcc-mcp-backup");
    std::fs::write(&backup, b"old").unwrap();
    std::fs::copy(fixture.source.join("dcc-cua"), &target).unwrap();
    let journal = PackageJournal {
        transaction_id: None,
        bound_executable: std::fs::canonicalize(&fixture.current).unwrap(),
        created_directories: vec![],
        #[cfg(unix)]
        original_modes: vec![Some(0o644)],
        update: UpdateJournal {
            state: JournalState::Applying,
            entries: vec![JournalEntry {
                target: target.clone(),
                prepared,
                backup,
                had_original: true,
                original_sha256: Some(sha256_bytes(b"old")),
                expected_sha256: sha256_file(&target).unwrap(),
            }],
        },
    };
    write_json_atomic(&fixture.journal(), &journal).unwrap();
    // Force a preflight conflict after recovery so the recovered old bytes are observable.
    std::fs::write(fixture.install.join("skills"), b"conflict").unwrap();
    assert!(fixture.run(|_| Ok(())).is_err());
    assert_eq!(std::fs::read(target).unwrap(), b"old");
    assert!(!fixture.journal().exists());
}

#[test]
fn committed_recovery_keeps_verified_files_and_finishes_cleanup() {
    let fixture = Fixture::new();
    let target = fixture.install.join("dcc-cua");
    let prepared = fixture.install.join("dcc-cua.dcc-mcp-new");
    let backup = fixture.install.join("dcc-cua.dcc-mcp-backup");
    std::fs::write(&backup, b"old").unwrap();
    std::fs::copy(fixture.source.join("dcc-cua"), &target).unwrap();
    let journal = PackageJournal {
        transaction_id: None,
        bound_executable: std::fs::canonicalize(&fixture.current).unwrap(),
        created_directories: vec![],
        #[cfg(unix)]
        original_modes: vec![Some(0o644)],
        update: UpdateJournal {
            state: JournalState::Committed,
            entries: vec![JournalEntry {
                target: target.clone(),
                prepared,
                backup: backup.clone(),
                had_original: true,
                original_sha256: Some(sha256_bytes(b"old")),
                expected_sha256: sha256_file(&target).unwrap(),
            }],
        },
    };
    write_json_atomic(&fixture.journal(), &journal).unwrap();
    recover_package(
        &fixture.journal(),
        &std::fs::canonicalize(&fixture.current).unwrap(),
    )
    .unwrap();
    assert_eq!(std::fs::read(target).unwrap(), b"foreground");
    assert!(!backup.exists());
    assert!(!fixture.journal().exists());
}

#[test]
fn legacy_runtime_and_data_conflicts_preserve_every_existing_file() {
    for path in ["dcc-cua", "assets/theme.json"] {
        let fixture = Fixture::new();
        let target = fixture.install.join(path);
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(&target, b"user-owned").unwrap();
        let error = fixture
            .run(|_| panic!("must not reach runtime validation"))
            .unwrap_err();
        assert!(error.to_string().contains("unowned or modified"));
        assert_eq!(std::fs::read(target).unwrap(), b"user-owned");
        assert!(!fixture.install.join("dcc-cua-background").exists());
        assert!(!fixture.install.join(receipt_name("dcc-cua")).exists());
        assert!(!fixture.journal().exists());
    }
}

#[test]
fn receipt_allows_owned_upgrades_without_removing_obsolete_or_unrelated_resources() {
    let mut fixture = Fixture::new();
    fixture.add("assets/obsolete.txt", b"keep old resource");
    fixture.run(|_| Ok(())).unwrap();
    fixture
        .files
        .retain(|(path, _)| path != "assets/obsolete.txt");
    fixture.replace_source("dcc-cua", b"foreground v2");
    fixture.replace_source("assets/theme.json", b"theme v2");
    fixture.run(|_| Ok(())).unwrap();
    assert_eq!(
        std::fs::read(fixture.install.join("dcc-cua")).unwrap(),
        b"foreground v2"
    );
    assert_eq!(
        std::fs::read(fixture.install.join("assets/theme.json")).unwrap(),
        b"theme v2"
    );
    assert_eq!(
        std::fs::read(fixture.install.join("assets/obsolete.txt")).unwrap(),
        b"keep old resource"
    );
    let receipt = std::fs::read(fixture.install.join(receipt_name("dcc-cua"))).unwrap();
    fixture.run(|_| Ok(())).unwrap();
    assert_eq!(
        std::fs::read(fixture.install.join(receipt_name("dcc-cua"))).unwrap(),
        receipt
    );
}

#[test]
fn modified_owned_files_block_upgrade_without_changing_the_receipt_or_other_files() {
    for path in ["dcc-cua", "assets/theme.json"] {
        let mut fixture = Fixture::new();
        fixture.run(|_| Ok(())).unwrap();
        let receipt_path = fixture.install.join(receipt_name("dcc-cua"));
        let old_receipt = std::fs::read(&receipt_path).unwrap();
        std::fs::write(fixture.install.join(path), b"user edit").unwrap();
        fixture.replace_source("dcc-cua-background", b"new background");
        assert!(fixture.run(|_| Ok(())).is_err());
        assert_eq!(
            std::fs::read(fixture.install.join(path)).unwrap(),
            b"user edit"
        );
        assert_eq!(
            std::fs::read(fixture.install.join("dcc-cua-background")).unwrap(),
            b"background"
        );
        assert_eq!(std::fs::read(receipt_path).unwrap(), old_receipt);
        assert!(!fixture.journal().exists());
    }
}

#[test]
fn failed_upgrade_rolls_back_the_receipt_and_all_existing_package_files() {
    let mut fixture = Fixture::new();
    fixture.run(|_| Ok(())).unwrap();
    let receipt_path = fixture.install.join(receipt_name("dcc-cua"));
    let old_receipt = std::fs::read(&receipt_path).unwrap();
    fixture.replace_source("dcc-cua", b"new foreground");
    fixture.replace_source("dcc-cua-background", b"new background");
    fixture.add("assets/new.txt", b"new asset");
    assert!(
        fixture
            .run(|root| {
                assert_eq!(
                    std::fs::read(root.join("dcc-cua")).unwrap(),
                    b"new foreground"
                );
                Err(UpdateError::Stage("invalid installed runtime".into()))
            })
            .is_err()
    );
    assert_eq!(
        std::fs::read(fixture.install.join("dcc-cua")).unwrap(),
        b"foreground"
    );
    assert_eq!(
        std::fs::read(fixture.install.join("dcc-cua-background")).unwrap(),
        b"background"
    );
    assert!(!fixture.install.join("assets/new.txt").exists());
    assert_eq!(std::fs::read(receipt_path).unwrap(), old_receipt);
    assert!(!fixture.journal().exists());
}

#[test]
fn malformed_or_escaping_receipts_never_authorize_a_replacement() {
    for path in [
        "../outside",
        "/absolute",
        "C:\\outside",
        ".dcc-cua-install-receipt.json",
    ] {
        let fixture = Fixture::new();
        let receipt_path = fixture.install.join(receipt_name("dcc-cua"));
        let bytes = serde_json::to_vec(&serde_json::json!({
            "schema_version": 1, "name": "dcc-cua", "version": "1.0.0", "directories": [],
            "files": [{"path": path, "sha256": sha256_bytes(b"user file")}]
        }))
        .unwrap();
        std::fs::write(&receipt_path, &bytes).unwrap();
        std::fs::write(fixture.install.join("dcc-cua"), b"user file").unwrap();
        assert!(fixture.run(|_| Ok(())).is_err());
        assert_eq!(
            std::fs::read(fixture.install.join("dcc-cua")).unwrap(),
            b"user file"
        );
        assert_eq!(std::fs::read(receipt_path).unwrap(), bytes);
        assert!(!fixture.journal().exists());
    }
}

#[test]
fn mid_apply_failure_recovers_the_full_set_and_preserves_unrecognized_content() {
    let fixture = Fixture::new();
    let mut entries = Vec::new();
    for (name, original, replacement) in [
        (
            "dcc-cua",
            b"old foreground".as_slice(),
            b"new foreground".as_slice(),
        ),
        ("dcc-cua-background", b"old background", b"new background"),
    ] {
        let target = fixture.install.join(name);
        let prepared = fixture.install.join(format!("{name}.dcc-mcp-new"));
        std::fs::write(&target, original).unwrap();
        if name == "dcc-cua" {
            std::fs::write(&prepared, replacement).unwrap();
        }
        entries.push(JournalEntry {
            target,
            prepared,
            backup: fixture.install.join(format!("{name}.dcc-mcp-backup")),
            had_original: true,
            original_sha256: Some(sha256_bytes(original)),
            expected_sha256: sha256_bytes(replacement),
        });
    }
    let journal = PackageJournal {
        transaction_id: None,
        bound_executable: std::fs::canonicalize(&fixture.current).unwrap(),
        created_directories: vec![],
        #[cfg(unix)]
        original_modes: entries
            .iter()
            .map(original_mode)
            .collect::<Result<_, _>>()
            .unwrap(),
        update: UpdateJournal {
            state: JournalState::Applying,
            entries,
        },
    };
    write_json_atomic(&fixture.journal(), &journal).unwrap();
    assert!(apply_entries(&journal.update).is_err());
    std::fs::write(fixture.install.join("unrelated.txt"), b"keep").unwrap();
    recover_package(&fixture.journal(), &journal.bound_executable).unwrap();
    assert_eq!(
        std::fs::read(fixture.install.join("dcc-cua")).unwrap(),
        b"old foreground"
    );
    assert_eq!(
        std::fs::read(fixture.install.join("dcc-cua-background")).unwrap(),
        b"old background"
    );
    assert_eq!(
        std::fs::read(fixture.install.join("unrelated.txt")).unwrap(),
        b"keep"
    );
    assert!(!fixture.journal().exists());
}

#[cfg(unix)]
#[test]
fn repairs_missing_runtime_executable_mode_even_when_all_hashes_match() {
    use std::os::unix::fs::PermissionsExt;
    let fixture = Fixture::new();
    let runtime = fixture.source.join("dcc-cua-background");
    std::fs::set_permissions(runtime, std::fs::Permissions::from_mode(0o755)).unwrap();
    fixture.run(|_| Ok(())).unwrap();
    let installed = fixture.install.join("dcc-cua-background");
    std::fs::set_permissions(&installed, std::fs::Permissions::from_mode(0o644)).unwrap();
    fixture.run(|_| Ok(())).unwrap();
    assert_eq!(
        std::fs::metadata(installed).unwrap().permissions().mode() & 0o777,
        0o755
    );
    assert_eq!(
        std::fs::metadata(fixture.install.join("assets/theme.json"))
            .unwrap()
            .permissions()
            .mode()
            & 0o111,
        0
    );
}

#[test]
fn receipt_directory_overlap_fails_before_creating_any_package_content() {
    let mut fixture = Fixture::new();
    fixture.add(".dcc-cua-install-receipt.json/child", b"invalid overlap");
    assert!(fixture.run(|_| Ok(())).is_err());
    assert!(!fixture.install.join("dcc-cua").exists());
    assert!(!fixture.install.join(receipt_name("dcc-cua")).exists());
    assert!(!fixture.journal().exists());
}

#[cfg(unix)]
#[test]
fn failed_mode_only_repair_restores_the_previous_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let fixture = Fixture::new();
    fixture.run(|_| Ok(())).unwrap();
    std::fs::set_permissions(
        fixture.source.join("dcc-cua-background"),
        std::fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    let installed = fixture.install.join("dcc-cua-background");
    std::fs::set_permissions(&installed, std::fs::Permissions::from_mode(0o600)).unwrap();
    assert!(
        fixture
            .run(|root| {
                assert_eq!(
                    std::fs::metadata(root.join("dcc-cua-background"))
                        .unwrap()
                        .permissions()
                        .mode()
                        & 0o777,
                    0o755
                );
                Err(UpdateError::Stage("final runtime validation failed".into()))
            })
            .is_err()
    );
    assert_eq!(
        std::fs::metadata(installed).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert!(!fixture.journal().exists());
}

#[cfg(unix)]
#[test]
fn applying_recovery_restores_permissions_when_original_and_new_hashes_match() {
    use std::os::unix::fs::PermissionsExt;
    let fixture = Fixture::new();
    let target = fixture.install.join("dcc-cua-background");
    let prepared = fixture.install.join("dcc-cua-background.dcc-mcp-new");
    let backup = fixture.install.join("dcc-cua-background.dcc-mcp-backup");
    std::fs::write(&target, b"background").unwrap();
    std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(&backup, b"background").unwrap();
    std::fs::set_permissions(&backup, std::fs::Permissions::from_mode(0o600)).unwrap();
    let journal = PackageJournal {
        transaction_id: None,
        bound_executable: std::fs::canonicalize(&fixture.current).unwrap(),
        created_directories: vec![],
        original_modes: vec![Some(0o600)],
        update: UpdateJournal {
            state: JournalState::Applying,
            entries: vec![JournalEntry {
                target: target.clone(),
                prepared,
                backup,
                had_original: true,
                original_sha256: Some(sha256_bytes(b"background")),
                expected_sha256: sha256_bytes(b"background"),
            }],
        },
    };
    write_json_atomic(&fixture.journal(), &journal).unwrap();
    recover_package(&fixture.journal(), &journal.bound_executable).unwrap();
    assert_eq!(
        std::fs::metadata(target).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert!(!fixture.journal().exists());
}

#[test]
fn fully_published_applying_package_restores_local_source_before_fresh_archive_retry() {
    let mut fixture = Fixture::new();
    fixture.run(|_| Ok(())).unwrap();
    fixture.replace_source("dcc-cua", b"new foreground");
    fixture.replace_source("dcc-cua-background", b"new background");
    let receipt = prepare_receipt(
        &fixture.install,
        PackageIdentity {
            name: "dcc-cua",
            version: "1.0.0",
            verified_legacy_foreground: None,
        },
        &fixture.directories,
        &fixture.plan(),
    )
    .unwrap();
    let receipt_path = receipt_name("dcc-cua");
    let receipt_hash = sha256_bytes(&receipt);
    let mut plan = fixture.plan();
    plan.push(PackageFile {
        path: &receipt_path,
        sha256: &receipt_hash,
    });
    let mut entries = Vec::new();
    for file in &plan {
        let target = fixture.install.join(file.path);
        let name = target.file_name().unwrap().to_str().unwrap();
        let prepared = target.with_file_name(format!("{name}.dcc-mcp-new"));
        if file.path == receipt_path {
            std::fs::write(&prepared, &receipt).unwrap();
        } else {
            std::fs::copy(fixture.source.join(file.path), &prepared).unwrap();
        }
        entries.push(JournalEntry {
            backup: target.with_file_name(format!("{name}.dcc-mcp-backup")),
            original_sha256: Some(sha256_file(&target).unwrap()),
            target,
            prepared,
            had_original: true,
            expected_sha256: file.sha256.into(),
        });
    }
    let journal = PackageJournal {
        transaction_id: None,
        bound_executable: std::fs::canonicalize(&fixture.current).unwrap(),
        created_directories: vec![],
        #[cfg(unix)]
        original_modes: entries
            .iter()
            .map(original_mode)
            .collect::<Result<_, _>>()
            .unwrap(),
        update: UpdateJournal {
            state: JournalState::Applying,
            entries,
        },
    };
    write_json_atomic(&fixture.journal(), &journal).unwrap();
    apply_entries(&journal.update).unwrap();
    // All published hashes now match, but no commit marker exists. Recovery
    // must restore the old source directory instead of accepting the new set.
    for file in fixture.plan() {
        require_file_hash(&fixture.install.join(file.path), file.sha256, "published").unwrap();
    }
    assert!(
        install_verified_package(
            &fixture.name,
            &fixture.current,
            PackageIdentity {
                name: "dcc-cua",
                version: "1.0.0",
                verified_legacy_foreground: None,
            },
            &fixture.install,
            &fixture.directories,
            &fixture.plan(),
            |_| Ok(())
        )
        .is_err()
    );
    assert_eq!(
        std::fs::read(fixture.install.join("dcc-cua")).unwrap(),
        b"foreground"
    );
    assert_eq!(
        std::fs::read(fixture.install.join("dcc-cua-background")).unwrap(),
        b"background"
    );
    assert!(!fixture.journal().exists());
    // The CLI's same-call fallback supplies a fresh verified archive root.
    fixture.run(|_| Ok(())).unwrap();
    for file in fixture.plan() {
        require_file_hash(
            &fixture.install.join(file.path),
            file.sha256,
            "fresh archive retry",
        )
        .unwrap();
    }
}

#[test]
fn verified_legacy_foreground_upgrade_commits_receipt_and_rolls_back_on_validation_failure() {
    for should_fail in [false, true] {
        let fixture = Fixture::new();
        let target = fixture.install.join("dcc-cua");
        std::fs::write(&target, b"official old foreground").unwrap();
        let old_hash = sha256_file(&target).unwrap();
        let result = fixture.run_with_legacy(
            Some(PackageFile {
                path: "dcc-cua",
                sha256: &old_hash,
            }),
            |_| {
                if should_fail {
                    Err(UpdateError::Stage("runtime validation failed".into()))
                } else {
                    Ok(())
                }
            },
        );
        if should_fail {
            assert!(result.is_err());
            assert_eq!(std::fs::read(target).unwrap(), b"official old foreground");
            assert!(!fixture.install.join("dcc-cua-background").exists());
            assert!(!fixture.install.join(receipt_name("dcc-cua")).exists());
        } else {
            result.unwrap();
            assert_eq!(std::fs::read(target).unwrap(), b"foreground");
            assert!(fixture.install.join("dcc-cua-background").is_file());
            assert!(fixture.install.join(receipt_name("dcc-cua")).is_file());
        }
        assert!(!fixture.journal().exists());
    }
}

#[test]
fn legacy_proof_is_rechecked_under_the_lock_and_never_adopts_background_or_data_files() {
    let old_hash = sha256_bytes(b"official old foreground");
    for conflicting in ["dcc-cua", "dcc-cua-background", "assets/theme.json"] {
        let fixture = Fixture::new();
        std::fs::write(fixture.install.join("dcc-cua"), b"official old foreground").unwrap();
        let path = fixture.install.join(conflicting);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        // A changed foreground invalidates a proof computed before the lock.
        // Identical bytes at other paths cannot inherit foreground ownership.
        let bytes = if conflicting == "dcc-cua" {
            b"replaced after verification".as_slice()
        } else {
            b"official old foreground"
        };
        std::fs::write(&path, bytes).unwrap();
        assert!(
            fixture
                .run_with_legacy(
                    Some(PackageFile {
                        path: "dcc-cua",
                        sha256: &old_hash
                    }),
                    |_| Ok(())
                )
                .is_err()
        );
        assert_eq!(std::fs::read(path).unwrap(), bytes);
        assert!(!fixture.install.join(receipt_name("dcc-cua")).exists());
        assert!(!fixture.journal().exists());
    }
}

#[test]
fn legacy_proof_cannot_override_modified_receipt_owned_files_or_name_a_resource() {
    let fixture = Fixture::new();
    fixture.run(|_| Ok(())).unwrap();
    std::fs::write(fixture.install.join("dcc-cua"), b"other official version").unwrap();
    let other_hash = sha256_bytes(b"other official version");
    let receipt = std::fs::read(fixture.install.join(receipt_name("dcc-cua"))).unwrap();
    assert!(
        fixture
            .run_with_legacy(
                Some(PackageFile {
                    path: "dcc-cua",
                    sha256: &other_hash
                }),
                |_| Ok(())
            )
            .is_err()
    );
    assert_eq!(
        std::fs::read(fixture.install.join("dcc-cua")).unwrap(),
        b"other official version"
    );
    assert_eq!(
        std::fs::read(fixture.install.join(receipt_name("dcc-cua"))).unwrap(),
        receipt
    );
    assert!(
        fixture
            .run_with_legacy(
                Some(PackageFile {
                    path: "assets/theme.json",
                    sha256: &other_hash
                }),
                |_| Ok(())
            )
            .is_err()
    );
}

#[path = "checked_tests.rs"]
mod checked_tests;
