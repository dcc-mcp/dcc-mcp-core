# Adapter Install SOP v2 Migration

This guide covers the move from `adapter-install-sop-v1.schema.json` to
`adapter-install-sop-v2.schema.json`.

## Why v2 exists

`adapter-install-sop-v1.schema.json` is a published artifact. Core `0.20.30`
changed that file **in place**, keeping the filename and `$id` while replacing
the bytes. Every adapter that pinned the file's digest as an integrity anchor
failed at once when the new core shipped.

That rewrite is contained here:

- `-v1` is **frozen** at the bytes it last shipped with, the `0.20.30` release.
  A release cannot be unpublished, so those bytes are now the permanent `-v1`
  anchor and no downstream `-v1` anchor moves again.
- The content change is republished under a new filename and a new `$id` as
  `-v2`, which is where all future Install SOP content changes belong.

## Artifacts

| Artifact | Status | Size | sha256 |
|---|---|---|---|
| `adapter-install-sop-v1.schema.json` | **frozen** (bytes of core `0.20.30`–`0.20.33`) | 4899 | `2b3a8a101384a5163c7569c4a2b0de6586c672c5ee291735f94334a33b7d37a0` |
| `adapter-install-sop-v2.schema.json` | **current** | 4899 | `daa5840e07c956d7c9269e5709d6993a3988b905f986c06e7c4c02f5023e9422` |

Both artifacts describe the same document: `-v2` republishes the `-v1` bytes
under a new `$id` and title, so the only content difference between them is that
identification. `-v1` is frozen; `-v2` is where content changes go from here.

Availability differs. `-v1` ships in every core release from `0.20.30` onward.
`-v2` was merged to `main` after `0.20.33` was tagged, so **no published core
release contains it yet**: it first ships in the core release cut after
`0.20.33`. Until that release is published, anchor on the `-v2` digest above
rather than on a version.

Canonical ids:

```text
https://dcc-mcp.github.io/schemas/adapter-install-sop-v1.schema.json   (frozen)
https://dcc-mcp.github.io/schemas/adapter-install-sop-v2.schema.json   (current)
```

Both artifacts ship in the `dcc-mcp-core` wheel, so a `-v1` anchor keeps
resolving after this change.

## Changed fields

There is **no field delta** from the frozen `-v1` to `-v2`: both artifacts carry
the catalog provenance object, because core `0.20.30` shipped it under the `-v1`
name before the rewrite was noticed. What changes from v1 to v2 is the artifact
identity, not the document shape.

| Path | Change |
|---|---|
| `$defs.catalog_provenance` | present in both `-v1` and `-v2` |
| `properties.catalog` | optional property in both, `$ref` to `#/$defs/catalog_provenance` |

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

The frozen `-v1` already knows about `catalog`, so a report carrying it validates
identically under `-v1` and `-v2`. The reason to move to `-v2` is that `-v1` will
never receive another content change: an adapter that needs a future Install SOP
addition has to track `-v2`.

## Recommended migration steps

### If you do not consume `catalog`

**No change is required.** Your existing `-v1` anchor resolves to the frozen
`0.20.30` bytes and stays valid.

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

3. If you pin per core version, record the mapping
   `core >= <first core release after 0.20.33> -> v2`. `-v2` is not in any
   tagged release yet, so keep the digest pin above for now and replace it with
   that version bound once the release is published. Digests alone are not a
   version policy: record that mapping rather than probing both artifacts at
   runtime.

### If you pinned `2b3a8a101384a5163c7569c4a2b0de6586c672c5ee291735f94334a33b7d37a0`

That digest is the bytes that shipped in `0.20.30`–`0.20.33` under the `-v1`
name. Those bytes are now the frozen `-v1` artifact, so **a `-v1` anchor on this
digest is correct and keeps working** — if you only pin the bytes, no change is
needed.

If you also want the artifact to keep evolving, move the pin to the v2 digest
above: `-v2` carries the same document under a new `$id` and title, hence a
different digest.

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
