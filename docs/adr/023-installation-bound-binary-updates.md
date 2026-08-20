# Installation-Bound Binary Updates

Status: Accepted

## Context

The gateway update transport allowed a missing SHA-256, while CLI and server
production paths staged `pending.bin` behind a literal `pending.marker` and did
not re-verify it before replacement. Gateway Admin could also stage selected
binaries without proving which executable installation owned the request.

The updater already contained a versioned update-set manifest, installation
binding, locking, journal recovery, rollback, and stage/apply hash checks. The
production single-binary path had diverged from those primitives.

## Decision

- Transport DTOs retain optional URL and SHA-256 fields so up-to-date and old
  gateway responses remain readable. An `update_available=true` response must
  cross a domain gate requiring a URL and exactly 64 hexadecimal SHA-256 bytes.
- Downloads are streamed into a bounded same-directory temporary file, hashed
  before atomic persistence, and represented as a verified asset carrying the
  validated manifest digest.
- CLI and server stage one `CurrentExecutable` component through update-set
  format v2. The manifest is bound to the canonical executable path and source
  hash. Single-binary apply rejects sibling or multi-component sets.
- Apply re-verifies the manifest, source executable, and staged component before
  creating a transaction journal or touching the target executable. Invalid
  pre-transaction sets are quarantined; the current binary remains usable.
- Legacy `pending.bin` / `pending.marker` state is unsigned and is never
  migrated into the verified format. It is quarantined with a stable warning.
- A successful CLI replacement re-executes the CLI with its original arguments.
  A successful server replacement follows the existing restart path.
- Gateway Admin is check-only for every binary. Staging must run inside the
  exact target installation that can establish executable ownership.

## Consequences

- Available updates fail closed on missing, malformed, mismatched, or tampered
  digests while up-to-date responses may omit download fields.
- Existing public optional DTO fields and updater entry-point signatures remain
  source-compatible. New verified types make the trusted boundary explicit.
- The first hop from an older release still executes its older updater. Users
  requiring a verified bootstrap must install the first fixed release through
  an externally SHA-256-verified asset.
- SHA-256 proves consistency with the fetched manifest, not publisher identity.
  Detached signed manifests remain a separate required authenticity layer.

## Alternatives considered

### Make transport SHA fields non-optional

Rejected because an up-to-date response has no download asset and downstream
Rust callers may construct the public transport structs directly.

### Keep the legacy marker and add a digest sidecar

Rejected because it would create a second transaction format beside the
existing crash-recoverable update-set implementation.

### Let Gateway Admin stage local binaries

Rejected because a gateway binary name does not prove the target executable
path, especially across multiple installations and remote DCC instances.
