//! Provenance for an immutable installation catalog snapshot.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CatalogSource {
    Remote,
    Cache,
    Bundled,
    Explicit,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogProvenance {
    pub source: CatalogSource,
    /// True only after this invocation has checked the official online channel.
    pub latest_checked: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_revision: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub issued_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<u64>,
}

impl CatalogProvenance {
    pub fn local(source: CatalogSource) -> Self {
        Self {
            source,
            latest_checked: false,
            sha256: None,
            source_revision: None,
            issued_at: None,
            expires_at: None,
        }
    }
}
