use std::collections::BTreeSet;
use std::path::PathBuf;
use std::time::Duration;

use thiserror::Error;

use crate::domain::marketplace::{
    MarketplaceHit, MarketplaceInspectResult, MarketplaceSearchResult, MarketplaceSource,
    MarketplaceSourceConfig, MarketplaceSourceOrigin, StoredMarketplaceSource, builtin_source,
    entry_targets_dcc, normalise_source,
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
