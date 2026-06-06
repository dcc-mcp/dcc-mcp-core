//! Shared [`MarketplaceService`] — catalog fetch, install/uninstall, source
//! management, installed state persistence, and integrity verification.

use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use dcc_mcp_catalog::{self, CatalogEntry, CatalogInstall};
use sha2::{Digest, Sha256};

use crate::error::MarketplaceError;
use crate::source::{builtin_source, dedupe_sources, normalise_source};
use crate::types::{
    InstalledMarketplacePackage, MarketplaceHit, MarketplaceInspectResult,
    MarketplaceInstallResult, MarketplaceInstalledList, MarketplaceInstalledState,
    MarketplaceOutdatedList, MarketplaceSearchResult, MarketplaceSource, MarketplaceSourceConfig,
    MarketplaceSourceOrigin, MarketplaceUninstallResult, MarketplaceUpdateResult,
    OutdatedMarketplacePackage, StoredMarketplaceSource, entry_targets_dcc,
};

const ENV_MARKETPLACE_SOURCES: &str = "DCC_MCP_MARKETPLACE_SOURCES";
const ENV_MARKETPLACE_NO_DEFAULT_SOURCES: &str = "DCC_MCP_MARKETPLACE_NO_DEFAULT_SOURCES";

#[derive(Debug, Clone)]
pub struct MarketplaceService {
    /// Root directory for marketplace data (installed packages, state).
    root: PathBuf,
    /// Optional path to the sources.json config file.
    config_path: Option<PathBuf>,
    /// HTTP client for fetching catalog entries and archives.
    client: reqwest::Client,
}

impl MarketplaceService {
    /// Create a new service rooted at `root`.
    ///
    /// `root` is typically `~/.dcc-mcp/marketplace`.
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            config_path: None,
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(20))
                .build()
                .expect("reqwest::Client should build with sane defaults"),
        }
    }

    /// Set the path to the sources config file (`sources.json`).
    #[must_use]
    pub fn with_config_path(mut self, config_path: PathBuf) -> Self {
        self.config_path = Some(config_path);
        self
    }

    /// Use a custom HTTP client.
    #[must_use]
    pub fn with_client(mut self, client: reqwest::Client) -> Self {
        self.client = client;
        self
    }

    /// Return a reference to the marketplace root directory.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Return the installed state file path.
    pub fn installed_state_path(&self) -> PathBuf {
        self.root.join("installed.json")
    }

    // ── source management ─────────────────────────────────────────────────────

    /// Add a source to the persistent config (if a config path is set).
    pub fn add_source(&self, raw_source: &str) -> Result<Vec<MarketplaceSource>, MarketplaceError> {
        let source = normalise_source(raw_source, MarketplaceSourceOrigin::Config);
        let Some(ref config_path) = self.config_path else {
            return self.list_sources();
        };
        let mut config = load_config(config_path)?;
        if !config
            .sources
            .iter()
            .any(|stored| stored.url == source.url || stored.name == source.name)
        {
            config.sources.push(StoredMarketplaceSource {
                name: source.name,
                url: source.url,
            });
            save_config(config_path, &config)?;
        }
        self.list_sources()
    }

    /// List all active sources (builtin + config + env).
    pub fn list_sources(&self) -> Result<Vec<MarketplaceSource>, MarketplaceError> {
        let mut sources = Vec::new();
        if !default_sources_disabled() {
            sources.push(builtin_source());
        }
        sources.extend(self.config_sources()?);
        sources.extend(env_sources());
        Ok(dedupe_sources(sources))
    }

    fn config_sources(&self) -> Result<Vec<MarketplaceSource>, MarketplaceError> {
        let Some(ref config_path) = self.config_path else {
            return Ok(Vec::new());
        };
        Ok(load_config(config_path)?
            .sources
            .into_iter()
            .map(|source| MarketplaceSource {
                name: source.name,
                url: source.url,
                origin: MarketplaceSourceOrigin::Config,
            })
            .collect())
    }

    // ── catalog / search / inspect ───────────────────────────────────────────

    /// Fetch the full catalog from all active sources.
    pub async fn catalog(&self) -> Result<Vec<MarketplaceHit>, MarketplaceError> {
        let sources = self.list_sources()?;
        let mut all_hits = Vec::new();

        for source in &sources {
            let entries = self.load_source_entries(source).await?;
            for entry in entries {
                all_hits.push(MarketplaceHit {
                    source: source.clone(),
                    entry,
                });
            }
        }

        // Deduplicate by name (first source wins).
        let mut seen = std::collections::HashSet::new();
        all_hits.retain(|hit| seen.insert(hit.entry.name.clone()));

        Ok(all_hits)
    }

    /// Search the catalog with an optional query and DCC filter.
    pub async fn search(
        &self,
        query: Option<String>,
        dcc: Option<String>,
        explicit_sources: Vec<String>,
        limit: Option<usize>,
    ) -> Result<MarketplaceSearchResult, MarketplaceError> {
        let sources = self.sources_for_query(explicit_sources)?;
        let mut hits = Vec::new();
        for source in sources {
            let entries = self.load_source_entries(&source).await?;
            let matched = dcc_mcp_catalog::search(&entries, query.as_deref().unwrap_or(""));
            for entry in matched {
                if let Some(dcc) = dcc.as_deref()
                    && !entry_targets_dcc(&entry, dcc)
                {
                    continue;
                }
                hits.push(MarketplaceHit {
                    source: source.clone(),
                    entry,
                });
                if limit.is_some_and(|limit| hits.len() >= limit) {
                    break;
                }
            }
            if limit.is_some_and(|limit| hits.len() >= limit) {
                break;
            }
        }
        Ok(MarketplaceSearchResult {
            query,
            dcc,
            count: hits.len(),
            hits,
        })
    }

    /// Inspect a specific entry by name.
    pub async fn inspect(
        &self,
        name: String,
        explicit_sources: Vec<String>,
    ) -> Result<MarketplaceInspectResult, MarketplaceError> {
        let sources = self.sources_for_query(explicit_sources)?;
        let mut matches = Vec::new();
        for source in sources {
            let entries = self.load_source_entries(&source).await?;
            if let Some(entry) = dcc_mcp_catalog::describe(&entries, &name) {
                matches.push(MarketplaceHit {
                    source: source.clone(),
                    entry,
                });
            }
        }
        if matches.is_empty() {
            return Err(MarketplaceError::NotFound(name));
        }
        Ok(MarketplaceInspectResult {
            name,
            count: matches.len(),
            matches,
        })
    }

    // ── install / uninstall ──────────────────────────────────────────────────

    pub async fn install(
        &self,
        name: String,
        dcc: Option<String>,
        explicit_sources: Vec<String>,
        force: bool,
    ) -> Result<MarketplaceInstallResult, MarketplaceError> {
        let hit = self
            .resolve_install_hit(&name, dcc.as_deref(), explicit_sources)
            .await?;
        let dcc = resolve_install_dcc(&hit.entry, dcc.as_deref())?;
        let install = hit
            .entry
            .install
            .clone()
            .ok_or_else(|| MarketplaceError::MissingInstall(hit.entry.name.clone()))?;
        let package_name = path_component("package name", &hit.entry.name)?;
        let dcc_root = self.dcc_dir(&dcc);
        let dest = dcc_root.join(&package_name);

        if dest.exists() && !force {
            return Err(MarketplaceError::AlreadyInstalled {
                name: package_name.clone(),
                dcc: dcc.clone(),
                path: dest.display().to_string(),
            });
        }
        fs::create_dir_all(&dcc_root)
            .map_err(|err| MarketplaceError::ConfigIo(dcc_root.display().to_string(), err))?;

        let staging = dcc_root.join(format!(".{package_name}.installing-{}", now_ms()));
        if staging.exists() {
            remove_path(&staging)?;
        }

        let install_result = match install.install_type.as_str() {
            "git" => install_from_git(&install, &staging),
            "path" => install_from_path(&install, &staging),
            "zip" => self.install_from_zip(&install, &staging).await,
            other => return Err(MarketplaceError::UnsupportedInstallType(other.into())),
        };
        if let Err(err) = install_result {
            let _ = remove_path(&staging);
            return Err(err);
        }

        let skill_md = staging.join("SKILL.md");
        if !skill_md.is_file() {
            let _ = remove_path(&staging);
            return Err(MarketplaceError::MissingSkill(
                skill_md.display().to_string(),
            ));
        }

        if dest.exists() {
            if !force {
                let _ = remove_path(&staging);
                return Err(MarketplaceError::AlreadyInstalled {
                    name: package_name.clone(),
                    dcc: dcc.clone(),
                    path: dest.display().to_string(),
                });
            }
            remove_path(&dest)?;
        }
        fs::rename(&staging, &dest)
            .map_err(|err| MarketplaceError::ConfigIo(dest.display().to_string(), err))?;

        let package = InstalledMarketplacePackage {
            name: package_name.clone(),
            dcc: dcc.clone(),
            version: hit.entry.version.clone(),
            path: dest.display().to_string(),
            source_name: hit.source.name.clone(),
            source_url: hit.source.url.clone(),
            install_type: install.install_type.clone(),
            install_url: install.url.clone(),
            install_ref: install.ref_.clone(),
            installed_at_ms: now_ms(),
        };
        self.upsert_installed(package)?;

        Ok(MarketplaceInstallResult {
            installed: true,
            name: package_name,
            dcc,
            version: hit.entry.version.clone(),
            path: dest.display().to_string(),
            skill_search_path: dcc_root.display().to_string(),
            source: hit.source,
            entry: hit.entry,
            install_type: install.install_type.clone(),
            reload_required: true,
        })
    }

    pub fn uninstall(
        &self,
        name: &str,
        dcc: &str,
    ) -> Result<MarketplaceUninstallResult, MarketplaceError> {
        let name = path_component("package name", name)?;
        let dcc_root = self.dcc_dir(dcc);
        let dest = dcc_root.join(&name);
        let removed_files = if dest.exists() {
            remove_path(&dest)?;
            true
        } else {
            false
        };
        let removed_state = self.remove_installed(&name, dcc)?;
        Ok(MarketplaceUninstallResult {
            uninstalled: removed_files || removed_state,
            name,
            dcc: dcc.to_string(),
            path: dest.display().to_string(),
            removed_state,
            removed_files,
            reload_required: removed_files || removed_state,
        })
    }

    // ── installed state ──────────────────────────────────────────────────────

    pub fn list_installed(
        &self,
        dcc: Option<&str>,
    ) -> Result<MarketplaceInstalledList, MarketplaceError> {
        let mut packages = self.load_installed_state()?.packages;
        if let Some(dcc) = dcc {
            packages.retain(|package| package.dcc.eq_ignore_ascii_case(dcc));
        }
        Ok(MarketplaceInstalledList {
            dcc: dcc.map(String::from),
            count: packages.len(),
            packages,
        })
    }

    // ── outdated / update ────────────────────────────────────────────────────

    pub async fn outdated(
        &self,
        dcc: Option<&str>,
        names: Vec<String>,
    ) -> Result<MarketplaceOutdatedList, MarketplaceError> {
        let packages = self.list_installed(dcc)?.packages;
        let filtered: Vec<InstalledMarketplacePackage> = if names.is_empty() {
            packages
        } else {
            packages
                .into_iter()
                .filter(|p| names.iter().any(|n| n == &p.name))
                .collect()
        };
        let sources = self.list_sources()?;
        let mut outdated = Vec::new();
        for pkg in filtered {
            let entry = self.find_latest_entry_for_package(&sources, &pkg).await?;
            let is_outdated = match (&entry, &pkg.version) {
                (Some(entry), Some(installed)) => {
                    entry.version.as_deref() != Some(installed.as_str())
                }
                (Some(_), None) => true,
                (None, _) => false,
            };
            if is_outdated && let Some(entry) = entry {
                let latest_install = entry.install.as_ref();
                outdated.push(OutdatedMarketplacePackage {
                    name: pkg.name,
                    dcc: pkg.dcc,
                    installed_version: pkg.version,
                    latest_version: entry.version,
                    source_name: pkg.source_name,
                    source_url: pkg.source_url,
                    install_type: latest_install
                        .map(|i| i.install_type.clone())
                        .unwrap_or(pkg.install_type),
                    install_url: latest_install
                        .and_then(|i| i.url.clone())
                        .or(pkg.install_url),
                    install_ref: latest_install
                        .and_then(|i| i.ref_.clone())
                        .or(pkg.install_ref),
                    path: pkg.path,
                });
            }
        }
        Ok(MarketplaceOutdatedList {
            dcc: dcc.map(String::from),
            count: outdated.len(),
            packages: outdated,
        })
    }

    pub async fn update(
        &self,
        name: Option<String>,
        all: bool,
        dcc: Option<String>,
    ) -> Result<Vec<MarketplaceUpdateResult>, MarketplaceError> {
        let outdated = self
            .outdated(dcc.as_deref(), name.into_iter().collect())
            .await?;
        if outdated.packages.is_empty() {
            return Ok(Vec::new());
        }
        if !all && outdated.packages.len() > 1 {
            return Err(MarketplaceError::CommandFailed(format!(
                "{} packages are outdated; use --all to update all, or specify a name.",
                outdated.packages.len()
            )));
        }

        let mut results = Vec::new();
        for pkg in outdated.packages {
            let dest = PathBuf::from(&pkg.path);
            let previous_version = pkg.installed_version.clone();

            let update_result = match pkg.install_type.as_str() {
                "git" => self.update_git_package(&pkg, &dest).await,
                _ => self
                    .install(
                        pkg.name.clone(),
                        Some(pkg.dcc.clone()),
                        vec![pkg.source_url.clone()],
                        true,
                    )
                    .await
                    .map(|result| MarketplaceUpdateResult {
                        updated: true,
                        name: pkg.name.clone(),
                        dcc: pkg.dcc.clone(),
                        previous_version,
                        new_version: result.version,
                        path: result.path,
                        install_type: result.install_type,
                        source_name: pkg.source_name.clone(),
                        source_url: pkg.source_url.clone(),
                        reload_required: true,
                    }),
            }?;

            if let Some(ref vs) = update_result.new_version {
                self.upsert_installed(InstalledMarketplacePackage {
                    name: update_result.name.clone(),
                    dcc: update_result.dcc.clone(),
                    version: Some(vs.clone()),
                    path: update_result.path.clone(),
                    source_name: update_result.source_name.clone(),
                    source_url: update_result.source_url.clone(),
                    install_type: update_result.install_type.clone(),
                    install_url: pkg.install_url.clone(),
                    install_ref: pkg.install_ref.clone(),
                    installed_at_ms: now_ms(),
                })?;
            }
            results.push(update_result);
        }
        Ok(results)
    }

    // ── internal helpers ─────────────────────────────────────────────────────

    fn dcc_dir(&self, dcc: &str) -> PathBuf {
        self.root.join(dcc.to_lowercase())
    }

    async fn load_source_entries(
        &self,
        source: &MarketplaceSource,
    ) -> Result<Vec<CatalogEntry>, MarketplaceError> {
        let text = if source.url.starts_with("http://") || source.url.starts_with("https://") {
            self.client
                .get(&source.url)
                .header("User-Agent", "dcc-mcp marketplace")
                .send()
                .await
                .map_err(|err| MarketplaceError::Fetch(source.url.clone(), err))?
                .error_for_status()
                .map_err(|err| MarketplaceError::Fetch(source.url.clone(), err))?
                .text()
                .await
                .map_err(|err| MarketplaceError::Fetch(source.url.clone(), err))?
        } else {
            let path = source
                .url
                .strip_prefix("file://")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(&source.url));
            fs::read_to_string(&path)
                .map_err(|err| MarketplaceError::Read(path.display().to_string(), err))?
        };
        dcc_mcp_catalog::load_from_str(&text).map_err(Into::into)
    }

    async fn resolve_install_hit(
        &self,
        name: &str,
        dcc: Option<&str>,
        explicit_sources: Vec<String>,
    ) -> Result<MarketplaceHit, MarketplaceError> {
        let sources = self.sources_for_query(explicit_sources)?;
        for source in sources {
            let entries = self.load_source_entries(&source).await?;
            if let Some(entry) = dcc_mcp_catalog::describe(&entries, name) {
                if let Some(dcc) = dcc
                    && !entry_targets_dcc(&entry, dcc)
                {
                    continue;
                }
                return Ok(MarketplaceHit { source, entry });
            }
        }
        Err(MarketplaceError::NotFound(name.to_string()))
    }

    fn sources_for_query(
        &self,
        explicit_sources: Vec<String>,
    ) -> Result<Vec<MarketplaceSource>, MarketplaceError> {
        if explicit_sources.is_empty() {
            return self.list_sources();
        }
        Ok(dedupe_sources(
            explicit_sources
                .iter()
                .map(|s| normalise_source(s, MarketplaceSourceOrigin::Explicit))
                .collect(),
        ))
    }

    async fn install_from_zip(
        &self,
        install: &CatalogInstall,
        dest: &Path,
    ) -> Result<(), MarketplaceError> {
        let (url, bytes) = self.load_archive(install).await?;
        verify_archive_sha256(&bytes, install.sha256.as_deref(), &url)?;
        extract_zip_archive(&bytes, dest)?;
        flatten_single_skill_directory(dest)?;
        Ok(())
    }

    async fn load_archive(
        &self,
        install: &CatalogInstall,
    ) -> Result<(String, Vec<u8>), MarketplaceError> {
        let url = install
            .url
            .as_deref()
            .ok_or_else(|| MarketplaceError::MissingInstall("zip.url".into()))?;
        if url.starts_with("http://") || url.starts_with("https://") {
            let bytes = self
                .client
                .get(url)
                .header("User-Agent", "dcc-mcp marketplace")
                .send()
                .await
                .map_err(|err| MarketplaceError::Fetch(url.to_string(), err))?
                .error_for_status()
                .map_err(|err| MarketplaceError::Fetch(url.to_string(), err))?
                .bytes()
                .await
                .map_err(|err| MarketplaceError::Fetch(url.to_string(), err))?;
            return Ok((url.to_string(), bytes.to_vec()));
        }

        let path = url
            .strip_prefix("file://")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(url));
        let bytes = fs::read(&path)
            .map_err(|err| MarketplaceError::Read(path.display().to_string(), err))?;
        Ok((url.to_string(), bytes))
    }

    async fn find_latest_entry_for_package(
        &self,
        sources: &[MarketplaceSource],
        pkg: &InstalledMarketplacePackage,
    ) -> Result<Option<CatalogEntry>, MarketplaceError> {
        for source in sources {
            if source.url == pkg.source_url {
                let entries = self.load_source_entries(source).await?;
                if let Some(entry) = dcc_mcp_catalog::describe(&entries, &pkg.name) {
                    return Ok(Some(entry));
                }
            }
        }
        let temp_source = MarketplaceSource {
            name: pkg.source_name.clone(),
            url: pkg.source_url.clone(),
            origin: MarketplaceSourceOrigin::Explicit,
        };
        let entries = self.load_source_entries(&temp_source).await?;
        Ok(dcc_mcp_catalog::describe(&entries, &pkg.name))
    }

    async fn update_git_package(
        &self,
        pkg: &OutdatedMarketplacePackage,
        dest: &Path,
    ) -> Result<MarketplaceUpdateResult, MarketplaceError> {
        let git_dir = dest.join(".git");
        let install_url_changed = pkg.install_url.as_deref().is_some_and(|url| {
            git_remote_url(dest)
                .ok()
                .is_some_and(|remote_url| remote_url.trim() != url)
        });

        if git_dir.is_dir() && !install_url_changed {
            if let Some(ref_) = pkg.install_ref.as_deref() {
                git_fetch_and_checkout(dest, ref_)?;
            } else {
                git_pull(dest)?;
            }
        } else {
            let result = self
                .install(
                    pkg.name.clone(),
                    Some(pkg.dcc.clone()),
                    vec![pkg.source_url.clone()],
                    true,
                )
                .await?;
            return Ok(MarketplaceUpdateResult {
                updated: true,
                name: pkg.name.clone(),
                dcc: pkg.dcc.clone(),
                previous_version: pkg.installed_version.clone(),
                new_version: result.version,
                path: result.path,
                install_type: result.install_type,
                source_name: pkg.source_name.clone(),
                source_url: pkg.source_url.clone(),
                reload_required: true,
            });
        }

        let sources = self.list_sources()?;
        let new_version = self
            .find_latest_entry_for_package(
                &sources,
                &InstalledMarketplacePackage {
                    name: pkg.name.clone(),
                    dcc: pkg.dcc.clone(),
                    version: pkg.installed_version.clone(),
                    path: pkg.path.clone(),
                    source_name: pkg.source_name.clone(),
                    source_url: pkg.source_url.clone(),
                    install_type: pkg.install_type.clone(),
                    install_url: pkg.install_url.clone(),
                    install_ref: pkg.install_ref.clone(),
                    installed_at_ms: 0,
                },
            )
            .await?
            .and_then(|entry| entry.version);

        Ok(MarketplaceUpdateResult {
            updated: true,
            name: pkg.name.clone(),
            dcc: pkg.dcc.clone(),
            previous_version: pkg.installed_version.clone(),
            new_version,
            path: dest.display().to_string(),
            install_type: pkg.install_type.clone(),
            source_name: pkg.source_name.clone(),
            source_url: pkg.source_url.clone(),
            reload_required: true,
        })
    }

    // ── installed state persistence ──────────────────────────────────────────

    fn load_installed_state(&self) -> Result<MarketplaceInstalledState, MarketplaceError> {
        let path = self.installed_state_path();
        if !path.exists() {
            return Ok(MarketplaceInstalledState::default());
        }
        let text = fs::read_to_string(&path)
            .map_err(|err| MarketplaceError::ConfigIo(path.display().to_string(), err))?;
        serde_json::from_str(&text)
            .map_err(|err| MarketplaceError::ConfigParse(path.display().to_string(), err))
    }

    fn save_installed_state(
        &self,
        state: &MarketplaceInstalledState,
    ) -> Result<(), MarketplaceError> {
        let path = self.installed_state_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|err| MarketplaceError::ConfigIo(parent.display().to_string(), err))?;
        }
        let text = serde_json::to_string_pretty(state)
            .expect("MarketplaceInstalledState serialization should not fail");
        write_atomic(&path, &text)
    }

    fn upsert_installed(
        &self,
        package: InstalledMarketplacePackage,
    ) -> Result<(), MarketplaceError> {
        let mut state = self.load_installed_state()?;
        state
            .packages
            .retain(|existing| !(existing.name == package.name && existing.dcc == package.dcc));
        state.packages.push(package);
        state.packages.sort_by(|a, b| {
            a.dcc
                .cmp(&b.dcc)
                .then_with(|| a.name.cmp(&b.name))
                .then_with(|| a.path.cmp(&b.path))
        });
        self.save_installed_state(&state)
    }

    fn remove_installed(&self, name: &str, dcc: &str) -> Result<bool, MarketplaceError> {
        let mut state = self.load_installed_state()?;
        let before = state.packages.len();
        state
            .packages
            .retain(|package| !(package.name == name && package.dcc.eq_ignore_ascii_case(dcc)));
        let changed = state.packages.len() != before;
        if changed {
            self.save_installed_state(&state)?;
        }
        Ok(changed)
    }
}

// ── free functions ────────────────────────────────────────────────────────────

/// Serialises all marketplace file writes so concurrent callers (two
/// `save_config()` or two `save_installed_state()`) never share the same
/// temp file.  A `static Mutex` is fine here because the critical section
/// is disk I/O — tens of ms, not nanoseconds.
static WRITE_LOCK: Mutex<()> = Mutex::new(());

/// Atomic write — write to a temp file, sync, then rename into place.
///
/// Same pattern as `FileRegistry::write_atomic` in `dcc-mcp-transport`.
/// Callers are serialised by [`WRITE_LOCK`] so same-target-path writes
/// cannot clobber one another.
fn write_atomic(path: &Path, content: &str) -> Result<(), MarketplaceError> {
    let _guard = WRITE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let pid = std::process::id();
    let temp_path = dir.join(format!(".tmp.{pid}.marketplace.json"));

    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&temp_path)
        .map_err(|err| {
            let _ = fs::remove_file(&temp_path);
            MarketplaceError::ConfigIo(temp_path.display().to_string(), err)
        })?;

    if let Err(err) = std::io::Write::write_all(&mut file, content.as_bytes()) {
        drop(file);
        let _ = fs::remove_file(&temp_path);
        return Err(MarketplaceError::ConfigIo(
            temp_path.display().to_string(),
            err,
        ));
    }

    if let Err(err) = file.sync_data() {
        drop(file);
        let _ = fs::remove_file(&temp_path);
        return Err(MarketplaceError::ConfigIo(
            temp_path.display().to_string(),
            err,
        ));
    }
    drop(file);

    const MAX_ATTEMPTS: u32 = 8;
    const BACKOFF_MS: u64 = 10;
    for attempt in 0..MAX_ATTEMPTS {
        match fs::rename(&temp_path, path) {
            Ok(()) => return Ok(()),
            Err(e) => {
                std::thread::sleep(std::time::Duration::from_millis(
                    BACKOFF_MS * (attempt as u64 + 1),
                ));
                if attempt == MAX_ATTEMPTS - 1 {
                    let _ = fs::remove_file(&temp_path);
                    return Err(MarketplaceError::ConfigIo(path.display().to_string(), e));
                }
            }
        }
    }
    unreachable!()
}

/// Check whether the `DCC_MCP_MARKETPLACE_NO_DEFAULT_SOURCES` env var is set.
pub fn default_sources_disabled() -> bool {
    std::env::var(ENV_MARKETPLACE_NO_DEFAULT_SOURCES)
        .map(|v| matches!(v.trim(), "1" | "true" | "TRUE" | "yes" | "YES"))
        .unwrap_or(false)
}

/// Parse sources from the `DCC_MCP_MARKETPLACE_SOURCES` env var (comma-separated).
pub fn env_sources() -> Vec<MarketplaceSource> {
    let Ok(value) = std::env::var(ENV_MARKETPLACE_SOURCES) else {
        return Vec::new();
    };
    value
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| normalise_source(s, MarketplaceSourceOrigin::Env))
        .collect()
}

/// Validate a path component for safe filesystem use.
pub fn path_component(kind: &str, value: &str) -> Result<String, MarketplaceError> {
    let trimmed = value.trim();
    if trimmed.is_empty()
        || trimmed == "."
        || trimmed == ".."
        || trimmed.starts_with('.')
        || !trimmed
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-'))
    {
        return Err(MarketplaceError::InvalidPathComponent {
            kind: kind.to_string(),
            value: value.to_string(),
        });
    }
    Ok(trimmed.to_string())
}

fn load_config(path: &Path) -> Result<MarketplaceSourceConfig, MarketplaceError> {
    if !path.exists() {
        return Ok(MarketplaceSourceConfig::default());
    }
    let text = fs::read_to_string(path)
        .map_err(|err| MarketplaceError::ConfigIo(path.display().to_string(), err))?;
    serde_json::from_str(&text)
        .map_err(|err| MarketplaceError::ConfigParse(path.display().to_string(), err))
}

fn save_config(path: &Path, config: &MarketplaceSourceConfig) -> Result<(), MarketplaceError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| MarketplaceError::ConfigIo(parent.display().to_string(), err))?;
    }
    let text = serde_json::to_string_pretty(config)
        .expect("MarketplaceSourceConfig serialization should not fail");
    write_atomic(path, &text)
}

fn resolve_install_dcc(
    entry: &CatalogEntry,
    requested: Option<&str>,
) -> Result<String, MarketplaceError> {
    if let Some(dcc) = requested {
        let dcc_name = path_component("DCC name", dcc)?.to_lowercase();
        if entry_targets_dcc(entry, &dcc_name) {
            return Ok(dcc_name);
        }
        return Err(MarketplaceError::DccMismatch {
            name: entry.name.clone(),
            dcc: dcc.to_string(),
        });
    }

    let mut dccs: Vec<String> = entry
        .dcc
        .iter()
        .map(|dcc| path_component("DCC name", dcc).map(|s| s.to_lowercase()))
        .collect::<Result<_, _>>()?;
    dccs.sort();
    dccs.dedup();
    match dccs.as_slice() {
        [dcc] => Ok(dcc.clone()),
        _ => Err(MarketplaceError::AmbiguousDcc {
            name: entry.name.clone(),
        }),
    }
}

// ── install backends ─────────────────────────────────────────────────────────

fn install_from_git(install: &CatalogInstall, dest: &Path) -> Result<(), MarketplaceError> {
    let url = install
        .url
        .as_deref()
        .ok_or_else(|| MarketplaceError::MissingInstall("git.url".into()))?;
    let mut command = Command::new("git");
    command.arg("clone").arg("--depth").arg("1");
    if let Some(ref_) = install.ref_.as_deref().filter(|v| !v.trim().is_empty()) {
        command.arg("--branch").arg(ref_);
    }
    command.arg(url).arg(dest);
    let output = command
        .output()
        .map_err(|err| MarketplaceError::CommandFailed(format!("git clone: {err}")))?;
    if output.status.success() {
        return Ok(());
    }
    Err(MarketplaceError::CommandFailed(format!(
        "git clone exited with {}: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    )))
}

fn install_from_path(install: &CatalogInstall, dest: &Path) -> Result<(), MarketplaceError> {
    let url = install
        .url
        .as_deref()
        .ok_or_else(|| MarketplaceError::MissingInstall("path.url".into()))?;
    let src = url
        .strip_prefix("file://")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(url));
    if !src.join("SKILL.md").is_file() {
        return Err(MarketplaceError::MissingSkill(src.display().to_string()));
    }
    copy_dir_recursive(&src, dest)
}

fn git_fetch_and_checkout(repo_path: &Path, ref_: &str) -> Result<(), MarketplaceError> {
    let output = Command::new("git")
        .args(["fetch", "origin", "--tags"])
        .current_dir(repo_path)
        .output()
        .map_err(|err| MarketplaceError::CommandFailed(format!("git fetch: {err}")))?;
    if !output.status.success() {
        return Err(MarketplaceError::CommandFailed(format!(
            "git fetch failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    let output = Command::new("git")
        .args(["checkout", ref_])
        .current_dir(repo_path)
        .output()
        .map_err(|err| MarketplaceError::CommandFailed(format!("git checkout: {err}")))?;
    if !output.status.success() {
        return Err(MarketplaceError::CommandFailed(format!(
            "git checkout failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    Ok(())
}

fn git_pull(repo_path: &Path) -> Result<(), MarketplaceError> {
    let output = Command::new("git")
        .args(["pull", "--ff-only"])
        .current_dir(repo_path)
        .output()
        .map_err(|err| MarketplaceError::CommandFailed(format!("git pull: {err}")))?;
    if !output.status.success() {
        return Err(MarketplaceError::CommandFailed(format!(
            "git pull failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    Ok(())
}

fn git_remote_url(repo_path: &Path) -> Result<String, MarketplaceError> {
    let output = Command::new("git")
        .args(["remote", "get-url", "origin"])
        .current_dir(repo_path)
        .output()
        .map_err(|err| MarketplaceError::CommandFailed(format!("git remote get-url: {err}")))?;
    if !output.status.success() {
        return Err(MarketplaceError::CommandFailed(format!(
            "git remote get-url failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

// ── zip / sha256 ─────────────────────────────────────────────────────────────

fn verify_archive_sha256(
    bytes: &[u8],
    expected: Option<&str>,
    url: &str,
) -> Result<(), MarketplaceError> {
    let Some(expected) = expected
        .map(normalize_sha256)
        .filter(|value| !value.is_empty())
    else {
        return Ok(());
    };
    let actual = sha256_hex(bytes);
    if actual.eq_ignore_ascii_case(&expected) {
        return Ok(());
    }
    Err(MarketplaceError::HashMismatch {
        url: url.to_string(),
        expected,
        actual,
    })
}

fn normalize_sha256(value: &str) -> String {
    value
        .trim()
        .strip_prefix("sha256:")
        .unwrap_or(value.trim())
        .to_ascii_lowercase()
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(out, "{byte:02x}");
    }
    out
}

fn extract_zip_archive(bytes: &[u8], dest: &Path) -> Result<(), MarketplaceError> {
    fs::create_dir_all(dest)
        .map_err(|err| MarketplaceError::ConfigIo(dest.display().to_string(), err))?;
    let cursor = Cursor::new(bytes);
    let mut archive = zip::ZipArchive::new(cursor)
        .map_err(|err| MarketplaceError::Archive("zip".into(), err.to_string()))?;

    for index in 0..archive.len() {
        let mut file = archive
            .by_index(index)
            .map_err(|err| MarketplaceError::Archive("zip".into(), err.to_string()))?;
        let Some(enclosed_name) = file.enclosed_name() else {
            return Err(MarketplaceError::Archive(
                file.name().to_string(),
                "archive entry escapes install root".into(),
            ));
        };
        let out_path = dest.join(enclosed_name);
        if file.is_dir() {
            fs::create_dir_all(&out_path)
                .map_err(|err| MarketplaceError::ConfigIo(out_path.display().to_string(), err))?;
        } else {
            if let Some(parent) = out_path.parent() {
                fs::create_dir_all(parent)
                    .map_err(|err| MarketplaceError::ConfigIo(parent.display().to_string(), err))?;
            }
            let mut out_file = fs::File::create(&out_path)
                .map_err(|err| MarketplaceError::ConfigIo(out_path.display().to_string(), err))?;
            std::io::copy(&mut file, &mut out_file)
                .map_err(|err| MarketplaceError::ConfigIo(out_path.display().to_string(), err))?;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Some(mode) = file.unix_mode() {
                let _ = fs::set_permissions(&out_path, fs::Permissions::from_mode(mode));
            }
        }
    }
    Ok(())
}

/// If the extracted directory already has a SKILL.md at the top, nothing to do.
///
/// Otherwise, if there is exactly one child directory that contains a SKILL.md,
/// flatten that child's contents into `dest`.
fn flatten_single_skill_directory(dest: &Path) -> Result<(), MarketplaceError> {
    if dest.join("SKILL.md").is_file() {
        return Ok(());
    }

    let child_dirs: Vec<PathBuf> = fs::read_dir(dest)
        .map_err(|err| MarketplaceError::ConfigIo(dest.display().to_string(), err))?
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let file_type = entry.file_type().ok()?;
            if file_type.is_dir() {
                Some(entry.path())
            } else {
                None
            }
        })
        .collect();

    let [child] = child_dirs.as_slice() else {
        return Ok(());
    };
    if !child.join("SKILL.md").is_file() {
        return Ok(());
    }

    let flatten_root = dest.join(format!(".flattening-{}", now_ms()));
    fs::rename(child, &flatten_root)
        .map_err(|err| MarketplaceError::ConfigIo(flatten_root.display().to_string(), err))?;

    for entry in fs::read_dir(&flatten_root)
        .map_err(|err| MarketplaceError::ConfigIo(flatten_root.display().to_string(), err))?
    {
        let entry = entry
            .map_err(|err| MarketplaceError::ConfigIo(flatten_root.display().to_string(), err))?;
        let from = entry.path();
        let to = dest.join(entry.file_name());
        fs::rename(&from, &to).map_err(|err| {
            MarketplaceError::ConfigIo(format!("move {} -> {}", from.display(), to.display()), err)
        })?;
    }

    let _ = remove_path(&flatten_root);
    Ok(())
}

// ── fs helpers ───────────────────────────────────────────────────────────────

fn copy_dir_recursive(src: &Path, dest: &Path) -> Result<(), MarketplaceError> {
    fs::create_dir_all(dest)
        .map_err(|err| MarketplaceError::ConfigIo(dest.display().to_string(), err))?;
    for entry in fs::read_dir(src)
        .map_err(|err| MarketplaceError::ConfigIo(src.display().to_string(), err))?
    {
        let entry =
            entry.map_err(|err| MarketplaceError::ConfigIo(src.display().to_string(), err))?;
        let src_path = entry.path();
        let dest_path = dest.join(entry.file_name());
        let file_type = entry
            .file_type()
            .map_err(|err| MarketplaceError::ConfigIo(src_path.display().to_string(), err))?;
        if file_type.is_dir() {
            copy_dir_recursive(&src_path, &dest_path)?;
        } else if file_type.is_file() {
            fs::copy(&src_path, &dest_path).map_err(|err| {
                MarketplaceError::ConfigIo(
                    format!("copy {} -> {}", src_path.display(), dest_path.display()),
                    err,
                )
            })?;
        }
    }
    Ok(())
}

fn remove_path(path: &Path) -> Result<(), MarketplaceError> {
    if path.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
    .map_err(|err| MarketplaceError::ConfigIo(path.display().to_string(), err))
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_component_rejects_empty() {
        assert!(path_component("name", "").is_err());
    }

    #[test]
    fn path_component_rejects_dot_dot() {
        assert!(path_component("name", "..").is_err());
    }

    #[test]
    fn path_component_rejects_special_chars() {
        assert!(path_component("name", "bad/name").is_err());
    }

    #[test]
    fn path_component_allows_valid_name() {
        let result = path_component("name", "my-package_v1.0").unwrap();
        assert_eq!(result, "my-package_v1.0");
    }

    #[test]
    fn path_component_trims_whitespace() {
        let result = path_component("name", "  hello  ").unwrap();
        assert_eq!(result, "hello");
    }

    #[test]
    fn installed_state_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        let svc = MarketplaceService::new(tmp.path().to_path_buf());
        let pkg = InstalledMarketplacePackage {
            name: "test-skill".into(),
            dcc: "maya".into(),
            version: Some("1.0.0".into()),
            path: "/tmp/test".into(),
            source_name: "dcc-mcp/marketplace".into(),
            source_url: "https://example.com".into(),
            install_type: "git".into(),
            install_url: None,
            install_ref: None,
            installed_at_ms: 1000,
        };
        svc.upsert_installed(pkg.clone()).unwrap();
        let list = svc.list_installed(None).unwrap();
        assert_eq!(list.count, 1);
        assert_eq!(list.packages[0].name, "test-skill");
    }

    #[test]
    fn default_sources_disabled_false_when_unset() {
        // SAFETY: test-only env manipulation; isolated to this specific var.
        unsafe { std::env::remove_var(ENV_MARKETPLACE_NO_DEFAULT_SOURCES) };
        assert!(!default_sources_disabled());
    }

    #[test]
    fn default_sources_disabled_respects_truthy_values() {
        // Save/restore to avoid leaking across tests.
        let saved = std::env::var(ENV_MARKETPLACE_NO_DEFAULT_SOURCES).ok();
        for v in ["1", "true", "TRUE", "yes", "YES"] {
            unsafe { std::env::set_var(ENV_MARKETPLACE_NO_DEFAULT_SOURCES, v) };
            assert!(default_sources_disabled(), "expected true for '{v}'");
        }
        for v in ["0", "false", "no", "", "FALSE", "NO"] {
            unsafe { std::env::set_var(ENV_MARKETPLACE_NO_DEFAULT_SOURCES, v) };
            assert!(!default_sources_disabled(), "expected false for '{v}'");
        }
        // Restore original value (or unset).
        match saved {
            Some(v) => unsafe { std::env::set_var(ENV_MARKETPLACE_NO_DEFAULT_SOURCES, v) },
            None => unsafe { std::env::remove_var(ENV_MARKETPLACE_NO_DEFAULT_SOURCES) },
        }
    }
}
