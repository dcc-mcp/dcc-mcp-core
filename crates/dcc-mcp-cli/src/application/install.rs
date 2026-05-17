use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::domain::install::{InstallPlan, InstallPlanError, InstallPlanner, InstallRequest};

const BUNDLED_CATALOG: &str = include_str!("../../../../dcc-mcp-catalog.yml");

#[derive(Debug, Error)]
pub enum InstallError {
    #[error(transparent)]
    Catalog(#[from] dcc_mcp_catalog::CatalogError),
    #[error(transparent)]
    Plan(#[from] InstallPlanError),
}

pub struct InstallService {
    default_catalog_path: PathBuf,
}

impl InstallService {
    #[must_use]
    pub fn new(default_catalog_path: PathBuf) -> Self {
        Self {
            default_catalog_path,
        }
    }

    pub fn plan(&self, request: InstallRequest) -> Result<InstallPlan, InstallError> {
        let entries = self.load_entries(request.catalog_path.as_deref())?;
        InstallPlanner::plan(&entries, request).map_err(Into::into)
    }

    fn load_entries(
        &self,
        requested_path: Option<&Path>,
    ) -> Result<Vec<dcc_mcp_catalog::CatalogEntry>, InstallError> {
        if let Some(path) = requested_path {
            return dcc_mcp_catalog::load_from_file(path).map_err(Into::into);
        }

        let entries = dcc_mcp_catalog::load_from_file(Path::new(&self.default_catalog_path))?;
        if entries.is_empty() {
            return dcc_mcp_catalog::load_from_str(BUNDLED_CATALOG).map_err(Into::into);
        }
        Ok(entries)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_uses_bundled_catalog_when_default_path_is_missing() {
        let service = InstallService::new(PathBuf::from("__missing_dcc_mcp_catalog__.yml"));
        let plan = service
            .plan(InstallRequest {
                dcc_type: "maya".into(),
                version: Some("2026".into()),
                catalog_path: None,
            })
            .unwrap();

        assert!(plan.adapter.dcc.iter().any(|dcc| dcc == "maya"));
    }
}
