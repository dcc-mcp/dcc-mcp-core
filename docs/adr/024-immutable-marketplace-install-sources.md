# Immutable Marketplace Install Sources

Status: Accepted

## Context

Marketplace Git entries accepted branches, tags, short object IDs, or a missing
ref. GitHub URLs were optimized into codeload ZIP downloads while discarding
the catalog checksum. ZIP verification also treated a missing checksum as
success. Direct `marketplace add-repo` installs cloned the current branch HEAD.

Those paths allowed catalog metadata to select mutable or unverified content.
An update could therefore install different bytes without changing the entry.

## Decision

- Every catalog Git install requires a full 40-character commit object ID.
  Runtime enforcement remains active even when a caller skips schema
  validation.
- Git installs initialize a staging repository, fetch the declared commit,
  check out `FETCH_HEAD` detached, and verify that `HEAD` equals the normalized
  object ID before package files are promoted.
- GitHub catalog entries use the same Git path. They are not converted into an
  archive whose bytes lack an independently declared digest.
- Git updates reinstall the next catalog-pinned commit through staging instead
  of pulling or checking out a mutable ref in place.
- Every ZIP install requires exactly 64 hexadecimal SHA-256 digits before a
  local file is read or a network request starts. Received bytes are verified
  before extraction.
- Direct repository installation requires `--commit`. The read-only `--list`
  preview may inspect the current repository head but cannot install it.

## Consequences

- Existing catalog entries using branches, tags, short object IDs, or ZIPs
  without SHA-256 must be migrated before installation.
- Official Git entries already use full commit IDs and remain installable.
- Existing direct-install Rust entry points remain callable but fail closed;
  callers migrate to the explicit `*_at_commit` API.
- Commit IDs and archive hashes provide integrity and immutability, not
  publisher identity. Signed marketplace manifests remain a separate required
  authenticity layer.

## Alternatives considered

### Trust HTTPS and release tags

Rejected because both the selected ref and its resolved content can change
without a catalog diff.

### Keep the GitHub codeload optimization

Rejected because a Git commit identifies repository objects, not the generated
archive bytes. Retaining codeload would require a separate archive digest for
every Git entry and duplicate ZIP semantics.

### Resolve mutable refs once and persist the result

Rejected because the first install would still trust content not declared by
the reviewed catalog, and repeated installations would not be reproducible.
