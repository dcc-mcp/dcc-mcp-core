#[allow(unused_imports)]
use super::*;

// ── Deserialization / defaults ──────────────────────────────────────────────

#[test]
fn test_skill_metadata_deserialize() {
    let json = r#"{
            "name": "test-skill",
            "description": "A test skill",
            "dcc": "maya",
            "tags": ["geometry", "creation"]
        }"#;
    let meta: SkillMetadata = serde_json::from_str(json).unwrap();
    assert_eq!(meta.name, "test-skill");
    assert_eq!(meta.dcc, "maya");
    assert_eq!(meta.tags, vec!["geometry", "creation"]);
    assert_eq!(meta.version, DEFAULT_VERSION);
    assert!(meta.depends.is_empty());
    assert!(meta.metadata_files.is_empty());
    assert!(meta.license.is_empty());
    assert!(meta.compatibility.is_empty());
    assert!(meta.allowed_tools.is_empty());
    assert!(meta.metadata.is_null());
}

#[test]
fn test_required_capabilities_aggregation() {
    // Issue #354 — per-tool required_capabilities aggregate to a
    // deduplicated, sorted union on the skill.
    let mut md = SkillMetadata {
        name: "usd-tools".into(),
        description: "USD".into(),
        ..Default::default()
    };
    md.tools = vec![
        ToolDeclaration {
            name: "import_usd".into(),
            required_capabilities: vec![
                "usd".into(),
                "scene.mutate".into(),
                "filesystem.read".into(),
            ],
            ..Default::default()
        },
        ToolDeclaration {
            name: "read_stage".into(),
            required_capabilities: vec!["usd".into(), "scene.read".into()],
            ..Default::default()
        },
        ToolDeclaration {
            name: "no_caps".into(),
            required_capabilities: vec![],
            ..Default::default()
        },
    ];
    assert_eq!(
        md.required_capabilities(),
        vec![
            "filesystem.read".to_string(),
            "scene.mutate".into(),
            "scene.read".into(),
            "usd".into(),
        ],
    );
}

#[test]
fn test_tool_declaration_parses_required_capabilities() {
    let json = r#"{
            "name": "import_usd",
            "description": "Import a USD file",
            "required_capabilities": ["usd", "scene.mutate", "filesystem.read"]
        }"#;
    let decl: ToolDeclaration = serde_json::from_str(json).unwrap();
    assert_eq!(
        decl.required_capabilities,
        vec!["usd", "scene.mutate", "filesystem.read"]
    );
}

#[test]
fn test_skill_and_tool_search_aliases_parse() {
    let json = r#"{
            "name": "export-skill",
            "search-aliases": ["write file", "interchange"],
            "tools": [{
                "name": "export_fbx",
                "search_aliases": ["destination path", "fbx"]
            }]
        }"#;
    let meta: SkillMetadata = serde_json::from_str(json).unwrap();
    assert_eq!(
        meta.search_aliases,
        vec!["write file".to_string(), "interchange".to_string()]
    );
    assert_eq!(
        meta.tools[0].search_aliases,
        vec!["destination path".to_string(), "fbx".to_string()]
    );
}

#[test]
fn test_tool_declaration_parses_thread_affinity_enforcement() {
    let json = r#"{
            "name": "bake_simulation",
            "affinity": "main",
            "enforce_thread_affinity": true
        }"#;
    let decl: ToolDeclaration = serde_json::from_str(json).unwrap();
    assert_eq!(decl.thread_affinity, ThreadAffinity::Main);
    assert!(decl.enforce_thread_affinity);
}

#[test]
fn test_tool_declaration_defaults_enforcement_when_affinity_is_declared() {
    let json = r#"{
            "name": "bake_simulation",
            "affinity": "main"
        }"#;
    let decl: ToolDeclaration = serde_json::from_str(json).unwrap();
    assert_eq!(decl.thread_affinity, ThreadAffinity::Main);
    assert!(decl.enforce_thread_affinity);
}

#[test]
fn test_tool_declaration_defaults_enforcement_when_any_affinity_is_declared() {
    let json = r#"{
            "name": "read_manifest",
            "thread_affinity": "any"
        }"#;
    let decl: ToolDeclaration = serde_json::from_str(json).unwrap();
    assert_eq!(decl.thread_affinity, ThreadAffinity::Any);
    assert!(decl.enforce_thread_affinity);
}

#[test]
fn test_tool_declaration_keeps_legacy_enforcement_default_without_affinity() {
    let json = r#"{
            "name": "legacy_tool"
        }"#;
    let decl: ToolDeclaration = serde_json::from_str(json).unwrap();
    assert_eq!(decl.thread_affinity, ThreadAffinity::Any);
    assert!(!decl.enforce_thread_affinity);
}

#[test]
fn test_tool_declaration_allows_explicit_affinity_enforcement_opt_out() {
    let json = r#"{
            "name": "standalone_compat",
            "affinity": "main",
            "enforce_thread_affinity": false
        }"#;
    let decl: ToolDeclaration = serde_json::from_str(json).unwrap();
    assert_eq!(decl.thread_affinity, ThreadAffinity::Main);
    assert!(!decl.enforce_thread_affinity);
}

#[test]
fn test_agentskills_standard_fields() {
    let json = r#"{
            "name": "pdf-tools",
            "description": "Extract text from PDFs. Use when working with PDF files.",
            "license": "MIT",
            "compatibility": "Requires Python 3.9+",
            "allowed-tools": "Bash Read Write",
            "metadata": {"author": "studio", "category": "documents"}
        }"#;
    let meta: SkillMetadata = serde_json::from_str(json).unwrap();
    assert_eq!(meta.license, "MIT");
    assert_eq!(meta.compatibility, "Requires Python 3.9+");
    assert_eq!(meta.allowed_tools, vec!["Bash", "Read", "Write"]);
    let flat = meta.flat_metadata();
    assert_eq!(flat.get("author"), Some(&"studio"));
    assert_eq!(flat.get("category"), Some(&"documents"));
}

#[test]
fn test_allowed_tools_yaml_list() {
    let json = r#"{
            "name": "test",
            "allowed-tools": ["Bash", "Read", "Edit"]
        }"#;
    let meta: SkillMetadata = serde_json::from_str(json).unwrap();
    assert_eq!(meta.allowed_tools, vec!["Bash", "Read", "Edit"]);
}

#[test]
fn test_allowed_tools_alias() {
    let json = r#"{"name": "test", "allowed_tools": ["Bash"]}"#;
    let meta: SkillMetadata = serde_json::from_str(json).unwrap();
    assert_eq!(meta.allowed_tools, vec!["Bash"]);
}

#[test]
fn test_clawhub_metadata_openclaw() {
    let yaml_json = r#"{
            "name": "ffmpeg-media",
            "description": "Media conversion via FFmpeg",
            "version": "1.0.0",
            "metadata": {
                "openclaw": {
                    "requires": {
                        "bins": ["ffmpeg", "ffprobe"],
                        "env": ["FFMPEG_PATH"]
                    },
                    "primaryEnv": "FFMPEG_PATH",
                    "emoji": "🎬",
                    "homepage": "https://ffmpeg.org",
                    "os": ["linux", "macos"],
                    "always": false
                }
            }
        }"#;
    let meta: SkillMetadata = serde_json::from_str(yaml_json).unwrap();
    assert_eq!(meta.required_bins(), vec!["ffmpeg", "ffprobe"]);
    assert_eq!(meta.required_env_vars(), vec!["FFMPEG_PATH"]);
    assert_eq!(meta.primary_env(), Some("FFMPEG_PATH"));
    assert_eq!(meta.emoji(), Some("🎬"));
    assert_eq!(meta.homepage(), Some("https://ffmpeg.org"));
    assert_eq!(meta.os_restrictions(), vec!["linux", "macos"]);
    assert!(!meta.always_active());
}

#[test]
fn test_clawhub_metadata_alias_clawdbot() {
    let json = r#"{
            "name": "test",
            "metadata": {
                "clawdbot": {
                    "emoji": "🦀"
                }
            }
        }"#;
    let meta: SkillMetadata = serde_json::from_str(json).unwrap();
    assert_eq!(meta.emoji(), Some("🦀"));
}

#[test]
fn test_all_three_standards_combined() {
    let json = r#"{
            "name": "maya-bevel",
            "description": "Bevel tools for Maya. Use when beveling polygon edges.",
            "license": "MIT",
            "compatibility": "Maya 2022+, Python 3.7+",
            "allowed-tools": "Bash Read",
            "metadata": {
                "author": "studio",
                "openclaw": {
                    "requires": {"bins": ["maya"]},
                    "emoji": "🎨"
                }
            },
            "dcc": "maya",
            "version": "2.0.0",
            "tags": ["modeling", "polygon"],
            "tools": [
                {"name": "bevel", "description": "Apply bevel to edges"}
            ]
        }"#;
    let meta: SkillMetadata = serde_json::from_str(json).unwrap();
    // agentskills.io fields
    assert_eq!(meta.license, "MIT");
    assert_eq!(meta.allowed_tools, vec!["Bash", "Read"]);
    // ClawHub fields
    assert_eq!(meta.required_bins(), vec!["maya"]);
    assert_eq!(meta.emoji(), Some("🎨"));
    // flat metadata
    assert_eq!(meta.flat_metadata().get("author"), Some(&"studio"));
    // dcc-mcp-core extensions
    assert_eq!(meta.dcc, "maya");
    assert_eq!(meta.tools[0].name, "bevel");
}

#[test]
fn test_validate_name_constraints() {
    let valid = SkillMetadata {
        name: "my-skill-v2".to_string(),
        ..Default::default()
    };
    assert!(valid.validate().is_empty());

    let too_long = SkillMetadata {
        name: "a".repeat(65),
        ..Default::default()
    };
    assert!(!too_long.validate().is_empty());

    let starts_hyphen = SkillMetadata {
        name: "-bad".to_string(),
        ..Default::default()
    };
    assert!(!starts_hyphen.validate().is_empty());

    let uppercase = SkillMetadata {
        name: "MySkill".to_string(),
        ..Default::default()
    };
    assert!(!uppercase.validate().is_empty());
}

#[test]
fn test_skill_metadata_with_depends() {
    let json = r#"{
            "name": "pipeline",
            "depends": ["geometry-tools", "usd-tools"]
        }"#;
    let meta: SkillMetadata = serde_json::from_str(json).unwrap();
    assert_eq!(meta.depends, vec!["geometry-tools", "usd-tools"]);
}

#[test]
fn test_skill_runtime_descriptors_resolve_safe_states() {
    let json = r#"{
            "name": "openusd-tools",
            "runtimes": [
                {
                    "name": "usd-core",
                    "type": "python_package",
                    "package": "usd-core",
                    "module": "dcc_mcp_runtime_probe_missing_pxr_1210",
                    "optional": true,
                    "feature_level": "full-usd",
                    "install_hint": "pip install dcc-mcp-openusd[usd-core]"
                },
                {
                    "name": "required-license",
                    "type": "env_var",
                    "env": "DCC_MCP_TEST_RUNTIME_DOES_NOT_EXIST",
                    "guidance": "Set DCC_MCP_TEST_RUNTIME_DOES_NOT_EXIST."
                }
            ]
        }"#;
    let meta: SkillMetadata = serde_json::from_str(json).unwrap();
    assert_eq!(meta.runtimes.len(), 2);

    let reports = resolve_runtime_reports(&meta.runtimes);
    assert_eq!(reports[0].name, "usd-core");
    assert_eq!(reports[0].state, SkillRuntimeState::Degraded);
    assert_eq!(reports[1].state, SkillRuntimeState::Missing);

    let summary = summarize_runtime_reports(&reports);
    assert_eq!(summary.total, 2);
    assert_eq!(summary.degraded, 1);
    assert_eq!(summary.missing, 1);
    assert_eq!(summary.state, SkillRuntimeState::Missing);
}

#[test]
fn test_skill_metadata_display() {
    let meta = SkillMetadata {
        name: "my-skill".to_string(),
        version: "2.0.0".to_string(),
        dcc: "maya".to_string(),
        ..Default::default()
    };
    assert_eq!(meta.to_string(), "my-skill v2.0.0 (maya)");
}

#[test]
fn test_skill_metadata_default_values() {
    let meta = SkillMetadata {
        name: "minimal".to_string(),
        ..Default::default()
    };
    assert_eq!(meta.name, "minimal");
    assert!(meta.tools.is_empty());
    assert!(meta.scripts.is_empty());
    assert!(meta.tags.is_empty());
    assert!(meta.license.is_empty());
    assert!(meta.allowed_tools.is_empty());
}

#[test]
fn test_skill_metadata_serde_round_trip() {
    let meta = SkillMetadata {
        name: "full-skill".to_string(),
        description: "A full skill".to_string(),
        license: "MIT".to_string(),
        compatibility: "Python 3.7+".to_string(),
        allowed_tools: vec!["Bash".to_string(), "Read".to_string()],
        metadata: serde_json::json!({"author": "test"}),
        tools: vec![
            ToolDeclaration {
                name: "create_mesh".to_string(),
                ..Default::default()
            },
            ToolDeclaration {
                name: "delete_mesh".to_string(),
                ..Default::default()
            },
        ],
        dcc: "blender".to_string(),
        tags: vec!["modeling".to_string()],
        search_hint: "mesh, modeling, geometry".to_string(),
        search_aliases: vec!["mesh authoring".to_string()],
        scripts: vec!["init.py".to_string()],
        skill_path: "/skills/full".to_string(),
        version: "1.2.3".to_string(),
        depends: vec!["base-skill".to_string()],
        metadata_files: vec!["help.md".to_string()],
        policy: None,
        external_deps: None,
        runtimes: Vec::new(),
        groups: Vec::new(),
        prompts_file: None,
        layer: Some("domain".to_string()),
        stage: Some("authoring".to_string()),
        recipes_file: None,
        introspection_file: None,
        intent: None,
        recall_context: None,
        preconditions: Vec::new(),
        side_effects: None,
        produces: Vec::new(),
        requires: Vec::new(),
        success_metrics: None,
        branding: None,
        links: None,
        example_prompts: Vec::new(),
    };
    let json = serde_json::to_string(&meta).unwrap();
    let back: SkillMetadata = serde_json::from_str(&json).unwrap();
    assert_eq!(meta, back);
}

#[test]
fn test_skill_metadata_tools_list() {
    let json = r#"{"name": "tools-skill", "tools": ["mesh_bevel", "mesh_extrude", "mesh_inset"]}"#;
    let meta: SkillMetadata = serde_json::from_str(json).unwrap();
    assert_eq!(meta.tools.len(), 3);
    assert_eq!(meta.tools[0].name, "mesh_bevel");
    assert_eq!(meta.tools[1].name, "mesh_extrude");
    assert_eq!(meta.tools[2].name, "mesh_inset");
}

#[test]
fn test_tool_declaration_full_object() {
    let json = r#"{"name": "tools-skill", "tools": [{"name": "bevel", "description": "Bevel edges", "read_only": false, "destructive": true, "idempotent": true}]}"#;
    let meta: SkillMetadata = serde_json::from_str(json).unwrap();
    assert_eq!(meta.tools.len(), 1);
    assert_eq!(meta.tools[0].name, "bevel");
    assert_eq!(meta.tools[0].description, "Bevel edges");
    assert!(!meta.tools[0].read_only);
    assert!(meta.tools[0].destructive);
    assert!(meta.tools[0].idempotent);
}

#[test]
fn test_skill_metadata_deserialize_all_dccs() {
    for dcc in &["maya", "blender", "houdini", "3dsmax", "unreal", "unity"] {
        let json = format!(r#"{{"name": "test", "dcc": "{dcc}"}}"#);
        let meta: SkillMetadata = serde_json::from_str(&json).unwrap();
        assert_eq!(&meta.dcc, dcc);
    }
}

#[test]
fn test_tool_declaration_next_tools() {
    // Test next-tools deserialization (issue #143)
    let json = r#"{"name": "pipeline-skill", "tools": [{
            "name": "export_fbx",
            "description": "Export to FBX",
            "next-tools": {
                "on-success": ["validate_naming", "inspect_usd"],
                "on-failure": ["dcc_diagnostics__screenshot", "dcc_diagnostics__audit_log"]
            }
        }]}"#;
    let meta: SkillMetadata = serde_json::from_str(json).unwrap();
    assert_eq!(meta.tools.len(), 1);
    assert_eq!(meta.tools[0].name, "export_fbx");
    assert_eq!(
        meta.tools[0].next_tools.on_success,
        vec!["validate_naming", "inspect_usd"]
    );
    assert_eq!(
        meta.tools[0].next_tools.on_failure,
        vec!["dcc_diagnostics__screenshot", "dcc_diagnostics__audit_log"]
    );
}

#[test]
fn test_tool_declaration_next_tools_snake_case_container_key_is_ignored() {
    let json = r#"{"name": "skill", "tools": [{
            "name": "my_tool",
            "next_tools": {
                "on-success": ["tool_a"],
                "on-failure": ["tool_b"]
            }
        }]}"#;
    let meta: SkillMetadata = serde_json::from_str(json).unwrap();
    assert!(meta.tools[0].next_tools.on_success.is_empty());
    assert!(meta.tools[0].next_tools.on_failure.is_empty());
}

// ── ExecutionMode (issue #317) ──────────────────────────────────────

#[test]
fn test_execution_mode_default_is_sync() {
    assert_eq!(ExecutionMode::default(), ExecutionMode::Sync);
    assert!(!ExecutionMode::default().is_deferred());
}

#[test]
fn test_execution_mode_is_deferred() {
    assert!(!ExecutionMode::Sync.is_deferred());
    assert!(ExecutionMode::Async.is_deferred());
}

#[test]
fn test_execution_mode_serde_round_trip() {
    let s = serde_json::to_string(&ExecutionMode::Sync).unwrap();
    assert_eq!(s, "\"sync\"");
    let a = serde_json::to_string(&ExecutionMode::Async).unwrap();
    assert_eq!(a, "\"async\"");
    let back: ExecutionMode = serde_json::from_str("\"async\"").unwrap();
    assert_eq!(back, ExecutionMode::Async);
}

#[test]
fn test_tool_declaration_execution_async() {
    let json = r#"{"name": "s", "tools": [
            {"name": "render", "execution": "async", "timeout_hint_secs": 600}
        ]}"#;
    let meta: SkillMetadata = serde_json::from_str(json).unwrap();
    assert_eq!(meta.tools[0].execution, ExecutionMode::Async);
    assert_eq!(meta.tools[0].timeout_hint_secs, Some(600));
}

#[test]
fn test_tool_declaration_execution_default_sync() {
    // Absence of `execution` → Sync, timeout_hint_secs → None.
    let json = r#"{"name": "s", "tools": [{"name": "quick"}]}"#;
    let meta: SkillMetadata = serde_json::from_str(json).unwrap();
    assert_eq!(meta.tools[0].execution, ExecutionMode::Sync);
    assert_eq!(meta.tools[0].timeout_hint_secs, None);
}

#[test]
fn test_tool_declaration_rejects_deferred_field() {
    let json = r#"{"name": "s", "tools": [{"name": "t", "deferred": true}]}"#;
    let err = serde_json::from_str::<SkillMetadata>(json).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("execution: async") || msg.contains("deferred"),
        "error must point to execution: async — got: {msg}",
    );
}

#[test]
fn test_tool_declaration_rejects_unknown_execution() {
    let json = r#"{"name": "s", "tools": [{"name": "t", "execution": "background"}]}"#;
    let err = serde_json::from_str::<SkillMetadata>(json).unwrap_err();
    assert!(err.to_string().contains("background") || err.to_string().contains("execution"));
}

#[test]
fn test_tool_declaration_next_tools_default_empty() {
    // Without next-tools, defaults are empty
    let json = r#"{"name": "skill", "tools": [{"name": "simple_tool"}]}"#;
    let meta: SkillMetadata = serde_json::from_str(json).unwrap();
    assert!(meta.tools[0].next_tools.on_success.is_empty());
    assert!(meta.tools[0].next_tools.on_failure.is_empty());
}

// ── ToolDeclaration schema keys (strict YAML) ─────────────────────────────

#[test]
fn test_tool_declaration_rejects_input_schema_camel_case_key() {
    let json = r#"{
        "name": "test-skill",
        "tools": [{
            "name": "my_tool",
            "inputSchema": {"type": "object"}
        }]
    }"#;
    let meta: SkillMetadata = serde_json::from_str(json).unwrap();
    assert!(
        meta.tools[0].input_schema.is_null(),
        "inputSchema camelCase key must not populate input_schema; use input_schema",
    );
}

#[test]
fn test_tool_declaration_input_schema_snake_case_still_works() {
    // snake_case input_schema must still work (backward compatibility)
    let json = r#"{
        "name": "test-skill",
        "tools": [{
            "name": "my_tool",
            "input_schema": {"type": "object", "properties": {}}
        }]
    }"#;
    let meta: SkillMetadata = serde_json::from_str(json).unwrap();
    assert_eq!(meta.tools[0].input_schema["type"], "object");
}

#[test]
fn test_tool_declaration_rejects_output_schema_camel_case_key() {
    let json = r#"{
        "name": "test-skill",
        "tools": [{
            "name": "my_tool",
            "outputSchema": {"type": "object"}
        }]
    }"#;
    let meta: SkillMetadata = serde_json::from_str(json).unwrap();
    assert!(
        meta.tools[0].output_schema.is_null(),
        "outputSchema camelCase key must not populate output_schema; use output_schema",
    );
}

#[test]
fn test_tool_declaration_output_schema_snake_case_still_works() {
    // snake_case output_schema must still work (backward compatibility)
    let json = r#"{
        "name": "test-skill",
        "tools": [{
            "name": "my_tool",
            "output_schema": {"type": "object"}
        }]
    }"#;
    let meta: SkillMetadata = serde_json::from_str(json).unwrap();
    assert_eq!(meta.tools[0].output_schema["type"], "object");
}

#[test]
fn test_tool_declaration_schema_null_when_omitted() {
    // When `input_schema` / `output_schema` are omitted, fields default to null
    let json = r#"{"name": "s", "tools": [{"name": "t"}]}"#;
    let meta: SkillMetadata = serde_json::from_str(json).unwrap();
    assert!(meta.tools[0].input_schema.is_null());
    assert!(meta.tools[0].output_schema.is_null());
}
