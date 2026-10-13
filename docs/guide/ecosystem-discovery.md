# Discover official ecosystem components

`update discover` reads the official installation and Marketplace catalogs. New
catalog entries, names, application targets, package formats, and declared
requirements appear without adding component names to the CLI.

```bash
dcc-mcp-cli --output json update discover
dcc-mcp-cli update discover --query "animation" --limit 20 --output json
dcc-mcp-cli update discover --target application:editor --output json
dcc-mcp-cli update discover --offline --provider official_install_catalog --output json
```

The command returns catalog observations. It does not install packages, inspect
installed packages, probe host runtimes, start a gateway, apply a staged CLI
update, or write caches. The executable parses the complete command before the
legacy startup path; invalid arguments and help also finish at that gate.

## Providers and offline behavior

| Provider ID | Evidence | Offline behavior |
| --- | --- | --- |
| `official_install_catalog` | Existing official signed installation feed; catalog digest, source revision, issue time, expiry, and anti-rollback checks | Reverify the existing signed cache; use the embedded catalog if no cache exists. Reject expired or invalid caches. |
| `official_marketplace` | Fixed official Marketplace catalog and detached Sigstore bundle; exact bytes, official workflow identity, and entry validation | Return `offline_no_verified_marketplace_snapshot`; this command does not maintain a Marketplace cache. |

An online installation-catalog fetch is verified in memory. It compares the
remote issue time and digest with the existing signed cache, including a second
cache read after fetching. It does not persist a new anti-rollback watermark.
The normal install flow continues to own cache persistence. Network failure can
fall back to a valid installation cache or embedded catalog, while retaining
`refresh_error` and an overall partial result. Invalid signatures, future issue
times outside the existing clock tolerance, conflicting revisions, and rollback
attempts fail closed.

Marketplace requests have a 1 MiB limit per response, a ten-second timeout, and
no redirects. Only the two fixed official URLs are contacted. Registered,
environment, and private Marketplace sources are never enumerated. Marketplace
signature log time is `attested_at`, not component publication time. The official
Marketplace format provides no signed issue/expiry window; those fields and
`ttl_secs` remain null instead of inventing freshness evidence.

`--provider` selects one provider. `--target` is an exact `kind:id` filter using
the existing `dcc`, `application`, `game`, and `web` namespaces. Legacy `dcc`
declarations use the shared Marketplace target normalization. `--query` reuses
the catalog search ranking. `--limit` is 1–200, defaults to 50, and applies across
all selected providers after filtering. Ties sort by provider ID, then name.

## JSON contract

`schema_version: "dcc-mcp.ecosystem-discovery.v1"` contains:

- `status`: `complete`, `partial`, or `unavailable`. Complete means all selected
  provider reads succeeded; it does not mean the catalog is current or exhaustive.
- `providers`: each official URL, source (`remote`, `cache`, `bundled`, or
  `unavailable`), freshness, check time, digest, revision, signed timestamps,
  attestation identity where available, and failure reason.
- `components`: provider-qualified `identity: {provider_id, name}`, description,
  targets, advertised version, package metadata, requirements, policy, URL, and
  catalog evidence. Same names from different providers remain separate.
- `total_matches`, `count`, and `truncated`: distinguish pagination from empty
  successful results. A failed provider is never represented as an empty success.

`availability: catalog_available` means the source advertises an entry. Installation
policy is reported separately and may prohibit installation. `component_type`
comes only from an explicit package format; otherwise it is `unknown`.
`published_at` is null because these catalogs do not supply component publication
evidence. `installed`, `runtime`, and `compatibility` are always `not_checked`.
The command makes no `newly_published` or `latest_compatible` claim.

For live installation metadata, `checked_at_unix_secs` records this invocation's
observation and `cache_age_secs` is zero. The existing installation cache does
not retain its fetch time, so offline check time and cache age are null. Its
`ttl_secs` reports the signed catalog lifetime, and `expires_at` determines cache
validity. Embedded entries have unknown freshness and no signed lifetime.

Partial and unavailable results still print their structured JSON and return the
CLI unavailable exit code. Successful empty results return success. Consumers
should inspect source evidence before treating an observation as current.

The application `DiscoveryProvider` trait is the extension boundary. Adding an
official provider requires an explicit registration and a verifier appropriate
to its source; it does not require changing component enumeration or filtering.
