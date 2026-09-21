# Adapter Install SOP v2 Migration

This guide covers the move from `adapter-install-sop-v1.schema.json` to
`adapter-install-sop-v2.schema.json`.

## Why v2 exists

`adapter-install-sop-v1.schema.json` is a published artifact. Core `0.20.30`
changed that file **in place**, keeping the filename and `$id` while replacing
the bytes. Every adapter that pinned the file's digest as an integrity anchor
failed at once when the new core shipped.

That in-place rewrite is reverted here:

- `-v1` is restored byte-for-byte to the `0.20.29` release and is now **frozen**.
- The content change that caused it is republished under a new filename and a
  new `$id` as `-v2`.

## Artifacts

| Artifact | Status | Size | sha256 |
|---|---|---|---|
| `adapter-install-sop-v1.schema.json` | **frozen** (restored to core `0.20.14`–`0.20.29`) | 4261 | `3ca25788439917b4d4c0617230a762f9797756b5b54f45c8c4149f975b90f904` |
| `adapter-install-sop-v2.schema.json` | **current** | 4899 | `daa5840e07c956d7c9269e5709d6993a3988b905f986c06e7c4c02f5023e9422` |

Canonical ids:

```text
https://dcc-mcp.github.io/schemas/adapter-install-sop-v1.schema.json   (frozen)
https://dcc-mcp.github.io/schemas/adapter-install-sop-v2.schema.json   (current)
```

Both artifacts ship in the `dcc-mcp-core` wheel, so a `-v1` anchor keeps
resolving after this change.

## Changed fields

The only delta from v1 to v2 is the catalog provenance object.

| Path | Change |
|---|---|
| `$defs.catalog_provenance` | **new** definition |
| `properties.catalog` | **new** optional property, `$ref` to `#/$defs/catalog_provenance` |

`catalog_provenance` is a closed object (`additionalProperties: false`) that
requires `source` and `latest_checked`:

| Field | Type | Notes |
|---|---|---|
| `source` | enum | `remote`, `cache`, `bundled`, `explicit`, `unavailable` |
| `latest_checked` | boolean | whether the catalog was confirmed current |
| `sha256` | string, `^[a-fA-F0-9]{64}$` | optional |
| `source_revision` | string, `^[a-fA-F0-9]{40}$` | optional |
| `issued_at` | integer, `>= 0` | optional |
| `expires_at` | integer, `>= 0` | optional |

Everything else is byte-identical: the same `required` list, the same `$defs`
entries, and the same `schema_version` constraint.

## Backward compatibility: compatible

**Yes — the new fields are backward compatible.** `catalog` is optional:

- It is **not** in `required`, so v1-era reports remain valid under v2.
- The report document's `schema_version` field stays at `1`. v2 does **not**
  introduce a new document format version, so a consumer that understands
  document version 1 keeps working unchanged.
- A consumer that ignores unknown properties needs no change at all.

Note on the constant name: `INSTALL_SOP_SCHEMA_VERSION` (now `2`) is the
**artifact** revision — the `-vN` suffix of the published schema file. It is not
the report document's `schema_version` field. Never copy it into a report:
the schema pins `properties.schema_version` to the constant `1`, so a report
emitted with `schema_version: 2` is rejected.

The one caveat: `-v1` itself does not know about `catalog`. Validating a report
that carries `catalog` against the frozen `-v1` artifact will not check its
contents (the top-level schema does not forbid additional properties, so it will
still pass). Only switch to `-v2` if you want the catalog provenance validated.

## Recommended migration steps

### If you do not consume `catalog`

**No change is required.** Your existing `-v1` anchor now resolves to the
restored, frozen bytes and stays valid.

### If you consume `catalog`, or want it validated

1. Point the validator at the new artifact:

   ```python
   from dcc_mcp_core.deployment import load_install_sop_schema
   ```

   `load_install_sop_schema()` now returns the v2 schema. If you resolve the
   file yourself, use `dcc_mcp_core/schemas/adapter-install-sop-v2.schema.json`
   and the `$id`
   `https://dcc-mcp.github.io/schemas/adapter-install-sop-v2.schema.json`.

2. Update any hard-coded digest to
   `daa5840e07c956d7c9269e5709d6993a3988b905f986c06e7c4c02f5023e9422`.

3. If you pin per core version, add the mapping
   `core >= 0.20.30 -> v2`. Digests alone are not a version policy: record which
   core release first shipped each artifact rather than probing both.

### If you pinned `2b3a8a101384a5163c7569c4a2b0de6586c672c5ee291735f94334a33b7d37a0`

That digest was the in-place rewrite that shipped in `0.20.30`–`0.20.33` under
the `-v1` name. It is **not** a legitimate `-v1` revision and is not restored.
Its content now lives in `-v2` (with the new `$id` and title, hence a different
digest). Use the v2 digest above.

## Adapter-owned schema snapshots

Some adapters vendor their own copy of the schema, for example
`src/<package>/schemas/adapter-install-sop-v1.schema.json`.

**Recommendation: do not vendor a copy of a core-owned contract.** A snapshot
adds a second source of truth and re-creates the drift this migration fixes.

- If the snapshot is unused (only its `$id` string is read), **delete it** and
  import the canonical id or the packaged schema from
  `dcc_mcp_core.deployment` instead.
- If the adapter must validate offline, **read the core-packaged artifact at
  runtime** rather than copying it, and treat the digest pin as an integrity
  check on a version-scoped basis, not as a contract of its own.
- If a snapshot must stay, name it after the revision it mirrors
  (`adapter-install-sop-v2.schema.json`), record the core version it was taken
  from, and refresh it deliberately — never silently.

## Freezing rule for future changes

A published `-vN` artifact is immutable. Any content change ships as `-v(N+1)`:

1. Copy the current artifact to the new revision.
2. Update `$id`, `title`, and the digest pins
   (`compatibility/python.json`, `python/dcc_mcp_core/deployment/install_sop.py`,
   `crates/dcc-mcp-models/src/schema_validation.rs`).
3. Leave the previous artifact untouched and keep shipping it.

Never edit a released `-vN` file.
