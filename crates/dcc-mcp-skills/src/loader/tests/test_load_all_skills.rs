use super::*;

#[test]
fn load_mixed_valid_and_invalid() {
    let tmp = tempfile::tempdir().unwrap();

    // Valid skill
    let valid_dir = tmp.path().join("valid");
    std::fs::create_dir_all(&valid_dir).unwrap();
    std::fs::write(
        valid_dir.join(SKILL_METADATA_FILE),
        "---\nname: valid\n---\n# Valid",
    )
    .unwrap();

    // Invalid skill (no frontmatter)
    let invalid_dir = tmp.path().join("invalid");
    std::fs::create_dir_all(&invalid_dir).unwrap();
    std::fs::write(
        invalid_dir.join(SKILL_METADATA_FILE),
        "plain text, no frontmatter",
    )
    .unwrap();

    let dirs = vec![
        valid_dir.to_string_lossy().to_string(),
        invalid_dir.to_string_lossy().to_string(),
    ];
    let (skills, skipped) = load_all_skills(&dirs);
    assert_eq!(skills.len(), 1);
    assert_eq!(skills[0].name, "valid");
    assert_eq!(skipped.len(), 1);
}

#[test]
fn load_nonexistent_dirs() {
    let dirs = vec!["/definitely/does/not/exist".to_string()];
    let (skills, skipped) = load_all_skills(&dirs);
    assert!(skills.is_empty());
    assert_eq!(skipped.len(), 1);
}

#[test]
fn load_keeps_first_occurrence_on_duplicate_name() {
    let tmp = tempfile::tempdir().unwrap();

    // Search-root order matters: the first directory models a host-specific
    // marketplace root, the second the shared host-neutral `any` root.
    let host_dir = tmp.path().join("maya").join("demo");
    let neutral_dir = tmp.path().join("any").join("demo");
    for dir in [&host_dir, &neutral_dir] {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(
            dir.join(SKILL_METADATA_FILE),
            "---\nname: demo\n---\n# Demo",
        )
        .unwrap();
    }

    let dirs = vec![
        host_dir.to_string_lossy().to_string(),
        neutral_dir.to_string_lossy().to_string(),
    ];
    let (skills, skipped) = load_all_skills(&dirs);

    // One name -> one skill, and it is the higher-priority (first) root.
    assert_eq!(skills.len(), 1);
    assert_eq!(skills[0].name, "demo");
    assert_eq!(skills[0].skill_path, host_dir.to_string_lossy());

    // A shadowed duplicate parsed fine, so it is not a load failure: strict
    // mode must not reject it.
    assert!(skipped.is_empty());
}

#[test]
fn load_deduplicates_only_by_name() {
    let tmp = tempfile::tempdir().unwrap();

    let alpha = tmp.path().join("alpha");
    let beta = tmp.path().join("beta");
    let alpha_dup = tmp.path().join("alpha_again");
    std::fs::create_dir_all(&alpha).unwrap();
    std::fs::create_dir_all(&beta).unwrap();
    std::fs::create_dir_all(&alpha_dup).unwrap();
    std::fs::write(alpha.join(SKILL_METADATA_FILE), "---\nname: alpha\n---\n").unwrap();
    std::fs::write(beta.join(SKILL_METADATA_FILE), "---\nname: beta\n---\n").unwrap();
    std::fs::write(
        alpha_dup.join(SKILL_METADATA_FILE),
        "---\nname: alpha\n---\n",
    )
    .unwrap();

    let dirs = vec![
        alpha.to_string_lossy().to_string(),
        beta.to_string_lossy().to_string(),
        alpha_dup.to_string_lossy().to_string(),
    ];
    let (skills, skipped) = load_all_skills(&dirs);

    // Distinct names survive; only the repeated name collapses.
    let names: Vec<&str> = skills.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, vec!["alpha", "beta"]);
    assert_eq!(skills[0].skill_path, alpha.to_string_lossy());
    assert!(skipped.is_empty());
}
