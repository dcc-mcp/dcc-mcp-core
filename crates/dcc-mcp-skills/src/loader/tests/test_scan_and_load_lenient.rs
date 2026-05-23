use super::*;

fn create_skill(base: &std::path::Path, name: &str, deps: &[&str]) {
    let skill_dir = base.join(name);
    std::fs::create_dir_all(&skill_dir).unwrap();

    let deps_str = if deps.is_empty() {
        String::new()
    } else {
        format!("\n    depends: \"{}\"", deps.join(", "))
    };
    let content = format!(
        "---\nname: {name}\ndescription: test skill\nmetadata:\n  dcc-mcp:\n    dcc: python{deps_str}\n---\n# {name}\n\nBody.",
    );
    std::fs::write(skill_dir.join(SKILL_METADATA_FILE), &content).unwrap();
}

#[test]
fn lenient_keeps_missing_deps_discoverable() {
    let tmp = tempfile::tempdir().unwrap();
    create_skill(tmp.path(), "good", &[]);
    create_skill(tmp.path(), "broken", &["nonexistent"]);

    let result =
        scan_and_load_lenient(Some(&[tmp.path().to_string_lossy().to_string()]), None).unwrap();
    let names: Vec<&str> = result
        .skills
        .iter()
        .map(|skill| skill.name.as_str())
        .collect();
    assert_eq!(names.len(), 2);
    assert!(names.contains(&"good"));
    assert!(names.contains(&"broken"));
    assert!(
        result.skipped.is_empty(),
        "missing soft dependencies are not parse/load skips"
    );
}

#[test]
fn lenient_still_fails_on_cycle() {
    let tmp = tempfile::tempdir().unwrap();
    create_skill(tmp.path(), "a", &["b"]);
    create_skill(tmp.path(), "b", &["a"]);

    let err =
        scan_and_load_lenient(Some(&[tmp.path().to_string_lossy().to_string()]), None).unwrap_err();
    assert!(matches!(
        err,
        crate::resolver::ResolveError::CyclicDependency { .. }
    ));
}

#[test]
fn lenient_preserves_valid_skills() {
    let tmp = tempfile::tempdir().unwrap();
    create_skill(tmp.path(), "base", &[]);
    create_skill(tmp.path(), "child", &["base"]);
    create_skill(tmp.path(), "orphan", &["missing-dep"]);

    let result =
        scan_and_load_lenient(Some(&[tmp.path().to_string_lossy().to_string()]), None).unwrap();
    let names: Vec<&str> = result.skills.iter().map(|s| s.name.as_str()).collect();
    assert!(names.contains(&"base"));
    assert!(names.contains(&"child"));
    assert!(names.contains(&"orphan"));
    assert!(
        names.iter().position(|name| *name == "base").unwrap()
            < names.iter().position(|name| *name == "child").unwrap()
    );
}

#[test]
fn lenient_empty_when_all_valid() {
    let tmp = tempfile::tempdir().unwrap();
    create_skill(tmp.path(), "a", &[]);
    create_skill(tmp.path(), "b", &["a"]);

    let result =
        scan_and_load_lenient(Some(&[tmp.path().to_string_lossy().to_string()]), None).unwrap();
    assert_eq!(result.skills.len(), 2);
    // No parse-failures, no dependency-failures
    assert!(result.skipped.is_empty());
}
