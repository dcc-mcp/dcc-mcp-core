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

/// Regression: `depends.md` opens with an H1 title and a `##` description
/// line, exactly like `examples/skills/maya-pipeline/metadata/depends.md`.
/// Neither heading is a dependency, and the description must never become
/// one -- it used to be parsed as a skill name and broke resolution.
#[test]
fn merge_ignores_headings_and_prose_descriptions() {
    let tmp = tempfile::tempdir().unwrap();
    let meta_dir = tmp.path().join(SKILL_METADATA_DIR);
    std::fs::create_dir_all(&meta_dir).unwrap();
    std::fs::write(
        meta_dir.join(DEPENDS_FILE),
        "# Dependencies\n\n## Skills that must be loaded before this skill can function.\n\n- maya-geometry\n- usd-tools\n",
    )
    .unwrap();

    let mut meta = make_skill_with_deps(&[]);
    merge_depends_from_metadata(tmp.path(), &mut meta);

    assert_eq!(meta.depends, vec!["maya-geometry", "usd-tools"]);
}

/// A demoted description is prose whether or not it keeps a heading marker:
/// the parser must reject it by shape, not by the leading `#`.
#[test]
fn merge_ignores_prose_without_a_heading_marker() {
    let tmp = tempfile::tempdir().unwrap();
    let meta_dir = tmp.path().join(SKILL_METADATA_DIR);
    std::fs::create_dir_all(&meta_dir).unwrap();
    std::fs::write(
        meta_dir.join(DEPENDS_FILE),
        "Skills that must be loaded before this skill can function.\ndep-a\n",
    )
    .unwrap();

    let mut meta = make_skill_with_deps(&[]);
    merge_depends_from_metadata(tmp.path(), &mut meta);

    assert_eq!(meta.depends, vec!["dep-a"]);
}

/// Regression: a single-token prose word is not a skill name. `Optional`,
/// `TODO` and a heading that lost its `#` all used to be accepted as
/// dependencies and broke resolution.
#[test]
fn merge_ignores_single_token_prose_lines() {
    let tmp = tempfile::tempdir().unwrap();
    let meta_dir = tmp.path().join(SKILL_METADATA_DIR);
    std::fs::create_dir_all(&meta_dir).unwrap();
    std::fs::write(
        meta_dir.join(DEPENDS_FILE),
        "Optional\nTODO\nDependencies\nNote\ndep-a\n",
    )
    .unwrap();

    let mut meta = make_skill_with_deps(&[]);
    merge_depends_from_metadata(tmp.path(), &mut meta);

    assert_eq!(meta.depends, vec!["dep-a"]);
}

/// Regression: a list item whose text has interior whitespace describes a
/// dependency, it does not name one.
#[test]
fn merge_ignores_list_items_with_interior_whitespace() {
    let tmp = tempfile::tempdir().unwrap();
    let meta_dir = tmp.path().join(SKILL_METADATA_DIR);
    std::fs::create_dir_all(&meta_dir).unwrap();
    std::fs::write(
        meta_dir.join(DEPENDS_FILE),
        "- maya geometry\n- dep-a\n* usd and friends\n",
    )
    .unwrap();

    let mut meta = make_skill_with_deps(&[]);
    merge_depends_from_metadata(tmp.path(), &mut meta);

    assert_eq!(meta.depends, vec!["dep-a"]);
}

#[test]
fn merge_noop_when_no_file() {
    let tmp = tempfile::tempdir().unwrap();
    // No metadata/ directory
    let mut meta = make_skill_with_deps(&["existing"]);
    merge_depends_from_metadata(tmp.path(), &mut meta);
    assert_eq!(meta.depends, vec!["existing"]);
}
