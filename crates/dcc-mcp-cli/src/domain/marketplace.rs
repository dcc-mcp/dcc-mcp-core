//! Marketplace domain types re-exported from the shared `dcc-mcp-marketplace` crate.

pub use dcc_mcp_marketplace::{
    InstalledMarketplacePackage, MarketplaceHit, MarketplaceInspectResult,
    MarketplaceInstallResult, MarketplaceInstalledList, MarketplaceInstalledState,
    MarketplaceOutdatedList, MarketplaceSearchResult, MarketplaceSource, MarketplaceSourceConfig,
    MarketplaceSourceOrigin, MarketplaceUninstallResult, MarketplaceUpdateResult,
    OFFICIAL_MARKETPLACE_SOURCE, OutdatedMarketplacePackage, StoredMarketplaceSource,
    builtin_source, entry_targets_dcc, normalise_source,
};
