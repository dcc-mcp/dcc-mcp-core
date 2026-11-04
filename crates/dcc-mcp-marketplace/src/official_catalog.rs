//! Read-only access to the verified official marketplace catalog.
//!
//! This boundary uses only the two official URLs. It does not consult source
//! configuration, persist a cache, resolve package assets, or install anything.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use dcc_mcp_attestation::{GitHubAttestationPolicy, verify_attested_bytes};
use dcc_mcp_catalog::{CatalogEntry, load_from_str, validate_catalog_entries};
use thiserror::Error;

pub use dcc_mcp_attestation::VerifiedAttestation;

use crate::{MarketplaceError, OFFICIAL_MARKETPLACE_ATTESTATION, OFFICIAL_MARKETPLACE_SOURCE};

const MAX_RESPONSE_BYTES: usize = 1024 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// Catalog entries and the evidence that authenticated their exact source bytes.
#[derive(Debug, Clone)]
pub struct VerifiedOfficialCatalog {
    pub entries: Vec<CatalogEntry>,
    /// `integrated_time` is a transparency-log timestamp, not a package
    /// publication time or the time at which the caller checked the catalog.
    pub attestation: VerifiedAttestation,
}

/// A failed fetch or verification never yields usable catalog entries.
#[derive(Debug, Error)]
pub enum OfficialCatalogError {
    #[error(transparent)]
    Marketplace(#[from] MarketplaceError),

    #[error("official marketplace response from '{url}' exceeds {limit} bytes")]
    ResponseTooLarge { url: String, limit: usize },

    #[error("official marketplace request to '{url}' returned HTTP {status}")]
    UnexpectedStatus { url: String, status: u16 },

    #[error("official marketplace response from '{url}' is not valid UTF-8: {source}")]
    InvalidUtf8 {
        url: String,
        #[source]
        source: std::str::Utf8Error,
    },

    #[error(
        "official marketplace attestation has invalid transparency-log time {integrated_time:?}"
    )]
    InvalidAttestationTime { integrated_time: Option<i64> },

    #[error("official marketplace check time is unavailable: {0}")]
    Clock(#[from] std::time::SystemTimeError),
}

impl OfficialCatalogError {
    /// Distinguish transport unavailability from evidence that must fail closed.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Marketplace(MarketplaceError::Fetch(..)) | Self::UnexpectedStatus { .. } => {
                "network_error"
            }
            Self::Clock(_) => "clock_error",
            _ => "integrity_error",
        }
    }
}

/// Fetch and verify the current official catalog without local state or fallback.
///
/// Each response is limited to 1 MiB and ten seconds. Redirects are rejected so
/// neither request can fan out to another source. Callers may record their own
/// check time after this function succeeds; attestation time is not freshness.
pub async fn fetch_official_catalog() -> Result<VerifiedOfficialCatalog, OfficialCatalogError> {
    let client = reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .connect_timeout(CONNECT_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|error| MarketplaceError::Fetch(OFFICIAL_MARKETPLACE_SOURCE.into(), error))?;
    let payload = fetch_bytes(&client, OFFICIAL_MARKETPLACE_SOURCE).await?;
    let bundle = fetch_bytes(&client, OFFICIAL_MARKETPLACE_ATTESTATION).await?;
    let catalog = verify_official_catalog(&payload, &bundle)?;
    let checked_at = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    validate_attestation_time(catalog.attestation.integrated_time, checked_at)?;
    Ok(catalog)
}

/// Verify exact catalog bytes with the official marketplace workflow policy.
///
/// Both inputs must fit the same 1 MiB limit as network responses. Verification
/// precedes catalog parsing, and every parsed entry must pass the shared catalog
/// validation before any entries are returned. This function performs no I/O.
/// Callers verifying supplied bytes must assess attestation time against their
/// own check time; [`fetch_official_catalog`] performs that check automatically.
pub fn verify_official_catalog(
    payload: &[u8],
    bundle: &[u8],
) -> Result<VerifiedOfficialCatalog, OfficialCatalogError> {
    ensure_size(OFFICIAL_MARKETPLACE_SOURCE, payload.len())?;
    ensure_size(OFFICIAL_MARKETPLACE_ATTESTATION, bundle.len())?;
    let bundle = decode_utf8(OFFICIAL_MARKETPLACE_ATTESTATION, bundle)?;
    let attestation = verify_attested_bytes(
        payload,
        bundle,
        &GitHubAttestationPolicy::official_marketplace(),
    )
    .map_err(MarketplaceError::from)?;
    let entries = decode_catalog_entries(decode_utf8(OFFICIAL_MARKETPLACE_SOURCE, payload)?)?;
    Ok(VerifiedOfficialCatalog {
        entries,
        attestation,
    })
}

fn decode_catalog_entries(text: &str) -> Result<Vec<CatalogEntry>, OfficialCatalogError> {
    let invalid = |message: &str| {
        MarketplaceError::Catalog(dcc_mcp_catalog::CatalogError::Parse(message.to_owned()))
    };
    let document: serde_json::Value = serde_json::from_str(text)
        .map_err(|_| invalid("official catalog is not a JSON document"))?;
    let fields: Vec<_> = ["entries", "items", "skills"]
        .into_iter()
        .filter_map(|key| document.as_object().and_then(|object| object.get(key)))
        .collect();
    if fields.len() != 1 || !fields[0].is_array() {
        return Err(invalid("official catalog must declare one entries array").into());
    }
    let entries = load_from_str(text).map_err(MarketplaceError::from)?;
    validate_catalog_entries(&entries).map_err(MarketplaceError::from)?;
    let mut names = std::collections::HashSet::new();
    if entries
        .iter()
        .any(|entry| !names.insert(entry.name.to_lowercase()))
    {
        return Err(invalid("official catalog contains duplicate package names").into());
    }
    Ok(entries)
}

async fn fetch_bytes(client: &reqwest::Client, url: &str) -> Result<Vec<u8>, OfficialCatalogError> {
    let mut response = client
        .get(url)
        .header("User-Agent", "dcc-mcp marketplace")
        .send()
        .await
        .map_err(|error| MarketplaceError::Fetch(url.into(), error))?;
    if !response.status().is_success() {
        return Err(OfficialCatalogError::UnexpectedStatus {
            url: url.into(),
            status: response.status().as_u16(),
        });
    }
    if response
        .content_length()
        .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
    {
        return Err(response_too_large(url));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| MarketplaceError::Fetch(url.into(), error))?
    {
        if chunk.len() > MAX_RESPONSE_BYTES - bytes.len() {
            return Err(response_too_large(url));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn ensure_size(url: &str, length: usize) -> Result<(), OfficialCatalogError> {
    if length > MAX_RESPONSE_BYTES {
        return Err(response_too_large(url));
    }
    Ok(())
}

fn response_too_large(url: &str) -> OfficialCatalogError {
    OfficialCatalogError::ResponseTooLarge {
        url: url.into(),
        limit: MAX_RESPONSE_BYTES,
    }
}

fn decode_utf8<'a>(url: &str, bytes: &'a [u8]) -> Result<&'a str, OfficialCatalogError> {
    std::str::from_utf8(bytes).map_err(|source| OfficialCatalogError::InvalidUtf8 {
        url: url.into(),
        source,
    })
}

fn validate_attestation_time(
    integrated_time: Option<i64>,
    checked_at: u64,
) -> Result<(), OfficialCatalogError> {
    if integrated_time
        .and_then(|time| u64::try_from(time).ok())
        .is_some_and(|time| time <= checked_at.saturating_add(300))
    {
        return Ok(());
    }
    Err(OfficialCatalogError::InvalidAttestationTime { integrated_time })
}

#[cfg(test)]
mod tests {
    use super::*;

    const MARKETPLACE_BUNDLE: &[u8] =
        include_bytes!("../../dcc-mcp-attestation/tests/fixtures/marketplace.sigstore.json");

    #[test]
    fn malformed_outer_document_is_not_an_empty_success() {
        for text in [
            "{}",
            "[]",
            r#"{"entries":null}"#,
            r#"{"entries":[],"skills":[]}"#,
        ] {
            assert_eq!(
                decode_catalog_entries(text).unwrap_err().code(),
                "integrity_error"
            );
        }
        for text in [r#"{"entries":[]}"#, r#"{"items":[]}"#, r#"{"skills":[]}"#] {
            assert!(decode_catalog_entries(text).unwrap().is_empty());
        }
    }

    #[test]
    fn rejects_tampered_catalog_before_parsing() {
        let error = verify_official_catalog(b"tampered marketplace bytes", MARKETPLACE_BUNDLE)
            .expect_err("a signed bundle must authenticate the exact catalog bytes");
        assert!(matches!(
            error,
            OfficialCatalogError::Marketplace(MarketplaceError::Attestation(_))
        ));
    }

    #[test]
    fn rejects_another_official_workflow() {
        let payload = include_bytes!("../../dcc-mcp-attestation/tests/fixtures/core-release.json");
        let bundle =
            include_bytes!("../../dcc-mcp-attestation/tests/fixtures/core-release.sigstore.json");
        let error = verify_official_catalog(payload, bundle)
            .expect_err("an official Core release signer cannot sign the marketplace catalog");
        assert!(matches!(
            error,
            OfficialCatalogError::Marketplace(MarketplaceError::Attestation(_))
        ));
    }

    #[test]
    fn rejects_oversized_catalog_and_bundle_independently() {
        let oversized = vec![b' '; MAX_RESPONSE_BYTES + 1];
        for (payload, bundle, expected_url) in [
            (
                oversized.as_slice(),
                b"{}".as_slice(),
                OFFICIAL_MARKETPLACE_SOURCE,
            ),
            (
                b"{}".as_slice(),
                oversized.as_slice(),
                OFFICIAL_MARKETPLACE_ATTESTATION,
            ),
        ] {
            let error = verify_official_catalog(payload, bundle).unwrap_err();
            assert!(
                matches!(error, OfficialCatalogError::ResponseTooLarge { url, limit }
                if url == expected_url && limit == MAX_RESPONSE_BYTES)
            );
        }
    }

    #[test]
    fn rejects_unsigned_catalog() {
        let error = verify_official_catalog(br#"{"entries":[]}"#, b"{}")
            .expect_err("an empty catalog still requires a valid official signature");
        assert!(matches!(
            error,
            OfficialCatalogError::Marketplace(MarketplaceError::Attestation(_))
        ));
    }

    #[test]
    fn rejects_missing_negative_or_future_attestation_time() {
        for integrated_time in [None, Some(-1), Some(1_301)] {
            let error = validate_attestation_time(integrated_time, 1_000).unwrap_err();
            assert!(matches!(
                error,
                OfficialCatalogError::InvalidAttestationTime { .. }
            ));
            assert_eq!(error.code(), "integrity_error");
        }
        for integrated_time in [0, 1_000, 1_300] {
            validate_attestation_time(Some(integrated_time), 1_000).unwrap();
        }
    }
}
