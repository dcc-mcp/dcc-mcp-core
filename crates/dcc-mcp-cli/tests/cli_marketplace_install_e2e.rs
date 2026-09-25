//! Marketplace install lifecycle end-to-end tests — install, uninstall,
//! update, and the per-host/shared install landing spots.

mod support;

use dcc_mcp_transport::discovery::file_registry::FileRegistry;
use dcc_mcp_transport::discovery::types::ServiceEntry;
use serde_json::json;
use tempfile::TempDir;

use support::*;

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
        &["marketplace", "uninstall", "dcc-asset-hunyuan-download"],
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
fn marketplace_installs_agent_plugin_as_one_multi_skill_package() {
    let tmp = TempDir::new().unwrap();
    let plugin_dir = tmp.path().join("rig-plugin");
    std::fs::create_dir_all(plugin_dir.join("skills/inspect")).unwrap();
    std::fs::create_dir_all(plugin_dir.join("skills/act")).unwrap();
    std::fs::write(
        plugin_dir.join("plugin.json"),
        r#"{"$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json","name":"rig-plugin","version":"1.0.0","description":"Rig workflows"}"#,
    )
    .unwrap();
    for name in ["inspect", "act"] {
        std::fs::write(
            plugin_dir.join(format!("skills/{name}/SKILL.md")),
            format!(
                "---\nname: {name}\ndescription: {name} rig data\nmetadata:\n  dcc-mcp:\n    dcc: maya\n---\n"
            ),
        )
        .unwrap();
    }
    let catalog_path = tmp.path().join("marketplace.json");
    std::fs::write(
        &catalog_path,
        json!({
            "version": "1",
            "entries": [{
                "name": "rig-plugin",
                "description": "Rig workflows",
                "dcc": ["maya"],
                "tags": ["rigging"],
                "version": "1.0.0",
                "package": {
                    "format": "agent-plugin",
                    "skills": ["inspect", "act"]
                },
                "install": {
                    "type": "path",
                    "url": plugin_dir.to_string_lossy()
                }
            }]
        })
        .to_string(),
    )
    .unwrap();
    let source = catalog_path.to_string_lossy().to_string();
    let sources_file = tmp
        .path()
        .join("sources.json")
        .to_string_lossy()
        .to_string();
    let install_root = tmp.path().join("marketplace-root");
    let install_root_value = install_root.to_string_lossy().to_string();
    let envs = [
        ("DCC_MCP_MARKETPLACE_SOURCES_FILE", sources_file.as_str()),
        ("DCC_MCP_MARKETPLACE_NO_DEFAULT_SOURCES", "1"),
        (
            "DCC_MCP_MARKETPLACE_INSTALL_ROOT",
            install_root_value.as_str(),
        ),
    ];

    let installed = run_json_with_env(
        &[
            "marketplace",
            "install",
            "rig-plugin",
            "--dcc",
            "maya",
            "--source",
            &source,
        ],
        &envs,
    );

    assert_eq!(installed["entry"]["package"]["format"], "agent-plugin");
    assert!(install_root.join("maya/inspect/SKILL.md").is_file());
    assert!(install_root.join("maya/act/SKILL.md").is_file());
    assert!(installed["path"].as_str().unwrap().contains(".packages"));

    let removed = run_json_with_env(
        &["marketplace", "uninstall", "rig-plugin", "--dcc", "maya"],
        &envs,
    );
    assert_eq!(removed["removed_files"], true);
    assert!(!install_root.join("maya/inspect").exists());
    assert!(!install_root.join("maya/act").exists());
}

#[test]
fn marketplace_install_exact_name_infers_dcc_and_reloads_running_dcc() {
    let tmp = TempDir::new().unwrap();
    let fixture = spawn_local_mcp_fixture();
    let registry_path = tmp.path().join("registry");
    let registry = FileRegistry::new(&registry_path).unwrap();
    let mut entry = ServiceEntry::new("maya", "127.0.0.1", 0);
    entry
        .metadata
        .insert("mcp_url".to_string(), fixture.mcp_url());
    registry.register(entry).unwrap();

    let skill_dir = write_skill(
        tmp.path(),
        "source-skill",
        "---\nname: dcc-mcp-maya-mgear\ndescription: mGear tools\n---\n",
    );
    let catalog_path = tmp.path().join("marketplace.json");
    std::fs::write(
        &catalog_path,
        serde_json::to_string_pretty(&json!({
            "version": "1",
            "entries": [{
                "name": "dcc-mcp-maya-mgear",
                "description": "mGear tools for Maya",
                "dcc": ["maya"],
                "tags": ["rigging"],
                "version": "0.1.0",
                "install": {"type": "path", "url": skill_dir.to_string_lossy()}
            }]
        }))
        .unwrap(),
    )
    .unwrap();

    let source = catalog_path.to_string_lossy().to_string();
    let sources_file = tmp
        .path()
        .join("sources.json")
        .to_string_lossy()
        .to_string();
    let install_root = tmp
        .path()
        .join("marketplace-root")
        .to_string_lossy()
        .to_string();
    let registry_dir = registry_path.to_string_lossy().to_string();
    let profiles_file = tmp
        .path()
        .join("gateway-profiles.json")
        .to_string_lossy()
        .to_string();
    let envs = [
        ("DCC_MCP_MARKETPLACE_SOURCES_FILE", sources_file.as_str()),
        ("DCC_MCP_MARKETPLACE_NO_DEFAULT_SOURCES", "1"),
        ("DCC_MCP_MARKETPLACE_INSTALL_ROOT", install_root.as_str()),
        ("DCC_MCP_REGISTRY_DIR", registry_dir.as_str()),
        ("DCC_MCP_GATEWAY_PROFILES_FILE", profiles_file.as_str()),
        ("DCC_MCP_GATEWAY_PROFILE", "local"),
        ("DCC_MCP_BASE_URL", ""),
        ("DCC_MCP_CLI_NO_AUTO_GATEWAY", "true"),
    ];

    let installed = run_json_with_env(
        &[
            "marketplace",
            "install",
            "dcc-mcp-maya-mgear",
            "--source",
            &source,
            "--reload",
        ],
        &envs,
    );

    assert_eq!(installed["installed"], true);
    assert_eq!(installed["dcc"], "maya");
    assert_eq!(installed["reload_required"], false);
    assert_eq!(installed["reload"]["reloaded"], true);
    assert_eq!(installed["reload"]["count"], 1);
    assert_eq!(
        installed["reload"]["results"][0]["backend_tool"],
        "dcc_admin__reload_skills"
    );
}

#[test]
fn marketplace_install_rejects_incompatible_or_unavailable_entries() {
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

    std::fs::write(
        &catalog_path,
        serde_json::to_string_pretty(&json!({
            "version": "1",
            "entries": [{
                "name": "incompatible-skill",
                "description": "Unavailable skill",
                "dcc": ["maya"],
                "tags": ["test"],
                "policy": {"installation": "not_available"},
                "install": {"type": "path", "url": skill_dir.to_string_lossy()}
            }]
        }))
        .unwrap(),
    )
    .unwrap();
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
    assert!(stderr.contains("is not available for installation"));
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
    let commit = git_head(&repo);

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
                "url": repo.to_string_lossy(),
                "ref": commit
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
                "sha256": format!("sha256:{}", "0".repeat(64))
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
    let commit_v1 = commit_git_skill_version(&repo, "v0.1.0", "v1");
    let commit_v2 = commit_git_skill_version(&repo, "v0.2.0", "v2");

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
                "ref": commit_v1
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
                "ref": commit_v2.clone()
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
    assert_eq!(outdated["packages"][0]["install_ref"], commit_v2);

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
    assert_eq!(listed["packages"][0]["install_ref"], commit_v2);
}

#[test]
fn marketplace_installs_and_uninstalls_cua_profile_through_exact_cli() {
    let tmp = TempDir::new().unwrap();
    let package = tmp.path().join("the-bazaar-profile");
    std::fs::create_dir_all(&package).unwrap();
    std::fs::write(package.join("profile.json"), "{}\n").unwrap();
    let catalog = tmp.path().join("marketplace.json");
    let published = run_json(&[
        "marketplace",
        "publish",
        package.to_str().unwrap(),
        "--catalog",
        catalog.to_str().unwrap(),
        "--install-url",
        package.to_str().unwrap(),
        "--install-type",
        "path",
        "--name",
        "the-bazaar-profile",
        "--description",
        "The Bazaar semantic profile",
        "--target",
        "game:the-bazaar",
        "--format",
        "cua-profile",
        "--component",
        "cua-profile:the-bazaar=.",
        "--version",
        "1.0.0",
        "--maintainer",
        "dcc-mcp",
        "--min-core-version",
        "0.20.0",
        "--tag",
        "profile",
    ]);
    assert_eq!(published["entry"]["package"]["format"], "cua-profile");
    let log = tmp.path().join("dcc-cua.log");
    let fake = tmp.path().join(if cfg!(windows) {
        "dcc-cua.cmd"
    } else {
        "dcc-cua"
    });
    if cfg!(windows) {
        std::fs::write(
            &fake,
            "@echo off\r\necho %*>>\"%DCC_CUA_TEST_LOG%\"\r\nexit /b 0\r\n",
        )
        .unwrap();
    } else {
        std::fs::write(
            &fake,
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$DCC_CUA_TEST_LOG\"\n",
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
    }
    let root = tmp.path().join("installed");
    let config = tmp.path().join("sources.json");
    let envs = [
        ("DCC_MCP_MARKETPLACE_SOURCES_FILE", config.to_str().unwrap()),
        ("DCC_MCP_MARKETPLACE_NO_DEFAULT_SOURCES", "1"),
        ("DCC_MCP_MARKETPLACE_INSTALL_ROOT", root.to_str().unwrap()),
        ("DCC_MCP_CUA_BINARY", fake.to_str().unwrap()),
        ("DCC_CUA_TEST_LOG", log.to_str().unwrap()),
    ];
    let source = catalog.to_str().unwrap();
    let installed = run_json_with_env(
        &[
            "marketplace",
            "install",
            "the-bazaar-profile",
            "--target",
            "game:the-bazaar",
            "--source",
            source,
            "--reload",
        ],
        &envs,
    );
    assert_eq!(installed["target"]["kind"], "game");
    assert_eq!(installed["activation"], "none");
    let removed = run_json_with_env(
        &[
            "marketplace",
            "uninstall",
            "the-bazaar-profile",
            "--target",
            "game:the-bazaar",
            "--reload",
        ],
        &envs,
    );
    assert_eq!(removed["uninstalled"], true);
    let calls = std::fs::read_to_string(log).unwrap();
    let lines = calls.lines().collect::<Vec<_>>();
    assert_eq!(lines.len(), 3);
    assert!(lines[0].starts_with("profile validate "));
    assert!(lines[1].starts_with("profile install "));
    assert_eq!(lines[2], "profile uninstall the-bazaar --confirm");
}

#[test]
fn marketplace_installs_a_multi_host_entry_once_into_the_shared_directory() {
    let tmp = TempDir::new().unwrap();
    let skill_dir = write_skill(
        tmp.path(),
        "source-skill",
        "---\nname: dcc-asset-polyhaven\ndescription: Poly Haven assets\n---\n",
    );
    std::fs::write(
        skill_dir.join("tools.yaml"),
        "tools:\n  - name: search\n    description: Search\n",
    )
    .unwrap();
    let catalog_path = tmp.path().join("marketplace.json");
    let catalog = json!({
        "version": "1",
        "entries": [{
            "name": "dcc-asset-polyhaven",
            "description": "Search and download Poly Haven CC0 assets",
            "dcc": ["maya", "blender", "houdini", "3dsmax"],
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
    let install_root = tmp.path().join("marketplace-root");
    let install_root_value = install_root.to_string_lossy().to_string();
    let envs = [
        ("DCC_MCP_MARKETPLACE_SOURCES_FILE", config_path.as_str()),
        ("DCC_MCP_MARKETPLACE_NO_DEFAULT_SOURCES", "1"),
        (
            "DCC_MCP_MARKETPLACE_INSTALL_ROOT",
            install_root_value.as_str(),
        ),
    ];

    // No --dcc: the install must succeed instead of asking for one.
    let installed = run_json_with_env(
        &[
            "marketplace",
            "install",
            "dcc-asset-polyhaven",
            "--source",
            &source,
        ],
        &envs,
    );
    assert_eq!(installed["installed"], true);
    assert_eq!(installed["dcc"], "any");
    assert_eq!(installed["target"]["id"], "any");
    // Nothing was installed per host before, so nothing is superseded.
    assert!(installed.get("superseded").is_none(), "{installed}");
    assert!(
        installed["skill_search_path"]
            .as_str()
            .unwrap()
            .ends_with("any")
    );
    let installed_path = installed["path"].as_str().unwrap();
    assert!(
        std::path::Path::new(installed_path)
            .join("SKILL.md")
            .is_file()
    );

    // One tree only: no per-host copy is written for any declared host.
    for dcc in ["maya", "blender", "houdini", "3dsmax"] {
        assert!(
            !install_root.join(dcc).exists(),
            "{dcc} must not receive its own copy"
        );
    }
    assert!(
        install_root
            .join("any")
            .join("dcc-asset-polyhaven")
            .is_dir()
    );

    // One ledger entry covers every declared host.
    let all = run_json_with_env(&["marketplace", "list-installed"], &envs);
    assert_eq!(all["count"], 1);
    for dcc in ["maya", "blender", "houdini", "3dsmax"] {
        let listed = run_json_with_env(&["marketplace", "list-installed", "--dcc", dcc], &envs);
        assert_eq!(
            listed["count"], 1,
            "{dcc} should resolve the shared install"
        );
        assert_eq!(listed["packages"][0]["name"], "dcc-asset-polyhaven");
    }

    // A host can remove the shared install by naming the host it runs.
    let uninstalled = run_json_with_env(
        &[
            "marketplace",
            "uninstall",
            "dcc-asset-polyhaven",
            "--dcc",
            "blender",
        ],
        &envs,
    );
    assert_eq!(uninstalled["uninstalled"], true);
    assert_eq!(uninstalled["dcc"], "any");
    assert_eq!(uninstalled["removed_files"], true);
    assert!(!std::path::Path::new(installed_path).exists());
    let listed = run_json_with_env(&["marketplace", "list-installed"], &envs);
    assert_eq!(listed["count"], 0);
}

#[test]
fn marketplace_install_accepts_dcc_all_for_a_multi_host_entry() {
    let tmp = TempDir::new().unwrap();
    let skill_dir = write_skill(
        tmp.path(),
        "source-skill",
        "---\nname: dcc-asset-polyhaven\ndescription: Poly Haven assets\n---\n",
    );
    let catalog_path = tmp.path().join("marketplace.json");
    let catalog = json!({
        "version": "1",
        "entries": [{
            "name": "dcc-asset-polyhaven",
            "description": "Search and download Poly Haven CC0 assets",
            "dcc": ["maya", "blender"],
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
            "dcc-asset-polyhaven",
            "--dcc",
            "all",
            "--source",
            &source,
        ],
        &envs,
    );
    assert_eq!(installed["installed"], true);
    assert_eq!(installed["dcc"], "any");
}

/// A shared install is loaded by every host, so `--reload` must refresh all
/// live instances. Passing the pseudo-host `any` down to the instance selector
/// matches nothing, which used to fail the reload and the whole command.
#[test]
fn marketplace_shared_install_reloads_every_live_instance() {
    let tmp = TempDir::new().unwrap();
    let fixture = spawn_local_mcp_fixture();
    let registry_path = tmp.path().join("registry");
    let registry = FileRegistry::new(&registry_path).unwrap();
    // Two hosts are live; a shared package must reach both.
    for dcc in ["maya", "blender"] {
        let mut entry = ServiceEntry::new(dcc, "127.0.0.1", 0);
        entry
            .metadata
            .insert("mcp_url".to_string(), fixture.mcp_url());
        registry.register(entry).unwrap();
    }

    let skill_dir = write_skill(
        tmp.path(),
        "source-skill",
        "---\nname: dcc-asset-polyhaven\ndescription: Poly Haven CC0 assets\n---\n",
    );
    let catalog_path = tmp.path().join("marketplace.json");
    std::fs::write(
        &catalog_path,
        serde_json::to_string_pretty(&json!({
            "version": "1",
            "entries": [{
                "name": "dcc-asset-polyhaven",
                "description": "Search and download Poly Haven CC0 assets",
                "dcc": ["maya", "blender"],
                "version": "0.1.0",
                "install": {"type": "path", "url": skill_dir.to_string_lossy()}
            }]
        }))
        .unwrap(),
    )
    .unwrap();

    let source = catalog_path.to_string_lossy().to_string();
    let sources_file = tmp
        .path()
        .join("sources.json")
        .to_string_lossy()
        .to_string();
    let install_root = tmp
        .path()
        .join("marketplace-root")
        .to_string_lossy()
        .to_string();
    let registry_dir = registry_path.to_string_lossy().to_string();
    let profiles_file = tmp
        .path()
        .join("gateway-profiles.json")
        .to_string_lossy()
        .to_string();
    let envs = [
        ("DCC_MCP_MARKETPLACE_SOURCES_FILE", sources_file.as_str()),
        ("DCC_MCP_MARKETPLACE_NO_DEFAULT_SOURCES", "1"),
        ("DCC_MCP_MARKETPLACE_INSTALL_ROOT", install_root.as_str()),
        ("DCC_MCP_REGISTRY_DIR", registry_dir.as_str()),
        ("DCC_MCP_GATEWAY_PROFILES_FILE", profiles_file.as_str()),
        ("DCC_MCP_GATEWAY_PROFILE", "local"),
        ("DCC_MCP_BASE_URL", ""),
        ("DCC_MCP_CLI_NO_AUTO_GATEWAY", "true"),
    ];

    let installed = run_json_with_env(
        &[
            "marketplace",
            "install",
            "dcc-asset-polyhaven",
            "--source",
            &source,
            "--reload",
        ],
        &envs,
    );

    assert_eq!(installed["installed"], true);
    assert_eq!(installed["dcc"], "any");
    // The reload reached both live hosts instead of failing on `any`.
    assert_eq!(installed["reload_required"], false, "{installed}");
    assert_eq!(installed["reload"]["reloaded"], true, "{installed}");
    assert_eq!(installed["reload"]["count"], 2, "{installed}");
}
