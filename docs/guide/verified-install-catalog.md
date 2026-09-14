# Verified install catalog

The installation catalog updates independently of the CLI binary. An online
`dcc-mcp-cli install --dcc-type godot` resolves the latest verified catalog before
building its plan; one execution keeps the selected version, artifact URL, and
digest together. It does not resolve a newer package halfway through installation.

## Official channel and trust

The fixed official channel is
[`install-catalog/install-catalog.json`](https://raw.githubusercontent.com/dcc-mcp/dcc-mcp-core/install-catalog/install-catalog.json).
One JSON envelope contains the exact signed catalog text and its detached Sigstore
bundle, preventing a client from mixing files fetched across two publications:

```json
{
  "catalog": "{\"schema_version\":1,\"source_revision\":\"...\",\"issued_at\":0,\"expires_at\":604800,\"entries\":[]}",
  "attestation": {}
}
```

The example illustrates the envelope shape; it is not a valid signed catalog.
The payload contains schema version `1`, the full Core source commit, Unix issue
and expiry times, and the existing `CatalogEntry` records. A publication is valid
for seven days. Verification binds the exact payload bytes to the GitHub Actions
issuer and this workflow identity:

```text
https://github.com/dcc-mcp/dcc-mcp-core/.github/workflows/publish-install-catalog.yml@refs/heads/main
```

Clients verify the signature, schema, validity period, and replay protection
before using a remote catalog or replacing their verified cache. Plans expose
catalog provenance so callers can distinguish an online result, cached result,
bundled fallback, and explicit local catalog. The official channel cannot be
redirected with an arbitrary catalog URL setting. `--catalog <path>` remains an
explicit operator-owned source with its existing package integrity requirements.

## Freshness and offline use

An online install checks the official channel on each plan request. A transport
failure can use an unexpired, reverified cached publication. When no verified
cache is available, the CLI can report its bundled fallback. `--offline` explicitly
skips the network. Neither a cache nor a bundled fallback claims to be the latest
publication.

An execution that could not check the online channel stops before mutation with
`INSTALL_CATALOG_UNAVAILABLE`. The operator can retry online or explicitly select
`--offline`; a valid signed cache still cannot be used after expiry. The default
cache lives at the platform cache directory under `dcc-mcp/install-catalog-v1.json`.
`DCC_MCP_INSTALL_CACHE` selects a different cache file, not a different trust source.
`DCC_MCP_INSTALL_OFFLINE=true` is the environment equivalent of `--offline`.

An invalid signature, unexpected schema, expired publication, or replayed older
publication is an integrity failure. It must not silently become an offline
success or replace a previously verified cache. Atomic cache replacement keeps
an interrupted write from truncating the last accepted publication.

## Publishing approved plans

`dcc-mcp-catalog.yml` remains the reviewed source of approved adapter versions.
The publisher runs on relevant `main` changes, once daily to refresh validity,
and manual workflow dispatch on `main`. A new PyPI release alone does not change
the approved version.

A catalog policy can include an optional `reason` explaining why an approved
version remains pinned or installation has been withdrawn. That explanation is
returned with the adapter in the install plan. For example, Godot remains at
`0.4.0` while the `0.8.0` release's timeout-cancellation regression awaits passing
release-commit CI. OBS advances to its validated `1.4.0` artifact independently.

The publisher runs this validation before creating an attestation:

```bash
vx uv run --no-project --with pyyaml==6.0.2 \
  python scripts/ci/prepare_install_catalog.py \
  --catalog dcc-mcp-catalog.yml \
  --output install-catalog-payload.json \
  --quarantine-invalid \
  --source-revision <full-core-commit>
```

Validation checks each curated artifact against its immutable source, expected
SHA-256, publication state, and package metadata. With `--quarantine-invalid`, a
missing or yanked artifact, wrong checksum, invalid package metadata, unsupported
active install contract, or failed package check emits a discovery entry with
`policy.installation: not_available`, a safe explanation, and no executable
install metadata. Other valid entries remain available. This allows a newly
detected withdrawal to reach clients even when another package check fails.
The publisher does not execute downloaded adapter code.

Package check network failures also temporarily make that package unavailable.
Every refresh checks the original curated catalog again, so an entry can become
available after its source recovers. The source catalog is not rewritten by this
process. Malformed catalog structure or other whole-document validation failures
still abort publication. Running the script without `--quarantine-invalid`
retains strict validation for maintainers: any invalid active entry fails the run.

Only the resulting validated entries and explicit withdrawals are attested. The
workflow wraps the exact signed text and bundle into one file, then publishes one commit to the `install-catalog`
branch. A shared concurrency group, source revision checks, and an explicit Git
ref lease prevent an older or conflicting run from replacing a newer publication.
The branch contains only the envelope. A whole-document or publication failure
leaves the previous public file intact; its original expiry still applies.

To promote an adapter, review its exact version, wheel URL or Git commit,
checksum, entry point, compatibility, and installation instructions in the
curated catalog. Merge that change through the ordinary required checks and
review the publisher result. If a release is defective, publish a reviewed
replacement or disable its installation in a new catalog. Do not overwrite old
artifact bytes or relax checksum validation to make an install succeed.

## What verification proves

The attestation proves that the catalog bytes came from the trusted publishing
workflow. Artifact digests prevent installing substituted or corrupted package
bytes. Publication checks reject known metadata and availability failures; the
installer also checks its selected version and integrity constraints.

These checks do not prove that an adapter has no functional defects. Python
dependencies resolved by pip are not a complete hash-locked dependency set. A
successful local installation also does not establish host readiness: retain the
existing adapter verification, exact host binding, and live DCC acceptance steps.
Offline clients cannot learn about a newer withdrawal until they reconnect.
Online clients check the latest published channel; package changes become known
when the publisher next validates them, ordinarily during the daily refresh.
