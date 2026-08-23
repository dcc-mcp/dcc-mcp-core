mod support;

use serde_json::{Value, json};
use tempfile::TempDir;

use support::{cli_command, write_skill};

#[test]
fn lint_recurses_two_levels_and_reports_validation_errors() {
    let tmp = TempDir::new().unwrap();
    write_skill(
        tmp.path(),
        "studio/maya-tools",
        "---\nname: maya-tools\ndescription: Valid test skill\n---\n",
    );
    write_skill(tmp.path(), "studio/bad-skill", "no frontmatter\n");
    write_skill(tmp.path(), "too/deep/ignored-skill", "no frontmatter\n");

    let output = cli_command().arg("lint").arg(tmp.path()).output().unwrap();

    assert!(!output.status.success());
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["checked"], 2);
    assert_eq!(value["errors"], 1);
    let reports = value["reports"].as_array().unwrap();
    assert!(reports.iter().any(|report| {
        report["skill_dir"]
            .as_str()
            .is_some_and(|path| path.contains("bad-skill"))
    }));
    assert!(!reports.iter().any(|report| {
        report["skill_dir"]
            .as_str()
            .is_some_and(|path| path.contains("ignored-skill"))
    }));
}

#[test]
fn lint_probes_declared_execution_contracts_through_core_dispatch() {
    let tmp = TempDir::new().unwrap();
    let skill_dir = write_skill(
        tmp.path(),
        "blender-contract-probe",
        "---\nname: blender-contract-probe\ndescription: Exercise sync and async routing\nmetadata:\n  dcc-mcp:\n    dcc: blender\n    tools: tools.yaml\n---\n",
    );
    std::fs::create_dir_all(skill_dir.join("scripts")).unwrap();
    std::fs::write(
        skill_dir.join("tools.yaml"),
        r#"tools:
  - name: inspect_scene
    description: Return a scene summary synchronously.
    source_file: scripts/inspect_scene.py
    execution: sync
    timeout_hint_secs: 5
    affinity: any
  - name: render_preview
    description: Queue a preview render asynchronously.
    source_file: scripts/render_preview.py
    execution: async
    timeout_hint_secs: 30
    affinity: main
"#,
    )
    .unwrap();
    for script in ["inspect_scene.py", "render_preview.py"] {
        std::fs::write(
            skill_dir.join("scripts").join(script),
            "def main(**kwargs):\n    return {'ok': True}\n",
        )
        .unwrap();
    }

    let output = cli_command().arg("lint").arg(&skill_dir).output().unwrap();

    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["execution_contracts_checked"], 2);
    assert_eq!(value["reports"][0]["execution_contract"]["checked"], 2);
    assert_eq!(
        value["reports"][0]["execution_contract"]["issues"],
        json!([])
    );
}

#[test]
fn lint_bundled_skills_are_present_and_clean() {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir
        .parent()
        .and_then(std::path::Path::parent)
        .unwrap();
    let builtin_skill_roots = [
        workspace_root.join("skills/dcc-mcp"),
        workspace_root.join("python/dcc_mcp_core/skills"),
    ];

    for root in &builtin_skill_roots {
        assert!(
            root.is_dir(),
            "missing bundled skill root: {}",
            root.display()
        );
    }

    let output = cli_command()
        .arg("lint")
        .arg("--max-depth")
        .arg("4")
        .args(&builtin_skill_roots)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(
        value["checked"].as_u64().unwrap() > 0,
        "expected bundled skills to be linted"
    );
    assert_eq!(value["errors"], 0);
    assert_eq!(value["warnings"], 0);
}
