mod support;

use serde_json::{Value, json};
use tempfile::TempDir;

use support::*;

#[test]
fn marketplace_add_list_search_and_inspect_local_source() {
    let tmp = TempDir::new().unwrap();
    let catalog_path = tmp.path().join("marketplace.json");
    std::fs::write(
        &catalog_path,
        r#"
{
  "version": "1",
  "entries": [{
    "name": "dcc-asset-hunyuan-download",
    "description": "Search and download Hunyuan 3D models via official API",
    "dcc": ["maya", "blender"],
    "tags": ["asset", "hunyuan", "download", "domain"],
    "version": "0.1.0",
    "min_core_version": "0.17.0",
    "maintainer": "dcc-mcp",
    "install": {
      "type": "git",
      "url": "https://github.com/dcc-mcp/dcc-asset-hunyuan-download",
      "ref": "0123456789abcdef0123456789abcdef01234567"
    }
  }, {
    "name": "dcc-asset-polyhaven",
    "description": "Search and download Poly Haven CC0 assets",
    "dcc": ["blender"],
    "tags": ["asset", "polyhaven", "download"],
    "version": "0.1.0",
    "install": {
      "type": "git",
      "url": "https://github.com/dcc-mcp/dcc-asset-polyhaven",
      "ref": "fedcba9876543210fedcba9876543210fedcba98"
    }
  }]
}
"#,
    )
    .unwrap();

    let source = catalog_path.to_string_lossy().to_string();
    let config_path = tmp
        .path()
        .join("sources.json")
        .to_string_lossy()
        .to_string();
    let envs = [
        ("DCC_MCP_MARKETPLACE_SOURCES_FILE", config_path.as_str()),
        ("DCC_MCP_MARKETPLACE_NO_DEFAULT_SOURCES", "1"),
    ];

    let sources = run_json_with_env(&["marketplace", "add", &source], &envs);
    assert_eq!(sources.as_array().unwrap().len(), 1);
    assert_eq!(sources[0]["url"], source);

    let listed = run_json_with_env(&["marketplace", "list"], &envs);
    assert_eq!(listed.as_array().unwrap().len(), 1);
    assert_eq!(listed[0]["origin"], "config");

    let search = run_json_with_env(
        &[
            "marketplace",
            "search",
            "--query",
            "download",
            "--dcc",
            "maya",
        ],
        &envs,
    );
    assert_eq!(search["count"], 1);
    assert_eq!(
        search["hits"][0]["entry"]["name"],
        "dcc-asset-hunyuan-download"
    );
    assert_eq!(search["hits"][0]["entry"]["install"]["type"], "git");

    let inspect = run_json_with_env(
        &[
            "marketplace",
            "inspect",
            "dcc-asset-hunyuan-download",
            "--source",
            &source,
        ],
        &envs,
    );
    assert_eq!(inspect["count"], 1);
    assert_eq!(
        inspect["matches"][0]["entry"]["install"]["ref"],
        "0123456789abcdef0123456789abcdef01234567"
    );
}

#[test]
fn marketplace_search_accepts_natural_language_and_ranks_the_best_skill() {
    let tmp = TempDir::new().unwrap();
    let catalog_path = tmp.path().join("marketplace.json");
    std::fs::write(
        &catalog_path,
        json!({
            "version": "1",
            "entries": [
                {
                    "name": "maya-asset-browser",
                    "description": "Browse reusable assets in Maya",
                    "dcc": ["maya"],
                    "tags": ["assets", "skills"]
                },
                {
                    "name": "maya-rigging-tools",
                    "description": "Rigging workflows for Maya character animation",
                    "dcc": ["maya"],
                    "tags": ["rigging", "skills"],
                    "category": "character"
                }
            ]
        })
        .to_string(),
    )
    .unwrap();

    let source = catalog_path.to_string_lossy().to_string();
    let search = run_json(&[
        "marketplace",
        "search",
        "I",
        "need",
        "a",
        "Maya",
        "rigging",
        "skill",
        "--source",
        &source,
    ]);

    assert_eq!(search["query"], "I need a Maya rigging skill");
    assert_eq!(search["hits"][0]["entry"]["name"], "maya-rigging-tools");
}

#[test]
fn marketplace_limit_is_applied_after_cross_source_ranking() {
    let tmp = TempDir::new().unwrap();
    let generic_catalog = tmp.path().join("generic.json");
    let rigging_catalog = tmp.path().join("rigging.json");
    std::fs::write(
        &generic_catalog,
        json!({
            "version": "1",
            "entries": [{
                "name": "maya-skill-collection",
                "description": "General Skills for Maya",
                "dcc": ["maya"],
                "tags": ["skills"]
            }]
        })
        .to_string(),
    )
    .unwrap();
    std::fs::write(
        &rigging_catalog,
        json!({
            "version": "1",
            "entries": [{
                "name": "maya-rigging-tools",
                "description": "Character rigging workflows for Maya",
                "dcc": ["maya"],
                "tags": ["rigging", "skills"]
            }]
        })
        .to_string(),
    )
    .unwrap();

    let config_path = tmp.path().join("sources.json");
    std::fs::write(
        &config_path,
        json!({"sources": [
            {"name": "generic", "url": generic_catalog.to_string_lossy()},
            {"name": "rigging", "url": rigging_catalog.to_string_lossy()}
        ]})
        .to_string(),
    )
    .unwrap();
    let config = config_path.to_string_lossy().to_string();
    let envs = [
        ("DCC_MCP_MARKETPLACE_SOURCES_FILE", config.as_str()),
        ("DCC_MCP_MARKETPLACE_NO_DEFAULT_SOURCES", "1"),
    ];

    let search = run_json_with_env(
        &[
            "marketplace",
            "search",
            "I",
            "need",
            "a",
            "Maya",
            "rigging",
            "skill",
            "--limit",
            "1",
        ],
        &envs,
    );

    assert_eq!(search["count"], 1);
    assert_eq!(search["hits"][0]["entry"]["name"], "maya-rigging-tools");
}

#[test]
fn marketplace_v1_catalog_preserves_curation_and_runtime_metadata() {
    let tmp = TempDir::new().unwrap();
    let catalog_path = tmp.path().join("marketplace.json");
    std::fs::write(
        &catalog_path,
        r#"
{
  "name": "dcc-mcp-official",
  "schemaVersion": "1",
  "version": "1.0.0",
  "skills": [{
    "name": "maya-rig-tools",
    "description": "Rigging helpers for Maya",
    "dcc": ["maya"],
    "tags": ["rigging", "domain"],
    "version": "1.2.3",
    "minCoreVersion": "0.19.0",
    "category": "Skills",
    "maintainer": "dcc-mcp",
    "source": {
      "type": "git",
      "url": "https://github.com/dcc-mcp/maya-rig-tools",
      "ref": "0123456789012345678901234567890123456789"
    },
    "policy": { "installation": "available" },
    "requires": { "env": ["RIG_TOKEN"], "bins": ["rigctl"] }
  }]
}
"#,
    )
    .unwrap();

    let source = catalog_path.to_string_lossy().to_string();
    let inspect = run_json(&[
        "marketplace",
        "inspect",
        "maya-rig-tools",
        "--source",
        &source,
    ]);
    let entry = &inspect["matches"][0]["entry"];
    assert_eq!(entry["min_core_version"], "0.19.0");
    assert_eq!(entry["category"], "Skills");
    assert_eq!(entry["policy"]["installation"], "available");
    assert_eq!(entry["requires"]["env"], json!(["RIG_TOKEN"]));
    assert_eq!(
        entry["install"]["ref"],
        "0123456789012345678901234567890123456789"
    );
}

#[test]
fn marketplace_pack_and_publish_updates_catalog() {
    let tmp = TempDir::new().unwrap();
    let skill_dir = write_skill(
        tmp.path(),
        "release-skill",
        "---\nname: release-skill\ndescription: Release package\nmetadata:\n  dcc-mcp:\n    dcc: maya\n    version: 0.2.0\n    tags: modeling, publish\n---\n",
    );
    std::fs::write(skill_dir.join("tools.yaml"), "tools: []\n").unwrap();
    let out_dir = tmp.path().join("dist");
    let skill_dir_s = skill_dir.to_string_lossy().to_string();
    let out_dir_s = out_dir.to_string_lossy().to_string();

    let packed = run_json(&["marketplace", "pack", &skill_dir_s, "--out", &out_dir_s]);
    let package_path = std::path::PathBuf::from(packed["package_path"].as_str().unwrap());
    assert!(package_path.is_file());
    assert_eq!(packed["sha256"].as_str().unwrap().len(), 64);

    let catalog_path = tmp.path().join("marketplace.json");
    let catalog_s = catalog_path.to_string_lossy().to_string();
    let sha256 = format!("sha256:{}", packed["sha256"].as_str().unwrap());
    let published = run_json(&[
        "marketplace",
        "publish",
        &skill_dir_s,
        "--catalog",
        &catalog_s,
        "--install-url",
        "https://github.com/dcc-mcp/release-skill/releases/download/v0.2.0/release-skill.zip",
        "--sha256",
        &sha256,
        "--maintainer",
        "dcc-mcp",
        "--min-core-version",
        "0.19.0",
        "--skill-root",
        "skill/release-skill",
        "--tag",
        "extra",
        "--showcase",
        "docs/images/showcase.webp",
        "--requires-env",
        "RELEASE_TOKEN",
        "--requires-bin",
        "release-cli",
        "--requires-python",
        "release_skill",
        "--requires-skill",
        "dcc-base",
    ]);
    assert_eq!(published["action"], "created");
    assert_eq!(published["entry"]["name"], "release-skill");
    assert_eq!(published["entry"]["dcc"], json!(["maya"]));
    assert_eq!(published["entry"]["version"], "0.2.0");
    assert_eq!(published["entry"]["install"]["type"], "zip");
    assert_eq!(published["entry"]["install"]["sha256"], sha256);
    assert_eq!(published["entry"]["showcase"], "docs/images/showcase.webp");
    assert_eq!(
        published["entry"]["requires"]["env"],
        json!(["RELEASE_TOKEN"])
    );
    let catalog =
        serde_json::from_str::<Value>(&std::fs::read_to_string(&catalog_path).unwrap()).unwrap();
    assert_eq!(catalog["schemaVersion"], "2");
    assert_eq!(catalog["skills"][0]["minCoreVersion"], "0.19.0");
    assert_eq!(catalog["skills"][0]["source"]["type"], "zip");
    assert_eq!(
        catalog["skills"][0]["source"]["skillRoots"],
        json!(["skill/release-skill"])
    );
    assert_eq!(
        catalog["skills"][0]["showcase"],
        "docs/images/showcase.webp"
    );
    assert_eq!(
        catalog["skills"][0]["requires"]["bins"],
        json!(["release-cli"])
    );

    let updated = run_json(&[
        "marketplace",
        "publish",
        &skill_dir_s,
        "--catalog",
        &catalog_s,
        "--install-url",
        "https://github.com/dcc-mcp/release-skill/releases/download/v0.3.0/release-skill.zip",
        "--sha256",
        &sha256,
        "--version",
        "0.3.0",
        "--requires-env",
        "UPDATED_RELEASE_TOKEN",
    ]);
    assert_eq!(updated["action"], "updated");
    assert_eq!(updated["count"], 1);
    assert_eq!(updated["entry"]["version"], "0.3.0");
    let catalog =
        serde_json::from_str::<Value>(&std::fs::read_to_string(&catalog_path).unwrap()).unwrap();
    assert_eq!(
        catalog["skills"][0]["showcase"],
        "docs/images/showcase.webp"
    );
    assert_eq!(
        catalog["skills"][0]["requires"]["env"],
        json!(["UPDATED_RELEASE_TOKEN"])
    );
    assert_eq!(
        catalog["skills"][0]["requires"]["python"],
        json!(["release_skill"])
    );
    assert_eq!(
        catalog["skills"][0]["requires"]["skills"],
        json!(["dcc-base"])
    );

    let updated_without_metadata = run_json(&[
        "marketplace",
        "publish",
        &skill_dir_s,
        "--catalog",
        &catalog_s,
        "--install-url",
        "https://github.com/dcc-mcp/release-skill/releases/download/v0.4.0/release-skill.zip",
        "--sha256",
        &sha256,
        "--version",
        "0.4.0",
    ]);
    assert_eq!(updated_without_metadata["action"], "updated");
    let catalog =
        serde_json::from_str::<Value>(&std::fs::read_to_string(&catalog_path).unwrap()).unwrap();
    assert_eq!(
        catalog["skills"][0]["requires"]["env"],
        json!(["UPDATED_RELEASE_TOKEN"])
    );
    assert_eq!(
        catalog["skills"][0]["showcase"],
        "docs/images/showcase.webp"
    );
}

#[test]
fn marketplace_schema_validation_rejects_empty_name() {
    let tmp = TempDir::new().unwrap();
    let catalog_path = tmp.path().join("marketplace.json");
    // Entry with empty name — passes serde but fails schema (minLength: 1).
    std::fs::write(
        &catalog_path,
        r#"{
  "version": "1",
  "entries": [{
    "name": "",
    "description": "Has empty name",
    "dcc": ["maya"]
  }]
}"#,
    )
    .unwrap();

    let source = catalog_path.to_string_lossy().to_string();
    let config_path = tmp
        .path()
        .join("sources.json")
        .to_string_lossy()
        .to_string();
    let envs = [
        ("DCC_MCP_MARKETPLACE_SOURCES_FILE", config_path.as_str()),
        ("DCC_MCP_MARKETPLACE_NO_DEFAULT_SOURCES", "1"),
    ];

    // Without --skip-validation, search should fail with a validation error.
    let stderr = run_failure_with_env(&["marketplace", "search", "--source", &source], &envs);
    assert!(
        stderr.contains("validation"),
        "expected validation error, got: {stderr}"
    );
}

#[test]
fn marketplace_skip_validation_flag_filters_invalid_entries() {
    let tmp = TempDir::new().unwrap();
    let catalog_path = tmp.path().join("marketplace.json");
    // One valid entry, one with empty name (schema-invalid).
    std::fs::write(
        &catalog_path,
        r#"{
  "version": "1",
  "entries": [
    {
      "name": "valid-skill",
      "description": "A valid skill",
      "dcc": ["maya"]
    },
    {
      "name": "",
      "description": "Empty name entry",
      "dcc": ["blender"]
    }
  ]
}"#,
    )
    .unwrap();

    let source = catalog_path.to_string_lossy().to_string();
    let config_path = tmp
        .path()
        .join("sources.json")
        .to_string_lossy()
        .to_string();
    let envs = [
        ("DCC_MCP_MARKETPLACE_SOURCES_FILE", config_path.as_str()),
        ("DCC_MCP_MARKETPLACE_NO_DEFAULT_SOURCES", "1"),
    ];

    // With --skip-validation, the invalid entry should be silently dropped.
    let search = run_json_with_env(
        &[
            "marketplace",
            "search",
            "--source",
            &source,
            "--skip-validation",
        ],
        &envs,
    );
    assert_eq!(search["count"], 1);
    assert_eq!(search["hits"][0]["entry"]["name"], "valid-skill");
}

#[test]
fn marketplace_merge_priority_explicit_overrides_config() {
    let tmp = TempDir::new().unwrap();

    // Config source (lower priority) — old version
    let config_catalog = tmp.path().join("config-marketplace.json");
    std::fs::write(
        &config_catalog,
        json!({
            "version": "1",
            "entries": [{
                "name": "shared-skill",
                "description": "From config source — old version",
                "dcc": ["maya"],
                "version": "0.1.0"
            }]
        })
        .to_string(),
    )
    .unwrap();

    // Explicit source (higher priority) — newer version
    let explicit_catalog = tmp.path().join("explicit-marketplace.json");
    std::fs::write(
        &explicit_catalog,
        json!({
            "version": "1",
            "entries": [{
                "name": "shared-skill",
                "description": "From explicit source — new version",
                "dcc": ["maya"],
                "version": "0.3.0"
            }]
        })
        .to_string(),
    )
    .unwrap();

    let config_path = tmp
        .path()
        .join("sources.json")
        .to_string_lossy()
        .to_string();
    // Pre-configure the config source
    let config_source_url = config_catalog.to_string_lossy().to_string();
    std::fs::write(
        &config_path,
        json!({"sources": [{"name": "config-catalog", "url": config_source_url}]}).to_string(),
    )
    .unwrap();

    let explicit_source = explicit_catalog.to_string_lossy().to_string();
    let envs = [
        ("DCC_MCP_MARKETPLACE_SOURCES_FILE", config_path.as_str()),
        ("DCC_MCP_MARKETPLACE_NO_DEFAULT_SOURCES", "1"),
    ];

    // Search with explicit source — explicit's entry should win.
    let search = run_json_with_env(
        &[
            "marketplace",
            "search",
            "--source",
            &explicit_source,
            "--query",
            "shared-skill",
        ],
        &envs,
    );
    assert_eq!(search["count"], 1);
    assert_eq!(search["hits"][0]["entry"]["version"], "0.3.0");
    assert_eq!(
        search["hits"][0]["entry"]["description"],
        "From explicit source — new version"
    );
    assert_eq!(search["hits"][0]["source"]["origin"], "explicit");
}

#[test]
fn marketplace_search_dedupes_same_entry_from_multiple_sources() {
    let tmp = TempDir::new().unwrap();

    // Two config sources with overlapping entry names
    let catalog1 = tmp.path().join("catalog1.json");
    std::fs::write(
        &catalog1,
        json!({
            "version": "1",
            "entries": [
                {"name": "skill-a", "description": "From catalog 1", "dcc": ["maya"]},
                {"name": "skill-b", "description": "Shared skill from catalog 1", "dcc": ["blender"]}
            ]
        })
        .to_string(),
    )
    .unwrap();

    let catalog2 = tmp.path().join("catalog2.json");
    std::fs::write(
        &catalog2,
        json!({
            "version": "1",
            "entries": [
                {"name": "skill-b", "description": "Shared skill from catalog 2", "dcc": ["blender"]},
                {"name": "skill-c", "description": "From catalog 2", "dcc": ["houdini"]}
            ]
        })
        .to_string(),
    )
    .unwrap();

    let source1 = catalog1.to_string_lossy().to_string();
    let source2 = catalog2.to_string_lossy().to_string();
    let config_path = tmp
        .path()
        .join("sources.json")
        .to_string_lossy()
        .to_string();
    // Register both sources. Source order defines which duplicate wins.
    std::fs::write(
        &config_path,
        json!({"sources": [
            {"name": "catalog1", "url": source1},
            {"name": "catalog2", "url": source2}
        ]})
        .to_string(),
    )
    .unwrap();

    let envs = [
        ("DCC_MCP_MARKETPLACE_SOURCES_FILE", config_path.as_str()),
        ("DCC_MCP_MARKETPLACE_NO_DEFAULT_SOURCES", "1"),
    ];

    let search = run_json_with_env(&["marketplace", "search"], &envs);
    assert_eq!(search["count"], 3);
    let skill_b = search["hits"]
        .as_array()
        .unwrap()
        .iter()
        .find(|h| h["entry"]["name"] == "skill-b")
        .unwrap();
    assert_eq!(
        skill_b["entry"]["description"],
        "Shared skill from catalog 1"
    );
    assert_eq!(skill_b["source"]["origin"], "config");
    assert!(
        search["hits"]
            .as_array()
            .unwrap()
            .iter()
            .any(|h| h["entry"]["name"] == "skill-a")
    );
}

#[test]
fn marketplace_explicit_source_is_exclusive_regression() {
    let tmp = TempDir::new().unwrap();

    // Configured source with one entry
    let config_catalog = tmp.path().join("config-catalog.json");
    std::fs::write(
        &config_catalog,
        json!({
            "version": "1",
            "entries": [
                {"name": "config-only", "description": "Only in configured source", "dcc": ["maya"]}
            ]
        })
        .to_string(),
    )
    .unwrap();

    // Explicit source with a different entry
    let explicit_catalog = tmp.path().join("explicit-catalog.json");
    std::fs::write(
        &explicit_catalog,
        json!({
            "version": "1",
            "entries": [
                {"name": "explicit-only", "description": "Only in explicit source", "dcc": ["blender"]}
            ]
        })
        .to_string(),
    )
    .unwrap();

    let config_path = tmp
        .path()
        .join("sources.json")
        .to_string_lossy()
        .to_string();
    std::fs::write(
        &config_path,
        json!({"sources": [{"name": "config", "url": config_catalog.to_string_lossy()}]})
            .to_string(),
    )
    .unwrap();

    let envs = [
        ("DCC_MCP_MARKETPLACE_SOURCES_FILE", config_path.as_str()),
        ("DCC_MCP_MARKETPLACE_NO_DEFAULT_SOURCES", "1"),
    ];

    // Search with explicit source — config-only must NOT appear.
    let search = run_json_with_env(
        &[
            "marketplace",
            "search",
            "--source",
            explicit_catalog.to_string_lossy().as_ref(),
            "--query",
            "config",
        ],
        &envs,
    );
    let hit_names: Vec<&str> = search["hits"]
        .as_array()
        .unwrap()
        .iter()
        .map(|h| h["entry"]["name"].as_str().unwrap())
        .collect();
    assert!(
        !hit_names.contains(&"config-only"),
        "configured-source entries must not appear when explicit --source is given; got {hit_names:?}"
    );
}

#[test]
fn marketplace_entry_with_icon_validates() {
    let tmp = TempDir::new().unwrap();
    let catalog_path = tmp.path().join("catalog-with-icon.json");
    // Entry with an icon field — must pass schema validation.
    std::fs::write(
        &catalog_path,
        json!({
            "version": "1",
            "entries": [{
                "name": "skill-with-icon",
                "description": "A skill that ships an icon",
                "dcc": ["maya"],
                "icon": "icon.png"
            }]
        })
        .to_string(),
    )
    .unwrap();

    let source = catalog_path.to_string_lossy().to_string();
    let config_path = tmp
        .path()
        .join("sources.json")
        .to_string_lossy()
        .to_string();
    let envs = [
        ("DCC_MCP_MARKETPLACE_SOURCES_FILE", config_path.as_str()),
        ("DCC_MCP_MARKETPLACE_NO_DEFAULT_SOURCES", "1"),
    ];

    // Without --skip-validation, search should succeed because icon is a valid
    // property in the schema.
    let search = run_json_with_env(&["marketplace", "search", "--source", &source], &envs);
    assert_eq!(search["count"], 1);
    assert_eq!(search["hits"][0]["entry"]["name"], "skill-with-icon");
}
