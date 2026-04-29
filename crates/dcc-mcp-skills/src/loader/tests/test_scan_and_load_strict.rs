//! Tests for the strict scan-and-load pipeline (issue maya#138).

use super::*;

fn write_skill(base: &std::path::Path, name: &str) {
    let dir = base.join(name);
    std::fs::create_dir_all(&dir).unwrap();
    let content = format!("---\nname: {name}\ndcc: python\n---\n# {name}\n\nBody.");
    std::fs::write(dir.join(SKILL_METADATA_FILE), &content).unwrap();
}

fn write_broken_skill(base: &std::path::Path, name: &str) {
    // Directory has a SKILL.md (so the scanner picks it up) but the file
    // is missing the YAML frontmatter required by `parse_skill_md`, so
    // `load_all_skills` reports it via the `skipped` channel — which
    // `scan_and_load_strict` then promotes to an error.
    let dir = base.join(name);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join(SKILL_METADATA_FILE),
        "# broken skill\n\nNo YAML frontmatter here.\n",
    )
    .unwrap();
}

#[test]
fn strict_returns_skills_when_no_directories_skipped() {
    let tmp = tempfile::tempdir().unwrap();
    write_skill(tmp.path(), "alpha");
    write_skill(tmp.path(), "beta");

    let result = crate::loader::scan_and_load_strict(
        Some(&[tmp.path().to_string_lossy().to_string()]),
        None,
    )
    .unwrap();

    assert_eq!(result.skills.len(), 2);
    assert!(result.skipped.is_empty());
}

#[test]
fn strict_errors_on_skipped_directory() {
    let tmp = tempfile::tempdir().unwrap();
    write_skill(tmp.path(), "good");
    write_broken_skill(tmp.path(), "broken");

    let err = crate::loader::scan_and_load_strict(
        Some(&[tmp.path().to_string_lossy().to_string()]),
        None,
    )
    .unwrap_err();

    match err {
        crate::resolver::ResolveError::SkippedDirectories { directories } => {
            assert_eq!(directories.len(), 1, "got: {directories:?}");
            assert!(directories[0].ends_with("broken"));
        }
        other => panic!("expected SkippedDirectories, got {other:?}"),
    }
}

#[test]
fn strict_surface_message_mentions_remediation() {
    // Display impl must point operators at scan_and_load_lenient so they
    // know how to opt back into the silent-skip behaviour.
    let err = crate::resolver::ResolveError::SkippedDirectories {
        directories: vec!["/tmp/skills/broken".to_string()],
    };
    let msg = err.to_string();
    assert!(msg.contains("scan_and_load_lenient"), "msg={msg}");
    assert!(msg.contains("/tmp/skills/broken"), "msg={msg}");
}
