//! Shared [`MarketplaceService`] — catalog fetch, install/uninstall, source
//! management, installed state persistence, and integrity verification.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use dcc_mcp_catalog::{self, CatalogEntry, CatalogInstall};

use crate::bundle::{bundle_package_dir, install_staged_package, remove_installed_path};
use crate::error::MarketplaceError;
use crate::source::{builtin_source, dedupe_sources, normalise_source};
use crate::types::{
    InstalledMarketplacePackage, MarketplaceHit, MarketplaceInspectResult,
    MarketplaceInstallResult, MarketplaceInstalledList, MarketplaceInstalledState,
    MarketplaceOutdatedList, MarketplaceSearchResult, MarketplaceSource, MarketplaceSourceOrigin,
    MarketplaceUninstallResult, MarketplaceUpdateResult, OutdatedMarketplacePackage,
    RepoInstallResult, RepoSkillList, StoredMarketplaceSource, entry_targets_dcc,
};

#[path = "service_internals.rs"]
mod service_internals;
use service_internals::*;
pub(crate) use service_internals::{
    copy_dir_recursive, promote_single_nested_skill_directory, remove_path, write_atomic,
};
pub use service_internals::{default_sources_disabled, env_sources, path_component};

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
        skip_validation: bool,
    ) -> Result<MarketplaceSearchResult, MarketplaceError> {
        let sources = self.sources_for_query(explicit_sources)?;
        let mut hits = Vec::new();
        for source in sources {
            let entries = self
                .load_source_entries_validated(&source, !skip_validation)
                .await?;
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
        skip_validation: bool,
    ) -> Result<MarketplaceInspectResult, MarketplaceError> {
        let sources = self.sources_for_query(explicit_sources)?;
        let mut matches = Vec::new();
        for source in sources {
            let entries = self
                .load_source_entries_validated(&source, !skip_validation)
                .await?;
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
        skip_validation: bool,
    ) -> Result<MarketplaceInstallResult, MarketplaceError> {
        let hit = self
            .resolve_install_hit(&name, dcc.as_deref(), explicit_sources, skip_validation)
            .await?;
        ensure_core_version_compatible(&hit.entry)?;
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
        let bundle_dest = bundle_package_dir(&dcc_root, &package_name);
        if bundle_dest.exists() && !force {
            return Err(MarketplaceError::AlreadyInstalled {
                name: package_name.clone(),
                dcc: dcc.clone(),
                path: bundle_dest.display().to_string(),
            });
        }
        fs::create_dir_all(&dcc_root)
            .map_err(|err| MarketplaceError::ConfigIo(dcc_root.display().to_string(), err))?;

        let staging = dcc_root.join(format!(".{package_name}.installing-{}", now_ms()));
        if staging.exists() {
            remove_path(&staging)?;
        }

        let install_result = match install.install_type.as_str() {
            "git" => self.install_from_git(&install, &staging).await,
            "path" => install_from_path(&install, &staging),
            "zip" => self.install_from_zip(&install, &staging).await,
            other => return Err(MarketplaceError::UnsupportedInstallType(other.into())),
        };
        if let Err(err) = install_result {
            let _ = remove_path(&staging);
            return Err(err);
        }

        let final_path = match install_staged_package(
            &staging,
            &dest,
            &dcc_root,
            &package_name,
            &dcc,
            install.skill_roots.as_deref(),
            force,
        ) {
            Ok(path) => path,
            Err(err) => {
                let _ = remove_path(&staging);
                return Err(err);
            }
        };
        let resolved_commit = resolved_git_commit(&install, &final_path);

        let package = InstalledMarketplacePackage {
            name: package_name.clone(),
            dcc: dcc.clone(),
            version: hit.entry.version.clone(),
            path: final_path.display().to_string(),
            source_name: hit.source.name.clone(),
            source_url: hit.source.url.clone(),
            install_type: install.install_type.clone(),
            install_url: install.url.clone(),
            install_ref: install.ref_.clone(),
            resolved_commit: resolved_commit.clone(),
            installed_at_ms: now_ms(),
        };
        self.upsert_installed(package)?;

        Ok(MarketplaceInstallResult {
            installed: true,
            name: package_name,
            dcc,
            version: hit.entry.version.clone(),
            path: final_path.display().to_string(),
            skill_search_path: dcc_root.display().to_string(),
            source: hit.source,
            entry: hit.entry,
            install_type: install.install_type.clone(),
            resolved_commit,
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
        let dest = self
            .load_installed_state()?
            .packages
            .into_iter()
            .find(|package| package.name == name && package.dcc.eq_ignore_ascii_case(dcc))
            .map(|package| PathBuf::from(package.path))
            .unwrap_or_else(|| dcc_root.join(&name));
        let removed_files = if dest.exists() {
            remove_installed_path(&dcc_root, &dest)?;
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
            let (is_outdated, latest_commit) = is_entry_outdated(entry.as_ref(), &pkg);
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
                    installed_commit: pkg.resolved_commit,
                    latest_commit,
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
            let previous_commit = pkg.installed_commit.clone();

            let update_result = match pkg.install_type.as_str() {
                "git" => self.update_git_package(&pkg, &dest).await,
                _ => self
                    .install(
                        pkg.name.clone(),
                        Some(pkg.dcc.clone()),
                        vec![pkg.source_url.clone()],
                        true,
                        false,
                    )
                    .await
                    .map(|result| MarketplaceUpdateResult {
                        updated: true,
                        name: pkg.name.clone(),
                        dcc: pkg.dcc.clone(),
                        previous_version,
                        new_version: result.version,
                        previous_commit,
                        new_commit: result.resolved_commit,
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
                    resolved_commit: update_result.new_commit.clone(),
                    installed_at_ms: now_ms(),
                })?;
            }
            results.push(update_result);
        }
        Ok(results)
    }

    // ── add-repo (direct GitHub install) ──────────────────────────────────────

    /// List SKILL.md entries from a GitHub repo without installing.
    pub fn list_repo_skills(&self, repo_ref: &str) -> Result<RepoSkillList, MarketplaceError> {
        crate::add_repo::list_repo_skills(repo_ref)
    }

    /// Install a skill directly from a GitHub repo (no marketplace.json needed).
    pub fn add_repo(
        &self,
        repo_ref: &str,
        dcc: Option<&str>,
        force: bool,
    ) -> Result<RepoInstallResult, MarketplaceError> {
        crate::add_repo::install_from_repo(repo_ref, dcc, force, &self.root)
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

    /// Same as [`load_source_entries`] but validates entries against the
    /// marketplace-v1 JSON Schema. When `validate` is true, invalid entries
    /// cause a hard error. When false, invalid entries are silently filtered out.
    async fn load_source_entries_validated(
        &self,
        source: &MarketplaceSource,
        validate: bool,
    ) -> Result<Vec<CatalogEntry>, MarketplaceError> {
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

    async fn resolve_install_hit(
        &self,
        name: &str,
        dcc: Option<&str>,
        explicit_sources: Vec<String>,
        skip_validation: bool,
    ) -> Result<MarketplaceHit, MarketplaceError> {
        let sources = self.sources_for_query(explicit_sources)?;
        for source in sources {
            let entries = self
                .load_source_entries_validated(&source, !skip_validation)
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

    async fn install_from_git(
        &self,
        install: &CatalogInstall,
        dest: &Path,
    ) -> Result<(), MarketplaceError> {
        if let Some(url) = github_archive_url(install) {
            let archive_install = CatalogInstall {
                install_type: "zip".into(),
                url: Some(url),
                ref_: None,
                sha256: None,
                skill_roots: None,
                pip_package: None,
                pip_extras: None,
                python_path: None,
                entry_point: None,
                instructions_url: None,
            };
            return self.install_from_zip(&archive_install, dest).await;
        }
        install_from_git_command(install, dest)
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
        let latest_entry = self
            .find_latest_entry_for_package(
                &self.list_sources()?,
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
                    resolved_commit: pkg.installed_commit.clone(),
                    installed_at_ms: 0,
                },
            )
            .await?;
        if let Some(entry) = latest_entry.as_ref() {
            ensure_core_version_compatible(entry)?;
        }

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
                    false,
                )
                .await?;
            return Ok(MarketplaceUpdateResult {
                updated: true,
                name: pkg.name.clone(),
                dcc: pkg.dcc.clone(),
                previous_version: pkg.installed_version.clone(),
                new_version: result.version,
                previous_commit: pkg.installed_commit.clone(),
                new_commit: result.resolved_commit,
                path: result.path,
                install_type: result.install_type,
                source_name: pkg.source_name.clone(),
                source_url: pkg.source_url.clone(),
                reload_required: true,
            });
        }

        let new_version = latest_entry.and_then(|entry| entry.version);

        Ok(MarketplaceUpdateResult {
            updated: true,
            name: pkg.name.clone(),
            dcc: pkg.dcc.clone(),
            previous_version: pkg.installed_version.clone(),
            new_version,
            previous_commit: pkg.installed_commit.clone(),
            new_commit: pkg.latest_commit.clone().or_else(|| git_head_commit(dest)),
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

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "service_tests.rs"]
mod service_tests;

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
            resolved_commit: None,
            installed_at_ms: 1000,
        };
        svc.upsert_installed(pkg.clone()).unwrap();
        let list = svc.list_installed(None).unwrap();
        assert_eq!(list.count, 1);
        assert_eq!(list.packages[0].name, "test-skill");
    }

    #[test]
    fn github_archive_url_converts_https_git_url() {
        let install = CatalogInstall {
            install_type: "git".into(),
            url: Some("https://github.com/dcc-mcp/dcc-mcp-maya-mgear.git".into()),
            ref_: Some("main".into()),
            sha256: None,
            skill_roots: None,
            pip_package: None,
            pip_extras: None,
            python_path: None,
            entry_point: None,
            instructions_url: None,
        };

        assert_eq!(
            github_archive_url(&install).as_deref(),
            Some("https://github.com/dcc-mcp/dcc-mcp-maya-mgear/archive/main.zip")
        );
    }

    #[test]
    fn github_archive_url_leaves_ssh_git_for_git_command() {
        let install = CatalogInstall {
            install_type: "git".into(),
            url: Some("git@github.com:dcc-mcp/private-pack.git".into()),
            ref_: Some("main".into()),
            sha256: None,
            skill_roots: None,
            pip_package: None,
            pip_extras: None,
            python_path: None,
            entry_point: None,
            instructions_url: None,
        };

        assert_eq!(github_archive_url(&install), None);
    }

    #[test]
    fn default_sources_disabled_false_when_unset() {
        let _guard = dcc_mcp_test_utils::EnvVarGuard::set(ENV_MARKETPLACE_NO_DEFAULT_SOURCES, None);
        assert!(!default_sources_disabled());
    }

    #[test]
    fn default_sources_disabled_respects_truthy_values() {
        for v in ["1", "true", "TRUE", "yes", "YES"] {
            let _g =
                dcc_mcp_test_utils::EnvVarGuard::set(ENV_MARKETPLACE_NO_DEFAULT_SOURCES, Some(v));
            assert!(default_sources_disabled(), "expected true for '{v}'");
        }
        for v in ["0", "false", "no", "", "FALSE", "NO"] {
            let _g =
                dcc_mcp_test_utils::EnvVarGuard::set(ENV_MARKETPLACE_NO_DEFAULT_SOURCES, Some(v));
            assert!(!default_sources_disabled(), "expected false for '{v}'");
        }
    }
}
