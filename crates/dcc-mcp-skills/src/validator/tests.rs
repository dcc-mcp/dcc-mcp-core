use super::*;
use std::io::Write;
use std::path::PathBuf;

fn make_skill_dir(tmp: &tempfile::TempDir, name: &str, content: &str) -> PathBuf {
    let dir = tmp.path().join(name);
    std::fs::create_dir_all(&dir).unwrap();
    let mut file = std::fs::File::create(dir.join("SKILL.md")).unwrap();
    file.write_all(content.as_bytes()).unwrap();
    dir
}

#[test]
fn test_missing_skill_md() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("no-skill");
    std::fs::create_dir(&dir).unwrap();
    let report = validate_skill_dir(&dir);
    assert!(report.has_errors());
    assert_eq!(report.issues[0].category, IssueCategory::SkillMd);
}

#[test]
fn test_missing_frontmatter() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = make_skill_dir(&tmp, "bad", "no frontmatter here\n");
    let report = validate_skill_dir(&dir);
    assert!(report.has_errors());
    assert!(report.issues[0].message.contains("frontmatter"));
}

#[test]
fn test_missing_required_fields() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = make_skill_dir(&tmp, "empty", "---\nname: empty\n---\n");
    let report = validate_skill_dir(&dir);
    assert!(report.has_errors());
    let messages: Vec<_> = report
        .issues
        .iter()
        .map(|issue| issue.message.as_str())
        .collect();
    assert!(
        messages
            .iter()
            .any(|message| message.contains("description"))
    );
}

#[test]
fn test_name_too_long() {
    let tmp = tempfile::tempdir().unwrap();
    let long_name = "a".repeat(65);
    let content = format!("---\nname: {}\ndescription: test\n---\n", long_name);
    let dir = make_skill_dir(&tmp, &long_name, &content);
    let report = validate_skill_dir(&dir);
    assert!(report.has_errors());
    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue.message.contains("64"))
    );
}

#[test]
fn test_name_not_kebab_case() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = make_skill_dir(
        &tmp,
        "bad-name",
        "---\nname: BadName\ndescription: test\n---\n",
    );
    let report = validate_skill_dir(&dir);
    assert!(report.has_errors());
    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue.message.contains("kebab-case"))
    );
}

#[test]
fn test_description_too_long() {
    let tmp = tempfile::tempdir().unwrap();
    let long_desc = "x".repeat(1025);
    let content = format!("---\nname: my-skill\ndescription: {}\n---\n", long_desc);
    let dir = make_skill_dir(&tmp, "my-skill", &content);
    let report = validate_skill_dir(&dir);
    assert!(report.has_errors());
    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue.message.contains("1024"))
    );
}

#[test]
fn test_valid_skill_passes() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = make_skill_dir(
        &tmp,
        "my-skill",
        "---\nname: my-skill\ndescription: A valid test skill\n---\n",
    );
    let report = validate_skill_dir(&dir);
    assert!(
        report.is_clean(),
        "expected clean report, got: {:?}",
        report.issues
    );
}

#[test]
fn test_name_dir_mismatch_warns() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = make_skill_dir(
        &tmp,
        "actual-dir",
        "---\nname: different-name\ndescription: test\n---\n",
    );
    let report = validate_skill_dir(&dir);
    assert!(report.issues.iter().any(|issue| {
        issue.severity == IssueSeverity::Warning && issue.message.contains("directory name")
    }));
}

#[test]
fn test_missing_source_file() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = make_skill_dir(
        &tmp,
        "my-skill",
        "---\nname: my-skill\ndescription: test\ntools:\n  - name: do_thing\n    source_file: scripts/do_thing.py\n---\n",
    );
    let report = validate_skill_dir(&dir);
    assert!(report.has_errors());
    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue.message.contains("source_file"))
    );
}

#[test]
fn test_python_source_rejects_sibling_sys_path_mutation() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("bad-import-contract");
    std::fs::create_dir_all(dir.join("scripts")).unwrap();
    std::fs::write(
        dir.join("SKILL.md"),
        "---\nname: bad-import-contract\ndescription: test\ntools:\n  - name: do_thing\n    source_file: scripts/do_thing.py\n---\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("scripts/do_thing.py"),
        "from _common import helper\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("scripts/_common.py"),
        "from pathlib import Path\nimport sys\nscript_dir = Path(__file__).parent\nsys.path.insert(0, str(script_dir))\n",
    )
    .unwrap();

    let report = validate_skill_dir(&dir);

    assert!(report.issues.iter().any(|issue| {
        issue.category == IssueCategory::Scripts
            && issue
                .message
                .contains("shared runner owns script-directory import setup")
    }));
}

#[test]
fn test_python_source_allows_intentional_project_path_attachment() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("project-loader");
    std::fs::create_dir_all(dir.join("scripts")).unwrap();
    std::fs::write(
        dir.join("SKILL.md"),
        "---\nname: project-loader\ndescription: test\ntools:\n  - name: attach_project\n    source_file: scripts/attach_project.py\n---\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("scripts/attach_project.py"),
        "import sys\ndef main(project_root):\n    sys.path.insert(0, project_root)\n",
    )
    .unwrap();

    let report = validate_skill_dir(&dir);

    assert!(!report.issues.iter().any(|issue| {
        issue.category == IssueCategory::Scripts
            && issue
                .message
                .contains("shared runner owns script-directory import setup")
    }));
}

#[test]
fn test_non_spec_top_level_keys_error() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = make_skill_dir(
        &tmp,
        "legacy-skill",
        "---\nname: legacy-skill\ndescription: test\ndcc: maya\ntags: [modeling]\n---\n",
    );
    let report = validate_skill_dir(&dir);
    assert!(report.issues.iter().any(|issue| {
        issue.severity == IssueSeverity::Error && issue.message.contains("Non-spec top-level")
    }));
}

#[test]
fn test_top_level_version_error_includes_actionable_replacement() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = make_skill_dir(
        &tmp,
        "maya-mgear",
        "---\nname: maya-mgear\ndescription: test\nversion: \"1.0.0\"\n---\n",
    );
    let report = validate_skill_dir(&dir);

    let message = report
        .issues
        .iter()
        .find(|issue| issue.severity == IssueSeverity::Error && issue.message.contains("version"))
        .map(|issue| issue.message.as_str())
        .expect("top-level version must be reported as an error");

    assert!(message.contains("metadata.dcc-mcp.version"));
    assert!(message.contains("metadata.dcc-mcp.version: \"1.0.0\""));
}

#[test]
fn test_duplicate_tool_names() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = make_skill_dir(
        &tmp,
        "dup-skill",
        "---\nname: dup-skill\ndescription: test\ntools:\n  - name: do_thing\n  - name: do_thing\n---\n",
    );
    let report = validate_skill_dir(&dir);
    assert!(report.has_errors());
    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue.message.contains("duplicate"))
    );
}

#[test]
fn test_unknown_group_reference() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = make_skill_dir(
        &tmp,
        "grp-skill",
        "---\nname: grp-skill\ndescription: test\ntools:\n  - name: do_thing\n    group: nonexistent\n---\n",
    );
    let report = validate_skill_dir(&dir);
    assert!(report.has_errors());
    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue.message.contains("unknown group"))
    );
}

#[test]
fn test_inline_only_script_tool_warns_about_file_backed_path() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = make_skill_dir(
        &tmp,
        "script-skill",
        "---\nname: script-skill\ndescription: test\ntools:\n  - name: execute_python\n    description: Execute Python in the host\n    input_schema:\n      type: object\n      properties:\n        code:\n          type: string\n---\n",
    );
    let report = validate_skill_dir(&dir);
    assert!(report.issues.iter().any(|issue| {
        issue.severity == IssueSeverity::Warning
            && issue.category == IssueCategory::Tools
            && issue.message.contains("file_path/script_path")
    }));
}

#[test]
fn test_script_tool_with_file_path_does_not_warn_about_inline_code() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = make_skill_dir(
        &tmp,
        "script-skill",
        "---\nname: script-skill\ndescription: test\ntools:\n  - name: execute_python\n    description: Execute Python in the host\n    input_schema:\n      type: object\n      properties:\n        code:\n          type: string\n        file_path:\n          type: string\n---\n",
    );
    let report = validate_skill_dir(&dir);
    assert!(
        !report
            .issues
            .iter()
            .any(|issue| issue.message.contains("file_path/script_path")),
        "file-backed schema should not warn, got: {:?}",
        report.issues
    );
}

#[test]
fn test_empty_dependency_entry() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = make_skill_dir(
        &tmp,
        "dep-skill",
        "---\nname: dep-skill\ndescription: test\ndepends: ['other-skill', ' ']\n---\n",
    );
    let report = validate_skill_dir(&dir);
    assert!(report.has_errors());
    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue.message.contains("empty"))
    );
}

#[test]
fn test_nested_dcc_mcp_depends_satisfies_depends_docs_warning() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = make_skill_dir(
        &tmp,
        "dep-skill",
        "---\nname: dep-skill\ndescription: test\nmetadata:\n  dcc-mcp:\n    depends: base-skill\n---\n",
    );
    std::fs::create_dir_all(dir.join("metadata")).unwrap();
    std::fs::write(dir.join("metadata").join("depends.md"), "- base-skill\n").unwrap();

    let report = validate_skill_dir(&dir);

    assert!(
        !report
            .issues
            .iter()
            .any(|issue| issue.message.contains("no depends declared")),
        "expected nested metadata.dcc-mcp.depends to count as a dependency declaration, got: {:?}",
        report.issues
    );
}

#[test]
fn test_unsupported_script_extension() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = make_skill_dir(
        &tmp,
        "ext-skill",
        "---\nname: ext-skill\ndescription: test\ntools:\n  - name: do_thing\n    source_file: scripts/do_thing.txt\n---\n",
    );
    let scripts_dir = dir.join("scripts");
    std::fs::create_dir_all(&scripts_dir).unwrap();
    std::fs::File::create(scripts_dir.join("do_thing.txt")).unwrap();
    let report = validate_skill_dir(&dir);
    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue.message.contains("unsupported extension"))
    );
}
