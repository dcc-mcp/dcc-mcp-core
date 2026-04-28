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
