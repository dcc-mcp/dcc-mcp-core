use dcc_mcp_attestation::{GitHubAttestationPolicy, verify_attested_bytes};

#[test]
fn verifies_published_core_release_manifest() {
    verify_attested_bytes(
        include_bytes!("fixtures/core-release.json"),
        include_str!("fixtures/core-release.sigstore.json"),
        &GitHubAttestationPolicy::official_core_release(),
    )
    .expect("the published Core update manifest must verify");
}
