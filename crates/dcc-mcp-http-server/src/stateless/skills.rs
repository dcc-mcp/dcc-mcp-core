//! `io.modelcontextprotocol/skills` handlers for the stateless path.
//!
//! Implements the Skills extension (ext-skills stable) on top of the existing
//! skill catalog, so `skills/list` / `skills/get` and the `*_skills` tools
//! describe the same skills from the same source of truth.
//!
//! Manifests are built from the bytes on disk on every call — the digest a
//! host verifies against is always the digest of what `resources/read` will
//! return. See `dcc-mcp-skills::skill_content` for the manifest builder.
//!
//! Reference: <https://github.com/modelcontextprotocol/ext-skills/blob/main/specification/stable/skills.mdx>

use std::path::{Path, PathBuf};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use dcc_mcp_jsonrpc::{
    GetSkillParams, GetSkillResult, JsonRpcRequest, JsonRpcResponse, ListSkillsResult, McpResource,
    ReadResourceDirectoryParams, ReadResourceDirectoryResult, ReadResourceResult, ResourceContents,
    Skill, SkillResource, SkillResources, encode_cursor, error_codes,
};
use dcc_mcp_skills::skill_content as content;
use serde_json::Value;
use tracing::{debug, warn};

use super::StatelessDispatchOutcome;
use crate::server_state::ServerState;

/// Skills returned per `skills/list` page.
///
/// An entry is atomic, so the page boundary never splits a skill's
/// `resources` set.
const SKILLS_LIST_PAGE_SIZE: usize = 32;

/// Direct children returned per `resources/directory/read` page.
const DIRECTORY_PAGE_SIZE: usize = 64;

/// Whether this server declares the Skills extension.
///
/// The extension is specified against base revision 2026-07-28, so it is only
/// reachable on the stateless path; and it requires the `resources`
/// capability, so a server that has resources switched off cannot declare it.
pub(super) fn extension_enabled(state: &ServerState) -> bool {
    state.features.enable_resources && state.features.enable_skills_extension
}

/// `skills/list` — enumerate the skills this server serves.
pub(super) fn handle_skills_list(
    state: &ServerState,
    request: &JsonRpcRequest,
    id: Value,
) -> StatelessDispatchOutcome {
    let offset = match list_offset(request) {
        Ok(offset) => offset,
        Err(message) => return invalid_params(id, message),
    };
    let skills = published_skills(state);
    if offset > skills.len() {
        return invalid_params(id, "cursor is past the end of the skill listing");
    }
    let end = offset
        .saturating_add(SKILLS_LIST_PAGE_SIZE)
        .min(skills.len());
    let page = &skills[offset..end];
    let next_cursor = (end < skills.len()).then(|| encode_cursor(end));
    ok(
        id,
        ListSkillsResult {
            skills: page.to_vec(),
            next_cursor,
        },
    )
}

/// `skills/get` — return the entry for one skill by `SKILL.md` URI.
///
/// Answers for every skill this server serves, whether or not it appeared in
/// a `skills/list` page.
pub(super) fn handle_skills_get(
    state: &ServerState,
    request: &JsonRpcRequest,
    id: Value,
) -> StatelessDispatchOutcome {
    let params = match parse_params::<GetSkillParams>(request) {
        Ok(params) => params,
        Err(message) => return invalid_params(id, message),
    };
    let skill = match skill_entry_for_uri(state, &params.uri) {
        Ok(Some(skill)) => skill,
        Ok(None) => {
            return invalid_params(id, format!("No skill is served at {}", params.uri.trim()));
        }
        Err(message) => return invalid_params(id, message),
    };
    ok(id, GetSkillResult { skill: Some(skill) })
}

/// `resources/directory/read` — list the direct children of a directory.
///
/// Only served when the extension declares `directoryRead: true`.
pub(super) fn handle_resources_directory_read(
    state: &ServerState,
    request: &JsonRpcRequest,
    id: Value,
) -> StatelessDispatchOutcome {
    let params = match parse_params::<ReadResourceDirectoryParams>(request) {
        Ok(params) => params,
        Err(message) => return invalid_params(id, message),
    };
    let offset = match cursor_offset(params.cursor.as_deref()) {
        Ok(offset) => offset,
        Err(message) => return invalid_params(id, message),
    };
    let (skill_name, relative) = match content::parse_skill_uri(&params.uri) {
        Some(parsed) => parsed,
        None => {
            return invalid_params(
                id,
                format!("{} is not a skill directory URI", params.uri.trim()),
            );
        }
    };
    let skill_dir = match publishable_skill_dir(state, skill_name) {
        Ok(Some(dir)) => dir,
        Ok(None) => return invalid_params(id, format!("No skill is served at {skill_name}")),
        Err(message) => return invalid_params(id, message),
    };
    // An empty relative path addresses the skill root directory itself.
    let directory = if relative.is_empty() {
        skill_dir.clone()
    } else {
        match content::resolve_skill_file(&skill_dir, relative) {
            Some(path) => path,
            None => {
                return invalid_params(
                    id,
                    format!("{} is not a directory resource", params.uri.trim()),
                );
            }
        }
    };
    if !directory.is_dir() {
        return invalid_params(
            id,
            format!("{} is not a directory resource", params.uri.trim()),
        );
    }
    let mut entries = match directory_children(skill_name, &skill_dir, &directory) {
        Ok(entries) => entries,
        Err(message) => return invalid_params(id, message),
    };
    entries.sort_by(|a, b| a.uri.cmp(&b.uri));
    if offset > entries.len() {
        return invalid_params(id, "cursor is past the end of the directory listing");
    }
    let end = offset
        .saturating_add(DIRECTORY_PAGE_SIZE)
        .min(entries.len());
    let page = entries[offset..end].to_vec();
    let next_cursor = (end < entries.len()).then(|| encode_cursor(end));
    ok(
        id,
        ReadResourceDirectoryResult {
            resources: page,
            next_cursor,
        },
    )
}

/// `resources/read` for a `skill://` URI.
///
/// Returns `None` when the request is not a `skill://` read, so the caller can
/// fall back to the shared resource provider. Handling it here rather than in
/// the provider layer keeps the extension's own error messages intact — the
/// provider mapper deliberately reports one generic message for every
/// not-found so internal paths never reach a client.
pub(super) fn handle_resources_read(
    state: &ServerState,
    request: &JsonRpcRequest,
    id: Value,
) -> Option<StatelessDispatchOutcome> {
    if !extension_enabled(state) {
        return None;
    }
    let params = request.params.as_ref()?;
    let uri = params.get("uri").and_then(Value::as_str)?;
    let (skill_name, relative) = content::parse_skill_uri(uri)?;
    Some(match read_skill_file(state, skill_name, relative, uri) {
        Ok(result) => ok(id, result),
        Err(message) => invalid_params(id, message),
    })
}

/// Resource descriptors contributed to `resources/list` by the extension.
///
/// Only the `SKILL.md` of each skill is advertised: it is the entry point the
/// extension defines, and listing every supporting file of every skill would
/// make `resources/list` unusable on a large catalog. Supporting files stay
/// individually addressable by URI.
pub(super) fn list_skill_resources(state: &ServerState) -> Vec<McpResource> {
    if !extension_enabled(state) {
        return Vec::new();
    }
    let mut resources = Vec::new();
    for (name, dir) in catalog_skill_dirs(state) {
        // Share the publishability gate with `skills/list` so the two agree on
        // which skills exist: a skill must not show up here while being absent
        // from the listing. The gate is cheap — it reads only SKILL.md and
        // directory metadata, never the skill's whole content.
        let Ok(frontmatter) = content::check_publishable(&dir, &name) else {
            continue;
        };
        resources.push(McpResource {
            uri: content::skill_uri(&name, dcc_mcp_skills::constants::SKILL_METADATA_FILE),
            name: frontmatter
                .name()
                .map(str::to_string)
                .unwrap_or_else(|| name.clone()),
            description: frontmatter.description().map(str::to_string),
            mime_type: Some(content::SKILL_MD_MIME_TYPE.to_string()),
        });
    }
    resources.sort_by(|a, b| a.uri.cmp(&b.uri));
    resources
}

// ── Manifest construction ──────────────────────────────────────────────────

/// Every skill this server publishes through the extension, sorted by URI.
///
/// Skills whose on-disk content cannot be published (unreadable, malformed
/// frontmatter, or over the extension's per-skill limits) are skipped with a
/// warning rather than served with a manifest that would fail verification.
fn published_skills(state: &ServerState) -> Vec<Skill> {
    let mut skills = Vec::new();
    for (name, dir) in catalog_skill_dirs(state) {
        match content::build_skill_manifest(&dir, &name) {
            Ok(manifest) => skills.push(to_wire_skill(&manifest)),
            Err(error) => warn!(
                skill = %name,
                path = %dir.display(),
                %error,
                "skills: not publishing skill through the skills extension"
            ),
        }
    }
    skills.sort_by(|a, b| a.uri.cmp(&b.uri));
    skills
}

/// Convert a disk-backed manifest into its wire form.
fn to_wire_skill(manifest: &dcc_mcp_skills::SkillManifest) -> Skill {
    let resources = match &manifest.resources {
        content::SkillFileSet::Files(files) => SkillResources::Entries(
            files
                .iter()
                .map(|file| SkillResource {
                    uri: file.uri.clone(),
                    digest: file.digest.clone(),
                    size: file.size,
                })
                .collect(),
        ),
        content::SkillFileSet::Dynamic => SkillResources::Dynamic,
    };
    Skill {
        uri: manifest.uri.clone(),
        frontmatter: manifest.frontmatter.clone().into_object(),
        resources,
    }
}

/// Resolve a `skills/get` URI to its wire entry.
fn skill_entry_for_uri(state: &ServerState, uri: &str) -> Result<Option<Skill>, String> {
    let Some((skill_name, relative)) = content::parse_skill_uri(uri) else {
        return Err(format!("{} is not a skill URI", uri.trim()));
    };
    if relative != dcc_mcp_skills::constants::SKILL_METADATA_FILE {
        return Err(format!(
            "{} is not the URI of a SKILL.md; `skills/get` names a skill by its SKILL.md URI",
            uri.trim()
        ));
    }
    let Some(dir) = publishable_skill_dir(state, skill_name)? else {
        return Ok(None);
    };
    match content::build_skill_manifest(&dir, skill_name) {
        Ok(manifest) => Ok(Some(to_wire_skill(&manifest))),
        Err(error) => Err(format!("skill `{skill_name}` cannot be published: {error}")),
    }
}

/// Read one file out of a skill directory.
fn read_skill_file(
    state: &ServerState,
    skill_name: &str,
    relative: &str,
    uri: &str,
) -> Result<ReadResourceResult, String> {
    let Some(skill_dir) = publishable_skill_dir(state, skill_name)? else {
        return Err(format!("No skill is served at {skill_name}"));
    };
    if relative.is_empty() {
        return Err(format!(
            "{uri} is a directory resource; read it with resources/directory/read"
        ));
    }
    let Some(path) = content::resolve_skill_file(&skill_dir, relative) else {
        return Err(format!("{uri} is not a file of skill `{skill_name}`"));
    };
    if !path.is_file() {
        return Err(format!(
            "{uri} is not a file resource; read it with resources/directory/read"
        ));
    }
    let bytes = std::fs::read(&path).map_err(|error| format!("cannot read {uri}: {error}"))?;
    let mime_type = content::mime_type_for(relative).to_string();
    let mut contents = ResourceContents {
        uri: uri.to_string(),
        mime_type: Some(mime_type),
        text: None,
        blob: None,
    };
    // Digest and size describe raw bytes; UTF-8 files are served as `text`
    // and everything else as a base64 `blob`.
    match String::from_utf8(bytes) {
        Ok(text) => contents.text = Some(text),
        Err(error) => contents.blob = Some(BASE64_STANDARD.encode(error.as_bytes())),
    }
    Ok(ReadResourceResult {
        contents: vec![contents],
    })
}

/// Direct children of `directory` as resource descriptors.
///
/// `skill_dir` and `directory` are both canonical (see
/// [`publishable_skill_dir`] and [`content::resolve_skill_file`]), so the
/// prefix strip that builds each child URI is well defined.
fn directory_children(
    skill_name: &str,
    skill_dir: &Path,
    directory: &Path,
) -> Result<Vec<McpResource>, String> {
    let read_dir = std::fs::read_dir(directory)
        .map_err(|error| format!("cannot read {}: {error}", directory.display()))?;
    let mut entries = Vec::new();
    for entry in read_dir {
        let entry =
            entry.map_err(|error| format!("cannot read {}: {error}", directory.display()))?;
        let path = entry.path();
        let file_name = entry.file_name().to_string_lossy().to_string();
        let file_type = entry
            .file_type()
            .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
        let relative = path
            .strip_prefix(skill_dir)
            .map(|relative| relative.to_string_lossy().replace('\\', "/"))
            .unwrap_or(file_name.clone());
        let uri = content::skill_uri(skill_name, &relative);
        if file_type.is_dir() {
            entries.push(McpResource {
                uri,
                name: file_name,
                description: None,
                mime_type: Some(content::DIRECTORY_MIME_TYPE.to_string()),
            });
            continue;
        }
        // `file_type()` does not follow symlinks, so a symlink-to-file would
        // be skipped here while the manifest still advertises it and
        // `resources/read` still serves it. Resolve through the same
        // containment-checked helper so all three surfaces agree.
        // Containment alone is not enough: it would also admit a
        // symlink-to-directory as a file.
        if !file_type.is_file() {
            let Some(resolved) = content::resolve_skill_file(skill_dir, &relative) else {
                continue;
            };
            if !resolved.is_file() {
                continue;
            }
        }
        entries.push(McpResource {
            uri,
            name: file_name,
            description: None,
            mime_type: Some(content::mime_type_for(&relative).to_string()),
        });
    }
    Ok(entries)
}

// ── Catalog access ─────────────────────────────────────────────────────────

/// `(skill name, skill directory)` for every catalogued skill, sorted by name.
///
/// The catalog is a `DashMap`, so iteration order is arbitrary; sorting keeps
/// pagination cursors stable between calls.
fn catalog_skill_dirs(state: &ServerState) -> Vec<(String, PathBuf)> {
    let mut skills: Vec<(String, PathBuf)> = state
        .catalog
        .list_skills(None)
        .into_iter()
        .filter_map(|summary| {
            state
                .catalog
                .get_skill(&summary.name)
                .map(|metadata| (summary.name, metadata))
        })
        .filter(|(_, metadata)| !metadata.skill_path.is_empty())
        .map(|(name, metadata)| (name, PathBuf::from(metadata.skill_path)))
        .collect();
    skills.sort_by(|a, b| a.0.cmp(&b.0));
    skills
}

/// Resolve a catalogued skill to its **canonical, publishable** directory.
///
/// `Ok(None)` means the catalog has no such skill; `Err` means the skill is
/// catalogued but not publishable. Callers turn both into `-32602`, which is
/// what the extension prescribes for "no skill is served at this URI" —
/// including a skill that exists on disk but cannot be published.
///
/// The path is canonicalized so that later containment checks
/// ([`content::resolve_skill_file`]) and the prefix stripping in
/// [`directory_children`] all work against one stable baseline.
fn publishable_skill_dir(state: &ServerState, skill_name: &str) -> Result<Option<PathBuf>, String> {
    let metadata = match state.catalog.get_skill(skill_name) {
        Some(metadata) => metadata,
        None => return Ok(None),
    };
    if metadata.skill_path.is_empty() {
        return Ok(None);
    }
    let dir = PathBuf::from(metadata.skill_path);
    match content::check_publishable(&dir, skill_name) {
        Ok(_) => dir
            .canonicalize()
            .map(Some)
            .map_err(|error| format!("skill `{skill_name}` is not readable: {error}")),
        Err(error) => Err(format!("skill `{skill_name}` is not published: {error}")),
    }
}

// ── Params / response helpers ──────────────────────────────────────────────

fn parse_params<T: serde::de::DeserializeOwned>(request: &JsonRpcRequest) -> Result<T, String> {
    let params = request
        .params
        .clone()
        .ok_or_else(|| "Missing params".to_string())?;
    serde_json::from_value::<T>(params).map_err(|_| "Invalid params".to_string())
}

fn list_offset(request: &JsonRpcRequest) -> Result<usize, String> {
    let Some(params) = request.params.as_ref() else {
        return Ok(0);
    };
    let Some(cursor) = params.get("cursor") else {
        return Ok(0);
    };
    let Some(cursor) = cursor.as_str() else {
        return Err("cursor must be a string".to_string());
    };
    cursor_offset(Some(cursor))
}

fn cursor_offset(cursor: Option<&str>) -> Result<usize, String> {
    let Some(cursor) = cursor else {
        return Ok(0);
    };
    if cursor.len() > 40 || !cursor.is_ascii() {
        return Err("cursor is not a valid pagination cursor".to_string());
    }
    dcc_mcp_jsonrpc::decode_cursor(cursor)
        .ok_or_else(|| "cursor is not a valid pagination cursor".to_string())
}

fn ok<T: serde::Serialize>(id: Value, result: T) -> StatelessDispatchOutcome {
    match serde_json::to_value(result) {
        Ok(result) => StatelessDispatchOutcome::Response(
            serde_json::to_value(JsonRpcResponse::success(Some(id), result)).unwrap_or(Value::Null),
        ),
        Err(error) => {
            debug!(%error, "skills: result serialization failed");
            StatelessDispatchOutcome::Response(
                serde_json::to_value(JsonRpcResponse::internal_error(
                    Some(id),
                    "Skill result serialization failed",
                ))
                .unwrap_or(Value::Null),
            )
        }
    }
}

fn invalid_params(id: Value, message: impl Into<String>) -> StatelessDispatchOutcome {
    StatelessDispatchOutcome::Response(
        serde_json::to_value(JsonRpcResponse::error(
            Some(id),
            // The extension mandates -32602 for an unknown skill URI and for a
            // `resources/directory/read` URI that is not a directory.
            error_codes::INVALID_PARAMS,
            message.into(),
        ))
        .unwrap_or(Value::Null),
    )
}

#[cfg(test)]
#[path = "skills_tests.rs"]
mod tests;
