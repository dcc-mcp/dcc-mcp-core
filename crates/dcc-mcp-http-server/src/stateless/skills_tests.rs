//! `io.modelcontextprotocol/skills` conformance tests.
//!
//! Covers the acceptance criteria for the dual-track rollout: the extension
//! methods, digest/read consistency, independent `skills/get`, pagination,
//! the per-skill limits, and that both tracks describe the same skills.

use std::sync::Arc;

use dcc_mcp_actions::{ToolDispatcher, ToolRegistry};
use dcc_mcp_http_types::config::FeatureFlags;
use dcc_mcp_jsonrpc::{JsonRpcRequest, JsonRpcResponse, SERVER_INFO_META_KEY, error_codes};
use dcc_mcp_models::SkillMetadata;
use dcc_mcp_skill_rest::StaticReadiness;
use dcc_mcp_skills::{SkillCatalog, skill_content as content};
use serde_json::{Value, json};

use crate::rmcp_registry_context::RegistryContext;
use crate::server_state::ServerState;
use crate::stateless::StatelessMcpService;

// ── Fixtures ───────────────────────────────────────────────────────────────

/// A temporary skill tree that removes itself on drop.
struct SkillTree {
    dir: tempfile::TempDir,
}

impl SkillTree {
    fn new() -> Self {
        Self {
            dir: tempfile::TempDir::new().expect("temp skill dir"),
        }
    }

    fn root(&self) -> &std::path::Path {
        self.dir.path()
    }

    /// Create a skill directory and return its path.
    fn skill(&self, name: &str, description: &str) -> std::path::PathBuf {
        let path = self.dir.path().join(name);
        std::fs::create_dir_all(&path).expect("skill dir");
        std::fs::write(
            path.join("SKILL.md"),
            format!(
                "---\nname: {name}\ndescription: {description}\nlicense: MIT\n---\n\n# {name}\n"
            ),
        )
        .expect("SKILL.md");
        path
    }

    fn file(&self, skill: &str, relative: &str, contents: &str) {
        let path = self.dir.path().join(skill).join(relative);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("subdir");
        std::fs::write(path, contents).expect("file");
    }
}

struct Harness {
    service: StatelessMcpService,
    /// Owns the temporary skill directory; it must outlive every request.
    tree: SkillTree,
}

impl Harness {
    /// Absolute path of a skill inside the temporary tree.
    fn skill_path(&self, name: &str) -> std::path::PathBuf {
        self.tree.root().join(name)
    }
}

/// Build a service whose catalog serves skills out of `tree`.
fn harness(skills: &[(&str, &str)], features: FeatureFlags) -> Harness {
    let tree = SkillTree::new();
    let registry = Arc::new(ToolRegistry::new());
    let dispatcher = Arc::new(ToolDispatcher::new((*registry).clone()));
    let catalog = Arc::new(SkillCatalog::new_with_dispatcher(
        Arc::clone(&registry),
        Arc::clone(&dispatcher),
    ));
    for (name, description) in skills {
        let path = tree.skill(name, description);
        catalog.add_skill(SkillMetadata {
            name: (*name).to_string(),
            description: (*description).to_string(),
            skill_path: path.to_string_lossy().to_string(),
            ..SkillMetadata::default()
        });
    }
    let state = ServerState::builder(registry, dispatcher, Arc::clone(&catalog))
        .with_features(features)
        .build();
    let context = Arc::new(RegistryContext {
        resource_provider: None,
        prompt_provider: None,
        readiness: Arc::new(StaticReadiness::fully_ready()),
        on_skill_catalog_mutated: Arc::new(|| {}),
    });
    Harness {
        service: StatelessMcpService::new(state, context),
        tree,
    }
}

fn default_harness(skills: &[(&str, &str)]) -> Harness {
    harness(skills, FeatureFlags::default())
}

fn request(method: &str, params: Option<Value>) -> JsonRpcRequest {
    let mut params = params.unwrap_or_else(|| json!({}));
    params["_meta"] = json!({
        dcc_mcp_jsonrpc::PROTOCOL_VERSION_META_KEY: "2026-07-28",
        dcc_mcp_jsonrpc::CLIENT_CAPABILITIES_META_KEY: {}
    });
    JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: Some(json!("skills-request")),
        method: method.to_string(),
        params: Some(params),
    }
}

async fn call(service: &StatelessMcpService, method: &str, params: Option<Value>) -> Value {
    let response = service
        .handle_request(&request(method, params))
        .await
        .expect("request has an id");
    assert_eq!(response["jsonrpc"], "2.0");
    response
}

fn result(response: &Value) -> &Value {
    assert!(
        response.get("error").is_none(),
        "expected success, got: {response}"
    );
    &response["result"]
}

fn error_code(response: &Value) -> i64 {
    response["error"]["code"]
        .as_i64()
        .unwrap_or_else(|| panic!("expected a JSON-RPC error, got: {response}"))
}

/// SHA-256 of `bytes` in the wire form the extension uses.
fn digest(bytes: &[u8]) -> String {
    content::digest_bytes(bytes)
}

// ── server/discover ────────────────────────────────────────────────────────

#[tokio::test]
async fn discover_declares_resources_and_the_skills_extension() {
    let h = default_harness(&[("demo", "Demo skill")]);
    let response = call(&h.service, "server/discover", None).await;
    let capabilities = &result(&response)["capabilities"];

    // The extension rides on the Resources primitive, so both are declared.
    assert!(capabilities["resources"].is_object(), "{capabilities}");
    assert_eq!(
        capabilities["extensions"]["io.modelcontextprotocol/skills"]["directoryRead"], true,
        "{capabilities}"
    );
}

#[tokio::test]
async fn discover_omits_the_extension_when_the_flag_is_off() {
    let h = harness(
        &[("demo", "Demo skill")],
        FeatureFlags {
            enable_skills_extension: false,
            ..FeatureFlags::default()
        },
    );
    let response = call(&h.service, "server/discover", None).await;
    let capabilities = &result(&response)["capabilities"];
    assert!(capabilities.get("extensions").is_none(), "{capabilities}");
    assert!(capabilities.get("resources").is_none(), "{capabilities}");
}

#[tokio::test]
async fn disabled_extension_methods_return_method_not_found() {
    let h = harness(
        &[("demo", "Demo skill")],
        FeatureFlags {
            enable_skills_extension: false,
            ..FeatureFlags::default()
        },
    );
    for method in ["skills/list", "skills/get", "resources/directory/read"] {
        let params = if method == "skills/list" {
            json!({})
        } else {
            json!({"uri": "skill://demo/SKILL.md"})
        };
        let response = call(&h.service, method, Some(params)).await;
        assert_eq!(
            error_code(&response),
            error_codes::METHOD_NOT_FOUND,
            "{method}: {response}"
        );
    }
}

// ── skills/list ────────────────────────────────────────────────────────────

#[tokio::test]
async fn skills_list_returns_a_cacheable_complete_result() {
    let h = default_harness(&[("demo", "Demo skill")]);
    let response = call(&h.service, "skills/list", Some(json!({}))).await;
    let result = result(&response);

    assert_eq!(result["resultType"], "complete");
    assert!(result["ttlMs"].is_u64(), "{result}");
    assert_eq!(result["cacheScope"], "private");
    assert!(result["_meta"][SERVER_INFO_META_KEY].is_object());
    // A single page carries no cursor.
    assert!(result.get("nextCursor").is_none(), "{result}");
}

#[tokio::test]
async fn skills_list_entries_carry_uri_verbatim_frontmatter_and_manifest() {
    let tree = SkillTree::new();
    tree.skill("pdf-processing", "Extract and assemble PDFs");
    tree.file("pdf-processing", "templates/invoice.md", "# Invoice\n");
    tree.file("pdf-processing", "scripts/extract.py", "print(1)\n");

    let h = harness_with_tree(tree, &[("pdf-processing", "Extract and assemble PDFs")]);
    let skill = h.skill_path("pdf-processing");
    let response = call(&h.service, "skills/list", Some(json!({}))).await;
    let skills = &result(&response)["skills"];
    assert_eq!(skills.as_array().expect("array").len(), 1);

    let entry = &skills[0];
    assert_eq!(entry["uri"], "skill://pdf-processing/SKILL.md");

    // Frontmatter is verbatim: `license` is not part of the dcc-mcp model but
    // was written by the skill author, so it must survive.
    assert_eq!(entry["frontmatter"]["name"], "pdf-processing");
    assert_eq!(
        entry["frontmatter"]["description"],
        "Extract and assemble PDFs"
    );
    assert_eq!(entry["frontmatter"]["license"], "MIT");

    // The manifest is complete: SKILL.md plus every supporting file.
    let resources = entry["resources"].as_array().expect("manifest array");
    assert_eq!(resources.len(), 3);
    let uris: Vec<&str> = resources
        .iter()
        .map(|r| r["uri"].as_str().expect("uri"))
        .collect();
    assert_eq!(
        uris,
        vec![
            "skill://pdf-processing/SKILL.md",
            "skill://pdf-processing/scripts/extract.py",
            "skill://pdf-processing/templates/invoice.md",
        ]
    );
    // Digests describe the exact bytes on disk.
    let skill_md = std::fs::read(skill.join("SKILL.md")).expect("read");
    let own = resources
        .iter()
        .find(|r| r["uri"] == "skill://pdf-processing/SKILL.md")
        .expect("SKILL.md entry");
    assert_eq!(own["digest"], digest(&skill_md));
    assert_eq!(own["size"], skill_md.len() as u64);
}

#[tokio::test]
async fn skills_list_paginates_without_splitting_an_entry() {
    let names: Vec<String> = (0..40).map(|i| format!("skill-{i:02}")).collect();
    let tree = SkillTree::new();
    let owned: Vec<(String, String)> = names
        .iter()
        .map(|name| (name.clone(), format!("Skill {name}")))
        .collect();
    for (name, description) in &owned {
        tree.skill(name, description);
    }
    let refs: Vec<(&str, &str)> = owned
        .iter()
        .map(|(n, d)| (n.as_str(), d.as_str()))
        .collect();
    let h = harness_with_tree(tree, &refs);

    let mut seen = Vec::new();
    let mut cursor: Option<String> = None;
    loop {
        let params = match &cursor {
            Some(cursor) => json!({"cursor": cursor}),
            None => json!({}),
        };
        let response = call(&h.service, "skills/list", Some(params)).await;
        let page = result(&response);
        for entry in page["skills"].as_array().expect("array") {
            seen.push(entry["uri"].as_str().expect("uri").to_string());
        }
        cursor = page["nextCursor"].as_str().map(str::to_string);
        if cursor.is_none() {
            break;
        }
    }
    // Every skill appears exactly once across pages, in sorted order.
    assert_eq!(seen.len(), 40, "paging lost or duplicated entries");
    let mut sorted = seen.clone();
    sorted.sort();
    assert_eq!(seen, sorted, "pages must be stable and ordered");
    assert_eq!(seen[0], "skill://skill-00/SKILL.md");
}

#[tokio::test]
async fn skills_list_rejects_a_cursor_past_the_end() {
    let h = default_harness(&[("demo", "Demo skill")]);
    let response = call(
        &h.service,
        "skills/list",
        Some(json!({"cursor": dcc_mcp_jsonrpc::encode_cursor(999)})),
    )
    .await;
    assert_eq!(error_code(&response), error_codes::INVALID_PARAMS);
}

#[tokio::test]
async fn skills_list_skills_over_the_per_skill_limits_are_not_published() {
    let tree = SkillTree::new();
    let skill = tree.skill("huge", "Too many files");
    for index in 0..content::MAX_SKILL_RESOURCE_ENTRIES {
        std::fs::write(skill.join(format!("file-{index}.md")), "x").expect("file");
    }
    let h = harness_with_tree(tree, &[("huge", "Too many files")]);

    let response = call(&h.service, "skills/list", Some(json!({}))).await;
    assert_eq!(result(&response)["skills"], json!([]));
}

// ── skills/get ─────────────────────────────────────────────────────────────

#[tokio::test]
async fn skills_get_answers_independently_of_any_listing() {
    let h = default_harness(&[("demo", "Demo skill")]);
    // No `skills/list` call happened before this one.
    let response = call(
        &h.service,
        "skills/get",
        Some(json!({"uri": "skill://demo/SKILL.md"})),
    )
    .await;
    let skill = &result(&response)["skill"];
    assert_eq!(skill["uri"], "skill://demo/SKILL.md");
    assert_eq!(skill["frontmatter"]["name"], "demo");
    assert_eq!(result(&response)["resultType"], "complete");
    assert!(result(&response)["ttlMs"].is_u64());
    assert_eq!(result(&response)["cacheScope"], "private");
}

#[tokio::test]
async fn skills_get_returns_invalid_params_for_an_unknown_uri() {
    let h = default_harness(&[("demo", "Demo skill")]);
    for uri in [
        "skill://missing/SKILL.md",
        "skill://demo/SKILL.md/../escape.md",
        "file:///etc/passwd",
        "not-a-uri",
    ] {
        let response = call(&h.service, "skills/get", Some(json!({"uri": uri}))).await;
        assert_eq!(
            error_code(&response),
            error_codes::INVALID_PARAMS,
            "{uri}: {response}"
        );
    }
}

#[tokio::test]
async fn skills_get_rejects_a_uri_that_is_not_a_skill_md() {
    let tree = SkillTree::new();
    tree.skill("demo", "Demo skill");
    tree.file("demo", "references/GUIDE.md", "guide\n");
    let h = harness_with_tree(tree, &[("demo", "Demo skill")]);

    let response = call(
        &h.service,
        "skills/get",
        Some(json!({"uri": "skill://demo/references/GUIDE.md"})),
    )
    .await;
    assert_eq!(error_code(&response), error_codes::INVALID_PARAMS);
}

#[tokio::test]
async fn skills_get_survives_a_missing_params_object() {
    let h = default_harness(&[("demo", "Demo skill")]);
    let mut request = request("skills/get", Some(json!({})));
    request.params = Some(json!({
        "_meta": {
            dcc_mcp_jsonrpc::PROTOCOL_VERSION_META_KEY: "2026-07-28",
            dcc_mcp_jsonrpc::CLIENT_CAPABILITIES_META_KEY: {}
        }
    }));
    let response = h
        .service
        .handle_request(&request)
        .await
        .expect("request has an id");
    assert_eq!(error_code(&response), error_codes::INVALID_PARAMS);
}

// ── resources/read ─────────────────────────────────────────────────────────

#[tokio::test]
async fn resources_read_content_matches_the_advertised_digest_and_size() {
    let tree = SkillTree::new();
    tree.skill("demo", "Demo skill");
    tree.file("demo", "references/GUIDE.md", "guide content\n");
    let h = harness_with_tree(tree, &[("demo", "Demo skill")]);

    for uri in ["skill://demo/SKILL.md", "skill://demo/references/GUIDE.md"] {
        // Read the entry first so the digest comes from the listing path.
        let listed = call(&h.service, "skills/list", Some(json!({}))).await;
        let entry = result(&listed)["skills"]
            .as_array()
            .expect("array")
            .iter()
            .find(|skill| skill["uri"] == "skill://demo/SKILL.md")
            .expect("entry");
        let manifest = entry["resources"]
            .as_array()
            .expect("manifest")
            .iter()
            .find(|resource| resource["uri"] == uri)
            .expect("manifest entry");

        let response = call(&h.service, "resources/read", Some(json!({"uri": uri}))).await;
        let contents = &result(&response)["contents"][0];
        assert_eq!(contents["uri"], uri);

        let text = contents["text"].as_str().expect("text").as_bytes().to_vec();
        assert_eq!(
            text.len() as u64,
            manifest["size"].as_u64().expect("size"),
            "size mismatch for {uri}"
        );
        assert_eq!(
            digest(&text),
            manifest["digest"],
            "digest mismatch for {uri}"
        );
    }
}

#[tokio::test]
async fn resources_read_marks_skill_md_as_markdown() {
    let h = default_harness(&[("demo", "Demo skill")]);
    let response = call(
        &h.service,
        "resources/read",
        Some(json!({"uri": "skill://demo/SKILL.md"})),
    )
    .await;
    let contents = &result(&response)["contents"][0];
    assert_eq!(contents["mimeType"], "text/markdown");
    assert!(contents["text"].as_str().unwrap().starts_with("---\nname:"));
}

#[tokio::test]
async fn resources_read_returns_invalid_params_for_unknown_skill_files() {
    let h = default_harness(&[("demo", "Demo skill")]);
    for uri in [
        "skill://demo/missing.md",
        "skill://missing/SKILL.md",
        "skill://demo/../../outside.md",
    ] {
        let response = call(&h.service, "resources/read", Some(json!({"uri": uri}))).await;
        assert_eq!(
            error_code(&response),
            error_codes::INVALID_PARAMS,
            "{uri}: {response}"
        );
    }
}

#[tokio::test]
async fn resources_read_directs_directory_uris_to_directory_read() {
    let tree = SkillTree::new();
    tree.skill("demo", "Demo skill");
    tree.file("demo", "templates/a.md", "a\n");
    let h = harness_with_tree(tree, &[("demo", "Demo skill")]);

    let response = call(
        &h.service,
        "resources/read",
        Some(json!({"uri": "skill://demo/templates"})),
    )
    .await;
    assert_eq!(error_code(&response), error_codes::INVALID_PARAMS);
    assert!(
        response["error"]["message"]
            .as_str()
            .unwrap()
            .contains("resources/directory/read"),
        "expected a pointer to directory read, got: {response}"
    );
}

#[tokio::test]
async fn resources_read_base64_encodes_non_utf8_skill_files() {
    let tree = SkillTree::new();
    tree.skill("demo", "Demo skill");
    let path = tree.root().join("demo").join("blob.bin");
    std::fs::write(path, [0xff_u8, 0xfe, 0x00, 0x01]).expect("binary file");

    let h = harness_with_tree(tree, &[("demo", "Demo skill")]);
    let response = call(
        &h.service,
        "resources/read",
        Some(json!({"uri": "skill://demo/blob.bin"})),
    )
    .await;
    let contents = &result(&response)["contents"][0];
    assert!(contents.get("text").is_none(), "{contents}");
    let blob = contents["blob"].as_str().expect("blob");
    use base64::Engine as _;
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(blob)
        .expect("valid base64");
    assert_eq!(decoded, vec![0xff_u8, 0xfe, 0x00, 0x01]);
}

// ── resources/directory/read ───────────────────────────────────────────────

/// A symlink to a file inside the skill is part of the manifest and readable
/// via `resources/read`, so the directory listing must not hide it. A symlink
/// to a directory must still be reported as a directory, not as a file.
#[cfg(unix)]
#[tokio::test]
async fn directory_read_lists_symlinked_files_that_the_manifest_contains() {
    let tree = SkillTree::new();
    let root = tree.skill("demo", "Demo skill");
    tree.file("demo", "real.md", "real\n");
    tree.file("demo", "scripts/run.py", "print(1)\n");
    std::os::unix::fs::symlink(root.join("real.md"), root.join("alias.md"))
        .expect("symlink to a file inside the skill");
    std::os::unix::fs::symlink(root.join("scripts"), root.join("scripts-link"))
        .expect("symlink to a directory inside the skill");

    let h = harness_with_tree(tree, &[("demo", "Demo skill")]);

    // The manifest advertises the symlinked file.
    let listed = call(&h.service, "skills/list", Some(json!({}))).await;
    let uris: Vec<&str> = result(&listed)["skills"][0]["resources"]
        .as_array()
        .expect("manifest")
        .iter()
        .map(|r| r["uri"].as_str().expect("uri"))
        .collect();
    assert!(uris.contains(&"skill://demo/alias.md"), "{uris:?}");

    // So the directory listing must include it too.
    let response = call(
        &h.service,
        "resources/directory/read",
        Some(json!({"uri": "skill://demo"})),
    )
    .await;
    let children = result(&response)["resources"].as_array().expect("array");
    let by_uri: std::collections::HashMap<&str, &str> = children
        .iter()
        .map(|child| {
            (
                child["uri"].as_str().expect("uri"),
                child["mimeType"].as_str().expect("mimeType"),
            )
        })
        .collect();
    assert_eq!(
        by_uri.get("skill://demo/alias.md"),
        Some(&"text/markdown"),
        "symlinked file must be listed: {by_uri:?}"
    );
    // A symlink to a directory is not followed anywhere: the manifest skips
    // it and `resources/read` cannot serve it, so the listing must omit it
    // too rather than advertise a file that resolves to nothing.
    assert!(
        !by_uri.contains_key("skill://demo/scripts-link"),
        "symlinked directories are not part of the skill: {by_uri:?}"
    );
    assert!(
        !uris.contains(&"skill://demo/scripts-link"),
        "manifest must skip symlinked directories: {uris:?}"
    );
    // The real directory is still listed.
    assert_eq!(
        by_uri.get("skill://demo/scripts"),
        Some(&"inode/directory"),
        "the real directory stays a directory: {by_uri:?}"
    );
}

#[tokio::test]
async fn directory_read_lists_direct_children_only() {
    let tree = SkillTree::new();
    tree.skill("demo", "Demo skill");
    tree.file("demo", "templates/invoice.md", "i\n");
    tree.file("demo", "templates/regional/eu.md", "eu\n");
    tree.file("demo", "scripts/run.py", "print(1)\n");
    let h = harness_with_tree(tree, &[("demo", "Demo skill")]);

    let response = call(
        &h.service,
        "resources/directory/read",
        Some(json!({"uri": "skill://demo/templates"})),
    )
    .await;
    let children = &result(&response)["resources"];
    let uris: Vec<&str> = children
        .as_array()
        .expect("array")
        .iter()
        .map(|child| child["uri"].as_str().expect("uri"))
        .collect();
    assert_eq!(
        uris,
        vec![
            "skill://demo/templates/invoice.md",
            "skill://demo/templates/regional",
        ]
    );
    // Subdirectories are directory resources; the listing is not recursive.
    let regional = &children
        .as_array()
        .unwrap()
        .iter()
        .find(|child| child["uri"] == "skill://demo/templates/regional")
        .expect("regional");
    assert_eq!(regional["mimeType"], "inode/directory");
    assert_eq!(regional["name"], "regional");
}

#[tokio::test]
async fn directory_read_accepts_the_skill_root() {
    let tree = SkillTree::new();
    tree.skill("demo", "Demo skill");
    tree.file("demo", "templates/invoice.md", "i\n");
    let h = harness_with_tree(tree, &[("demo", "Demo skill")]);

    let response = call(
        &h.service,
        "resources/directory/read",
        Some(json!({"uri": "skill://demo"})),
    )
    .await;
    let children = result(&response)["resources"].as_array().expect("array");
    let uris: Vec<&str> = children
        .iter()
        .map(|child| child["uri"].as_str().expect("uri"))
        .collect();
    assert_eq!(
        uris,
        vec!["skill://demo/SKILL.md", "skill://demo/templates"]
    );
}

#[tokio::test]
async fn directory_read_returns_invalid_params_for_non_directories() {
    let h = default_harness(&[("demo", "Demo skill")]);
    for uri in [
        "skill://demo/SKILL.md",
        "skill://demo/nope",
        "skill://missing/templates",
        "scene://blender/current",
    ] {
        let response = call(
            &h.service,
            "resources/directory/read",
            Some(json!({"uri": uri})),
        )
        .await;
        assert_eq!(
            error_code(&response),
            error_codes::INVALID_PARAMS,
            "{uri}: {response}"
        );
    }
}

// ── Dual-track consistency ─────────────────────────────────────────────────

#[tokio::test]
async fn both_tracks_describe_the_same_skills() {
    let h = default_harness(&[("alpha", "Alpha skill"), ("beta", "Beta skill")]);

    // Track one: the skills extension.
    let listed = call(&h.service, "skills/list", Some(json!({}))).await;
    let mut extension_names: Vec<String> = result(&listed)["skills"]
        .as_array()
        .expect("array")
        .iter()
        .map(|skill| {
            skill["frontmatter"]["name"]
                .as_str()
                .expect("name")
                .to_string()
        })
        .collect();
    extension_names.sort();

    // Track two: the existing `list_skills` tool.
    let tools = call(
        &h.service,
        "tools/call",
        Some(json!({"name": "list_skills", "arguments": {}})),
    )
    .await;
    let payload: Value = serde_json::from_str(
        result(&tools)["content"][0]["text"]
            .as_str()
            .expect("tool text"),
    )
    .expect("tool payload");
    let mut tool_names: Vec<String> = payload["skills"]
        .as_array()
        .expect("array")
        .iter()
        .map(|skill| skill["name"].as_str().expect("name").to_string())
        .collect();
    tool_names.sort();

    assert_eq!(
        extension_names, tool_names,
        "skills/list and list_skills must describe the same catalog"
    );
    assert_eq!(extension_names, vec!["alpha", "beta"]);
}

#[tokio::test]
async fn both_tracks_agree_on_skill_content() {
    let tree = SkillTree::new();
    tree.skill("alpha", "Alpha skill");
    tree.file("alpha", "references/GUIDE.md", "guide\n");
    let h = harness_with_tree(tree, &[("alpha", "Alpha skill")]);
    let skill_dir = h.skill_path("alpha");

    // The tools track exposes the SKILL.md body through `get_skill_info`.
    let info = call(
        &h.service,
        "tools/call",
        Some(json!({"name": "get_skill_info", "arguments": {"skill_name": "alpha"}})),
    )
    .await;
    let payload: Value = serde_json::from_str(
        result(&info)["content"][0]["text"]
            .as_str()
            .expect("tool text"),
    )
    .expect("tool payload");
    let on_disk = std::fs::read(skill_dir.join("SKILL.md")).expect("read SKILL.md");
    assert_eq!(
        digest(&on_disk),
        digest(payload["markdown"].as_str().unwrap().as_bytes())
    );

    // The extension track exposes the same bytes with the same digest.
    let got = call(
        &h.service,
        "skills/get",
        Some(json!({"uri": "skill://alpha/SKILL.md"})),
    )
    .await;
    let manifest = &result(&got)["skill"]["resources"][0];
    assert_eq!(manifest["uri"], "skill://alpha/SKILL.md");
    assert_eq!(manifest["digest"], digest(&on_disk));
    assert_eq!(manifest["size"], on_disk.len() as u64);
}

// ── Helpers ────────────────────────────────────────────────────────────────

/// Build a harness around a skill tree the caller has already populated.
///
/// The tree is moved into the returned harness so the temporary directory
/// outlives every request issued against the service.
fn harness_with_tree(tree: SkillTree, skills: &[(&str, &str)]) -> Harness {
    let registry = Arc::new(ToolRegistry::new());
    let dispatcher = Arc::new(ToolDispatcher::new((*registry).clone()));
    let catalog = Arc::new(SkillCatalog::new_with_dispatcher(
        Arc::clone(&registry),
        Arc::clone(&dispatcher),
    ));
    for (name, description) in skills {
        let path = tree.root().join(name);
        catalog.add_skill(SkillMetadata {
            name: (*name).to_string(),
            description: (*description).to_string(),
            skill_path: path.to_string_lossy().to_string(),
            ..SkillMetadata::default()
        });
    }
    let state = ServerState::builder(registry, dispatcher, Arc::clone(&catalog))
        .with_features(FeatureFlags::default())
        .build();
    let context = Arc::new(RegistryContext {
        resource_provider: None,
        prompt_provider: None,
        readiness: Arc::new(StaticReadiness::fully_ready()),
        on_skill_catalog_mutated: Arc::new(|| {}),
    });
    Harness {
        service: StatelessMcpService::new(state, context),
        tree,
    }
}

/// Guard against accidental regressions in the error envelope shape.
#[test]
fn skill_errors_use_the_json_rpc_envelope() {
    let response = JsonRpcResponse::error(
        Some(json!(1)),
        error_codes::INVALID_PARAMS,
        "No skill is served at skill://x/SKILL.md",
    );
    let value = serde_json::to_value(response).expect("envelope");
    assert_eq!(value["jsonrpc"], "2.0");
    assert_eq!(value["error"]["code"], -32602);
    assert!(value.get("result").is_none());
}
