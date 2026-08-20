# ADR 026: Verify Official Release Metadata with Sigstore

## Status

Accepted

## Context

Mandatory SHA-256 values protect downloaded bytes against accidental drift,
but a compromised catalog or update manifest can replace both an asset URL and
its digest. DCC-MCP needs publisher authentication without distributing a
long-lived private signing key or requiring operator GitHub credentials.

GitHub Actions can mint short-lived Sigstore certificates from OIDC and publish
SLSA provenance. Public Sigstore bundles contain the certificate, signature,
and transparency-log proof needed for offline verification. Querying GitHub's
attestation API at runtime would add an anonymous API rate-limit dependency.

## Decision

- Official release metadata is attested with `actions/attest` from a workflow
  on `refs/heads/main`.
- The official marketplace publishes `marketplace.sigstore.json` next to
  `marketplace.json`. Core releases publish one
  `dcc-mcp-update-manifest-<platform>.sigstore.json` beside each manifest.
- Clients verify the exact metadata bytes, the public Sigstore trust root, the
  transparency-log proof, the GitHub Actions issuer, and the exact repository
  workflow identity before parsing the metadata.
- Official metadata fails closed when its bundle is missing or invalid.
- Explicit studio and local catalog/manifest overrides remain operator-trusted
  and do not silently inherit the official policy. Studios can add their own
  authenticated distribution boundary without impersonating DCC-MCP's
  workflow identity.
- Asset SHA-256 and immutable Git/pip requirements remain mandatory. Metadata
  provenance complements those controls; it does not replace them.

## Consequences

- Runtime verification is offline after two ordinary HTTPS downloads and does
  not consume GitHub REST API quota.
- The first release containing this decision publishes its own detached update
  bundles. Older clients reach that release through the existing mandatory
  SHA-256 path; clients from that release onward require provenance.
- A short catalog publication window can expose new catalog bytes before the
  workflow commits their new bundle. Clients fail closed during that window.
- Workflow file paths, repository ownership, and the `main` ref are part of the
  public trust contract and require a coordinated migration if renamed.

## Alternatives Considered

- **Minisign with a repository secret:** rejected because it introduces a
  long-lived private key, rotation, backup, and compromise-recovery burden.
- **Runtime GitHub attestation API lookup:** rejected because anonymous clients
  share a small rate limit and availability would depend on an API call.
- **Checksums only:** rejected because an attacker who controls metadata can
  replace the checksum together with the asset URL.
