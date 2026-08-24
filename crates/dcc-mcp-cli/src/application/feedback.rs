use std::fs;
use std::path::{Path, PathBuf};

use dcc_mcp_models::FindingV1;
use thiserror::Error;

use crate::domain::feedback::{FeedbackRoute, FeedbackRouteError, route_finding};

const BUNDLED_CATALOG: &str = include_str!("../../../../dcc-mcp-catalog.yml");
const MAX_FINDING_FILE_BYTES: u64 = 512 * 1024;

pub(crate) struct FeedbackRouteSnapshot {
    pub canonical_finding_path: PathBuf,
    pub finding_bytes: Vec<u8>,
    pub finding: FindingV1,
    pub canonical_catalog_path: Option<PathBuf>,
    pub catalog_bytes: Vec<u8>,
    pub route: FeedbackRoute,
}

struct FindingSnapshot {
    canonical_path: PathBuf,
    bytes: Vec<u8>,
    finding: FindingV1,
}

struct CatalogSnapshot {
    canonical_path: Option<PathBuf>,
    bytes: Vec<u8>,
    entries: Vec<dcc_mcp_catalog::CatalogEntry>,
}

/// Offline application service for deterministic Finding v1 routing.
pub struct FeedbackRouteService {
    default_catalog_path: PathBuf,
}

impl FeedbackRouteService {
    #[must_use]
    pub fn new(default_catalog_path: PathBuf) -> Self {
        Self {
            default_catalog_path,
        }
    }

    pub fn route(
        &self,
        finding_path: &Path,
        catalog_path: Option<&Path>,
    ) -> Result<FeedbackRoute, FeedbackRouteServiceError> {
        Ok(self.snapshot(finding_path, catalog_path)?.route)
    }

    pub(crate) fn snapshot(
        &self,
        finding_path: &Path,
        catalog_path: Option<&Path>,
    ) -> Result<FeedbackRouteSnapshot, FeedbackRouteServiceError> {
        let finding = read_finding_snapshot(finding_path)?;
        let catalog = self.load_catalog_snapshot(catalog_path)?;
        dcc_mcp_catalog::validate_catalog_entries(&catalog.entries)
            .map_err(|error| FeedbackRouteServiceError::InvalidCatalog(error.to_string()))?;
        let route = route_finding(&finding.finding, &catalog.entries)?;
        Ok(FeedbackRouteSnapshot {
            canonical_finding_path: finding.canonical_path,
            finding_bytes: finding.bytes,
            finding: finding.finding,
            canonical_catalog_path: catalog.canonical_path,
            catalog_bytes: catalog.bytes,
            route,
        })
    }

    fn load_catalog_snapshot(
        &self,
        requested_path: Option<&Path>,
    ) -> Result<CatalogSnapshot, FeedbackRouteServiceError> {
        if let Some(path) = requested_path {
            if !path.is_file() {
                return Err(FeedbackRouteServiceError::InvalidCatalogPath(
                    path.display().to_string(),
                ));
            }
            let canonical = canonical_catalog_path(path)?;
            let bytes = read_catalog_bytes(&canonical)?;
            let entries = parse_catalog_bytes(&bytes)?;
            return Ok(CatalogSnapshot {
                canonical_path: Some(canonical),
                bytes,
                entries,
            });
        }
        if self.default_catalog_path.exists() {
            let canonical = canonical_catalog_path(&self.default_catalog_path)?;
            let bytes = read_catalog_bytes(&canonical)?;
            let entries = parse_catalog_bytes(&bytes)?;
            if !entries.is_empty() {
                return Ok(CatalogSnapshot {
                    canonical_path: Some(canonical),
                    bytes,
                    entries,
                });
            }
        }
        let bytes = BUNDLED_CATALOG.as_bytes().to_vec();
        let entries = dcc_mcp_catalog::load_from_str(BUNDLED_CATALOG)?;
        Ok(CatalogSnapshot {
            canonical_path: None,
            bytes,
            entries,
        })
    }
}

#[derive(Debug, Error)]
pub enum FeedbackRouteServiceError {
    #[error("finding path is not a regular file: {0}")]
    InvalidFindingPath(String),
    #[error("finding file exceeds the {MAX_FINDING_FILE_BYTES}-byte limit: {0}")]
    FindingTooLarge(String),
    #[error("could not read finding file '{path}': {source}")]
    ReadFinding {
        path: String,
        source: std::io::Error,
    },
    #[error("finding file '{path}' is not valid Finding v1 JSON: {source}")]
    ParseFinding {
        path: String,
        source: serde_json::Error,
    },
    #[error("catalog path is not a regular file: {0}")]
    InvalidCatalogPath(String),
    #[error("could not read catalog file '{path}': {source}")]
    ReadCatalog {
        path: String,
        source: std::io::Error,
    },
    #[error("catalog failed validation: {0}")]
    InvalidCatalog(String),
    #[error(transparent)]
    Catalog(#[from] dcc_mcp_catalog::CatalogError),
    #[error(transparent)]
    Route(#[from] FeedbackRouteError),
}

pub(crate) fn read_finding(path: &Path) -> Result<FindingV1, FeedbackRouteServiceError> {
    Ok(read_finding_snapshot(path)?.finding)
}

fn read_finding_snapshot(path: &Path) -> Result<FindingSnapshot, FeedbackRouteServiceError> {
    let canonical =
        fs::canonicalize(path).map_err(|source| FeedbackRouteServiceError::ReadFinding {
            path: path.display().to_string(),
            source,
        })?;
    let metadata =
        fs::metadata(&canonical).map_err(|source| FeedbackRouteServiceError::ReadFinding {
            path: path.display().to_string(),
            source,
        })?;
    if !metadata.is_file() {
        return Err(FeedbackRouteServiceError::InvalidFindingPath(
            path.display().to_string(),
        ));
    }
    if metadata.len() > MAX_FINDING_FILE_BYTES {
        return Err(FeedbackRouteServiceError::FindingTooLarge(
            path.display().to_string(),
        ));
    }
    let bytes = fs::read(&canonical).map_err(|source| FeedbackRouteServiceError::ReadFinding {
        path: path.display().to_string(),
        source,
    })?;
    if bytes.len() as u64 > MAX_FINDING_FILE_BYTES {
        return Err(FeedbackRouteServiceError::FindingTooLarge(
            path.display().to_string(),
        ));
    }
    let finding = serde_json::from_slice(&bytes).map_err(|source| {
        FeedbackRouteServiceError::ParseFinding {
            path: path.display().to_string(),
            source,
        }
    })?;
    Ok(FindingSnapshot {
        canonical_path: canonical,
        bytes,
        finding,
    })
}

fn canonical_catalog_path(path: &Path) -> Result<PathBuf, FeedbackRouteServiceError> {
    fs::canonicalize(path)
        .map_err(|_| FeedbackRouteServiceError::InvalidCatalogPath(path.display().to_string()))
}

fn read_catalog_bytes(path: &Path) -> Result<Vec<u8>, FeedbackRouteServiceError> {
    fs::read(path).map_err(|source| FeedbackRouteServiceError::ReadCatalog {
        path: path.display().to_string(),
        source,
    })
}

fn parse_catalog_bytes(
    bytes: &[u8],
) -> Result<Vec<dcc_mcp_catalog::CatalogEntry>, FeedbackRouteServiceError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| FeedbackRouteServiceError::InvalidCatalog("catalog is not UTF-8".into()))?;
    dcc_mcp_catalog::load_from_str(text).map_err(Into::into)
}
