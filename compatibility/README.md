# Compatibility contracts

Machine-readable contracts that CI enforces on every pull request.

| File | Enforced by | Purpose |
| --- | --- | --- |
| [`python.json`](python.json) | `scripts/ci/check_python_support.py` (`python37-contract` job) | Long-term-support Python policy, wheel profiles, and release projections. |
| [`schema-pins.json`](schema-pins.json) | `tests/test_released_schema_immutability.py` (`released-schema-immutability` job) | Digests of schema revisions that have already shipped in a release. |
| [`core-bounds.json`](core-bounds.json) | `tests/test_core_bounds.py` + `cargo test -p dcc-mcp-catalog` | Core `0.x` version-bound contract for adapter dependency declarations. |

## Released schema pins

A published `*-vN.schema.json` is a contract: once a release carries it, clients resolve it
by `$id` and by digest, so its bytes must never change. Editing a released schema in place
is invisible to the four live digest pins (`compatibility/python.json`,
`python/dcc_mcp_core/deployment/install_sop.py`,
`crates/dcc-mcp-models/src/schema_validation.rs`, and the cross-assert in
`tests/test_install_catalog_provenance_contract.py`) because all four are computed from the
current file — updating them in the same commit keeps CI green. That is exactly how the
0.20.30 rewrite of `adapter-install-sop-v1` reached `main`.

`schema-pins.json` removes the loophole. Each entry is keyed by the schema `$id` and maps
released core tags to the sha256 of the committed bytes at that tag:

```json
"https://dcc-mcp.github.io/schemas/adapter-install-sop-v1.schema.json": {
  "path": "python/dcc_mcp_core/schemas/adapter-install-sop-v1.schema.json",
  "released": {
    "v0.20.29": "3ca25788439917b4d4c0617230a762f9797756b5b54f45c8c4149f975b90f904",
    "v0.20.30": "2b3a8a101384a5163c7569c4a2b0de6586c672c5ee291735f94334a33b7d37a0"
  }
}
```

The gate asserts four things:

1. **Released revisions are immutable.** `git cat-file blob <tag>:<path>` must still hash to
   the registered digest for every registered `(schema, tag)` pair.
2. **No in-place edits on the current tree.** The bytes git would commit for `path` must
   equal the digest of the newest registered tag. Rewriting a released `-vN` schema now
   fails until a new `-v(N+1)` revision is added.
3. **The registry is complete.** Discover stable Core releases independently from local
   `refs/tags/vMAJOR.MINOR.PATCH`, then inspect every released tree. Every shipped
   `*-vN.schema.json` must appear in `schema-pins.json`, even if a later release removed it.
   The newest registered digest must equal the newest released bytes for that path.
   Omitting a new release from the registry cannot hide its schemas; unchanged later
   releases need no redundant pins. Prerelease and non-Core tags are outside this gate.
4. **Every registered tag declares the registered `$id`.** Clients resolve a released revision
   by `$id` alone, so each registered tag — not only the newest — must match the key it is
   filed under.

### Adding or updating an entry

Add a `(tag, sha256)` pair under the schema's `$id` after its first release, before
changing it again. Use committed bytes from the actual release tag:

```bash
git cat-file blob v0.20.34:python/dcc_mcp_core/schemas/adapter-install-sop-v1.schema.json | sha256sum
```

Rules:

- Only add tags that already exist on `origin`. Never edit or delete a registered digest —
  that is the tamper evidence the gate relies on, and check 1 fails if the tag content
  moves. Deleting a whole entry also fails: check 3 discovers release refs independently
  and requires every `*-vN.schema.json` they ship to be present here. Branches and `HEAD`
  are not release tags. A pin for a later unchanged release is optional.
- Register a schema the first time a release ships it; `path` is the repository-relative
  location at that tag, and the key must equal the schema's `$id` at that tag.
- Changing a released `-vN` schema is not possible through this registry. Add
  `-v(N+1)` next to it and leave `-vN` untouched.
- Digests are taken from git object bytes, not from a raw file read, so `core.autocrlf`
  differences on Windows checkouts do not affect them.

## Core version bounds

`core-bounds.json` is the single source of the adapter dependency policy:
`dcc-mcp-core` is `0.x`, so a minor bump may break an adapter and every
adapter declaration must carry an upper bound that admits one minor line.
Both implementations read the same numbers — `dcc_mcp_core.deployment.core_bounds`
(Python) and `dcc-mcp-catalog::core_bounds` (Rust) — and both test suites fail
when the JSON and the code drift apart. The `reported_cases` array replays the
declarations that shipped without a usable ceiling, so the regression cannot be
re-introduced silently.

See [`docs/guide/adapter-core-version-contract.md`](../docs/guide/adapter-core-version-contract.md)
for the operator-facing rules.

### Running the bound contract locally

```bash
pytest tests/test_core_bounds.py
cargo test -p dcc-mcp-catalog
```

### Running the schema pins locally

```bash
pytest tests/test_released_schema_immutability.py
```

The gate requires complete history and all stable release tags, and fails explicitly on
shallow checkouts, missing registered refs, unreadable release objects, or an empty set of
release tags. Prepare a local clone once, while online:

```bash
# Only for a shallow clone:
git fetch --unshallow --tags origin
# For an existing full clone:
git fetch --tags origin
```

Tests only read local Git objects; they never fetch or call a release API, and disable Git
lazy fetching. As with any offline check, the tag inventory must have been fetched before
running it. CI supplies it with `fetch-depth: 0` in the dedicated gate and the full Python
matrices. The dedicated job also runs isolated Git-fixture regressions and rejects skipped
or empty execution. A new schema that exists only on untagged HEAD is not yet released and
needs no pin; it can be registered as soon as its first stable tag exists.

The six schemas added after the original registry are anchored to their introduction
releases: `anim-curves-v1`, `graph-state-v1`, `production-acceptance-v1`, `rig-state-v1`,
and `sim-status-v1` at `v0.20.31`, and `adapter-install-sop-v2` at `v0.20.34`. Their bytes
remain unchanged in `v0.20.44`. The recorded historical `v0.20.29`/`v0.20.30` digests are
preserved; this gate does not rewrite published schemas or the Core version contract.
