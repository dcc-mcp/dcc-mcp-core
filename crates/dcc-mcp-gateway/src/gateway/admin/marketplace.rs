//! Marketplace admin API handlers — PIP-521.
//!
//! Exposes four endpoints under `/admin/api/marketplace/`:
//! - `GET  /catalog`   — list available packages from marketplace sources
//! - `GET  /installed` — list installed packages
//! - `POST /install`   — install a package
//! - `POST /uninstall` — uninstall a package

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::skill_reload::reload_skill_paths_and_refresh_backends;
use super::state::AdminState;
use crate::gateway::capability::RefreshReason;

/// Official marketplace catalog URL.
const OFFICIAL_MARKETPLACE_SOURCE: &str =
    "https://raw.githubusercontent.com/dcc-mcp/marketplace/main/marketplace.json";

/// Default fetch timeout for marketplace catalog sources.
const FETCH_TIMEOUT: Duration = Duration::from_secs(20);

// ── Response types (mirror frontend admin-types.ts) ────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketplaceEntryResponse {
    pub name: String,
    pub description: String,
    pub dcc: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    pub tags: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_core_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub maintainer: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub install: Option<InstallMetadataResponse>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallMetadataResponse {
    #[serde(rename = "type")]
    pub install_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "ref")]
    pub ref_: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledPackageResponse {
    pub name: String,
    pub dcc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    pub path: String,
    pub source_name: String,
    pub source_url: String,
    pub install_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub install_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub install_ref: Option<String>,
    pub installed_at_ms: u128,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallResultResponse {
    pub installed: bool,
    pub name: String,
    pub dcc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    pub path: String,
    pub skill_search_path: String,
    pub install_type: String,
    pub reload_required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UninstallResultResponse {
    pub uninstalled: bool,
    pub name: String,
    pub dcc: String,
    pub path: String,
    pub removed_state: bool,
    pub removed_files: bool,
    pub reload_required: bool,
}

#[derive(Debug, Deserialize)]
pub struct InstallRequestBody {
    pub name: String,
    pub dcc: String,
    #[serde(default)]
    pub source: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UninstallRequestBody {
    pub name: String,
    pub dcc: String,
}

// ── Installed state persistence ──────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct InstalledState {
    #[serde(default)]
    packages: Vec<InstalledPackageResponse>,
}

fn installed_state_path() -> PathBuf {
    marketplace_root().join("installed.json")
}

fn marketplace_root() -> PathBuf {
    if let Ok(root) = std::env::var("DCC_MCP_MARKETPLACE_INSTALL_ROOT")
        && !root.trim().is_empty()
    {
        return PathBuf::from(root);
    }
    home_dir().join(".dcc-mcp").join("marketplace")
}

fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn load_installed_state() -> Result<InstalledState, String> {
    let path = installed_state_path();
    if !path.exists() {
        return Ok(InstalledState::default());
    }
    let text = fs::read_to_string(&path)
        .map_err(|e| format!("read installed state at {}: {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("parse installed state: {e}"))
}

fn save_installed_state(state: &InstalledState) -> Result<(), String> {
    let path = installed_state_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("create marketplace dir: {e}"))?;
    }
    let text = serde_json::to_string_pretty(state).map_err(|e| format!("serialize state: {e}"))?;
    fs::write(&path, text).map_err(|e| format!("write installed state: {e}"))
}

// ── Handlers ─────────────────────────────────────────────────────────

/// `GET /admin/api/marketplace/catalog`
///
/// Fetches entries from the built-in marketplace catalog source and any
/// additional sources configured via env.
pub async fn handle_marketplace_catalog(State(s): State<AdminState>) -> impl IntoResponse {
    match fetch_catalog_entries(&s.gateway.http_client).await {
        Ok(entries) => Json(json!({ "entries": entries })).into_response(),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": err })),
        )
            .into_response(),
    }
}

/// `GET /admin/api/marketplace/installed`
pub async fn handle_marketplace_installed(State(_s): State<AdminState>) -> impl IntoResponse {
    match load_installed_state() {
        Ok(state) => Json(json!({ "packages": state.packages })).into_response(),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": err })),
        )
            .into_response(),
    }
}

/// `POST /admin/api/marketplace/install`
///
/// Body: `{ "name": "...", "dcc": "...", "source?": "..." }`
pub async fn handle_marketplace_install(
    State(s): State<AdminState>,
    Json(body): Json<InstallRequestBody>,
) -> impl IntoResponse {
    match install_package(
        &s.gateway.http_client,
        &body.name,
        &body.dcc,
        body.source.as_deref(),
    )
    .await
    {
        Ok(result) => {
            if result.reload_required {
                reload_skill_paths_and_refresh_backends(&s, RefreshReason::ToolsListChanged).await;
            }
            Json(result).into_response()
        }
        Err(err) => (StatusCode::BAD_REQUEST, Json(json!({ "error": err }))).into_response(),
    }
}

/// `POST /admin/api/marketplace/uninstall`
///
/// Body: `{ "name": "...", "dcc": "..." }`
pub async fn handle_marketplace_uninstall(
    State(s): State<AdminState>,
    Json(body): Json<UninstallRequestBody>,
) -> impl IntoResponse {
    match uninstall_package(&body.name, &body.dcc) {
        Ok(result) => {
            if result.reload_required {
                reload_skill_paths_and_refresh_backends(&s, RefreshReason::ToolsListChanged).await;
            }
            Json(result).into_response()
        }
        Err(err) => (StatusCode::BAD_REQUEST, Json(json!({ "error": err }))).into_response(),
    }
}

// ── Catalog fetching ─────────────────────────────────────────────────

async fn fetch_catalog_entries(
    client: &reqwest::Client,
) -> Result<Vec<MarketplaceEntryResponse>, String> {
    let sources = marketplace_sources();
    let mut all_entries = Vec::new();

    for source in &sources {
        let entries = fetch_source_entries(client, source).await?;
        for entry in entries {
            let response = catalog_entry_to_response(&entry, Some(source));
            all_entries.push(response);
        }
    }

    // Deduplicate by name (first source wins).
    let mut seen = std::collections::HashSet::new();
    all_entries.retain(|e| seen.insert(e.name.clone()));

    Ok(all_entries)
}

fn marketplace_sources() -> Vec<MarketplaceSourceInfo> {
    let mut sources = vec![MarketplaceSourceInfo {
        name: "dcc-mcp/marketplace".to_string(),
        url: OFFICIAL_MARKETPLACE_SOURCE.to_string(),
    }];

    // Additional sources from env: DCC_MCP_MARKETPLACE_SOURCES=org/repo,...
    if let Ok(extra) = std::env::var("DCC_MCP_MARKETPLACE_SOURCES") {
        for raw in extra.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            let url = resolve_source_url(raw);
            if sources.iter().any(|s| s.url == url) {
                continue;
            }
            sources.push(MarketplaceSourceInfo {
                name: raw.to_string(),
                url,
            });
        }
    }

    sources
}

struct MarketplaceSourceInfo {
    name: String,
    url: String,
}

fn resolve_source_url(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.eq_ignore_ascii_case("dcc-mcp/marketplace") {
        return OFFICIAL_MARKETPLACE_SOURCE.to_string();
    }
    if trimmed.starts_with('/') || Path::new(trimmed).is_absolute() {
        return trimmed.to_string();
    }
    let looks_like_slug =
        trimmed.contains('/') && !trimmed.contains("://") && !trimmed.contains('\\');
    if looks_like_slug {
        return format!("https://raw.githubusercontent.com/{trimmed}/main/marketplace.json");
    }
    trimmed.to_string()
}

async fn fetch_source_entries(
    client: &reqwest::Client,
    source: &MarketplaceSourceInfo,
) -> Result<Vec<dcc_mcp_catalog::CatalogEntry>, String> {
    let text = if source.url.starts_with("http://") || source.url.starts_with("https://") {
        client
            .get(&source.url)
            .header("User-Agent", "dcc-mcp-gateway marketplace")
            .timeout(FETCH_TIMEOUT)
            .send()
            .await
            .map_err(|e| format!("fetch {}: {e}", source.url))?
            .error_for_status()
            .map_err(|e| format!("fetch {}: {e}", source.url))?
            .text()
            .await
            .map_err(|e| format!("read {}: {e}", source.url))?
    } else {
        let path = source
            .url
            .strip_prefix("file://")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(&source.url));
        fs::read_to_string(&path).map_err(|e| format!("read {}: {e}", path.display()))?
    };

    dcc_mcp_catalog::load_from_str(&text).map_err(|e| format!("parse {}: {e}", source.url))
}

fn catalog_entry_to_response(
    entry: &dcc_mcp_catalog::CatalogEntry,
    source: Option<&MarketplaceSourceInfo>,
) -> MarketplaceEntryResponse {
    MarketplaceEntryResponse {
        name: entry.name.clone(),
        description: entry.description.clone(),
        dcc: entry.dcc.clone(),
        url: entry.url.clone(),
        tags: entry.tags.clone(),
        version: entry.version.clone(),
        min_core_version: entry.min_core_version.clone(),
        maintainer: entry.maintainer.clone(),
        source_name: source.map(|s| s.name.clone()),
        source_url: source.map(|s| s.url.clone()),
        install: entry.install.as_ref().map(|i| InstallMetadataResponse {
            install_type: i.install_type.clone(),
            url: i.url.clone(),
            ref_: i.ref_.clone(),
        }),
    }
}

// ── Package install / uninstall ──────────────────────────────────────

async fn install_package(
    client: &reqwest::Client,
    name: &str,
    dcc: &str,
    explicit_source: Option<&str>,
) -> Result<InstallResultResponse, String> {
    let safe_name = sanitise_path_component("package name", name)?;
    let safe_dcc = sanitise_path_component("DCC name", dcc)?.to_lowercase();

    // Fetch catalog and find the matching entry.
    let sources = if let Some(raw) = explicit_source {
        vec![MarketplaceSourceInfo {
            name: raw.to_string(),
            url: resolve_source_url(raw),
        }]
    } else {
        marketplace_sources()
    };

    let mut matched_entry: Option<dcc_mcp_catalog::CatalogEntry> = None;
    let mut matched_source: Option<MarketplaceSourceInfo> = None;

    for source in &sources {
        let entries = fetch_source_entries(client, source).await?;
        if let Some(entry) = entries.into_iter().find(|e| e.name == safe_name)
            && entry_targets_dcc(&entry, &safe_dcc)
        {
            matched_entry = Some(entry);
            matched_source = Some(MarketplaceSourceInfo {
                name: source.name.clone(),
                url: source.url.clone(),
            });
            break;
        }
    }

    let entry = matched_entry
        .ok_or_else(|| format!("package '{safe_name}' not found for DCC '{safe_dcc}'"))?;
    let source = matched_source.unwrap();

    let install = entry
        .install
        .as_ref()
        .ok_or_else(|| format!("package '{safe_name}' has no install metadata"))?;

    let dcc_root = marketplace_root().join(&safe_dcc);
    let dest = dcc_root.join(&safe_name);

    if dest.exists() {
        return Err(format!(
            "package '{safe_name}' is already installed for DCC '{safe_dcc}' at {}",
            dest.display()
        ));
    }

    fs::create_dir_all(&dcc_root)
        .map_err(|e| format!("create dcc dir {}: {e}", dcc_root.display()))?;

    let staging = dcc_root.join(format!(".{safe_name}.installing-{}", now_ms()));
    if staging.exists() {
        let _ = remove_path(&staging);
    }

    // Execute the install based on type.
    let install_result = match install.install_type.as_str() {
        "git" => install_from_git(install, &staging),
        "path" => install_from_path(install, &staging),
        "zip" => {
            return Err("install type 'zip' is not supported yet".to_string());
        }
        other => {
            return Err(format!("unsupported install type: {other}"));
        }
    };

    if let Err(err) = install_result {
        let _ = remove_path(&staging);
        return Err(err);
    }

    // Verify SKILL.md exists in the installed package.
    let skill_md = staging.join("SKILL.md");
    if !skill_md.is_file() {
        let _ = remove_path(&staging);
        return Err(format!(
            "installed package does not contain SKILL.md at {}",
            skill_md.display()
        ));
    }

    fs::rename(&staging, &dest).map_err(|e| format!("rename staging to dest: {e}"))?;

    // Record in installed state.
    let mut state = load_installed_state()?;
    state
        .packages
        .retain(|p| !(p.name == safe_name && p.dcc == safe_dcc));
    let pkg = InstalledPackageResponse {
        name: safe_name.clone(),
        dcc: safe_dcc.clone(),
        version: entry.version.clone(),
        path: dest.display().to_string(),
        source_name: source.name.clone(),
        source_url: source.url.clone(),
        install_type: install.install_type.clone(),
        install_url: install.url.clone(),
        install_ref: install.ref_.clone(),
        installed_at_ms: now_ms(),
    };
    state.packages.push(pkg);
    save_installed_state(&state)?;

    Ok(InstallResultResponse {
        installed: true,
        name: safe_name,
        dcc: safe_dcc,
        version: entry.version,
        path: dest.display().to_string(),
        skill_search_path: dcc_root.display().to_string(),
        install_type: install.install_type.clone(),
        reload_required: true,
    })
}

fn uninstall_package(name: &str, dcc: &str) -> Result<UninstallResultResponse, String> {
    let safe_name = sanitise_path_component("package name", name)?;
    let safe_dcc = sanitise_path_component("DCC name", dcc)?.to_lowercase();

    let dcc_root = marketplace_root().join(&safe_dcc);
    let dest = dcc_root.join(&safe_name);

    let removed_files = if dest.exists() {
        remove_path(&dest)?;
        true
    } else {
        false
    };

    let mut state = load_installed_state()?;
    let before = state.packages.len();
    state
        .packages
        .retain(|p| !(p.name == safe_name && p.dcc == safe_dcc));
    let removed_state = state.packages.len() != before;
    if removed_state {
        save_installed_state(&state)?;
    }

    Ok(UninstallResultResponse {
        uninstalled: removed_files || removed_state,
        name: safe_name,
        dcc: safe_dcc,
        path: dest.display().to_string(),
        removed_state,
        removed_files,
        reload_required: removed_files || removed_state,
    })
}

// ── Helpers ──────────────────────────────────────────────────────────

fn entry_targets_dcc(entry: &dcc_mcp_catalog::CatalogEntry, dcc: &str) -> bool {
    entry.dcc.iter().any(|v| v.eq_ignore_ascii_case(dcc))
}

fn sanitise_path_component(kind: &str, value: &str) -> Result<String, String> {
    let trimmed = value.trim();
    if trimmed.is_empty()
        || trimmed == "."
        || trimmed == ".."
        || trimmed.starts_with('.')
        || !trimmed
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-'))
    {
        return Err(format!(
            "invalid {kind} '{value}'; use only ASCII letters, numbers, '.', '_' or '-'"
        ));
    }
    Ok(trimmed.to_string())
}

fn install_from_git(install: &dcc_mcp_catalog::CatalogInstall, dest: &Path) -> Result<(), String> {
    let url = install
        .url
        .as_deref()
        .ok_or_else(|| "missing git URL in install metadata".to_string())?;
    let mut command = Command::new("git");
    command.arg("clone").arg("--depth").arg("1");
    if let Some(ref_) = install.ref_.as_deref().filter(|v| !v.trim().is_empty()) {
        command.arg("--branch").arg(ref_);
    }
    command.arg(url).arg(dest);
    let output = command.output().map_err(|e| format!("git clone: {e}"))?;
    if output.status.success() {
        return Ok(());
    }
    Err(format!(
        "git clone exited with {}: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    ))
}

fn install_from_path(install: &dcc_mcp_catalog::CatalogInstall, dest: &Path) -> Result<(), String> {
    let url = install
        .url
        .as_deref()
        .ok_or_else(|| "missing path URL in install metadata".to_string())?;
    let src = url
        .strip_prefix("file://")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(url));
    if !src.join("SKILL.md").is_file() {
        return Err(format!(
            "source path does not contain SKILL.md at {}",
            src.display()
        ));
    }
    copy_dir_recursive(&src, dest)
}

fn copy_dir_recursive(src: &Path, dest: &Path) -> Result<(), String> {
    fs::create_dir_all(dest).map_err(|e| format!("create dir {}: {e}", dest.display()))?;
    for entry in fs::read_dir(src).map_err(|e| format!("read dir {}: {e}", src.display()))? {
        let entry = entry.map_err(|e| format!("read entry in {}: {e}", src.display()))?;
        let src_path = entry.path();
        let dest_path = dest.join(entry.file_name());
        let file_type = entry
            .file_type()
            .map_err(|e| format!("file type for {}: {e}", src_path.display()))?;
        if file_type.is_dir() {
            copy_dir_recursive(&src_path, &dest_path)?;
        } else if file_type.is_file() {
            fs::copy(&src_path, &dest_path).map_err(|e| {
                format!(
                    "copy {} -> {}: {e}",
                    src_path.display(),
                    dest_path.display()
                )
            })?;
        }
    }
    Ok(())
}

fn remove_path(path: &Path) -> Result<(), String> {
    if path.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
    .map_err(|e| format!("remove {}: {e}", path.display()))
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitise_rejects_empty() {
        assert!(sanitise_path_component("name", "").is_err());
    }

    #[test]
    fn sanitise_rejects_dot_dot() {
        assert!(sanitise_path_component("name", "..").is_err());
    }

    #[test]
    fn sanitise_rejects_special_chars() {
        assert!(sanitise_path_component("name", "bad/name").is_err());
    }

    #[test]
    fn sanitise_allows_valid_name() {
        let result = sanitise_path_component("name", "my-package_v1.0").unwrap();
        assert_eq!(result, "my-package_v1.0");
    }

    #[test]
    fn sanitise_trims_whitespace() {
        let result = sanitise_path_component("name", "  hello  ").unwrap();
        assert_eq!(result, "hello");
    }

    #[test]
    fn resolve_source_url_handles_official() {
        let url = resolve_source_url("dcc-mcp/marketplace");
        assert_eq!(url, OFFICIAL_MARKETPLACE_SOURCE);
    }

    #[test]
    fn resolve_source_url_handles_github_slug() {
        let url = resolve_source_url("studio/private");
        assert_eq!(
            url,
            "https://raw.githubusercontent.com/studio/private/main/marketplace.json"
        );
    }

    #[test]
    fn resolve_source_url_passes_through_urls() {
        let url = resolve_source_url("https://example.com/catalog.json");
        assert_eq!(url, "https://example.com/catalog.json");
    }

    #[test]
    fn resolve_source_url_passes_through_absolute_paths() {
        let url = resolve_source_url("/tmp/catalog.json");
        assert_eq!(url, "/tmp/catalog.json");
    }

    #[test]
    fn entry_targets_dcc_matches_case_insensitive() {
        let entry = dcc_mcp_catalog::CatalogEntry {
            name: "test".into(),
            description: "desc".into(),
            dcc: vec!["maya".into(), "blender".into()],
            url: None,
            tags: vec![],
            version: None,
            min_core_version: None,
            install: None,
            maintainer: None,
        };
        assert!(entry_targets_dcc(&entry, "Maya"));
        assert!(entry_targets_dcc(&entry, "BLENDER"));
        assert!(!entry_targets_dcc(&entry, "houdini"));
    }
}
