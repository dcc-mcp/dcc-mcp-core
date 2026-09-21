# Compatibility contracts

Machine-readable contracts that CI enforces on every pull request.

| File | Enforced by | Purpose |
| --- | --- | --- |
| [`python.json`](python.json) | `scripts/ci/check_python_support.py` (`python37-contract` job) | Long-term-support Python policy, wheel profiles, and release projections. |
| [`schema-pins.json`](schema-pins.json) | `tests/test_released_schema_immutability.py` (`released-schema-immutability` job) | Digests of schema revisions that have already shipped in a release. |

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

The gate asserts two things:

1. **Released revisions are immutable.** `git cat-file blob <tag>:<path>` must still hash to
   the registered digest for every registered `(schema, tag)` pair.
2. **No in-place edits on the current tree.** The bytes git would commit for `path` must
   equal the digest of the newest registered tag. Rewriting a released `-vN` schema now
   fails until a new `-v(N+1)` revision is added.

### Adding or updating an entry

Add a `(tag, sha256)` pair under the schema's `$id` when you cut a release:

```bash
git cat-file blob v0.20.34:python/dcc_mcp_core/schemas/adapter-install-sop-v1.schema.json | sha256sum
```

Rules:

- Only add tags that already exist on `origin`. Never edit or delete a registered digest —
  that is the tamper evidence the gate relies on, and check 1 fails if the tag content
  moves.
- Register a schema the first time a release ships it; `path` is the repository-relative
  location at that tag, and the key must equal the schema's `$id` at that tag.
- Changing a released `-vN` schema is not possible through this registry. Add
  `-v(N+1)` next to it and leave `-vN` untouched.
- Digests are taken from git object bytes, not from a raw file read, so `core.autocrlf`
  differences on Windows checkouts do not affect them.

### Running locally

```bash
pytest tests/test_released_schema_immutability.py
```

The test needs the release tags, so a `--depth 1` clone skips it. CI runs it in the
`released-schema-immutability` job, which checks out with `fetch-depth: 0` and fails if the
gate is skipped or collects no tests.
