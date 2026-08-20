use dcc_mcp_attestation::{
    AttestationError, GitHubAttestationPolicy, verify_attested_bytes, verify_bundle_digest,
};

const MARKETPLACE_DIGEST: &str = "5d60f114393406b06050192b46c9f7637f6c3ae85d82e92d50c5f8250fd0380f";
const MARKETPLACE_BUNDLE: &str = include_str!("fixtures/marketplace.sigstore.json");

#[test]
fn verifies_real_github_marketplace_attestation() {
    let policy = GitHubAttestationPolicy::official_marketplace();
    let verified = verify_bundle_digest(MARKETPLACE_DIGEST, MARKETPLACE_BUNDLE, &policy)
        .expect("the published marketplace attestation should verify");

    assert_eq!(verified.sha256, MARKETPLACE_DIGEST);
    assert_eq!(verified.identity, policy.workflow_identity);
    assert_eq!(
        verified.issuer,
        "https://token.actions.githubusercontent.com"
    );
    assert!(verified.integrated_time.is_some());
}

#[test]
fn rejects_tampered_artifact_digest() {
    let error = verify_attested_bytes(
        b"tampered marketplace bytes",
        MARKETPLACE_BUNDLE,
        &GitHubAttestationPolicy::official_marketplace(),
    )
    .expect_err("a different artifact digest must not verify");

    assert!(matches!(error, AttestationError::Verification(_)));
}

#[test]
fn rejects_wrong_workflow_identity() {
    let mut policy = GitHubAttestationPolicy::official_marketplace();
    policy.workflow_identity =
        "https://github.com/dcc-mcp/marketplace/.github/workflows/release.yml@refs/heads/main"
            .into();

    let error = verify_bundle_digest(MARKETPLACE_DIGEST, MARKETPLACE_BUNDLE, &policy)
        .expect_err("a different workflow identity must not verify");

    assert!(matches!(error, AttestationError::Verification(_)));
}
