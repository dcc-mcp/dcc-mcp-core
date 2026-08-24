use std::fs;
use std::path::{Path, PathBuf};

use dcc_mcp_models::FindingV1;
use thiserror::Error;

use crate::domain::feedback::{FeedbackRoute, FeedbackRouteError, route_finding};

const BUNDLED_CATALOG: &str = include_str!("../../../../dcc-mcp-catalog.yml");
const MAX_FINDING_FILE_BYTES: u64 = 512 * 1024;

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
        let finding = read_finding(finding_path)?;
        let entries = self.load_catalog(catalog_path)?;
        dcc_mcp_catalog::validate_catalog_entries(&entries)
            .map_err(|error| FeedbackRouteServiceError::InvalidCatalog(error.to_string()))?;
        route_finding(&finding, &entries).map_err(Into::into)
    }

    fn load_catalog(
        &self,
        requested_path: Option<&Path>,
    ) -> Result<Vec<dcc_mcp_catalog::CatalogEntry>, FeedbackRouteServiceError> {
        if let Some(path) = requested_path {
            if !path.is_file() {
                return Err(FeedbackRouteServiceError::InvalidCatalogPath(
                    path.display().to_string(),
                ));
            }
            return dcc_mcp_catalog::load_from_file(path).map_err(Into::into);
        }
        let entries = dcc_mcp_catalog::load_from_file(&self.default_catalog_path)?;
        if entries.is_empty() {
            return dcc_mcp_catalog::load_from_str(BUNDLED_CATALOG).map_err(Into::into);
        }
        Ok(entries)
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
    #[error("catalog failed validation: {0}")]
    InvalidCatalog(String),
    #[error(transparent)]
    Catalog(#[from] dcc_mcp_catalog::CatalogError),
    #[error(transparent)]
    Route(#[from] FeedbackRouteError),
}

fn read_finding(path: &Path) -> Result<FindingV1, FeedbackRouteServiceError> {
    let metadata = fs::metadata(path).map_err(|source| FeedbackRouteServiceError::ReadFinding {
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
    let bytes = fs::read(path).map_err(|source| FeedbackRouteServiceError::ReadFinding {
        path: path.display().to_string(),
        source,
    })?;
    if bytes.len() as u64 > MAX_FINDING_FILE_BYTES {
        return Err(FeedbackRouteServiceError::FindingTooLarge(
            path.display().to_string(),
        ));
    }
    serde_json::from_slice(&bytes).map_err(|source| FeedbackRouteServiceError::ParseFinding {
        path: path.display().to_string(),
        source,
    })
}
