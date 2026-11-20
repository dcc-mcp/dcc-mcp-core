# Capabilities & Workspace Roots

> **Issue:** [#354](https://github.com/dcc-mcp/dcc-mcp-core/issues/354) — capability declaration + typed workspace path handshake
>
> **Status:** Available since v0.15

This guide covers two loosely-coupled features that make DCC tools safer and more
portable across hosts:

1. **Capability declaration** — tools declare what DCC features they need; adapters
   declare what the host can provide. The server blocks tool calls whose requirements
   are not satisfied.
2. **Typed workspace path handshake** — tools can use the `workspace://` URI scheme
   and the server resolves it against the MCP client's advertised filesystem roots.

---

## 1. Capability Declaration

### Why

Not every DCC exposes the same feature surface. Maya has USD; 3ds Max does not.
Some adapters run headless without filesystem access; others have full write
privileges. Declaring capabilities lets the runtime refuse a tool call **before**
the Python script runs and return a well-formed MCP error.

### Per-tool: `required_capabilities` in `tools.yaml`

Per **issue #356**, tool declarations live in a sibling `tools.yaml` file referenced
from `SKILL.md` via `metadata.dcc-mcp.tools`. Add `required_capabilities` to any
tool that needs a non-trivial host feature:

```yaml
# tools.yaml
tools:
  - name: import_usd
    description: Import a USD stage into the scene
    required_capabilities: [usd, scene.mutate, filesystem.read]

  - name: read_stage_metadata
    description: Read metadata from a USD stage without mutating the scene
    required_capabilities: [usd, scene.read, filesystem.read]

  - name: ping
    description: No capabilities required
```

Capability strings are freeform — treat them as convention between the skill author
and the adapter author. Common namespaces used by bundled skills:

| Namespace          | Meaning                                              |
|--------------------|------------------------------------------------------|
| `usd`              | USD stage / layer manipulation available             |
| `scene.read`       | Read the current DCC scene graph                     |
| `scene.mutate`     | Mutate the current DCC scene graph                   |
| `filesystem.read`  | Read files from disk                                 |
| `filesystem.write` | Write files to disk                                  |
| `viewport`         | Render / screenshot the active viewport              |

### Per-skill: aggregated via `SkillMetadata.required_capabilities()`

The loader automatically unions all per-tool capabilities on a skill:

```python
from dcc_mcp_core import SkillMetadata, scan_and_load

skills, _ = scan_and_load(dcc_name="maya")
for md in skills:
    print(md.name, md.required_capabilities)  # sorted deduplicated union
```

This is useful for `search_skills` filtering and for surfacing to AI agents via
`SKILL.md` overview.

### Host-side: `McpHttpConfig.declared_capabilities`

The DCC adapter declares what the current host can provide when it starts the
server:

```python
from dcc_mcp_core import create_skill_server, McpHttpConfig

cfg = McpHttpConfig(port=8765)
cfg.declared_capabilities = [
    "usd",
    "scene.read",
    "scene.mutate",
    "filesystem.read",
    # filesystem.write deliberately omitted for a read-only session
]
server = create_skill_server("maya", cfg)
handle = server.start()
```

### Runtime behaviour

**`tools/list`** — every tool is listed regardless of capabilities, but un-satisfied
tools carry a `_meta` hint so AI clients can skip them:

```jsonc
{
  "name": "import_usd",
  "description": "...",
  "inputSchema": { "...": "..." },
  "_meta": {
    "dcc": {
      "required_capabilities": ["usd", "scene.mutate", "filesystem.read"],
      "missing_capabilities": ["filesystem.write"]  // only if non-empty
    }
  }
}
```

**`tools/call`** — the server refuses the call with a structured JSON-RPC error:

```jsonc
{
  "jsonrpc": "2.0",
  "id": 1,
  "error": {
    "code": -32001,
    "message": "capability_missing: tool 'import_usd' requires filesystem.write",
    "data": {
      "tool": "import_usd",
      "required": ["usd", "scene.mutate", "filesystem.write"],
      "missing": ["filesystem.write"],
      "declared": ["usd", "scene.read", "scene.mutate", "filesystem.read"]
    }
  }
}
```

The error code `-32001` is dcc-mcp-core's `CAPABILITY_MISSING`. AI clients should
treat this as *permanently* failing for the current session rather than retrying.

### Probed host capabilities (issue #2867)

A declaration says what an adapter **claims**; it is not evidence that the host
actually does it. `dcc-mcp/host-capabilities@1` records the probe that proves
each capability, so an agent can plan a long, expensive job without discovering
an unavailable feature afterwards.

The contract is published at
`python/dcc_mcp_core/schemas/host-capabilities-v1.schema.json` and is validated
in pure Python, so an embedded DCC interpreter can check a manifest without the
native extension:

```python
from dcc_mcp_core.capabilities import (
    PROBE_NOT_RUN,
    PROBE_PASS,
    PROBE_UNSUPPORTED,
    build_host_capabilities,
    make_host_capability,
    summarize_host_capabilities,
)

manifest = build_host_capabilities(
    host_version="0.32.0",
    adapter_version="0.1.6",
    dcc_type="comfyui",
    capabilities=[
        make_host_capability(
            "workflow.api_format", True, PROBE_PASS, evidence_ref="artifact://probe-1"
        ),
        make_host_capability(
            "queue.cancel_running",
            False,
            PROBE_UNSUPPORTED,
            reason="legacy interrupt is global, not per-prompt",
        ),
        make_host_capability(
            "load3d.sync_node",
            True,
            PROBE_NOT_RUN,
            reason="custom node ships but the probe is not implemented yet",
            declaration_only=True,
        ),
    ],
)

summary = summarize_host_capabilities(manifest)
# {"available": [...], "declared": [...], "unavailable": [...], "not_run": [...], ...}
```

The contract fails closed:

- `supported: true` needs a `PASS` probe, or `declaration_only: true` **and** a
  reason. A declaration is reported as `declared`, never as `available`.
- A declaration must not claim `PASS`; `PASS` is reserved for a recorded probe.
- `supported: false` needs a concrete blocker in `reason`, not a generic
  "unsupported".
- A `PASS` probe cannot coexist with `supported: false`, and `FAIL` /
  `UNSUPPORTED` cannot coexist with `supported: true`.
- `NOT_RUN` never counts as available; unknown keys, duplicate keys, malformed
  versions, and unknown fields are rejected. Version strings use a numeric
  grammar, and false-like strings such as `"false"` and `"0"` are rejected.
- `probed_at` must be an RFC 3339 date-time with a timezone at both manifest and
  record level. The published JSON Schema and Python validator share a strict
  self-contained profile: common `jsonschema` installations treat `format:
  date-time` as an annotation, so the schema's `pattern` also enforces calendar
  validity (including leap years), 00-23 hours, 00-59 minutes and seconds, and
  offsets below 24 hours. Leap seconds are rejected.

`DccServerBase` registers `describe_capabilities` automatically during standard
builtin registration. `dcc_host_capabilities` remains a compatibility alias. Both
names use the same provider, cache, and summary contract. The tools are ordinary
backend tools, so the existing search, describe, call, and REST paths expose them
without loading every skill.

Adapters provide the manifest provider and may inject a bounded probe runner:

```python
from dcc_mcp_core.capabilities import CapabilityProbeRunner
from dcc_mcp_core.capabilities import HostCapabilityCache
from dcc_mcp_core.capabilities import build_host_capabilities


def probe_capability(key, context):
    # The adapter owns the host operation and its concrete evidence.
    return adapter_probe(key, context)


def describe_host():
    return build_host_capabilities(
        host_version=current_host_version(),
        adapter_version=current_adapter_version(),
        capabilities=adapter_declarations(),
    )


runner = CapabilityProbeRunner(
    probe_capability,
    timeout_secs=5,
    context=current_probe_context(),
    cancel=cancel_adapter_probe,
)
server.set_host_capability_provider(
    describe_host,
    provider_cache_key=current_host_identity,
    cache=HostCapabilityCache(ttl_secs=300),
    probe_runner=runner,
)
```

`describe_capabilities` accepts `keys`, `force_refresh`, and `invalidate`. The
`capability_probe` tool accepts `key` or `keys`, runs only the requested adapter
probes, and stores their typed result in the same host/adapter-version cache.
A subsequent probe call reuses a cached result until `invalidate: true` or
`force_refresh: true` is supplied. `provider_cache_key` returns
`(dcc_type, host_version, adapter_version)` so a host or adapter update cannot
reuse evidence from the previous build. The standard server supplies a default
identity callback when an adapter does not provide one.

Core bounds how long it waits for each adapter probe. It cannot safely terminate
an arbitrary host callable. Adapters should implement the optional `cancel`
callback and stop their underlying host operation when Core reports a timeout.
Timed-out, malformed, or conflicting probe records become fail-closed `FAIL`
records and cannot claim support. Adapter-specific blocker text and evidence
references remain adapter-owned.

The CLI exposes the canonical `describe-capabilities` command and keeps
`capabilities` as a compatibility command. Both accept `--keys`,
`--force-refresh`, and `--invalidate`. JSON output retains the canonical
`backend_tool` name and the evidence fields needed by planning clients.
Backend routing is provided by the standard `describe_capabilities` registration;
host probes remain owned by adapter integrations.

---

## 2. Typed Workspace Path Handshake

### Why

MCP clients advertise filesystem roots (`file:///home/user/project/...`) via the
`initialize` request's `roots` capability. Tools that accept paths historically
had to either:

- Trust the AI to pass absolute paths (risky — escapes the workspace),
- Or accept raw strings and re-implement root resolution (boilerplate).

The `WorkspaceRoots` helper centralises this. Tools accept the `workspace://`
URI scheme and the server resolves it against the session's first root.

### Using `WorkspaceRoots` from a tool

`WorkspaceRoots` is exposed as a Python class. When a tool declares a
`filesystem.*` capability, the server injects a `_workspace_roots` arg into the
tool context:

```python
def import_usd(path: str, _workspace_roots=None):
    if _workspace_roots is None:
        return error_result("No workspace roots advertised", "missing_workspace_roots")
    try:
        resolved = _workspace_roots.resolve(path)
    except ValueError as e:
        return error_result(
            "Import path is invalid",
            "invalid_path",
            _meta={"dcc.error": {"type": type(e).__name__, "message": str(e)}},
        )
    # ...continue with `resolved` as an absolute PathBuf-equivalent
```

### Resolution rules

| Input                           | Behaviour                                            |
|---------------------------------|------------------------------------------------------|
| `workspace://assets/hero.usd`   | Joined with first advertised root                    |
| `/abs/path/scene.ma`            | Returned unchanged                                   |
| `C:\Users\me\scene.max`         | Returned unchanged (Windows absolute)                |
| `assets/hero.usd` (relative)    | Joined with first root if available; else unchanged  |
| `workspace://...` with no roots | Raises `no workspace roots` (MCP error `-32602`)     |

### Constructing manually (for tests)

```python
from dcc_mcp_core import WorkspaceRoots

roots = WorkspaceRoots(["/projects/hero"])
assert roots.resolve("workspace://char/bob.usd") == "/projects/hero/char/bob.usd"
assert roots.resolve("/tmp/abs").endswith("abs")
```

### Rust API

```rust
use dcc_mcp_http::{WorkspaceRoots, WorkspaceResolveError};

let roots = WorkspaceRoots::from_client_roots(&session.roots());
let path = roots.resolve("workspace://assets/hero.usd")?;
// path is an absolute std::path::PathBuf
```

`WorkspaceResolveError::NoRoots` maps to JSON-RPC error code `-32602`
(`NO_WORKSPACE_ROOTS`).

---

## See also

- [Skills guide](skills.md) — `tools.yaml` sibling-file pattern (#356)
- [`docs/guide/naming.md`](naming.md) — client-safe tool-name validation
- MCP roots spec: <https://modelcontextprotocol.io/specification/2025-03-26/client/roots>
