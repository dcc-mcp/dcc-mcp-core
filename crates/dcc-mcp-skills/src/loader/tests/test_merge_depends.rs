use super::*;
use dcc_mcp_models::SkillMetadata;

fn make_skill_with_deps(deps: &[&str]) -> SkillMetadata {
    SkillMetadata {
        depends: deps.iter().map(|s| s.to_string()).collect(),
        ..Default::default()
    }
}

#[test]
fn merge_plain_text_format() {
    let tmp = tempfile::tempdir().unwrap();
    let meta_dir = tmp.path().join(SKILL_METADATA_DIR);
    std::fs::create_dir_all(&meta_dir).unwrap();
    std::fs::write(meta_dir.join(DEPENDS_FILE), "dep-a\ndep-b\n").unwrap();

    let mut meta = make_skill_with_deps(&[]);
    merge_depends_from_metadata(tmp.path(), &mut meta);

    assert_eq!(meta.depends, vec!["dep-a", "dep-b"]);
}

#[test]
fn merge_yaml_list_format() {
    let tmp = tempfile::tempdir().unwrap();
    let meta_dir = tmp.path().join(SKILL_METADATA_DIR);
    std::fs::create_dir_all(&meta_dir).unwrap();
    std::fs::write(meta_dir.join(DEPENDS_FILE), "- alpha\n- beta\n").unwrap();

    let mut meta = make_skill_with_deps(&[]);
    merge_depends_from_metadata(tmp.path(), &mut meta);

    assert_eq!(meta.depends, vec!["alpha", "beta"]);
}

#[test]
fn merge_skips_comments_and_blanks() {
    let tmp = tempfile::tempdir().unwrap();
    let meta_dir = tmp.path().join(SKILL_METADATA_DIR);
    std::fs::create_dir_all(&meta_dir).unwrap();
    std::fs::write(
        meta_dir.join(DEPENDS_FILE),
        "# Comment\n\ndep-a\n\n# Another comment\ndep-b\n",
    )
    .unwrap();

    let mut meta = make_skill_with_deps(&[]);
    merge_depends_from_metadata(tmp.path(), &mut meta);

    assert_eq!(meta.depends, vec!["dep-a", "dep-b"]);
}

#[test]
fn merge_deduplicates_with_existing() {
    let tmp = tempfile::tempdir().unwrap();
    let meta_dir = tmp.path().join(SKILL_METADATA_DIR);
    std::fs::create_dir_all(&meta_dir).unwrap();
    std::fs::write(meta_dir.join(DEPENDS_FILE), "dep-a\ndep-b\ndep-a\n").unwrap();

    let mut meta = make_skill_with_deps(&["dep-a"]);
    merge_depends_from_metadata(tmp.path(), &mut meta);

    // dep-a should not be duplicated
    assert_eq!(meta.depends, vec!["dep-a", "dep-b"]);
}

#[test]
fn merge_noop_when_no_file() {
    let tmp = tempfile::tempdir().unwrap();
    // No metadata/ directory
    let mut meta = make_skill_with_deps(&["existing"]);
    merge_depends_from_metadata(tmp.path(), &mut meta);
    assert_eq!(meta.depends, vec!["existing"]);
}
