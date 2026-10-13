# Core release version contract

`.release-please-manifest.json["."]` is the Core release source. Release Please
moves it and the registered source declarations together. The dependency-free
`scripts/ci/core_version_contract.py` checks their semantic locations and fills
release projections that updater annotations alone cannot keep consistent.

## Which versions move together?

| Category | Source and policy |
| --- | --- |
| Core package declarations | Root `pyproject.toml`, Cargo workspace package version, and the three `pkg/*/pyproject.toml` distributions follow the release manifest. The root Rust package and Core crates must inherit `version.workspace = true`. |
| Core release claims | The explicit marker inventory covers the Python stub, CLI/server package `__version__`, repository skill metadata, VitePress labels, current-version documentation, and `llms.txt`. Every marker is discovered across tracked files, including hidden paths; an unregistered claim fails. |
| Typed JSON projections | Admin UI package version, both npm lock root versions, and the MCP server descriptor plus its `dcc-mcp-server` package version follow Core. Exact field paths and package identities are checked; no recursive version replacement is used. |
| Core-coupled skill seeds | Only the two explicitly marked marketplace skill sources follow Core. Their seed rows project both `version` and `metadata["dcc-mcp.version"]`, exactly as the production loader does. |
| Generated dependency metadata | The existing bounded generators own `Cargo.lock`, `uv.lock`, and `crates/workspace-hack/Cargo.toml`. The workspace-hack package keeps its independent version. |
| Independent versions | Other skills, marketplace catalog entries, compatibility/minimum-Core ranges, protocol/schema revisions, examples, and release history do not follow Core. Neither npm dependency pins nor already-published PyPI versions in `uv.lock` are rewritten by the projection synchronizer. |

The uv editable root must match Core. Registry-resolved server/semantic packages
are different: a release PR can precede publication of those wheels. Their
existing released-floor check remains in `check_lock_versions.py`; substituting
the not-yet-published Core version would break resolution.

## Developer commands

```bash
python scripts/ci/core_version_contract.py check
python scripts/ci/core_version_contract.py sync
```

Do not bump the manifest manually. `sync` derives the version from the manifest;
it is intended after Release Please has moved that source. It validates the
complete input set before writing, updates only explicit projections, and is
idempotent. It does not install tools, resolve dependencies, run project code,
or regenerate unrelated seed content.

When adding a new Core-coupled location, register its exact updater and semantic
path, extend the fixed output allowlist, and add a mutation test. Removing a
marker or registration from an existing location is an error. New package
manifests also require explicit classification. Marker-looking examples in tests
must construct the token, rather than introduce a real updater annotation.

## Automation and trust boundary

The version check runs on every eligible PR, without path filters, so a newly
introduced release claim cannot evade it. The full Rust benchmark live-harvest
gate remains authoritative for all non-version seed content; the Python
synchronizer only projects the two known version fields.

Release-please/renovate synchronization executes two self-contained Python
scripts from the same immutable trusted commit, piped directly from Git objects
into `python -I -`. Pull-request scripts are never imported or executed by the
projection step. Existing bounded dependency generation and its scrubbed
environment remain unchanged.

The fixed generated-file allowlist is checked independently of PR configuration.
Before committing, projection bytes must equal a canonical render of the
original HEAD. After committing, the committed blobs must equal the render of
the exact original parent. This rejects non-version changes even inside an
allowlisted file. Existing identity, remote, exact-head and force-with-lease
checks still guard the push. Update the trusted pin only to an already-created,
reviewable commit containing both standalone scripts; do not use a mutable ref.

The merge-race repair action runs the same synchronization on trusted `main`,
opens a reviewable repair PR, and never auto-merges it. Registry dispatch without
a version input reads the release manifest; explicit historical backfills still
use the selected version.
