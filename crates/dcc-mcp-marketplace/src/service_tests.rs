use super::*;

#[test]
fn pinned_git_revision_marks_unchanged_version_as_outdated() {
    let entry = CatalogEntry {
        name: "test-skill".into(),
        description: "desc".into(),
        dcc: vec!["maya".into()],
        url: None,
        tags: vec![],
        version: Some("1.0.0".into()),
        min_core_version: None,
        install: Some(CatalogInstall {
            install_type: "git".into(),
            url: Some("https://example.invalid/skill".into()),
            ref_: Some("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".into()),
            sha256: None,
            skill_roots: None,
            pip_package: None,
            pip_extras: None,
            python_path: None,
            entry_point: None,
            instructions_url: None,
        }),
        maintainer: None,
        category: None,
        policy: None,
        requires: None,
        icon: None,
    };
    let installed = InstalledMarketplacePackage {
        name: "test-skill".into(),
        dcc: "maya".into(),
        version: Some("1.0.0".into()),
        path: "/tmp/test".into(),
        source_name: "official".into(),
        source_url: "https://example.invalid/catalog.json".into(),
        install_type: "git".into(),
        install_url: entry
            .install
            .as_ref()
            .and_then(|install| install.url.clone()),
        install_ref: entry
            .install
            .as_ref()
            .and_then(|install| install.ref_.clone()),
        resolved_commit: Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into()),
        installed_at_ms: 1,
    };

    let (outdated, latest_commit) = is_entry_outdated(Some(&entry), &installed);
    assert!(outdated);
    assert_eq!(
        latest_commit.as_deref(),
        Some("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb")
    );
}

#[test]
fn immutable_git_commit_only_accepts_full_object_ids() {
    let mut install = CatalogInstall {
        install_type: "git".into(),
        url: Some("https://example.invalid/skill".into()),
        ref_: Some("main".into()),
        sha256: None,
        skill_roots: None,
        pip_package: None,
        pip_extras: None,
        python_path: None,
        entry_point: None,
        instructions_url: None,
    };
    assert_eq!(immutable_git_commit(&install), None);
    install.ref_ = Some("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".into());
    assert_eq!(
        immutable_git_commit(&install).as_deref(),
        Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
    );
}
