use super::*;

fn create_skill(base: &std::path::Path, name: &str, dcc: &str, deps: &[&str]) {
    let skill_dir = base.join(name);
    std::fs::create_dir_all(&skill_dir).unwrap();

    let deps_str = if deps.is_empty() {
        String::new()
    } else {
        format!(
            "\ndepends:\n{}",
            deps.iter()
                .map(|d| format!("  - {d}"))
                .collect::<Vec<_>>()
                .join("\n")
        )
    };
    let content = format!("---\nname: {name}\ndcc: {dcc}{deps_str}\n---\n# {name}\n\nBody.");
    std::fs::write(skill_dir.join(SKILL_METADATA_FILE), &content).unwrap();
}

#[test]
fn load_empty_paths() {
    let result = scan_and_load(Some(&["/nonexistent-path".to_string()]), None).unwrap();
    assert!(result.skills.is_empty());
    assert!(result.skipped.is_empty());
}

#[test]
fn load_single_skill() {
    let tmp = tempfile::tempdir().unwrap();
    create_skill(tmp.path(), "basic", "python", &[]);

    let result = scan_and_load(Some(&[tmp.path().to_string_lossy().to_string()]), None).unwrap();
    assert_eq!(result.skills.len(), 1);
    assert_eq!(result.skills[0].name, "basic");
}

#[test]
fn load_with_dependency_order() {
    let tmp = tempfile::tempdir().unwrap();
    create_skill(tmp.path(), "base", "python", &[]);
    create_skill(tmp.path(), "middle", "python", &["base"]);
    create_skill(tmp.path(), "top", "python", &["middle"]);

    let result = scan_and_load(Some(&[tmp.path().to_string_lossy().to_string()]), None).unwrap();
    assert_eq!(result.skills.len(), 3);

    let names: Vec<&str> = result.skills.iter().map(|s| s.name.as_str()).collect();
    let base_pos = names.iter().position(|&n| n == "base").unwrap();
    let middle_pos = names.iter().position(|&n| n == "middle").unwrap();
    let top_pos = names.iter().position(|&n| n == "top").unwrap();
    assert!(base_pos < middle_pos, "base must come before middle");
    assert!(middle_pos < top_pos, "middle must come before top");
}

#[test]
fn load_fails_on_missing_dependency() {
    let tmp = tempfile::tempdir().unwrap();
    create_skill(tmp.path(), "broken", "python", &["nonexistent"]);

    let err = scan_and_load(Some(&[tmp.path().to_string_lossy().to_string()]), None).unwrap_err();
    assert!(matches!(
        err,
        crate::resolver::ResolveError::MissingDependency { .. }
    ));
}

#[test]
fn load_fails_on_cycle() {
    let tmp = tempfile::tempdir().unwrap();
    create_skill(tmp.path(), "a", "python", &["b"]);
    create_skill(tmp.path(), "b", "python", &["a"]);

    let err = scan_and_load(Some(&[tmp.path().to_string_lossy().to_string()]), None).unwrap_err();
    assert!(matches!(
        err,
        crate::resolver::ResolveError::CyclicDependency { .. }
    ));
}

#[test]
fn load_tracks_skipped_dirs() {
    let tmp = tempfile::tempdir().unwrap();
    create_skill(tmp.path(), "good", "python", &[]);

    // Create a directory without a valid SKILL.md
    let bad_dir = tmp.path().join("bad");
    std::fs::create_dir_all(&bad_dir).unwrap();
    std::fs::write(bad_dir.join(SKILL_METADATA_FILE), "no frontmatter at all").unwrap();

    let result = scan_and_load(Some(&[tmp.path().to_string_lossy().to_string()]), None).unwrap();
    assert_eq!(result.skills.len(), 1);
    assert_eq!(result.skills[0].name, "good");
    assert_eq!(result.skipped.len(), 1);
}
