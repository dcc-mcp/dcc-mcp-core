use std::collections::BTreeSet;
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

use dcc_mcp_catalog::{CatalogEntry, CatalogInstall};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::domain::marketplace::{
    InstalledMarketplacePackage, MarketplaceHit, MarketplaceInspectResult,
    MarketplaceInstallResult, MarketplaceInstalledList, MarketplaceInstalledState,
    MarketplaceOutdatedList, MarketplaceSearchResult, MarketplaceSource, MarketplaceSourceConfig,
    MarketplaceSourceOrigin, MarketplaceUninstallResult, MarketplaceUpdateResult,
    OutdatedMarketplacePackage, StoredMarketplaceSource, builtin_source, entry_targets_dcc,
    normalise_source,
};

const ENV_MARKETPLACE_SOURCES: &str = "DCC_MCP_MARKETPLACE_SOURCES";
const ENV_MARKETPLACE_SOURCES_FILE: &str = "DCC_MCP_MARKETPLACE_SOURCES_FILE";
const ENV_MARKETPLACE_NO_DEFAULT_SOURCES: &str = "DCC_MCP_MARKETPLACE_NO_DEFAULT_SOURCES";

#[derive(Debug, Error)]
pub enum MarketplaceError {
    #[error("marketplace source config path could not be resolved: {0}")]
    ConfigPath(String),
    #[error("marketplace source config I/O error for '{0}': {1}")]
    ConfigIo(String, #[source] std::io::Error),
    #[error("marketplace source config parse error for '{0}': {1}")]
    ConfigParse(String, #[source] serde_json::Error),
    #[error("marketplace source fetch failed for '{0}': {1}")]
    Fetch(String, #[source] reqwest::Error),
    #[error("marketplace source read failed for '{0}': {1}")]
    Read(String, #[source] std::io::Error),
    #[error(transparent)]
    Catalog(#[from] dcc_mcp_catalog::CatalogError),
    #[error("marketplace catalog entry validation failed: {0}")]
    Validation(#[from] dcc_mcp_catalog::CatalogValidationError),
    #[error("marketplace entry '{0}' was not found")]
    NotFound(String),
    #[error("marketplace entry '{0}' does not declare install metadata")]
    MissingInstall(String),
    #[error("marketplace entry '{name}' targets multiple DCCs; pass --dcc")]
    AmbiguousDcc { name: String },
    #[error("marketplace entry '{name}' does not target DCC '{dcc}'")]
    DccMismatch { name: String, dcc: String },
    #[error("marketplace install type '{0}' is not supported yet")]
    UnsupportedInstallType(String),
    #[error("marketplace package '{name}' is already installed for DCC '{dcc}' at '{path}'")]
    AlreadyInstalled {
        name: String,
        dcc: String,
        path: String,
    },
    #[error("marketplace install command failed: {0}")]
    CommandFailed(String),
    #[error("installed package does not contain SKILL.md at '{0}'")]
    MissingSkill(String),
    #[error("marketplace archive SHA-256 mismatch for '{url}': expected {expected}, got {actual}")]
    HashMismatch {
        url: String,
        expected: String,
        actual: String,
    },
    #[error("marketplace archive error for '{0}': {1}")]
    Archive(String, String),
    #[error(
        "invalid marketplace {kind} '{value}'; use only ASCII letters, numbers, '.', '_' or '-'"
    )]
    InvalidPathComponent { kind: String, value: String },
}

pub struct MarketplaceService {
    config_path: PathBuf,
    client: reqwest::Client,
}

impl MarketplaceService {
    pub fn new() -> Result<Self, MarketplaceError> {
        Ok(Self {
            config_path: default_config_path()?,
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(20))
                .build()
                .map_err(|err| MarketplaceError::Fetch("client".into(), err))?,
        })
    }

    #[cfg(test)]
    pub fn with_config_path(config_path: PathBuf) -> Self {
        Self {
            config_path,
            client: reqwest::Client::new(),
        }
    }

    #[cfg(test)]
    pub fn with_config_path_and_client(config_path: PathBuf, client: reqwest::Client) -> Self {
        Self {
            config_path,
            client,
        }
    }

    pub fn add_source(&self, raw_source: &str) -> Result<Vec<MarketplaceSource>, MarketplaceError> {
        let source = normalise_source(raw_source, MarketplaceSourceOrigin::Config);
        let mut config = self.load_config()?;
        if !config
            .sources
            .iter()
            .any(|stored| stored.url == source.url || stored.name == source.name)
        {
            config.sources.push(StoredMarketplaceSource {
                name: source.name,
                url: source.url,
            });
            self.save_config(&config)?;
        }
        self.list_sources()
    }

    pub fn list_sources(&self) -> Result<Vec<MarketplaceSource>, MarketplaceError> {
        let mut sources = Vec::new();
        if !default_sources_disabled() {
            sources.push(builtin_source());
        }

        sources.extend(self.config_sources()?);
        sources.extend(env_sources());
        // Sort by priority: Explicit(0) → Env(1) → Config(2) → Builtin(3)
        sources.sort_by_key(|s| s.origin.priority());
        Ok(dedupe_sources(sources))
    }

    pub async fn search(
        &self,
        query: Option<String>,
        dcc: Option<String>,
        explicit_sources: Vec<String>,
        limit: Option<usize>,
        skip_validation: bool,
    ) -> Result<MarketplaceSearchResult, MarketplaceError> {
        let sources = self.sources_for_query(explicit_sources)?;
        let validate = !skip_validation;
        let mut hits = Vec::new();
        for source in &sources {
            let entries = self.load_source_entries_validated(source, validate).await?;
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
            }
        }
        // Entry-level dedup: keep first hit (highest-priority source) per name.
        hits = merge_entries(hits);
        if let Some(limit) = limit {
            hits.truncate(limit);
        }
        Ok(MarketplaceSearchResult {
            query,
            dcc,
            count: hits.len(),
            hits,
        })
    }

    pub async fn inspect(
        &self,
        name: String,
        explicit_sources: Vec<String>,
        skip_validation: bool,
    ) -> Result<MarketplaceInspectResult, MarketplaceError> {
        let sources = self.sources_for_query(explicit_sources)?;
        let validate = !skip_validation;
        let mut matches = Vec::new();
        for source in &sources {
            let entries = self.load_source_entries_validated(source, validate).await?;
            if let Some(entry) = dcc_mcp_catalog::describe(&entries, &name) {
                matches.push(MarketplaceHit {
                    source: source.clone(),
                    entry,
                });
            }
        }
        // Entry-level dedup: keep only highest-priority source's match.
        matches = merge_entries(matches);
        if matches.is_empty() {
            return Err(MarketplaceError::NotFound(name));
        }
        Ok(MarketplaceInspectResult {
            name,
            count: matches.len(),
            matches,
        })
    }

    pub async fn install(
        &self,
        name: String,
        dcc: Option<String>,
        explicit_sources: Vec<String>,
        force: bool,
        skip_validation: bool,
    ) -> Result<MarketplaceInstallResult, MarketplaceError> {
        let hit = self
            .resolve_install_hit(&name, dcc.as_deref(), explicit_sources, skip_validation)
            .await?;
        let dcc = resolve_install_dcc(&hit.entry, dcc.as_deref())?;
        let install = hit
            .entry
            .install
            .clone()
            .ok_or_else(|| MarketplaceError::MissingInstall(hit.entry.name.clone()))?;
        let package_name = marketplace_path_component("package name", &hit.entry.name)?;
        let dcc_root = marketplace_dcc_dir(&dcc)?;
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
            remove_install_path(&staging)?;
        }

        let install_result = match install.install_type.as_str() {
            "git" => install_from_git(&install, &staging),
            "path" => install_from_path(&install, &staging),
            "zip" => self.install_from_zip(&install, &staging).await,
            other => return Err(MarketplaceError::UnsupportedInstallType(other.into())),
        };
        if let Err(err) = install_result {
            let _ = remove_install_path(&staging);
            return Err(err);
        }

        let skill_md = staging.join("SKILL.md");
        if !skill_md.is_file() {
            let _ = remove_install_path(&staging);
            return Err(MarketplaceError::MissingSkill(
                skill_md.display().to_string(),
            ));
        }

        if dest.exists() {
            if !force {
                let _ = remove_install_path(&staging);
                return Err(MarketplaceError::AlreadyInstalled {
                    name: package_name.clone(),
                    dcc: dcc.clone(),
                    path: dest.display().to_string(),
                });
            }
            remove_install_path(&dest)?;
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
        name: String,
        dcc: String,
    ) -> Result<MarketplaceUninstallResult, MarketplaceError> {
        let name = marketplace_path_component("package name", &name)?;
        let dcc_root = marketplace_dcc_dir(&dcc)?;
        let dest = dcc_root.join(&name);
        let removed_files = if dest.exists() {
            remove_install_path(&dest)?;
            true
        } else {
            false
        };
        let removed_state = self.remove_installed(&name, &dcc)?;
        Ok(MarketplaceUninstallResult {
            uninstalled: removed_files || removed_state,
            name,
            dcc,
            path: dest.display().to_string(),
            removed_state,
            removed_files,
            reload_required: removed_files || removed_state,
        })
    }

    pub fn list_installed(
        &self,
        dcc: Option<String>,
    ) -> Result<MarketplaceInstalledList, MarketplaceError> {
        let mut packages = self.load_installed_state()?.packages;
        if let Some(dcc) = dcc.as_deref() {
            packages.retain(|package| package.dcc.eq_ignore_ascii_case(dcc));
        }
        Ok(MarketplaceInstalledList {
            dcc,
            count: packages.len(),
            packages,
        })
    }

    pub async fn outdated(
        &self,
        dcc: Option<String>,
        names: Vec<String>,
    ) -> Result<MarketplaceOutdatedList, MarketplaceError> {
        let packages = self.list_installed(dcc)?.packages;
        let filtered: Vec<InstalledMarketplacePackage> = if names.is_empty() {
            packages
        } else {
            packages
                .into_iter()
                .filter(|p| names.iter().any(|name| name == &p.name))
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
                        .map(|install| install.install_type.clone())
                        .unwrap_or(pkg.install_type),
                    install_url: latest_install
                        .and_then(|install| install.url.clone())
                        .or(pkg.install_url),
                    install_ref: latest_install
                        .and_then(|install| install.ref_.clone())
                        .or(pkg.install_ref),
                    path: pkg.path,
                });
            }
        }
        Ok(MarketplaceOutdatedList {
            dcc: None,
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
            .outdated(dcc.clone(), name.into_iter().collect())
            .await?;
        if outdated.packages.is_empty() {
            return Ok(Vec::new());
        }
        if !all && outdated.packages.len() > 1 {
            // If --all not set and multiple outdated, suggest passing --all
            return Err(MarketplaceError::CommandFailed(format!(
                "{} packages are outdated; use --all to update all, or specify a name. Use 'marketplace outdated' to list them.",
                outdated.packages.len()
            )));
        }

        let mut results = Vec::new();
        for pkg in outdated.packages {
            let dest = PathBuf::from(&pkg.path);
            let previous_version = pkg.installed_version.clone();

            let update_result = match pkg.install_type.as_str() {
                "git" => self.update_git_package(&pkg, &dest).await,
                _ => {
                    // For non-git types, re-install from catalog
                    self.install(
                        pkg.name.clone(),
                        Some(pkg.dcc.clone()),
                        vec![pkg.source_url.clone()],
                        true,  // force
                        false, // skip_validation
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
                    })
                }
            }?;

            // Update installed state with new version
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

    async fn update_git_package(
        &self,
        pkg: &OutdatedMarketplacePackage,
        dest: &Path,
    ) -> Result<MarketplaceUpdateResult, MarketplaceError> {
        let git_dir = dest.join(".git");
        let install_url_changed = pkg.install_url.as_deref().is_some_and(|url| {
            self.git_remote_url(dest)
                .ok()
                .is_some_and(|remote_url| remote_url.trim() != url)
        });

        if git_dir.is_dir() && !install_url_changed {
            // Existing git repo: fetch the ref and checkout
            if let Some(ref_) = pkg.install_ref.as_deref() {
                self.git_fetch_and_checkout(dest, ref_)?;
            } else {
                // No ref specified, pull default branch
                self.git_pull(dest)?;
            }
        } else {
            // Missing or changed git checkout: go through install(), which stages
            // the replacement before deleting the existing package.
            let result = self
                .install(
                    pkg.name.clone(),
                    Some(pkg.dcc.clone()),
                    vec![pkg.source_url.clone()],
                    true,  // force
                    false, // skip_validation
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

        // Determine new version by re-reading catalog
        let sources = self.list_sources()?;
        let new_version = self
            .find_latest_entry_for_package_by_source_url(&sources, &pkg.source_url, &pkg.name)
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

    fn git_fetch_and_checkout(&self, repo_path: &Path, ref_: &str) -> Result<(), MarketplaceError> {
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

    fn git_pull(&self, repo_path: &Path) -> Result<(), MarketplaceError> {
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

    fn git_remote_url(&self, repo_path: &Path) -> Result<String, MarketplaceError> {
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

    async fn install_from_zip(
        &self,
        install: &CatalogInstall,
        dest: &PathBuf,
    ) -> Result<(), MarketplaceError> {
        let (url, bytes) = self.load_install_archive(install).await?;
        verify_archive_sha256(&bytes, install.sha256.as_deref(), &url)?;
        extract_zip_archive(&bytes, dest)?;
        flatten_single_skill_directory(dest)?;
        Ok(())
    }

    async fn load_install_archive(
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
                .header("User-Agent", "dcc-mcp-cli marketplace")
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
    ) -> Result<Option<dcc_mcp_catalog::CatalogEntry>, MarketplaceError> {
        for source in sources {
            if source.url == pkg.source_url {
                let entries = self.load_source_entries(source).await?;
                if let Some(entry) = dcc_mcp_catalog::describe(&entries, &pkg.name) {
                    return Ok(Some(entry));
                }
            }
        }
        // Source may have been removed; try direct URL
        let temp_source = MarketplaceSource {
            name: pkg.source_name.clone(),
            url: pkg.source_url.clone(),
            origin: MarketplaceSourceOrigin::Explicit,
        };
        let entries = self.load_source_entries(&temp_source).await?;
        Ok(dcc_mcp_catalog::describe(&entries, &pkg.name))
    }

    async fn find_latest_entry_for_package_by_source_url(
        &self,
        sources: &[MarketplaceSource],
        source_url: &str,
        name: &str,
    ) -> Result<Option<dcc_mcp_catalog::CatalogEntry>, MarketplaceError> {
        for source in sources {
            if source.url == source_url {
                let entries = self.load_source_entries(source).await?;
                if let Some(entry) = dcc_mcp_catalog::describe(&entries, name) {
                    return Ok(Some(entry));
                }
            }
        }
        let temp_source = MarketplaceSource {
            name: source_url.to_string(),
            url: source_url.to_string(),
            origin: MarketplaceSourceOrigin::Explicit,
        };
        let entries = self.load_source_entries(&temp_source).await?;
        Ok(dcc_mcp_catalog::describe(&entries, name))
    }

    async fn resolve_install_hit(
        &self,
        name: &str,
        dcc: Option<&str>,
        explicit_sources: Vec<String>,
        skip_validation: bool,
    ) -> Result<MarketplaceHit, MarketplaceError> {
        let sources = self.sources_for_query(explicit_sources)?;
        let validate = !skip_validation;
        for source in sources {
            let entries = self
                .load_source_entries_validated(&source, validate)
                .await?;
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
        let configured = self.list_sources()?;
        if explicit_sources.is_empty() {
            return Ok(configured);
        }

        // Explicit sources get highest priority; configured sources follow.
        let mut all: Vec<MarketplaceSource> = explicit_sources
            .iter()
            .map(|raw| normalise_source(raw, MarketplaceSourceOrigin::Explicit))
            .collect();
        all.extend(configured);
        // Sort by priority so higher-priority sources are searched first.
        all.sort_by_key(|s| s.origin.priority());
        Ok(dedupe_sources(all))
    }

    async fn load_source_entries(
        &self,
        source: &MarketplaceSource,
    ) -> Result<Vec<dcc_mcp_catalog::CatalogEntry>, MarketplaceError> {
        let text = if source.url.starts_with("http://") || source.url.starts_with("https://") {
            self.client
                .get(&source.url)
                .header("User-Agent", "dcc-mcp-cli marketplace")
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
            std::fs::read_to_string(&path)
                .map_err(|err| MarketplaceError::Read(path.display().to_string(), err))?
        };
        let entries = dcc_mcp_catalog::load_from_str(&text)?;
        Ok(entries)
    }

    /// Same as [`load_source_entries`] but validates entries against the
    /// marketplace-v1 JSON Schema. When `validate` is true, invalid entries
    /// cause a hard error. When false, invalid entries are silently filtered out
    /// with a `tracing::warn!` log.
    async fn load_source_entries_validated(
        &self,
        source: &MarketplaceSource,
        validate: bool,
    ) -> Result<Vec<dcc_mcp_catalog::CatalogEntry>, MarketplaceError> {
        let entries = self.load_source_entries(source).await?;
        if validate {
            dcc_mcp_catalog::validate_catalog_entries(&entries)?;
            Ok(entries)
        } else {
            let mut valid = Vec::with_capacity(entries.len());
            for entry in entries {
                match dcc_mcp_catalog::validate_entry(&entry) {
                    Ok(()) => valid.push(entry),
                    Err(err) => {
                        eprintln!(
                            "warning: skipping invalid marketplace entry from '{}': {err}",
                            source.name
                        );
                    }
                }
            }
            Ok(valid)
        }
    }

    fn config_sources(&self) -> Result<Vec<MarketplaceSource>, MarketplaceError> {
        Ok(self
            .load_config()?
            .sources
            .into_iter()
            .map(|source| MarketplaceSource {
                name: source.name,
                url: source.url,
                origin: MarketplaceSourceOrigin::Config,
            })
            .collect())
    }

    fn load_config(&self) -> Result<MarketplaceSourceConfig, MarketplaceError> {
        if !self.config_path.exists() {
            return Ok(MarketplaceSourceConfig::default());
        }
        let text = std::fs::read_to_string(&self.config_path).map_err(|err| {
            MarketplaceError::ConfigIo(self.config_path.display().to_string(), err)
        })?;
        serde_json::from_str(&text).map_err(|err| {
            MarketplaceError::ConfigParse(self.config_path.display().to_string(), err)
        })
    }

    fn save_config(&self, config: &MarketplaceSourceConfig) -> Result<(), MarketplaceError> {
        if let Some(parent) = self.config_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|err| MarketplaceError::ConfigIo(parent.display().to_string(), err))?;
        }
        let text = serde_json::to_string_pretty(config)
            .expect("MarketplaceSourceConfig serialization should not fail");
        std::fs::write(&self.config_path, text)
            .map_err(|err| MarketplaceError::ConfigIo(self.config_path.display().to_string(), err))
    }

    fn load_installed_state(&self) -> Result<MarketplaceInstalledState, MarketplaceError> {
        let path = installed_state_path()?;
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
        let path = installed_state_path()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|err| MarketplaceError::ConfigIo(parent.display().to_string(), err))?;
        }
        let text = serde_json::to_string_pretty(state)
            .expect("MarketplaceInstalledState serialization should not fail");
        fs::write(&path, text)
            .map_err(|err| MarketplaceError::ConfigIo(path.display().to_string(), err))
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

fn default_config_path() -> Result<PathBuf, MarketplaceError> {
    if let Ok(path) = std::env::var(ENV_MARKETPLACE_SOURCES_FILE)
        && !path.trim().is_empty()
    {
        return Ok(PathBuf::from(path));
    }
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .ok_or_else(|| MarketplaceError::ConfigPath("home directory is unavailable".into()))?;
    Ok(PathBuf::from(home)
        .join(".dcc-mcp")
        .join("marketplace")
        .join("sources.json"))
}

fn marketplace_root() -> Result<PathBuf, MarketplaceError> {
    if let Ok(value) = std::env::var("DCC_MCP_MARKETPLACE_INSTALL_ROOT")
        && !value.trim().is_empty()
    {
        return Ok(PathBuf::from(value));
    }
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .ok_or_else(|| MarketplaceError::ConfigPath("home directory is unavailable".into()))?;
    Ok(PathBuf::from(home).join(".dcc-mcp").join("marketplace"))
}

fn marketplace_dcc_dir(dcc: &str) -> Result<PathBuf, MarketplaceError> {
    Ok(marketplace_root()?.join(marketplace_dcc_name(dcc)?))
}

fn installed_state_path() -> Result<PathBuf, MarketplaceError> {
    Ok(marketplace_root()?.join("installed.json"))
}

fn resolve_install_dcc(
    entry: &CatalogEntry,
    requested: Option<&str>,
) -> Result<String, MarketplaceError> {
    if let Some(dcc) = requested {
        let dcc_name = marketplace_dcc_name(dcc)?;
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
        .map(|dcc| marketplace_dcc_name(dcc))
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

fn marketplace_dcc_name(dcc: &str) -> Result<String, MarketplaceError> {
    Ok(marketplace_path_component("DCC name", dcc)?.to_lowercase())
}

fn marketplace_path_component(kind: &str, value: &str) -> Result<String, MarketplaceError> {
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

fn remove_install_path(path: &Path) -> Result<(), MarketplaceError> {
    if path.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
    .map_err(|err| MarketplaceError::ConfigIo(path.display().to_string(), err))
}

fn install_from_path(install: &CatalogInstall, dest: &PathBuf) -> Result<(), MarketplaceError> {
    let source = install_url_path(install)?;
    copy_dir_recursive(&source, dest)
}

fn install_from_git(install: &CatalogInstall, dest: &PathBuf) -> Result<(), MarketplaceError> {
    let url = install
        .url
        .as_deref()
        .ok_or_else(|| MarketplaceError::MissingInstall("git.url".into()))?;
    let mut command = Command::new("git");
    command.arg("clone").arg("--depth").arg("1");
    if let Some(ref_) = install
        .ref_
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
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
            continue;
        }
        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|err| MarketplaceError::ConfigIo(parent.display().to_string(), err))?;
        }
        let mut out = fs::File::create(&out_path)
            .map_err(|err| MarketplaceError::ConfigIo(out_path.display().to_string(), err))?;
        std::io::copy(&mut file, &mut out)
            .map_err(|err| MarketplaceError::ConfigIo(out_path.display().to_string(), err))?;
    }
    Ok(())
}

fn flatten_single_skill_directory(dest: &PathBuf) -> Result<(), MarketplaceError> {
    if dest.join("SKILL.md").is_file() {
        return Ok(());
    }

    let child_dirs = fs::read_dir(dest)
        .map_err(|err| MarketplaceError::Read(dest.display().to_string(), err))?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            entry
                .file_type()
                .ok()
                .filter(|file_type| file_type.is_dir())
                .map(|_| entry.path())
        })
        .collect::<Vec<_>>();

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
        .map_err(|err| MarketplaceError::Read(flatten_root.display().to_string(), err))?
    {
        let entry =
            entry.map_err(|err| MarketplaceError::Read(flatten_root.display().to_string(), err))?;
        let target = dest.join(entry.file_name());
        fs::rename(entry.path(), &target)
            .map_err(|err| MarketplaceError::ConfigIo(target.display().to_string(), err))?;
    }
    remove_install_path(&flatten_root)?;
    Ok(())
}

fn install_url_path(install: &CatalogInstall) -> Result<PathBuf, MarketplaceError> {
    let url = install
        .url
        .as_deref()
        .ok_or_else(|| MarketplaceError::MissingInstall("path.url".into()))?;
    Ok(url
        .strip_prefix("file://")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(url)))
}

fn copy_dir_recursive(src: &PathBuf, dest: &PathBuf) -> Result<(), MarketplaceError> {
    if !src.join("SKILL.md").is_file() {
        return Err(MarketplaceError::MissingSkill(
            src.join("SKILL.md").display().to_string(),
        ));
    }
    fs::create_dir_all(dest)
        .map_err(|err| MarketplaceError::ConfigIo(dest.display().to_string(), err))?;
    for entry in
        fs::read_dir(src).map_err(|err| MarketplaceError::Read(src.display().to_string(), err))?
    {
        let entry = entry.map_err(|err| MarketplaceError::Read(src.display().to_string(), err))?;
        let src_path = entry.path();
        let dest_path = dest.join(entry.file_name());
        let file_type = entry
            .file_type()
            .map_err(|err| MarketplaceError::Read(src_path.display().to_string(), err))?;
        if file_type.is_dir() {
            copy_dir_recursive_unchecked(&src_path, &dest_path)?;
        } else if file_type.is_file() {
            fs::copy(&src_path, &dest_path)
                .map_err(|err| MarketplaceError::ConfigIo(dest_path.display().to_string(), err))?;
        }
    }
    Ok(())
}

fn copy_dir_recursive_unchecked(src: &PathBuf, dest: &PathBuf) -> Result<(), MarketplaceError> {
    fs::create_dir_all(dest)
        .map_err(|err| MarketplaceError::ConfigIo(dest.display().to_string(), err))?;
    for entry in
        fs::read_dir(src).map_err(|err| MarketplaceError::Read(src.display().to_string(), err))?
    {
        let entry = entry.map_err(|err| MarketplaceError::Read(src.display().to_string(), err))?;
        let src_path = entry.path();
        let dest_path = dest.join(entry.file_name());
        let file_type = entry
            .file_type()
            .map_err(|err| MarketplaceError::Read(src_path.display().to_string(), err))?;
        if file_type.is_dir() {
            copy_dir_recursive_unchecked(&src_path, &dest_path)?;
        } else if file_type.is_file() {
            fs::copy(&src_path, &dest_path)
                .map_err(|err| MarketplaceError::ConfigIo(dest_path.display().to_string(), err))?;
        }
    }
    Ok(())
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0)
}

fn env_sources() -> Vec<MarketplaceSource> {
    std::env::var(ENV_MARKETPLACE_SOURCES)
        .ok()
        .into_iter()
        .flat_map(|value| {
            value
                .split(',')
                .map(str::trim)
                .filter(|source| !source.is_empty())
                .map(|source| normalise_source(source, MarketplaceSourceOrigin::Env))
                .collect::<Vec<_>>()
        })
        .collect()
}

fn default_sources_disabled() -> bool {
    std::env::var(ENV_MARKETPLACE_NO_DEFAULT_SOURCES)
        .map(|value| matches!(value.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"))
        .unwrap_or(false)
}

/// Merge marketplace hits, keeping only the first occurrence of each entry name.
///
/// Input `hits` must be ordered by source priority (highest priority first).
/// Returns a deduplicated vector where duplicates (same entry name) retain the
/// hit from the highest-priority source.
fn merge_entries(hits: Vec<MarketplaceHit>) -> Vec<MarketplaceHit> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for hit in hits {
        if seen.insert(hit.entry.name.clone()) {
            out.push(hit);
        }
    }
    out
}

fn dedupe_sources(sources: Vec<MarketplaceSource>) -> Vec<MarketplaceSource> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for source in sources {
        if seen.insert(source.url.clone()) {
            out.push(source);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn add_source_persists_to_config_file() {
        let dir = tempdir().unwrap();
        let service = MarketplaceService::with_config_path(dir.path().join("sources.json"));

        let sources = service.add_source("studio/private").unwrap();

        assert!(sources.iter().any(|source| source.name == "studio/private"));
        let saved = std::fs::read_to_string(dir.path().join("sources.json")).unwrap();
        assert!(saved.contains("studio/private"));
    }

    fn catalog_entry(name: &str, description: &str) -> dcc_mcp_catalog::CatalogEntry {
        dcc_mcp_catalog::CatalogEntry {
            name: name.into(),
            description: description.into(),
            dcc: vec![],
            url: None,
            tags: vec![],
            version: None,
            min_core_version: None,
            install: None,
            maintainer: None,
            icon: None,
        }
    }

    fn marketplace_source(
        name: &str,
        url: &str,
        origin: MarketplaceSourceOrigin,
    ) -> MarketplaceSource {
        MarketplaceSource {
            name: name.into(),
            url: url.into(),
            origin,
        }
    }

    #[test]
    fn merge_entries_keeps_highest_priority_source() {
        let hits = vec![
            MarketplaceHit {
                source: marketplace_source("explicit", "url1", MarketplaceSourceOrigin::Explicit),
                entry: catalog_entry("my-skill", "From explicit source"),
            },
            MarketplaceHit {
                source: marketplace_source("env", "url2", MarketplaceSourceOrigin::Env),
                entry: catalog_entry("my-skill", "From env source"),
            },
        ];
        let merged = merge_entries(hits);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].entry.description, "From explicit source");
        assert_eq!(merged[0].source.origin, MarketplaceSourceOrigin::Explicit);
    }

    #[test]
    fn merge_entries_preserves_unique_entries() {
        let hits = vec![
            MarketplaceHit {
                source: marketplace_source("s1", "url1", MarketplaceSourceOrigin::Builtin),
                entry: catalog_entry("skill-a", "First skill"),
            },
            MarketplaceHit {
                source: marketplace_source("s2", "url2", MarketplaceSourceOrigin::Builtin),
                entry: catalog_entry("skill-b", "Second skill"),
            },
        ];
        let merged = merge_entries(hits);
        assert_eq!(merged.len(), 2);
    }

    #[test]
    fn merge_entries_empty_input() {
        let merged = merge_entries(vec![]);
        assert!(merged.is_empty());
    }

    #[test]
    fn list_sources_sorts_by_priority() {
        let dir = tempdir().unwrap();
        let config_path = dir.path().join("sources.json");
        let service = MarketplaceService::with_config_path(config_path.clone());

        // Add a config source (priority 2)
        std::fs::write(
            &config_path,
            r#"{"sources":[{"name":"custom","url":"https://example.com/marketplace.json"}]}"#,
        )
        .unwrap();

        // Set env source (priority 1)
        // SAFETY: single-threaded test environment
        unsafe {
            std::env::set_var("DCC_MCP_MARKETPLACE_SOURCES", "studio/private");
        }
        // Disable default so builtin source does not appear
        // SAFETY: single-threaded test environment
        unsafe {
            std::env::set_var("DCC_MCP_MARKETPLACE_NO_DEFAULT_SOURCES", "1");
        }

        let sources = service.list_sources().unwrap();

        // Env (priority 1) should come before Config (priority 2)
        let env_idx = sources
            .iter()
            .position(|s| s.origin == MarketplaceSourceOrigin::Env);
        let config_idx = sources
            .iter()
            .position(|s| s.origin == MarketplaceSourceOrigin::Config);
        assert!(env_idx.is_some());
        assert!(config_idx.is_some());
        assert!(
            env_idx.unwrap() < config_idx.unwrap(),
            "Env sources (priority 1) must come before Config sources (priority 2)"
        );

        // SAFETY: single-threaded test environment
        unsafe {
            std::env::remove_var("DCC_MCP_MARKETPLACE_SOURCES");
            std::env::remove_var("DCC_MCP_MARKETPLACE_NO_DEFAULT_SOURCES");
        }
    }
}
