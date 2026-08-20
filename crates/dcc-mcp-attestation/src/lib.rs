//! Fail-closed verification of GitHub Artifact Attestations.
//!
//! DCC-MCP uses GitHub Actions OIDC identities and public Sigstore bundles for
//! release manifests and the official marketplace catalog. This crate keeps
//! certificate identity policy, transparency-log proof, and artifact-digest
//! verification behind one dependency-light boundary.

use sha2::{Digest, Sha256};
use sigstore_verify::trust_root::{SIGSTORE_PRODUCTION_TRUSTED_ROOT, TrustedRoot};
use sigstore_verify::types::{Bundle, Sha256Hash};
use sigstore_verify::{VerificationPolicy, verify};
use thiserror::Error;

const GITHUB_ACTIONS_ISSUER: &str = "https://token.actions.githubusercontent.com";

/// Exact GitHub repository and workflow identity trusted to attest an artifact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitHubAttestationPolicy {
    pub workflow_identity: String,
}

impl GitHubAttestationPolicy {
    /// Trust the official marketplace main-branch attestation workflow.
    pub fn official_marketplace() -> Self {
        Self {
            workflow_identity: "https://github.com/dcc-mcp/marketplace/.github/workflows/attest-catalog.yml@refs/heads/main".into(),
        }
    }

    /// Trust release-manifest attestations created by the Core release workflow.
    pub fn official_core_release() -> Self {
        Self {
            workflow_identity: "https://github.com/dcc-mcp/dcc-mcp-core/.github/workflows/release.yml@refs/heads/main".into(),
        }
    }
}

/// Successful verification evidence safe to record in diagnostics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedAttestation {
    pub sha256: String,
    pub identity: String,
    pub issuer: String,
    pub integrated_time: Option<i64>,
}

#[derive(Debug, Error)]
pub enum AttestationError {
    #[error("Sigstore bundle is invalid: {0}")]
    InvalidBundle(String),
    #[error("Artifact attestation verification failed: {0}")]
    Verification(String),
    #[error("Embedded Sigstore trust root is invalid: {0}")]
    TrustRoot(String),
}

/// Cryptographically verify exact artifact bytes against a detached bundle.
pub fn verify_attested_bytes(
    artifact: &[u8],
    bundle_json: &str,
    policy: &GitHubAttestationPolicy,
) -> Result<VerifiedAttestation, AttestationError> {
    let digest = Sha256::digest(artifact);
    let digest_hex = hex_lower(&digest);
    verify_bundle_digest(&digest_hex, bundle_json, policy)
}

/// Verify a detached Sigstore bundle against an already-computed SHA-256.
pub fn verify_bundle_digest(
    digest_hex: &str,
    bundle_json: &str,
    policy: &GitHubAttestationPolicy,
) -> Result<VerifiedAttestation, AttestationError> {
    let digest = Sha256Hash::from_hex(digest_hex)
        .map_err(|error| AttestationError::Verification(error.to_string()))?;
    let bundle = Bundle::from_json(bundle_json)
        .map_err(|error| AttestationError::InvalidBundle(error.to_string()))?;
    let trust_root = TrustedRoot::from_json(SIGSTORE_PRODUCTION_TRUSTED_ROOT)
        .map_err(|error| AttestationError::TrustRoot(error.to_string()))?;
    let verification_policy = VerificationPolicy::default()
        .require_identity(&policy.workflow_identity)
        .require_issuer(GITHUB_ACTIONS_ISSUER);
    let result = verify(digest, &bundle, &verification_policy, &trust_root)
        .map_err(|error| AttestationError::Verification(error.to_string()))?;

    Ok(VerifiedAttestation {
        sha256: digest_hex.to_ascii_lowercase(),
        identity: result.identity.unwrap_or_default(),
        issuer: result.issuer.unwrap_or_default(),
        integrated_time: result.integrated_time,
    })
}

fn hex_lower(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write;
        let _ = write!(output, "{byte:02x}");
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn official_policies_are_exact_and_distinct() {
        let marketplace = GitHubAttestationPolicy::official_marketplace();
        let core = GitHubAttestationPolicy::official_core_release();
        assert_ne!(marketplace, core);
        assert!(marketplace.workflow_identity.ends_with("@refs/heads/main"));
        assert!(core.workflow_identity.contains("/release.yml@"));
    }

    #[test]
    fn digest_encoding_is_canonical_lowercase() {
        assert_eq!(hex_lower(&[0, 15, 16, 255]), "000f10ff");
    }
}
