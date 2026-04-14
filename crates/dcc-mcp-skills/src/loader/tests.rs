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
