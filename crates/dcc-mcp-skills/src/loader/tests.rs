use super::*;

use dcc_mcp_utils::constants::{DEPENDS_FILE, SKILL_METADATA_DIR, SKILL_METADATA_FILE};

// ── extract_frontmatter ──

mod test_extract_frontmatter {
    use super::*;

    #[test]
    fn valid_frontmatter() {
        let content = "---\nname: test\ndescription: hello\n---\n# Body";
        let fm = extract_frontmatter(content).unwrap();
        assert!(fm.contains("name: test"));
        assert!(fm.contains("description: hello"));
    }

    #[test]
    fn no_frontmatter() {
        assert!(extract_frontmatter("no frontmatter").is_none());
    }

    #[test]
    fn empty_frontmatter() {
        let content = "---\n---\n# Body";
        let fm = extract_frontmatter(content).unwrap();
        assert!(fm.is_empty());
    }

    #[test]
    fn frontmatter_with_lists() {
        let content = "---\nname: test\ntags:\n  - geometry\n  - creation\n---\nBody";
        let fm = extract_frontmatter(content).unwrap();
        assert!(fm.contains("tags:"));
        assert!(fm.contains("- geometry"));
    }

    #[test]
    fn no_closing_delimiter() {
        let content = "---\nname: test\nno closing delimiter";
        assert!(extract_frontmatter(content).is_none());
    }
}

// ── enumerate helpers (using tempfile) ──

mod test_enumerate {
    use super::*;
    use dcc_mcp_utils::constants::SKILL_SCRIPTS_DIR;

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
}

// ── merge_depends_from_metadata ──

mod test_merge_depends {
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
}

// ── parse_skill_md (full integration) ──

mod test_parse_skill_md {
    use super::*;

    /// Helper to create a minimal SKILL.md content.
    fn skill_md(name: &str, dcc: &str, deps: &[&str]) -> String {
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
        format!("---\nname: {name}\ndcc: {dcc}{deps_str}\n---\n# {name}\n\nDescription text.")
    }

    #[test]
    fn parse_valid_skill() {
        let tmp = tempfile::tempdir().unwrap();
        let content = skill_md("my-skill", "maya", &[]);
        std::fs::write(tmp.path().join(SKILL_METADATA_FILE), &content).unwrap();

        let meta = parse_skill_md(tmp.path()).unwrap();
        assert_eq!(meta.name, "my-skill");
        assert_eq!(meta.dcc, "maya");
        assert!(meta.depends.is_empty());
        assert!(!meta.skill_path.is_empty());
    }

    #[test]
    fn parse_skill_with_depends() {
        let tmp = tempfile::tempdir().unwrap();
        let content = skill_md("pipeline", "houdini", &["geometry", "usd-tools"]);
        std::fs::write(tmp.path().join(SKILL_METADATA_FILE), &content).unwrap();

        let meta = parse_skill_md(tmp.path()).unwrap();
        assert_eq!(meta.name, "pipeline");
        assert_eq!(meta.depends, vec!["geometry", "usd-tools"]);
    }

    #[test]
    fn parse_skill_with_scripts() {
        let tmp = tempfile::tempdir().unwrap();
        let content = skill_md("scripted", "blender", &[]);
        std::fs::write(tmp.path().join(SKILL_METADATA_FILE), &content).unwrap();

        use dcc_mcp_utils::constants::SKILL_SCRIPTS_DIR;
        let scripts_dir = tmp.path().join(SKILL_SCRIPTS_DIR);
        std::fs::create_dir_all(&scripts_dir).unwrap();
        std::fs::write(scripts_dir.join("run.py"), "print('hello')").unwrap();

        let meta = parse_skill_md(tmp.path()).unwrap();
        assert_eq!(meta.scripts.len(), 1);
        assert!(meta.scripts[0].ends_with("run.py"));
    }

    #[test]
    fn parse_skill_with_metadata_depends() {
        let tmp = tempfile::tempdir().unwrap();
        let content = skill_md("composite", "maya", &["frontmatter-dep"]);
        std::fs::write(tmp.path().join(SKILL_METADATA_FILE), &content).unwrap();

        let meta_dir = tmp.path().join(SKILL_METADATA_DIR);
        std::fs::create_dir_all(&meta_dir).unwrap();
        std::fs::write(meta_dir.join(DEPENDS_FILE), "file-dep\n").unwrap();

        let meta = parse_skill_md(tmp.path()).unwrap();
        assert!(meta.depends.contains(&"frontmatter-dep".to_string()));
        assert!(meta.depends.contains(&"file-dep".to_string()));
    }

    #[test]
    fn parse_skill_fallback_name_from_dir() {
        let tmp = tempfile::tempdir().unwrap();
        // Frontmatter with empty name => should use directory name
        std::fs::write(
            tmp.path().join(SKILL_METADATA_FILE),
            "---\nname: \"\"\ndcc: python\n---\n# Unnamed",
        )
        .unwrap();

        let meta = parse_skill_md(tmp.path()).unwrap();
        // Name should be the directory name (tempdir's last component)
        assert!(!meta.name.is_empty());
    }

    #[test]
    fn parse_skill_with_tool_defer_loading_aliases() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join(SKILL_METADATA_FILE),
            "---\nname: deferred-skill\ndcc: python\ntools:\n  - name: slow_tool\n    defer-loading: true\n  - name: alias_tool\n    defer_loading: true\n---\n# Deferred\n",
        )
        .unwrap();

        let meta = parse_skill_md(tmp.path()).unwrap();
        assert_eq!(meta.tools.len(), 2);
        assert!(meta.tools[0].defer_loading);
        assert!(meta.tools[1].defer_loading);
    }

    #[test]
    fn parse_skill_with_tool_execution_async() {
        // Issue #317 — `execution: async` and `timeout_hint_secs` round-trip.
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join(SKILL_METADATA_FILE),
            "---\nname: render-farm\ndcc: python\ntools:\n  - name: render_frames\n    execution: async\n    timeout_hint_secs: 600\n  - name: quick_check\n---\n# Render\n",
        )
        .unwrap();

        let meta = parse_skill_md(tmp.path()).unwrap();
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
        // Issue #317 — `deferred: true` at the user level must be rejected,
        // parse_skill_md logs and returns None.
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join(SKILL_METADATA_FILE),
            "---\nname: bad\ndcc: python\ntools:\n  - name: x\n    deferred: true\n---\n# Bad\n",
        )
        .unwrap();
        assert!(parse_skill_md(tmp.path()).is_none());
    }

    #[test]
    fn parse_skill_rejects_unknown_execution_value() {
        // Issue #317 — unknown execution value fails at load time.
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join(SKILL_METADATA_FILE),
            "---\nname: bad\ndcc: python\ntools:\n  - name: x\n    execution: background\n---\n# Bad\n",
        )
        .unwrap();
        assert!(parse_skill_md(tmp.path()).is_none());
    }

    #[test]
    fn parse_skill_backward_compat_without_execution() {
        // Issue #317 — existing pre-change SKILL.md files (no `execution` key)
        // must continue to load and default to Sync / None.
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join(SKILL_METADATA_FILE),
            "---\nname: legacy\ndcc: python\ntools:\n  - name: do_thing\n    description: does a thing\n---\n# Legacy\n",
        )
        .unwrap();
        let meta = parse_skill_md(tmp.path()).unwrap();
        assert_eq!(meta.tools.len(), 1);
        assert_eq!(meta.tools[0].execution, dcc_mcp_models::ExecutionMode::Sync,);
        assert_eq!(meta.tools[0].timeout_hint_secs, None);
    }

    #[test]
    fn parse_returns_none_for_missing_skill_md() {
        let tmp = tempfile::tempdir().unwrap();
        // No SKILL.md file
        assert!(parse_skill_md(tmp.path()).is_none());
    }

    #[test]
    fn parse_returns_none_for_invalid_yaml() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join(SKILL_METADATA_FILE),
            "---\n: invalid: yaml: [broken\n---\n",
        )
        .unwrap();
        assert!(parse_skill_md(tmp.path()).is_none());
    }

    #[test]
    fn parse_returns_none_for_no_frontmatter() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join(SKILL_METADATA_FILE),
            "Just plain markdown without frontmatter.",
        )
        .unwrap();
        assert!(parse_skill_md(tmp.path()).is_none());
    }
}

// ── scan_and_load pipeline ──

mod test_scan_and_load {
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

        let result =
            scan_and_load(Some(&[tmp.path().to_string_lossy().to_string()]), None).unwrap();
        assert_eq!(result.skills.len(), 1);
        assert_eq!(result.skills[0].name, "basic");
    }

    #[test]
    fn load_with_dependency_order() {
        let tmp = tempfile::tempdir().unwrap();
        create_skill(tmp.path(), "base", "python", &[]);
        create_skill(tmp.path(), "middle", "python", &["base"]);
        create_skill(tmp.path(), "top", "python", &["middle"]);

        let result =
            scan_and_load(Some(&[tmp.path().to_string_lossy().to_string()]), None).unwrap();
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

        let err =
            scan_and_load(Some(&[tmp.path().to_string_lossy().to_string()]), None).unwrap_err();
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

        let err =
            scan_and_load(Some(&[tmp.path().to_string_lossy().to_string()]), None).unwrap_err();
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

        let result =
            scan_and_load(Some(&[tmp.path().to_string_lossy().to_string()]), None).unwrap();
        assert_eq!(result.skills.len(), 1);
        assert_eq!(result.skills[0].name, "good");
        assert_eq!(result.skipped.len(), 1);
    }
}

// ── scan_and_load_lenient ──

mod test_scan_and_load_lenient {
    use super::*;

    fn create_skill(base: &std::path::Path, name: &str, deps: &[&str]) {
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
        let content = format!("---\nname: {name}\ndcc: python{deps_str}\n---\n# {name}\n\nBody.");
        std::fs::write(skill_dir.join(SKILL_METADATA_FILE), &content).unwrap();
    }

    #[test]
    fn lenient_skips_missing_deps() {
        let tmp = tempfile::tempdir().unwrap();
        create_skill(tmp.path(), "good", &[]);
        create_skill(tmp.path(), "broken", &["nonexistent"]);

        let result =
            scan_and_load_lenient(Some(&[tmp.path().to_string_lossy().to_string()]), None).unwrap();
        assert_eq!(result.skills.len(), 1);
        assert_eq!(result.skills[0].name, "good");
        // broken should be in skipped
        assert!(!result.skipped.is_empty());
    }

    #[test]
    fn lenient_still_fails_on_cycle() {
        let tmp = tempfile::tempdir().unwrap();
        create_skill(tmp.path(), "a", &["b"]);
        create_skill(tmp.path(), "b", &["a"]);

        let err = scan_and_load_lenient(Some(&[tmp.path().to_string_lossy().to_string()]), None)
            .unwrap_err();
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
        assert!(!names.contains(&"orphan"));
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
}

// ── load_all_skills helper ──

mod test_load_all_skills {
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
}

// ── Issue #356: metadata.dcc-mcp.* compat ──

mod test_metadata_compat {
    use super::*;

    fn write_skill(skill_dir: &Path, body: &str) {
        std::fs::create_dir_all(skill_dir).unwrap();
        std::fs::write(skill_dir.join(SKILL_METADATA_FILE), body).unwrap();
    }

    #[test]
    fn legacy_form_flags_non_compliant() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("legacy");
        write_skill(
            &dir,
            "---\nname: legacy\ndcc: maya\nversion: \"2.0.0\"\ntags: [a, b]\n---\n# body\n",
        );
        let meta = parse_skill_md(&dir).expect("parsed");
        assert_eq!(meta.dcc, "maya");
        assert_eq!(meta.version, "2.0.0");
        assert_eq!(meta.tags, vec!["a".to_string(), "b".to_string()]);
        assert!(!meta.is_spec_compliant());
        assert!(meta.legacy_extension_fields.iter().any(|s| s == "dcc"));
    }

    #[test]
    fn new_form_is_spec_compliant() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("new");
        let body = r#"---
name: new
description: new-form skill
metadata:
  dcc-mcp.dcc: maya
  dcc-mcp.version: "2.0.0"
  dcc-mcp.tags: "a, b"
  dcc-mcp.search-hint: "hint words"
  dcc-mcp.depends: "other-skill"
---
# body
"#;
        write_skill(&dir, body);
        let meta = parse_skill_md(&dir).expect("parsed");
        assert!(meta.is_spec_compliant(), "expected spec compliant");
        assert_eq!(meta.dcc, "maya");
        assert_eq!(meta.version, "2.0.0");
        assert_eq!(meta.tags, vec!["a".to_string(), "b".to_string()]);
        assert_eq!(meta.search_hint, "hint words");
        assert_eq!(meta.depends, vec!["other-skill".to_string()]);
    }

    #[test]
    fn new_form_overrides_legacy_when_both_present() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("both");
        let body = r#"---
name: both
dcc: blender
metadata:
  dcc-mcp.dcc: maya
---
# body
"#;
        write_skill(&dir, body);
        let meta = parse_skill_md(&dir).expect("parsed");
        assert_eq!(meta.dcc, "maya", "metadata.dcc-mcp.dcc must win");
        // still marked legacy because top-level dcc was present
        assert!(!meta.is_spec_compliant());
    }

    #[test]
    fn nested_form_is_spec_compliant() {
        // Canonical agentskills.io shape produced by the migration tool
        // and by `yaml.safe_dump` — `metadata.dcc-mcp` is a nested map.
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("nested");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("tools.yaml"),
            "tools:\n  - name: create_sphere\n    description: make a sphere\n",
        )
        .unwrap();
        let body = r#"---
name: nested
description: nested metadata form
metadata:
  dcc-mcp:
    dcc: maya
    version: "1.0.0"
    tags: [maya, animation]
    search-hint: "keyframe, timeline"
    tools: tools.yaml
---
# body
"#;
        std::fs::write(dir.join(SKILL_METADATA_FILE), body).unwrap();
        let meta = parse_skill_md(&dir).expect("parsed");
        assert!(meta.is_spec_compliant(), "nested form must be compliant");
        assert_eq!(meta.dcc, "maya");
        assert_eq!(meta.version, "1.0.0");
        assert_eq!(meta.tags, vec!["maya".to_string(), "animation".to_string()]);
        assert_eq!(meta.search_hint, "keyframe, timeline");
        assert_eq!(meta.tools.len(), 1);
        assert_eq!(meta.tools[0].name, "create_sphere");
    }

    #[test]
    fn new_form_sibling_tools_yaml_resolves() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("sidecar");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("tools.yaml"),
            "tools:\n  - name: create_sphere\n    description: make a sphere\n  - ping\ngroups:\n  - name: advanced\n    default-active: false\n    tools: [create_sphere]\n",
        )
        .unwrap();
        let body = r#"---
name: sidecar
metadata:
  dcc-mcp.dcc: maya
  dcc-mcp.tools: tools.yaml
---
# body
"#;
        std::fs::write(dir.join(SKILL_METADATA_FILE), body).unwrap();
        let meta = parse_skill_md(&dir).expect("parsed");
        assert!(meta.is_spec_compliant());
        assert_eq!(meta.tools.len(), 2);
        assert_eq!(meta.tools[0].name, "create_sphere");
        assert_eq!(meta.tools[0].description, "make a sphere");
        assert_eq!(meta.tools[1].name, "ping");
        assert_eq!(meta.groups.len(), 1);
        assert_eq!(meta.groups[0].name, "advanced");
        assert!(!meta.groups[0].default_active);
    }

    #[test]
    fn new_form_products_and_implicit_invocation() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("policy");
        let body = r#"---
name: policy
metadata:
  dcc-mcp.products: "maya, houdini"
  dcc-mcp.allow-implicit-invocation: "false"
---
# body
"#;
        write_skill(&dir, body);
        let meta = parse_skill_md(&dir).expect("parsed");
        let policy = meta.policy.expect("policy must be set");
        assert_eq!(
            policy.products,
            vec!["maya".to_string(), "houdini".to_string()]
        );
        assert_eq!(policy.allow_implicit_invocation, Some(false));
    }

    #[test]
    fn both_forms_produce_same_values() {
        let tmp = tempfile::tempdir().unwrap();

        let legacy_dir = tmp.path().join("legacy");
        write_skill(
            &legacy_dir,
            "---\nname: same\ndcc: maya\nversion: \"1.2.3\"\ntags: [x, y]\nsearch-hint: hello\n---\n",
        );
        let legacy = parse_skill_md(&legacy_dir).expect("parsed");

        let new_dir = tmp.path().join("new");
        write_skill(
            &new_dir,
            r#"---
name: same
metadata:
  dcc-mcp.dcc: maya
  dcc-mcp.version: "1.2.3"
  dcc-mcp.tags: "x, y"
  dcc-mcp.search-hint: hello
---
"#,
        );
        let newf = parse_skill_md(&new_dir).expect("parsed");

        assert_eq!(legacy.dcc, newf.dcc);
        assert_eq!(legacy.version, newf.version);
        assert_eq!(legacy.tags, newf.tags);
        assert_eq!(legacy.search_hint, newf.search_hint);
        assert!(!legacy.is_spec_compliant());
        assert!(newf.is_spec_compliant());
    }

    // ── Issue #344 — ToolAnnotations from sibling tools.yaml ──────────

    /// Canonical nested `annotations:` map on a per-tool entry parses
    /// into `ToolDeclaration::annotations` with every hint set.
    #[test]
    fn annotations_canonical_nested_map_parses() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("canon");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("tools.yaml"),
            "tools:\n  - name: delete_keyframes\n    description: danger\n    annotations:\n      read_only_hint: false\n      destructive_hint: true\n      idempotent_hint: true\n      open_world_hint: false\n      deferred_hint: false\n",
        )
        .unwrap();
        let body = "---\nname: canon\nmetadata:\n  dcc-mcp.tools: tools.yaml\n---\n";
        std::fs::write(dir.join(SKILL_METADATA_FILE), body).unwrap();

        let meta = parse_skill_md(&dir).expect("parsed");
        assert_eq!(meta.tools.len(), 1);
        let ann = &meta.tools[0].annotations;
        assert_eq!(ann.read_only_hint, Some(false));
        assert_eq!(ann.destructive_hint, Some(true));
        assert_eq!(ann.idempotent_hint, Some(true));
        assert_eq!(ann.open_world_hint, Some(false));
        assert_eq!(ann.deferred_hint, Some(false));
    }

    /// Shorthand flat hint keys (e.g. `destructive_hint: true`) on the
    /// tool entry still parse — backward compatibility path.
    #[test]
    fn annotations_shorthand_flat_keys_parse() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("short");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("tools.yaml"),
            "tools:\n  - name: get_keyframes\n    read_only_hint: true\n    idempotent_hint: true\n",
        )
        .unwrap();
        let body = "---\nname: short\nmetadata:\n  dcc-mcp.tools: tools.yaml\n---\n";
        std::fs::write(dir.join(SKILL_METADATA_FILE), body).unwrap();

        let meta = parse_skill_md(&dir).expect("parsed");
        assert_eq!(meta.tools.len(), 1);
        let ann = &meta.tools[0].annotations;
        assert_eq!(ann.read_only_hint, Some(true));
        assert_eq!(ann.idempotent_hint, Some(true));
        // Undeclared hints stay None.
        assert_eq!(ann.destructive_hint, None);
        assert_eq!(ann.open_world_hint, None);
        assert_eq!(ann.deferred_hint, None);
    }

    /// When both the nested `annotations:` map and the shorthand flat
    /// keys are present for the same tool, the nested map wins entirely
    /// (whole-map replacement, not per-field merge).
    #[test]
    fn annotations_nested_wins_over_shorthand() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("mixed");
        std::fs::create_dir_all(&dir).unwrap();
        // Shorthand declares read_only=true, idempotent=true.
        // Nested declares destructive=true only.  After merge, the nested
        // map wins whole-map: read_only_hint/idempotent_hint MUST be None.
        std::fs::write(
            dir.join("tools.yaml"),
            "tools:\n  - name: risky\n    read_only_hint: true\n    idempotent_hint: true\n    annotations:\n      destructive_hint: true\n",
        )
        .unwrap();
        let body = "---\nname: mixed\nmetadata:\n  dcc-mcp.tools: tools.yaml\n---\n";
        std::fs::write(dir.join(SKILL_METADATA_FILE), body).unwrap();

        let meta = parse_skill_md(&dir).expect("parsed");
        let ann = &meta.tools[0].annotations;
        assert_eq!(ann.destructive_hint, Some(true));
        assert_eq!(
            ann.read_only_hint, None,
            "nested map wins whole-map; shorthand read_only_hint must be dropped"
        );
        assert_eq!(
            ann.idempotent_hint, None,
            "nested map wins whole-map; shorthand idempotent_hint must be dropped"
        );
    }

    /// Tools without any annotations declared leave the field empty.
    #[test]
    fn annotations_absent_is_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("bare");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("tools.yaml"),
            "tools:\n  - name: plain\n    description: nothing special\n",
        )
        .unwrap();
        let body = "---\nname: bare\nmetadata:\n  dcc-mcp.tools: tools.yaml\n---\n";
        std::fs::write(dir.join(SKILL_METADATA_FILE), body).unwrap();

        let meta = parse_skill_md(&dir).expect("parsed");
        assert!(meta.tools[0].annotations.is_empty());
    }

    // ── issue #342: next-tools in sibling tools.yaml ──

    #[test]
    fn sibling_tools_yaml_parses_next_tools() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("nt");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("tools.yaml"),
            r#"tools:
  - name: create_sphere
    description: make a sphere
    next-tools:
      on-success:
        - maya_geometry__bevel_edges
        - maya_geometry__assign_material
      on-failure:
        - diagnostics__screenshot
"#,
        )
        .unwrap();
        let body = r#"---
name: nt
metadata:
  dcc-mcp.dcc: maya
  dcc-mcp.tools: tools.yaml
---
"#;
        std::fs::write(dir.join(SKILL_METADATA_FILE), body).unwrap();
        let meta = parse_skill_md(&dir).expect("parsed");
        assert!(meta.is_spec_compliant());
        assert_eq!(meta.tools.len(), 1);
        let nt = &meta.tools[0].next_tools;
        assert_eq!(
            nt.on_success,
            vec![
                "maya_geometry__bevel_edges".to_string(),
                "maya_geometry__assign_material".to_string(),
            ]
        );
        assert_eq!(nt.on_failure, vec!["diagnostics__screenshot".to_string()]);
    }

    #[test]
    fn top_level_next_tools_is_legacy_and_non_compliant() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("legacy_nt");
        let body = r#"---
name: legacy_nt
dcc: maya
next-tools:
  on-success: [foo]
---
"#;
        write_skill(&dir, body);
        let meta = parse_skill_md(&dir).expect("parsed");
        assert!(
            !meta.is_spec_compliant(),
            "top-level next-tools must be flagged as legacy",
        );
        assert!(
            meta.legacy_extension_fields
                .iter()
                .any(|s| s == "next-tools"),
            "legacy_extension_fields must name next-tools; got {:?}",
            meta.legacy_extension_fields,
        );
    }
}
