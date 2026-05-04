use super::fixtures::SkillTestFixture;
use super::*;

/// Helper: produce a minimal spec-compliant SKILL.md content string
/// (agentskills.io 1.0 + `metadata.dcc-mcp.*` nested form).
fn skill_md(name: &str, dcc: &str, deps: &[&str]) -> String {
    let deps_str = if deps.is_empty() {
        String::new()
    } else {
        format!("\n    depends: \"{}\"", deps.join(", "))
    };
    format!(
        "---\nname: {name}\ndescription: test skill\nmetadata:\n  dcc-mcp:\n    dcc: {dcc}{deps_str}\n---\n# {name}\n\nDescription text.",
    )
}

#[test]
fn parse_valid_skill() {
    let fx = SkillTestFixture::with_body(&skill_md("my-skill", "maya", &[]));
    let meta = parse_skill_md(fx.path()).unwrap();
    assert_eq!(meta.name, "my-skill");
    assert_eq!(meta.dcc, "maya");
    assert!(meta.depends.is_empty());
    assert!(!meta.skill_path.is_empty());
}

#[test]
fn parse_skill_with_depends() {
    let fx =
        SkillTestFixture::with_body(&skill_md("pipeline", "houdini", &["geometry", "usd-tools"]));
    let meta = parse_skill_md(fx.path()).unwrap();
    assert_eq!(meta.name, "pipeline");
    assert_eq!(meta.depends, vec!["geometry", "usd-tools"]);
}

#[test]
fn parse_skill_with_scripts() {
    let fx = SkillTestFixture::with_body(&skill_md("scripted", "blender", &[]));
    use crate::constants::SKILL_SCRIPTS_DIR;
    fx.write_file(&format!("{SKILL_SCRIPTS_DIR}/run.py"), "print('hello')");
    let meta = parse_skill_md(fx.path()).unwrap();
    assert_eq!(meta.scripts.len(), 1);
    assert!(meta.scripts[0].ends_with("run.py"));
}

#[test]
fn parse_skill_with_metadata_depends() {
    let fx = SkillTestFixture::with_body(&skill_md("composite", "maya", &["frontmatter-dep"]));
    fx.write_file(
        &format!("{SKILL_METADATA_DIR}/{DEPENDS_FILE}"),
        "file-dep\n",
    );
    let meta = parse_skill_md(fx.path()).unwrap();
    assert!(meta.depends.contains(&"frontmatter-dep".to_string()));
    assert!(meta.depends.contains(&"file-dep".to_string()));
}

#[test]
fn parse_skill_fallback_name_from_dir() {
    let fx = SkillTestFixture::with_body(
        "---\nname: \"\"\ndescription: unnamed\nmetadata:\n  dcc-mcp:\n    dcc: pytho\n---\n# Unnamed",
    );
    let meta = parse_skill_md(fx.path()).unwrap();
    // Name should be the directory name (tempdir's last component)
    assert!(!meta.name.is_empty());
}

#[test]
fn parse_skill_with_tool_defer_loading_aliases() {
    let fx = SkillTestFixture::with_body(
        "---\nname: deferred-skill\ndescription: defer\nmetadata:\n  dcc-mcp:\n    dcc: python\n    tools: tools.yaml\n---\n# Deferred\n",
    );
    fx.write_file(
        "tools.yaml",
        "tools:\n  - name: slow_tool\n    defer-loading: true\n  - name: alias_tool\n    defer_loading: true\n",
    );
    let meta = parse_skill_md(fx.path()).unwrap();
    assert_eq!(meta.tools.len(), 2);
    assert!(meta.tools[0].defer_loading);
    assert!(meta.tools[1].defer_loading);
}

#[test]
fn parse_skill_with_tool_execution_async() {
    // Issue #317 — `execution: async` and `timeout_hint_secs` round-trip.
    let fx = SkillTestFixture::with_body(
        "---\nname: render-farm\ndescription: render\nmetadata:\n  dcc-mcp:\n    dcc: python\n    tools: tools.yaml\n---\n# Render\n",
    );
    fx.write_file(
        "tools.yaml",
        "tools:\n  - name: render_frames\n    execution: async\n    timeout_hint_secs: 600\n  - name: quick_check\n",
    );
    let meta = parse_skill_md(fx.path()).unwrap();
    assert_eq!(meta.tools.len(), 2);
    assert_eq!(
        meta.tools[0].execution,
        dcc_mcp_models::ExecutionMode::Async,
    );
    assert_eq!(meta.tools[0].timeout_hint_secs, Some(600));
    // Absence defaults to Sync / None.
    assert_eq!(meta.tools[1].execution, dcc_mcp_models::ExecutionMode::Sync,);
    assert_eq!(meta.tools[1].timeout_hint_secs, None);
}

#[test]
fn parse_skill_rejects_user_level_deferred_flag() {
    // Issue #317 — `deferred: true` at the user level must be rejected.
    // The bad value lives in the sibling tools.yaml; the loader must
    // still reject the skill as a whole.
    let fx = SkillTestFixture::with_body(
        "---\nname: bad\ndescription: bad\nmetadata:\n  dcc-mcp:\n    dcc: python\n    tools: tools.yaml\n---\n# Bad\n",
    );
    fx.write_file("tools.yaml", "tools:\n  - name: x\n    deferred: true\n");
    let meta = parse_skill_md(fx.path());
    // Either the whole skill load returns None, or the offending tool
    // is simply not surfaced on `meta.tools`. Both count as "not
    // accepted" — assert neither admits a tool with `deferred: true`.
    match meta {
        None => {}
        Some(m) => assert!(
            m.tools.is_empty(),
            "deferred tool must not appear in tools: {:?}",
            m.tools
        ),
    }
}

#[test]
fn parse_skill_rejects_unknown_execution_value() {
    // Issue #317 — an unknown `execution` value must not silently produce
    // a tool declaration. The value lives in the sibling tools.yaml.
    let fx = SkillTestFixture::with_body(
        "---\nname: bad\ndescription: bad\nmetadata:\n  dcc-mcp:\n    dcc: python\n    tools: tools.yaml\n---\n# Bad\n",
    );
    fx.write_file(
        "tools.yaml",
        "tools:\n  - name: x\n    execution: background\n",
    );
    let meta = parse_skill_md(fx.path());
    match meta {
        None => {}
        Some(m) => assert!(
            m.tools.is_empty(),
            "unknown execution value must not produce a tool: {:?}",
            m.tools
        ),
    }
}

#[test]
fn parse_skill_without_execution_defaults_to_sync() {
    // Issue #317 — a SKILL.md that does not set `execution` must
    // continue to load; tools default to Sync with no timeout hint.
    let fx = SkillTestFixture::with_body(
        "---\nname: no-exec\ndescription: no exec\nmetadata:\n  dcc-mcp:\n    dcc: python\n    tools: tools.yaml\n---\n# No Exec\n",
    );
    fx.write_file(
        "tools.yaml",
        "tools:\n  - name: do_thing\n    description: does a thing\n",
    );
    let meta = parse_skill_md(fx.path()).unwrap();
    assert_eq!(meta.tools.len(), 1);
    assert_eq!(meta.tools[0].execution, dcc_mcp_models::ExecutionMode::Sync,);
    assert_eq!(meta.tools[0].timeout_hint_secs, None);
}

#[test]
fn parse_returns_none_for_missing_skill_md() {
    let fx = SkillTestFixture::empty();
    // No SKILL.md file
    assert!(parse_skill_md(fx.path()).is_none());
}

#[test]
fn parse_returns_none_for_invalid_yaml() {
    let fx = SkillTestFixture::with_body("---\n: invalid: yaml: [broken\n---\n");
    assert!(parse_skill_md(fx.path()).is_none());
}

#[test]
fn parse_returns_none_for_no_frontmatter() {
    let fx = SkillTestFixture::with_body("Just plain markdown without frontmatter.");
    assert!(parse_skill_md(fx.path()).is_none());
}
