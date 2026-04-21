# Artefact Hand-Off (FileRef + ArtefactStore)

> **Issue**: [#349](https://github.com/loonghao/dcc-mcp-core/issues/349)
> **Crate**: `dcc-mcp-artefact`
> **Scheme**: `artefact://sha256/<hex>`

Pipelines move *files* between tools and between workflow steps: an imported
scene, a QC report, a staged `.uasset`, a baked simulation. Passing the raw
bytes inline bloats the MCP transport; passing an absolute path breaks
across machines and strips MIME / size / checksum metadata.

`dcc-mcp-artefact` ships a small value type (`FileRef`) plus a
content-addressed storage backend, and wires an `artefact://` URI scheme
into the MCP Resources primitive so MCP clients can `resources/read` a
file hand-off by URI.

## Concepts

- **`FileRef`** — a serializable reference to a stored file:
  - `uri` — canonical, e.g. `artefact://sha256/<hex>`
  - `mime`, `size_bytes`, `digest` (`sha256:<hex>`)
  - `producer_job_id` (optional, for workflow step outputs)
  - `created_at` (RFC-3339)
  - `metadata` (tool-defined JSON — width/height, frame number, etc.)

- **`ArtefactStore`** — trait with `put` / `get` / `head` / `delete` /
  `list`. Stores are content-addressed: submitting the same bytes twice
  returns the same URI.

- **`FilesystemArtefactStore`** — default persistent backend. Each
  artefact lives under `<root>/<sha256>.bin` with a `<sha256>.json`
  sidecar carrying the `FileRef`. `McpHttpServer` anchors its store at
  `<registry_dir>/dcc-mcp-artefacts` (or the OS temp dir when no
  registry is configured).

- **`InMemoryArtefactStore`** — non-persistent backend for tests.

## Enabling Artefact Resources

```python
from dcc_mcp_core import McpHttpConfig, McpHttpServer, ToolRegistry

cfg = McpHttpConfig(port=8765)
cfg.enable_artefact_resources = True    # off by default
server = McpHttpServer(ToolRegistry(), cfg)
handle = server.start()
```

Once enabled:

- `resources/list` includes every stored artefact as an `artefact://` URI.
- `resources/read` returns the body as a base64 blob with the correct MIME.
- `resources/read` of an unknown URI returns MCP error `-32002`.

Disabled (default) paths still recognize the scheme and respond with a
`-32002` "not enabled" error, so clients can distinguish "scheme unknown"
from "scheme recognized but backing store off".

## Python Helpers

```python
from dcc_mcp_core import (
    FileRef,
    artefact_put_bytes,
    artefact_put_file,
    artefact_get_bytes,
    artefact_list,
)

# Put a file on disk and get back a FileRef.
ref = artefact_put_file("/tmp/render.png", mime="image/png")
print(ref.uri)           # artefact://sha256/<hex>
print(ref.digest)        # sha256:<hex>
print(ref.size_bytes)    # 1024

# Round-trip a byte buffer.
bref = artefact_put_bytes(b"hello", mime="text/plain")
assert artefact_get_bytes(bref.uri) == b"hello"

# Inventory.
for entry in artefact_list():
    print(entry.uri, entry.mime, entry.size_bytes)
```

The helpers target a process-global default store
(`<temp_dir>/dcc-mcp-artefacts`). Inside a server process the
`McpHttpServer` points the helpers at its own store automatically.

## Rust API

```rust
use dcc_mcp_artefact::{
    ArtefactBody, ArtefactFilter, ArtefactStore,
    FilesystemArtefactStore, InMemoryArtefactStore,
    put_bytes, put_file,
};

// Persistent store — default for real servers.
let store = FilesystemArtefactStore::new_in("/var/cache/dcc/artefacts")?;
let fr = put_bytes(&store, b"payload".to_vec(), Some("text/plain".into()))?;
assert!(fr.uri.starts_with("artefact://sha256/"));

// Look up by URI.
let body = store.get(&fr.uri)?.unwrap();
assert_eq!(body.into_bytes()?, b"payload");

// List artefacts filtered by producing job.
let refs = store.list(ArtefactFilter {
    producer_job_id: Some(job_id),
    ..Default::default()
})?;
```

## Workflow Integration

The workflow runner (landing in a follow-up PR under issue #348)
propagates `FileRef`s through step outputs:

1. A step emits a `ToolResult` whose `context` contains
   `{"file_refs": [{"uri": "artefact://sha256/...", "mime": "image/png"}]}`.
2. The runner stores the `FileRef`s on the step record and substitutes
   them into the next step's argument context.
3. The downstream step fetches the bytes via
   `artefact_get_bytes(uri)` — or by `resources/read` when using the
   MCP resources primitive from outside the runner process.

This PR lands the types, storage, and resource wiring. Runner
integration is out of scope.

## Gotchas

- **Duplicate content → same URI.** Don't rely on URI uniqueness for
  logical ordering — use `producer_job_id` and `metadata` instead.
- **`put_file` copies.** The source path is left untouched; the store
  owns the canonical copy.
- **Sidecar is authoritative for metadata.** Editing the JSON sidecar
  by hand is supported; editing the `.bin` file is not (the digest
  would mismatch).
- **No GC yet.** Stores never auto-delete. TTL / reference-count GC is
  tracked in a future issue.
- **No remote backends yet.** S3 / SFTP / HTTP pointers are declared
  in the roadmap but not implemented — a future issue.

## See Also

- [`docs/api/resources.md`](../api/resources.md) — the broader Resources
  primitive that hosts `artefact://`.
- Issue [#348](https://github.com/loonghao/dcc-mcp-core/issues/348) —
  workflow runner that consumes `FileRef`s.
- Issue [#350](https://github.com/loonghao/dcc-mcp-core/issues/350) —
  Resources primitive (merged).
