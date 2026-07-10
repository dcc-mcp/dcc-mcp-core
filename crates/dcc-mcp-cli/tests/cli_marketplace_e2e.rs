mod support;

use dcc_mcp_skills::parse_skill_md;
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
      "ref": "v0.1.0"
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
      "ref": "v0.1.0"
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
    assert_eq!(inspect["matches"][0]["entry"]["install"]["ref"], "v0.1.0");
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
    ]);
    assert_eq!(published["action"], "created");
    assert_eq!(published["entry"]["name"], "release-skill");
    assert_eq!(published["entry"]["dcc"], json!(["maya"]));
    assert_eq!(published["entry"]["version"], "0.2.0");
    assert_eq!(published["entry"]["install"]["type"], "zip");
    assert_eq!(published["entry"]["install"]["sha256"], sha256);
    let catalog =
        serde_json::from_str::<Value>(&std::fs::read_to_string(&catalog_path).unwrap()).unwrap();
    assert_eq!(catalog["schemaVersion"], "1");
    assert_eq!(catalog["skills"][0]["minCoreVersion"], "0.19.0");
    assert_eq!(catalog["skills"][0]["source"]["type"], "zip");
    assert_eq!(
        catalog["skills"][0]["source"]["skillRoots"],
        json!(["skill/release-skill"])
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
    ]);
    assert_eq!(updated["action"], "updated");
    assert_eq!(updated["count"], 1);
    assert_eq!(updated["entry"]["version"], "0.3.0");
}

#[test]
fn marketplace_install_list_and_uninstall_path_package() {
    let tmp = TempDir::new().unwrap();
    let skill_dir = write_skill(
        tmp.path(),
        "source-skill",
        "---\nname: dcc-asset-hunyuan-download\ndescription: Hunyuan downloads\n---\n",
    );
    std::fs::write(
        skill_dir.join("tools.yaml"),
        "tools:\n  - name: download\n    description: Download\n",
    )
    .unwrap();
    let catalog_path = tmp.path().join("marketplace.json");
    let catalog = json!({
        "version": "1",
        "entries": [{
            "name": "dcc-asset-hunyuan-download",
            "description": "Search and download Hunyuan 3D models via official API",
            "dcc": ["maya", "blender"],
            "tags": ["asset", "hunyuan", "download", "domain"],
            "version": "0.1.0",
            "install": {
                "type": "path",
                "url": skill_dir.to_string_lossy()
            }
        }]
    });
    std::fs::write(
        &catalog_path,
        serde_json::to_string_pretty(&catalog).unwrap(),
    )
    .unwrap();

    let source = catalog_path.to_string_lossy().to_string();
    let config_path = tmp
        .path()
        .join("sources.json")
        .to_string_lossy()
        .to_string();
    let install_root = tmp
        .path()
        .join("marketplace-root")
        .to_string_lossy()
        .to_string();
    let envs = [
        ("DCC_MCP_MARKETPLACE_SOURCES_FILE", config_path.as_str()),
        ("DCC_MCP_MARKETPLACE_NO_DEFAULT_SOURCES", "1"),
        ("DCC_MCP_MARKETPLACE_INSTALL_ROOT", install_root.as_str()),
    ];

    let installed = run_json_with_env(
        &[
            "marketplace",
            "install",
            "dcc-asset-hunyuan-download",
            "--dcc",
            "maya",
            "--source",
            &source,
        ],
        &envs,
    );
    assert_eq!(installed["installed"], true);
    assert_eq!(installed["dcc"], "maya");
    assert_eq!(installed["install_type"], "path");
    assert_eq!(installed["reload_required"], true);
    let installed_path = installed["path"].as_str().unwrap();
    assert!(
        std::path::Path::new(installed_path)
            .join("SKILL.md")
            .is_file()
    );
    assert!(
        installed["skill_search_path"]
            .as_str()
            .unwrap()
            .ends_with("maya")
    );

    let listed = run_json_with_env(&["marketplace", "list-installed", "--dcc", "maya"], &envs);
    assert_eq!(listed["count"], 1);
    assert_eq!(listed["packages"][0]["name"], "dcc-asset-hunyuan-download");
    assert_eq!(listed["packages"][0]["install_type"], "path");

    let uninstalled = run_json_with_env(
        &[
            "marketplace",
            "uninstall",
            "dcc-asset-hunyuan-download",
            "--dcc",
            "maya",
        ],
        &envs,
    );
    assert_eq!(uninstalled["uninstalled"], true);
    assert_eq!(uninstalled["removed_files"], true);
    assert_eq!(uninstalled["removed_state"], true);
    assert!(!std::path::Path::new(installed_path).exists());

    let listed = run_json_with_env(&["marketplace", "list-installed", "--dcc", "maya"], &envs);
    assert_eq!(listed["count"], 0);
}

#[test]
fn marketplace_install_rejects_incompatible_core_version() {
    let tmp = TempDir::new().unwrap();
    let skill_dir = write_skill(
        tmp.path(),
        "source-skill",
        "---\nname: incompatible-skill\ndescription: Incompatible\n---\n",
    );
    let catalog_path = tmp.path().join("marketplace.json");
    std::fs::write(
        &catalog_path,
        serde_json::to_string_pretty(&json!({
            "version": "1",
            "entries": [{
                "name": "incompatible-skill",
                "description": "Requires a newer core",
                "dcc": ["maya"],
                "tags": ["test"],
                "min_core_version": "999.0.0",
                "install": {"type": "path", "url": skill_dir.to_string_lossy()}
            }]
        }))
        .unwrap(),
    )
    .unwrap();
    let source = catalog_path.to_string_lossy().to_string();
    let config_path = tmp
        .path()
        .join("sources.json")
        .to_string_lossy()
        .to_string();
    let install_root = tmp
        .path()
        .join("marketplace-root")
        .to_string_lossy()
        .to_string();
    let envs = [
        ("DCC_MCP_MARKETPLACE_SOURCES_FILE", config_path.as_str()),
        ("DCC_MCP_MARKETPLACE_NO_DEFAULT_SOURCES", "1"),
        ("DCC_MCP_MARKETPLACE_INSTALL_ROOT", install_root.as_str()),
    ];

    let stderr = run_failure_with_env(
        &[
            "marketplace",
            "install",
            "incompatible-skill",
            "--dcc",
            "maya",
            "--source",
            &source,
        ],
        &envs,
    );
    assert!(stderr.contains("requires dcc-mcp-core >= 999.0.0"));
    assert!(!std::path::Path::new(&install_root).exists());
}

#[test]
fn marketplace_install_git_package_promotes_single_nested_skill_dir() {
    let tmp = TempDir::new().unwrap();
    let repo = tmp.path().join("nested-git-skill-repo");
    std::fs::create_dir_all(&repo).unwrap();
    run_git(&repo, &["init"]);
    run_git(&repo, &["config", "user.name", "dcc-mcp-test"]);
    run_git(&repo, &["config", "user.email", "dcc-mcp-test@example.com"]);
    let skill_dir = write_skill(
        &repo,
        "skill/nested-skill",
        "---\nname: nested-skill\ndescription: Nested git skill\nmetadata:\n  dcc-mcp:\n    dcc: python\n---\n",
    );
    std::fs::write(skill_dir.join("marker.txt"), "nested").unwrap();
    run_git(&repo, &["add", "."]);
    run_git(&repo, &["commit", "-m", "nested skill"]);

    let catalog_path = tmp.path().join("marketplace.json");
    let source = catalog_path.to_string_lossy().to_string();
    let config_path = tmp
        .path()
        .join("sources.json")
        .to_string_lossy()
        .to_string();
    let install_root = tmp
        .path()
        .join("marketplace-root")
        .to_string_lossy()
        .to_string();
    let envs = [
        ("DCC_MCP_MARKETPLACE_SOURCES_FILE", config_path.as_str()),
        ("DCC_MCP_MARKETPLACE_NO_DEFAULT_SOURCES", "1"),
        ("DCC_MCP_MARKETPLACE_INSTALL_ROOT", install_root.as_str()),
    ];
    let catalog = json!({
        "version": "1",
        "entries": [{
            "name": "nested-git-package",
            "description": "Nested git package",
            "dcc": ["maya"],
            "tags": ["test"],
            "version": "0.1.0",
            "install": {
                "type": "git",
                "url": repo.to_string_lossy()
            }
        }]
    });
    std::fs::write(
        &catalog_path,
        serde_json::to_string_pretty(&catalog).unwrap(),
    )
    .unwrap();

    let installed = run_json_with_env(
        &[
            "marketplace",
            "install",
            "nested-git-package",
            "--dcc",
            "maya",
            "--source",
            &source,
        ],
        &envs,
    );
    let installed_path = std::path::PathBuf::from(installed["path"].as_str().unwrap());
    assert!(installed_path.join("SKILL.md").is_file());
    assert_eq!(
        std::fs::read_to_string(installed_path.join("marker.txt")).unwrap(),
        "nested"
    );

    let listed = run_json_with_env(&["marketplace", "list-installed", "--dcc", "maya"], &envs);
    assert_eq!(listed["count"], 1);
    assert_eq!(listed["packages"][0]["name"], "nested-git-package");

    let uninstalled = run_json_with_env(
        &[
            "marketplace",
            "uninstall",
            "nested-git-package",
            "--dcc",
            "maya",
        ],
        &envs,
    );
    assert_eq!(uninstalled["uninstalled"], true);
    assert!(!installed_path.exists());
}

#[test]
fn marketplace_install_path_package_uses_declared_skill_roots() {
    let tmp = TempDir::new().unwrap();
    let pack = tmp.path().join("multi-pack");
    write_skill(
        &pack,
        "skill/maya-first",
        "---\nname: maya-first\ndescription: First pack skill\nmetadata:\n  dcc-mcp:\n    dcc: maya\n---\n",
    );
    write_skill(
        &pack,
        "skill/maya-second",
        "---\nname: maya-second\ndescription: Second pack skill\nmetadata:\n  dcc-mcp:\n    dcc: maya\n---\n",
    );
    write_skill(
        &pack,
        "examples/skills/example-skill",
        "---\nname: example-skill\ndescription: Example skill\nmetadata:\n  dcc-mcp:\n    dcc: maya\n---\n",
    );

    let catalog_path = tmp.path().join("marketplace.json");
    let catalog = json!({
        "version": "1",
        "entries": [{
            "name": "multi-skill-pack",
            "description": "Multi skill pack",
            "dcc": ["maya"],
            "tags": ["test"],
            "version": "0.1.0",
            "install": {
                "type": "path",
                "url": pack.to_string_lossy(),
                "skillRoots": ["skill/maya-first"]
            }
        }]
    });
    std::fs::write(
        &catalog_path,
        serde_json::to_string_pretty(&catalog).unwrap(),
    )
    .unwrap();

    let source = catalog_path.to_string_lossy().to_string();
    let config_path = tmp
        .path()
        .join("sources.json")
        .to_string_lossy()
        .to_string();
    let install_root = tmp
        .path()
        .join("marketplace-root")
        .to_string_lossy()
        .to_string();
    let envs = [
        ("DCC_MCP_MARKETPLACE_SOURCES_FILE", config_path.as_str()),
        ("DCC_MCP_MARKETPLACE_NO_DEFAULT_SOURCES", "1"),
        ("DCC_MCP_MARKETPLACE_INSTALL_ROOT", install_root.as_str()),
    ];

    let installed = run_json_with_env(
        &[
            "marketplace",
            "install",
            "multi-skill-pack",
            "--dcc",
            "maya",
            "--source",
            &source,
        ],
        &envs,
    );
    assert_eq!(installed["installed"], true);
    let dcc_root = std::path::Path::new(&install_root).join("maya");
    assert!(dcc_root.join("maya-first").join("SKILL.md").is_file());
    assert!(!dcc_root.join("maya-second").exists());
    assert!(!dcc_root.join("example-skill").exists());

    let mut scanner = dcc_mcp_skills::SkillScanner::new();
    let found = scanner.scan(
        Some(&[dcc_root.to_string_lossy().to_string()]),
        Some("maya"),
        true,
    );
    assert!(found.iter().any(|path| path.ends_with("maya-first")));
    assert!(!found.iter().any(|path| path.ends_with("maya-second")));
    assert!(!found.iter().any(|path| path.ends_with("example-skill")));

    let uninstalled = run_json_with_env(
        &[
            "marketplace",
            "uninstall",
            "multi-skill-pack",
            "--dcc",
            "maya",
        ],
        &envs,
    );
    assert_eq!(uninstalled["uninstalled"], true);
    assert!(!dcc_root.join("maya-first").exists());
}

#[test]
fn marketplace_install_zip_package_verifies_sha256_and_flattens_archive_root() {
    let tmp = TempDir::new().unwrap();
    let zip_path = tmp.path().join("zip-skill.zip");
    let zip_bytes = write_zip(
        &[
            (
                "zip-skill-main/SKILL.md",
                "---\nname: zip-skill\ndescription: Zip skill\n---\n",
            ),
            ("zip-skill-main/tools.yaml", "tools: []\n"),
        ],
        &zip_path,
    );
    let digest = sha256_hex(&zip_bytes);

    let catalog_path = tmp.path().join("marketplace.json");
    let catalog = json!({
        "version": "1",
        "entries": [{
            "name": "zip-skill",
            "description": "Zip skill package",
            "dcc": ["maya"],
            "tags": ["test"],
            "version": "0.1.0",
            "install": {
                "type": "zip",
                "url": zip_path.to_string_lossy(),
                "sha256": format!("sha256:{digest}")
            }
        }]
    });
    std::fs::write(
        &catalog_path,
        serde_json::to_string_pretty(&catalog).unwrap(),
    )
    .unwrap();

    let source = catalog_path.to_string_lossy().to_string();
    let config_path = tmp
        .path()
        .join("sources.json")
        .to_string_lossy()
        .to_string();
    let install_root = tmp
        .path()
        .join("marketplace-root")
        .to_string_lossy()
        .to_string();
    let envs = [
        ("DCC_MCP_MARKETPLACE_SOURCES_FILE", config_path.as_str()),
        ("DCC_MCP_MARKETPLACE_NO_DEFAULT_SOURCES", "1"),
        ("DCC_MCP_MARKETPLACE_INSTALL_ROOT", install_root.as_str()),
    ];

    let installed = run_json_with_env(
        &[
            "marketplace",
            "install",
            "zip-skill",
            "--dcc",
            "maya",
            "--source",
            &source,
        ],
        &envs,
    );
    let installed_path = std::path::PathBuf::from(installed["path"].as_str().unwrap());
    assert_eq!(installed["install_type"], "zip");
    assert!(installed_path.join("SKILL.md").is_file());
    assert!(installed_path.join("tools.yaml").is_file());
    assert!(!installed_path.join("zip-skill-main").exists());

    let listed = run_json_with_env(&["marketplace", "list-installed", "--dcc", "maya"], &envs);
    assert_eq!(listed["packages"][0]["install_type"], "zip");
}

#[test]
fn marketplace_install_zip_rejects_sha256_mismatch_without_replacing_existing_package() {
    let tmp = TempDir::new().unwrap();
    let good_skill = write_skill(
        tmp.path(),
        "good-skill",
        "---\nname: zip-skill\ndescription: Existing skill\n---\n",
    );
    let zip_path = tmp.path().join("zip-skill.zip");
    write_zip(
        &[(
            "SKILL.md",
            "---\nname: zip-skill\ndescription: Broken hash skill\n---\n",
        )],
        &zip_path,
    );

    let catalog_path = tmp.path().join("marketplace.json");
    let config_path = tmp
        .path()
        .join("sources.json")
        .to_string_lossy()
        .to_string();
    let install_root = tmp
        .path()
        .join("marketplace-root")
        .to_string_lossy()
        .to_string();
    let envs = [
        ("DCC_MCP_MARKETPLACE_SOURCES_FILE", config_path.as_str()),
        ("DCC_MCP_MARKETPLACE_NO_DEFAULT_SOURCES", "1"),
        ("DCC_MCP_MARKETPLACE_INSTALL_ROOT", install_root.as_str()),
    ];

    let good_catalog = json!({
        "version": "1",
        "entries": [{
            "name": "zip-skill",
            "description": "Existing skill",
            "dcc": ["maya"],
            "tags": ["test"],
            "version": "0.1.0",
            "install": {
                "type": "path",
                "url": good_skill.to_string_lossy()
            }
        }]
    });
    std::fs::write(
        &catalog_path,
        serde_json::to_string_pretty(&good_catalog).unwrap(),
    )
    .unwrap();
    let source = catalog_path.to_string_lossy().to_string();
    let installed = run_json_with_env(
        &[
            "marketplace",
            "install",
            "zip-skill",
            "--dcc",
            "maya",
            "--source",
            &source,
        ],
        &envs,
    );
    let installed_path = std::path::PathBuf::from(installed["path"].as_str().unwrap());

    let bad_catalog = json!({
        "version": "1",
        "entries": [{
            "name": "zip-skill",
            "description": "Bad hash skill",
            "dcc": ["maya"],
            "tags": ["test"],
            "version": "0.2.0",
            "install": {
                "type": "zip",
                "url": zip_path.to_string_lossy(),
                "sha256": "sha256:0000"
            }
        }]
    });
    std::fs::write(
        &catalog_path,
        serde_json::to_string_pretty(&bad_catalog).unwrap(),
    )
    .unwrap();

    let stderr = run_failure_with_env(
        &[
            "marketplace",
            "install",
            "zip-skill",
            "--dcc",
            "maya",
            "--source",
            &source,
            "--force",
        ],
        &envs,
    );
    assert!(stderr.contains("SHA-256 mismatch"));
    assert!(installed_path.join("SKILL.md").is_file());

    let listed = run_json_with_env(&["marketplace", "list-installed", "--dcc", "maya"], &envs);
    assert_eq!(listed["packages"][0]["version"], "0.1.0");
    assert_eq!(listed["packages"][0]["install_type"], "path");
}

#[test]
fn marketplace_rejects_unsafe_install_components() {
    let tmp = TempDir::new().unwrap();
    let skill_dir = write_skill(
        tmp.path(),
        "source-skill",
        "---\nname: safe-skill\ndescription: Safe skill\n---\n",
    );
    let catalog_path = tmp.path().join("marketplace.json");
    let catalog = json!({
        "version": "1",
        "entries": [{
            "name": "../unsafe-skill",
            "description": "Unsafe name",
            "dcc": ["maya"],
            "tags": ["test"],
            "version": "0.1.0",
            "install": {
                "type": "path",
                "url": skill_dir.to_string_lossy()
            }
        }]
    });
    std::fs::write(
        &catalog_path,
        serde_json::to_string_pretty(&catalog).unwrap(),
    )
    .unwrap();

    let source = catalog_path.to_string_lossy().to_string();
    let config_path = tmp
        .path()
        .join("sources.json")
        .to_string_lossy()
        .to_string();
    let install_root = tmp
        .path()
        .join("marketplace-root")
        .to_string_lossy()
        .to_string();
    let envs = [
        ("DCC_MCP_MARKETPLACE_SOURCES_FILE", config_path.as_str()),
        ("DCC_MCP_MARKETPLACE_NO_DEFAULT_SOURCES", "1"),
        ("DCC_MCP_MARKETPLACE_INSTALL_ROOT", install_root.as_str()),
    ];

    let stderr = run_failure_with_env(
        &[
            "marketplace",
            "install",
            "../unsafe-skill",
            "--dcc",
            "maya",
            "--source",
            &source,
        ],
        &envs,
    );
    assert!(stderr.contains("invalid marketplace package name"));

    let stderr = run_failure_with_env(
        &[
            "marketplace",
            "uninstall",
            "../unsafe-skill",
            "--dcc",
            "maya",
        ],
        &envs,
    );
    assert!(stderr.contains("invalid marketplace package name"));
}

#[test]
fn marketplace_force_install_keeps_existing_package_when_replacement_fails() {
    let tmp = TempDir::new().unwrap();
    let good_skill = write_skill(
        tmp.path(),
        "good-skill",
        "---\nname: replaceable-skill\ndescription: Replaceable skill\n---\n",
    );
    let bad_skill = tmp.path().join("bad-skill");
    std::fs::create_dir_all(&bad_skill).unwrap();

    let catalog_path = tmp.path().join("marketplace.json");
    let config_path = tmp
        .path()
        .join("sources.json")
        .to_string_lossy()
        .to_string();
    let install_root = tmp
        .path()
        .join("marketplace-root")
        .to_string_lossy()
        .to_string();
    let envs = [
        ("DCC_MCP_MARKETPLACE_SOURCES_FILE", config_path.as_str()),
        ("DCC_MCP_MARKETPLACE_NO_DEFAULT_SOURCES", "1"),
        ("DCC_MCP_MARKETPLACE_INSTALL_ROOT", install_root.as_str()),
    ];

    let good_catalog = json!({
        "version": "1",
        "entries": [{
            "name": "replaceable-skill",
            "description": "Replaceable skill",
            "dcc": ["maya"],
            "tags": ["test"],
            "version": "0.1.0",
            "install": {
                "type": "path",
                "url": good_skill.to_string_lossy()
            }
        }]
    });
    std::fs::write(
        &catalog_path,
        serde_json::to_string_pretty(&good_catalog).unwrap(),
    )
    .unwrap();
    let source = catalog_path.to_string_lossy().to_string();
    let installed = run_json_with_env(
        &[
            "marketplace",
            "install",
            "replaceable-skill",
            "--dcc",
            "maya",
            "--source",
            &source,
        ],
        &envs,
    );
    let installed_path = std::path::PathBuf::from(installed["path"].as_str().unwrap());
    assert!(installed_path.join("SKILL.md").is_file());

    let bad_catalog = json!({
        "version": "1",
        "entries": [{
            "name": "replaceable-skill",
            "description": "Broken replacement",
            "dcc": ["maya"],
            "tags": ["test"],
            "version": "0.2.0",
            "install": {
                "type": "path",
                "url": bad_skill.to_string_lossy()
            }
        }]
    });
    std::fs::write(
        &catalog_path,
        serde_json::to_string_pretty(&bad_catalog).unwrap(),
    )
    .unwrap();

    let stderr = run_failure_with_env(
        &[
            "marketplace",
            "install",
            "replaceable-skill",
            "--dcc",
            "maya",
            "--source",
            &source,
            "--force",
        ],
        &envs,
    );
    assert!(stderr.contains("does not contain SKILL.md"));
    assert!(installed_path.join("SKILL.md").is_file());
}

#[test]
fn marketplace_update_git_package_uses_latest_catalog_ref() {
    let tmp = TempDir::new().unwrap();
    let repo = tmp.path().join("git-skill-repo");
    std::fs::create_dir_all(&repo).unwrap();
    run_git(&repo, &["init"]);
    run_git(&repo, &["config", "user.name", "dcc-mcp-test"]);
    run_git(&repo, &["config", "user.email", "dcc-mcp-test@example.com"]);
    commit_git_skill_version(&repo, "v0.1.0", "v1");
    commit_git_skill_version(&repo, "v0.2.0", "v2");

    let catalog_path = tmp.path().join("marketplace.json");
    let source = catalog_path.to_string_lossy().to_string();
    let config_path = tmp
        .path()
        .join("sources.json")
        .to_string_lossy()
        .to_string();
    let install_root = tmp
        .path()
        .join("marketplace-root")
        .to_string_lossy()
        .to_string();
    let envs = [
        ("DCC_MCP_MARKETPLACE_SOURCES_FILE", config_path.as_str()),
        ("DCC_MCP_MARKETPLACE_NO_DEFAULT_SOURCES", "1"),
        ("DCC_MCP_MARKETPLACE_INSTALL_ROOT", install_root.as_str()),
    ];

    let catalog_v1 = json!({
        "version": "1",
        "entries": [{
            "name": "git-skill",
            "description": "Git skill",
            "dcc": ["maya"],
            "tags": ["test"],
            "version": "0.1.0",
            "install": {
                "type": "git",
                "url": repo.to_string_lossy(),
                "ref": "v0.1.0"
            }
        }]
    });
    std::fs::write(
        &catalog_path,
        serde_json::to_string_pretty(&catalog_v1).unwrap(),
    )
    .unwrap();

    let installed = run_json_with_env(
        &[
            "marketplace",
            "install",
            "git-skill",
            "--dcc",
            "maya",
            "--source",
            &source,
        ],
        &envs,
    );
    let installed_path = std::path::PathBuf::from(installed["path"].as_str().unwrap());
    assert_eq!(
        std::fs::read_to_string(installed_path.join("marker.txt")).unwrap(),
        "v1"
    );

    let catalog_v2 = json!({
        "version": "1",
        "entries": [{
            "name": "git-skill",
            "description": "Git skill",
            "dcc": ["maya"],
            "tags": ["test"],
            "version": "0.2.0",
            "install": {
                "type": "git",
                "url": repo.to_string_lossy(),
                "ref": "v0.2.0"
            }
        }]
    });
    std::fs::write(
        &catalog_path,
        serde_json::to_string_pretty(&catalog_v2).unwrap(),
    )
    .unwrap();

    let outdated = run_json_with_env(
        &["marketplace", "outdated", "git-skill", "--dcc", "maya"],
        &envs,
    );
    assert_eq!(outdated["count"], 1);
    assert_eq!(outdated["packages"][0]["latest_version"], "0.2.0");
    assert_eq!(outdated["packages"][0]["install_ref"], "v0.2.0");

    let updated = run_json_with_env(
        &["marketplace", "update", "git-skill", "--dcc", "maya"],
        &envs,
    );
    assert_eq!(updated[0]["new_version"], "0.2.0");
    assert_eq!(
        std::fs::read_to_string(installed_path.join("marker.txt")).unwrap(),
        "v2"
    );

    let listed = run_json_with_env(&["marketplace", "list-installed", "--dcc", "maya"], &envs);
    assert_eq!(listed["packages"][0]["version"], "0.2.0");
    assert_eq!(listed["packages"][0]["install_ref"], "v0.2.0");
}

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
fn lint_bundled_skills_are_present_and_clean() {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir
        .parent()
        .and_then(std::path::Path::parent)
        .unwrap();
    let builtin_skill_roots = [
        workspace_root.join("skills/dcc-cli-gateway"),
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

#[test]
fn dcc_cli_gateway_skill_is_local_first_without_required_gateway_env() {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir
        .parent()
        .and_then(std::path::Path::parent)
        .unwrap();
    let skill_dir = workspace_root.join("skills/dcc-cli-gateway");

    let meta = parse_skill_md(&skill_dir).expect("dcc-cli-gateway SKILL.md parses");

    assert_eq!(meta.name, "dcc-cli-gateway");
    assert!(meta.description.contains("dcc-mcp-cli local registry"));
    assert!(meta.required_env_vars().is_empty());
    assert_eq!(meta.primary_env(), None);
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
    // Register catalog1 (lower priority — registered first in config)
    // and pass catalog2 as explicit (higher priority).
    std::fs::write(
        &config_path,
        json!({"sources": [{"name": "catalog1", "url": source1}]}).to_string(),
    )
    .unwrap();

    let envs = [
        ("DCC_MCP_MARKETPLACE_SOURCES_FILE", config_path.as_str()),
        ("DCC_MCP_MARKETPLACE_NO_DEFAULT_SOURCES", "1"),
    ];

    // Search with explicit source for catalog2 — only catalog2 is searched
    // because explicit sources are exclusive (replace configured sources).
    let search = run_json_with_env(&["marketplace", "search", "--source", &source2], &envs);
    // Should have exactly 2 unique entries (skill-b, skill-c) from catalog2.
    // catalog1 is not searched because --source is exclusive.
    assert_eq!(search["count"], 2);
    let skill_b = search["hits"]
        .as_array()
        .unwrap()
        .iter()
        .find(|h| h["entry"]["name"] == "skill-b")
        .unwrap();
    assert_eq!(
        skill_b["entry"]["description"],
        "Shared skill from catalog 2"
    );
    assert_eq!(skill_b["source"]["origin"], "explicit");
    // skill-a from catalog1 must not appear.
    assert!(
        !search["hits"]
            .as_array()
            .unwrap()
            .iter()
            .any(|h| h["entry"]["name"] == "skill-a"),
        "configured-source entries must not appear when explicit --source is given"
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
