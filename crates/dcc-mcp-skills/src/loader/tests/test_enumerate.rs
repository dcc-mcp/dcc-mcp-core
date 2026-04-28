use super::*;
use crate::constants::SKILL_SCRIPTS_DIR;

#[test]
fn enumerate_scripts_discovers_supported_files() {
    let tmp = tempfile::tempdir().unwrap();
    let scripts_dir = tmp.path().join(SKILL_SCRIPTS_DIR);
    std::fs::create_dir_all(&scripts_dir).unwrap();

    std::fs::write(scripts_dir.join("setup.py"), "# python").unwrap();
    std::fs::write(scripts_dir.join("run.mel"), "// mel").unwrap();
    std::fs::write(scripts_dir.join("notes.txt"), "not a script").unwrap();

    let result = enumerate_scripts(tmp.path());
    // .py and .mel are supported; .txt is not
    assert!(
        result.iter().any(|p| p.ends_with("setup.py")),
        "Expected .py file in {result:?}"
    );
    assert!(
        result.iter().any(|p| p.ends_with("run.mel")),
        "Expected .mel file in {result:?}"
    );
    assert!(
        !result.iter().any(|p| p.ends_with("notes.txt")),
        "Should not include .txt in {result:?}"
    );
}

#[test]
fn enumerate_scripts_empty_when_no_dir() {
    let tmp = tempfile::tempdir().unwrap();
    // No scripts/ directory exists
    let result = enumerate_scripts(tmp.path());
    assert!(result.is_empty());
}

#[test]
fn enumerate_metadata_files_discovers_md() {
    let tmp = tempfile::tempdir().unwrap();
    let meta_dir = tmp.path().join(SKILL_METADATA_DIR);
    std::fs::create_dir_all(&meta_dir).unwrap();

    std::fs::write(meta_dir.join("help.md"), "# Help").unwrap();
    std::fs::write(meta_dir.join("install.md"), "# Install").unwrap();
    std::fs::write(meta_dir.join("data.json"), "{}").unwrap();

    let result = enumerate_metadata_files(tmp.path());
    assert_eq!(result.len(), 2, "Should find exactly 2 .md files");
    assert!(result.iter().any(|p| p.ends_with("help.md")));
    assert!(result.iter().any(|p| p.ends_with("install.md")));
    assert!(!result.iter().any(|p| p.ends_with("data.json")));
}
