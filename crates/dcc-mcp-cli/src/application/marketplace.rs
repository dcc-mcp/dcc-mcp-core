use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

use dcc_mcp_catalog::{CatalogEntry, CatalogInstall};
use thiserror::Error;

use crate::domain::marketplace::{
    InstalledMarketplacePackage, MarketplaceHit, MarketplaceInspectResult,
    MarketplaceInstallResult, MarketplaceInstalledList, MarketplaceInstalledState,
    MarketplaceSearchResult, MarketplaceSource, MarketplaceSourceConfig, MarketplaceSourceOrigin,
    MarketplaceUninstallResult, StoredMarketplaceSource, builtin_source, entry_targets_dcc,
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
        Ok(dedupe_sources(sources))
    }

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
            "zip" => return Err(MarketplaceError::UnsupportedInstallType("zip".into())),
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
                .map(|source| normalise_source(source, MarketplaceSourceOrigin::Explicit))
                .collect(),
        ))
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
        dcc_mcp_catalog::load_from_str(&text).map_err(Into::into)
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
}
