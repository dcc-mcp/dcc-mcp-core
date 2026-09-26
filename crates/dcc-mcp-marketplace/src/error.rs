//! Unified error types for marketplace operations.

use dcc_mcp_catalog::CatalogTarget;
use thiserror::Error;

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

    #[error("official marketplace attestation verification failed: {0}")]
    Attestation(#[from] dcc_mcp_attestation::AttestationError),

    #[error("marketplace source read failed for '{0}': {1}")]
    Read(String, #[source] std::io::Error),

    #[error(transparent)]
    Catalog(#[from] dcc_mcp_catalog::CatalogError),

    #[error("marketplace catalog entry validation failed: {0}")]
    Validation(#[from] dcc_mcp_catalog::CatalogValidationError),

    #[error("marketplace entry '{0}' was not found")]
    NotFound(String),

    #[error("marketplace entry '{0}' does not declare install metadata")]
    MissingInstall(String),

    #[error("marketplace entry '{0}' is not available for installation")]
    NotAvailable(String),

    #[error("marketplace entry '{name}' has an invalid minCoreVersion '{required}'")]
    InvalidMinCoreVersion { name: String, required: String },

    #[error(
        "marketplace entry '{name}' requires dcc-mcp-core >= {required}, but this CLI is {current}"
    )]
    IncompatibleCoreVersion {
        name: String,
        required: String,
        current: String,
    },

    #[error(
        "marketplace entry '{name}' targets multiple DCCs; pass --dcc (supported: {})",
        format_dcc_list(supported)
    )]
    AmbiguousDcc {
        name: String,
        supported: Vec<String>,
    },

    #[error(
        "installed package '{name}' targets multiple DCCs; pass --dcc (supported: {})",
        format_dcc_list(supported)
    )]
    AmbiguousInstalledDcc {
        name: String,
        supported: Vec<String>,
    },

    /// An entry that declares no host at all.
    ///
    /// Split out from [`MarketplaceError::AmbiguousDcc`] because the two cases
    /// are not the same failure: "several hosts, pick one" is actionable, while
    /// "no host declared" means `--dcc` can never succeed. Rendering both under
    /// one message produced `targets multiple DCCs ... (supported: none)`.
    #[error("{}", format_no_declared_dcc(name, targets))]
    NoDeclaredDcc {
        name: String,
        targets: Vec<CatalogTarget>,
    },

    #[error("installed marketplace package '{0}' was not found")]
    InstalledPackageNotFound(String),

    #[error(
        "marketplace entry '{name}' does not target DCC '{dcc}' (supported: {})",
        format_dcc_list(supported)
    )]
    DccMismatch {
        name: String,
        dcc: String,
        supported: Vec<String>,
    },

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

    #[error(
        "marketplace git install requires a full 40-character commit object ID, got '{reference}'"
    )]
    UnpinnedGitReference { reference: String },

    #[error("marketplace git checkout mismatch: expected commit {expected}, got {actual}")]
    GitCommitMismatch { expected: String, actual: String },

    #[error("installed package does not contain SKILL.md at '{0}'")]
    MissingSkill(String),

    #[error("marketplace archive SHA-256 mismatch for '{url}': expected {expected}, got {actual}")]
    HashMismatch {
        url: String,
        expected: String,
        actual: String,
    },

    #[error("marketplace archive '{url}' requires SHA-256 before it can be read or downloaded")]
    MissingArchiveChecksum { url: String },

    #[error("marketplace archive has invalid SHA-256 '{value}'; expected 64 hexadecimal digits")]
    InvalidArchiveChecksum { value: String },

    #[error("marketplace archive error for '{0}': {1}")]
    Archive(String, String),

    #[error(
        "invalid marketplace {kind} '{value}'; use only ASCII letters, numbers, '.', '_' or '-'"
    )]
    InvalidPathComponent { kind: String, value: String },
}

/// Render the supported-DCC list carried by host-selection errors.
///
/// Errors that reject a DCC name must stay actionable: they name the hosts the
/// entry actually declares instead of leaving the operator to guess whether the
/// package name itself was wrong.
fn format_dcc_list(dccs: &[String]) -> String {
    if dccs.is_empty() {
        return "none".to_string();
    }
    dccs.join(", ")
}

/// Render the host-selection failure for an entry that declares no DCC.
///
/// Non-DCC targets are listed when the entry has any: they are the installable
/// surface the entry does offer, so the message points at `--target` instead of
/// asking for a `--dcc` value that no entry could satisfy.
fn format_no_declared_dcc(name: &str, targets: &[CatalogTarget]) -> String {
    if targets.is_empty() {
        return format!(
            "marketplace entry '{name}' declares no DCC and no target; it cannot be installed"
        );
    }
    let declared = targets
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "marketplace entry '{name}' declares no DCC; install it with --target (declared: {declared})"
    )
}
