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
fn load_duplicate_skill_names_keeps_highest_priority_directory() {
    let tmp = tempfile::tempdir().unwrap();
    let preferred = tmp.path().join("preferred");
    let shadowed = tmp.path().join("shadowed");
    std::fs::create_dir_all(&preferred).unwrap();
    std::fs::create_dir_all(&shadowed).unwrap();
    std::fs::write(
        preferred.join(SKILL_METADATA_FILE),
        "---\nname: duplicate\nmetadata:\n  dcc-mcp:\n    version: \"2.0.0\"\n---\n",
    )
    .unwrap();
    std::fs::write(
        shadowed.join(SKILL_METADATA_FILE),
        "---\nname: duplicate\nmetadata:\n  dcc-mcp:\n    version: \"1.0.0\"\n---\n",
    )
    .unwrap();

    let (skills, skipped) = load_all_skills(&[
        preferred.to_string_lossy().to_string(),
        shadowed.to_string_lossy().to_string(),
    ]);

    assert!(skipped.is_empty());
    assert_eq!(skills.len(), 1);
    assert_eq!(skills[0].version.as_str(), "2.0.0");
}
