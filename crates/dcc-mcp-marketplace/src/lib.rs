//! Shared marketplace domain layer — PIP-626.
//!
//! This crate is the canonical marketplace implementation used by the CLI,
//! the Gateway admin panel, and any future marketplace bridge.
//!
//! It provides:
//! - Domain types ([`MarketplaceSource`], [`MarketplaceInstallResult`], …)
//! - Source resolution ([`normalise_source`], [`builtin_source`])
//! - The [`MarketplaceService`] for catalog fetch, install/uninstall,
//!   installed state persistence, source management, outdated/update,
//!   and integrity verification (zip + SHA-256).
//! - A unified [`MarketplaceError`] covering all known failure modes.

pub mod add_repo;
pub mod error;
pub mod path;
pub mod service;
pub mod source;
pub mod types;

// Re-export the public API for convenience.
pub use add_repo::{install_from_repo, list_repo_skills, parse_repo_ref};
pub use error::MarketplaceError;
pub use path::{default_config_path, home_dir, marketplace_root, marketplace_root_or_default};
pub use service::{MarketplaceService, default_sources_disabled, env_sources, path_component};
pub use source::{builtin_source, dedupe_sources, normalise_source};
pub use types::{
    InstalledMarketplacePackage, MarketplaceHit, MarketplaceInspectResult,
    MarketplaceInstallResult, MarketplaceInstalledList, MarketplaceInstalledState,
    MarketplaceOutdatedList, MarketplaceSearchResult, MarketplaceSource, MarketplaceSourceConfig,
    MarketplaceSourceOrigin, MarketplaceUninstallResult, MarketplaceUpdateResult,
    OFFICIAL_MARKETPLACE_SOURCE, OutdatedMarketplacePackage, RepoInstallResult, RepoSkillInfo,
    RepoSkillList, StoredMarketplaceSource, entry_targets_dcc,
};
