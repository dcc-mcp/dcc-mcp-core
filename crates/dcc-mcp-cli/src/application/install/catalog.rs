//! Refresh a signed catalog independently of the CLI release lifecycle.

use std::collections::HashSet;
use std::io::{Read, Write};
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use dcc_mcp_attestation::{GitHubAttestationPolicy, verify_attested_bytes};
use dcc_mcp_catalog::CatalogEntry;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::domain::install_catalog::{CatalogProvenance, CatalogSource};

const FEED_URL: &str =
    "https://raw.githubusercontent.com/dcc-mcp/dcc-mcp-core/install-catalog/install-catalog.json";
const WORKFLOW: &str = "https://github.com/dcc-mcp/dcc-mcp-core/.github/workflows/publish-install-catalog.yml@refs/heads/main";
const MAX_BYTES: usize = 4 * 1024 * 1024;
const MAX_LIFETIME: u64 = 7 * 24 * 60 * 60;

#[derive(Debug, Clone)]
pub(super) struct ResolvedCatalog {
    pub entries: Vec<CatalogEntry>,
    pub provenance: CatalogProvenance,
}

impl ResolvedCatalog {
    fn bundled() -> Result<Self, dcc_mcp_catalog::CatalogError> {
        Ok(Self {
            entries: dcc_mcp_catalog::load_from_str(super::BUNDLED_CATALOG)?,
            provenance: CatalogProvenance {
                sha256: Some(
                    Sha256::digest(super::BUNDLED_CATALOG.as_bytes())
                        .iter()
                        .map(|byte| format!("{byte:02x}"))
                        .collect(),
                ),
                ..CatalogProvenance::local(CatalogSource::Bundled)
            },
        })
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    catalog: String,
    attestation: serde_json::Value,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Payload {
    schema_version: u8,
    source_revision: String,
    issued_at: u64,
    expires_at: u64,
    entries: Vec<CatalogEntry>,
}

#[derive(Debug, thiserror::Error)]
pub(super) enum RefreshError {
    #[error("official installation catalog is unavailable")]
    Unavailable,
    #[error("installation catalog rejected: {0}")]
    Invalid(String),
    #[error("installation catalog cache could not be persisted")]
    CacheWrite,
}

fn invalid(message: impl ToString) -> RefreshError {
    RefreshError::Invalid(message.to_string())
}

impl super::InstallService {
    /// Resolve once; discovery, planning, and execution share these exact bytes.
    pub async fn refreshed(offline: bool) -> Self {
        let mut service = Self::bundled();
        service.require_fresh_catalog = !offline;
        match resolve(offline).await {
            Ok(snapshot) => service.catalog_snapshot = Some(snapshot),
            Err(error) => service.catalog_error = Some(error.to_string()),
        }
        service
    }

    pub fn catalog_provenance(&self, requested_path: Option<&Path>) -> CatalogProvenance {
        self.load_catalog(requested_path)
            .map(|catalog| catalog.provenance)
            .unwrap_or_else(|_| CatalogProvenance::local(CatalogSource::Unavailable))
    }

    pub(super) fn load_entries(
        &self,
        requested_path: Option<&Path>,
    ) -> Result<Vec<CatalogEntry>, super::InstallError> {
        self.load_catalog(requested_path)
            .map(|catalog| catalog.entries)
    }

    pub(super) fn load_catalog(
        &self,
        requested_path: Option<&Path>,
    ) -> Result<ResolvedCatalog, super::InstallError> {
        if let Some(path) = requested_path {
            return Ok(ResolvedCatalog {
                entries: dcc_mcp_catalog::load_from_file(path)?,
                provenance: CatalogProvenance::local(CatalogSource::Explicit),
            });
        }
        if let Some(error) = &self.catalog_error {
            return Err(super::InstallError::CatalogRefresh(error.clone()));
        }
        if let Some(snapshot) = &self.catalog_snapshot {
            return Ok(snapshot.clone());
        }
        if let Some(path) = &self.default_catalog_path {
            let entries = dcc_mcp_catalog::load_from_file(path)?;
            if !entries.is_empty() {
                return Ok(ResolvedCatalog {
                    entries,
                    provenance: CatalogProvenance::local(CatalogSource::Explicit),
                });
            }
        }
        ResolvedCatalog::bundled().map_err(Into::into)
    }
}

pub(super) async fn resolve(offline: bool) -> Result<ResolvedCatalog, RefreshError> {
    let cache = match std::env::var_os("DCC_MCP_INSTALL_CACHE") {
        Some(path) => std::path::PathBuf::from(path),
        None => dirs::cache_dir()
            .ok_or_else(|| invalid("cache directory is unavailable"))?
            .join("dcc-mcp/install-catalog-v1.json"),
    };
    let now = now_seconds()?;
    let cached_bytes = read_cache(&cache)?;
    // Authentication and rollback history remain mandatory even if the cached
    // payload has expired. A fresh remote payload may replace an expired one.
    let cached = cached_bytes
        .as_deref()
        .map(|bytes| verify(bytes, now, false))
        .transpose()?;
    if !offline {
        match download().await {
            Ok(bytes) => {
                let now = now_seconds()?;
                let remote = verify(&bytes, now, true)?;
                if let Some(previous) = &cached {
                    reject_rollback(previous, &remote)?;
                }
                persist_verified(&cache, &bytes, &remote, now)?;
                return Ok(remote);
            }
            Err(RefreshError::Unavailable) => {}
            Err(error) => return Err(error),
        }
    }
    if let Some(mut snapshot) = cached {
        if snapshot
            .provenance
            .expires_at
            .is_none_or(|expires| now_seconds().map_or(true, |now| expires <= now))
        {
            return Err(invalid("cached catalog expired; reconnect to refresh"));
        }
        snapshot.provenance.source = CatalogSource::Cache;
        snapshot.provenance.latest_checked = false;
        return Ok(snapshot);
    }
    ResolvedCatalog::bundled().map_err(invalid)
}

pub(super) fn now_seconds() -> Result<u64, RefreshError> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(invalid)?
        .as_secs())
}

async fn download() -> Result<Vec<u8>, RefreshError> {
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(invalid)?;
    let mut response = client
        .get(FEED_URL)
        .header(reqwest::header::USER_AGENT, "dcc-mcp-cli install-catalog")
        .header(reqwest::header::CACHE_CONTROL, "no-cache")
        .send()
        .await
        .map_err(|_| RefreshError::Unavailable)?;
    if response.status() == reqwest::StatusCode::NOT_FOUND || response.status().is_server_error() {
        return Err(RefreshError::Unavailable);
    }
    if !response.status().is_success() {
        return Err(invalid("unexpected HTTP response from official channel"));
    }
    if response
        .content_length()
        .is_some_and(|length| length > MAX_BYTES as u64)
    {
        return Err(invalid("envelope exceeds size limit"));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| RefreshError::Unavailable)?
    {
        if bytes.len() + chunk.len() > MAX_BYTES {
            return Err(invalid("envelope exceeds size limit"));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn verify(bytes: &[u8], now: u64, require_current: bool) -> Result<ResolvedCatalog, RefreshError> {
    if bytes.len() > MAX_BYTES {
        return Err(invalid("envelope exceeds size limit"));
    }
    let envelope: Envelope = serde_json::from_slice(bytes).map_err(invalid)?;
    let evidence = verify_attested_bytes(
        envelope.catalog.as_bytes(),
        &envelope.attestation.to_string(),
        &GitHubAttestationPolicy {
            workflow_identity: WORKFLOW.into(),
        },
    )
    .map_err(|_| invalid("signature, workflow identity, or digest verification failed"))?;
    let payload: Payload = serde_json::from_str(&envelope.catalog).map_err(invalid)?;
    validate_payload(&payload, now, require_current)?;
    let signed_at = evidence
        .integrated_time
        .and_then(|time| u64::try_from(time).ok())
        .ok_or_else(|| invalid("attestation has no valid transparency-log time"))?;
    if signed_at > now.saturating_add(300)
        || signed_at < payload.issued_at.saturating_sub(300)
        || signed_at > payload.expires_at
    {
        return Err(invalid(
            "attestation time does not match the catalog lifetime",
        ));
    }
    Ok(ResolvedCatalog {
        entries: payload.entries,
        provenance: CatalogProvenance {
            source: CatalogSource::Remote,
            latest_checked: true,
            sha256: Some(evidence.sha256),
            source_revision: Some(payload.source_revision),
            issued_at: Some(payload.issued_at),
            expires_at: Some(payload.expires_at),
        },
    })
}

fn validate_payload(
    payload: &Payload,
    now: u64,
    require_current: bool,
) -> Result<(), RefreshError> {
    if payload.schema_version != 1 || payload.entries.is_empty() {
        return Err(invalid("unsupported schema or empty catalog"));
    }
    if payload.source_revision.len() != 40
        || !payload
            .source_revision
            .bytes()
            .all(|b| b.is_ascii_hexdigit())
    {
        return Err(invalid("source revision must be a full commit SHA"));
    }
    if payload.issued_at > now.saturating_add(300)
        || payload.expires_at <= payload.issued_at
        || payload.expires_at - payload.issued_at > MAX_LIFETIME
        || (require_current && payload.expires_at <= now)
    {
        return Err(invalid("catalog is expired or has an invalid lifetime"));
    }
    let mut names = HashSet::new();
    if payload
        .entries
        .iter()
        .any(|entry| !names.insert(entry.name.to_lowercase()))
    {
        return Err(invalid("duplicate package names"));
    }
    dcc_mcp_catalog::validate_catalog_entries(&payload.entries).map_err(invalid)?;
    Ok(())
}

fn reject_rollback(
    previous: &ResolvedCatalog,
    remote: &ResolvedCatalog,
) -> Result<(), RefreshError> {
    let old = &previous.provenance;
    let new = &remote.provenance;
    if new.issued_at < old.issued_at || (new.issued_at == old.issued_at && new.sha256 != old.sha256)
    {
        return Err(invalid("catalog rollback or conflicting revision"));
    }
    Ok(())
}

fn read_cache(path: &Path) -> Result<Option<Vec<u8>>, RefreshError> {
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(invalid("cache cannot be read")),
    };
    let mut bytes = Vec::new();
    file.take(MAX_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(invalid)?;
    if bytes.len() > MAX_BYTES {
        return Err(invalid("cached envelope exceeds size limit"));
    }
    Ok(Some(bytes))
}

fn persist(path: &Path, bytes: &[u8]) -> Result<(), RefreshError> {
    let parent = path.parent().ok_or(RefreshError::CacheWrite)?;
    std::fs::create_dir_all(parent).map_err(|_| RefreshError::CacheWrite)?;
    let mut file = tempfile::NamedTempFile::new_in(parent).map_err(|_| RefreshError::CacheWrite)?;
    file.write_all(bytes)
        .map_err(|_| RefreshError::CacheWrite)?;
    file.as_file()
        .sync_all()
        .map_err(|_| RefreshError::CacheWrite)?;
    file.persist(path).map_err(|_| RefreshError::CacheWrite)?;
    Ok(())
}

fn persist_verified(
    path: &Path,
    bytes: &[u8],
    remote: &ResolvedCatalog,
    now: u64,
) -> Result<(), RefreshError> {
    let parent = path.parent().ok_or(RefreshError::CacheWrite)?;
    std::fs::create_dir_all(parent).map_err(|_| RefreshError::CacheWrite)?;
    let lock = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path.with_extension("lock"))
        .map_err(|_| RefreshError::CacheWrite)?;
    lock.lock().map_err(|_| RefreshError::CacheWrite)?;
    if remote
        .provenance
        .expires_at
        .is_none_or(|expires| now_seconds().map_or(true, |now| expires <= now))
    {
        return Err(invalid("catalog expired before cache acceptance"));
    }
    // A concurrent invocation may have accepted a newer snapshot during fetch.
    if let Some(current) = read_cache(path)? {
        reject_rollback(&verify(&current, now, false)?, remote)?;
    }
    persist(path, bytes)
}

#[cfg(test)]
mod tests;
