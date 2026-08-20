# Pinned Pip Adapter Artifacts

Status: Accepted

## Context

Catalog-backed adapter installation passed a package name and optional caller
version to `pip install --upgrade`. The catalog version was descriptive: when a
caller omitted `--version`, pip selected the latest release available at
execution time. A later upload or catalog drift could therefore change the
installed adapter without changing the reviewed catalog.

The bundled catalog also advertised pip installation for three adapter projects
that had no published PyPI project, and referenced an unpublished Unreal
version. Those entries could produce plans that were impossible to execute.

## Decision

- An executable `install.type: pip` entry requires an exact catalog `version`,
  an HTTPS `py3-none-any` wheel URL whose filename binds the normalized package
  name and version, and the wheel's SHA-256.
- Catalog validation checks that binding. The install planner repeats it before
  producing an executable action, and the executor repeats it for deserialized
  or externally supplied plans.
- Pip receives a PEP 508 direct reference with a `#sha256=` fragment. The
  adapter package name is retained separately for rollback and `pip show`.
- `--version` may only repeat the catalog version. Installing another version
  requires a reviewed catalog entry with its own artifact URL and digest.
- Post-install verification requires `pip show` to report the catalog version.
- Catalog entries without a published wheel remain discoverable but omit
  automatic install metadata. Unreal moves to its published 0.3.0 wheel.
- The public Rust `PipInstall` plan variant adds optional `artifact_url` and
  `sha256` fields so old serialized plans still decode, then fail closed at
  execution. Rust source that constructs the variant must add those fields;
  this change belongs in the next pre-1 minor release.

The migration downloaded every declared wheel, verified its SHA-256, and
checked its `METADATA` name/version plus `py3-none-any` wheel tag.

## Consequences

- A package release changes the catalog URL and digest in the same reviewed
  update; pip no longer resolves the adapter package by mutable name.
- Removed or replaced wheel files fail closed instead of selecting another
  release.
- This decision authenticates the selected adapter wheel only to the extent
  that the reviewed catalog is trusted. It does not sign the catalog.
- Transitive dependencies still follow normal pip index resolution. Fully
  reproducible dependency graphs require per-runtime lock manifests with every
  dependency hash; a single cross-DCC lock cannot represent all supported
  Python and platform combinations.

## Alternatives considered

### Pin only `package==version`

Rejected because the installer would still trust whatever bytes the package
index serves for that release and the catalog would not bind an artifact.

### Use `pip --require-hashes` immediately

Deferred because that mode requires hashes for the complete transitive graph.
The graph differs across host Python versions and operating systems, so it
needs a typed artifact matrix rather than one global requirements file.

### Keep unpublished packages marked installable

Rejected because an executable plan that can never resolve is a false contract.
