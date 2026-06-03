# Naming your actions and tools

> **Status**: mandatory. Every DCC-MCP crate, Python wheel and skill author
> must pick names that pass the two validators shipped in
> [`dcc_mcp_core::naming`](https://github.com/dcc-mcp/dcc-mcp-core/tree/main/crates/dcc-mcp-naming).
> Related spec: [MCP `draft/server/tools#tool-names`](https://modelcontextprotocol.io/specification/draft/server/tools#tool-names).

There are **two** naming rules in the ecosystem. Know which one applies to
the string you are writing before you reach for a keyboard.

| Concept | Purpose | Who sees it | Validator | Regex |
|---------|---------|-------------|-----------|-------|
| **Tool name** | MCP wire-visible string published in `tools/list` | The LLM / the MCP client | `validate_tool_name` | `^[A-Za-z0-9_-]{1,64}$` |
| **Action id** | Internal, stable id used by hosts to route `tools/call` | Rust/Python code, hand-wired registrations | `validate_action_id` | `^[a-z][a-z0-9_]*(?:\.[a-z][a-z0-9_]*)*$` |

## Why two rules?

The MCP spec is permissive on tool names, but common desktop clients validate
the stricter `^[A-Za-z0-9_-]{1,64}$` shape. `dcc-mcp-core` uses that common
wire contract directly so `tools/list` remains accepted everywhere.

`dcc-mcp-core` therefore keeps two layers:

1. **Tool names** use only ASCII letters, digits, `_`, and `-`, capped at
   64 characters.
2. **Action ids** are stricter: dotted, lowercase, snake-case segments. You
   write these by hand in your host code; the library turns them into tool
   names when publishing.

## Using the validators

### Rust

```rust
use dcc_mcp_naming::{validate_tool_name, validate_action_id};

validate_tool_name("geometry_create_sphere")?;
validate_action_id("scene.get_info")?;
```

Both functions are `O(n)`, allocation-free, and return a structured
[`NamingError`](https://docs.rs/dcc-mcp-naming) pointing at the first
violation.

### Python

```python
from dcc_mcp_core import (
    TOOL_NAME_RE,
    ACTION_ID_RE,
    MAX_TOOL_NAME_LEN,
    validate_tool_name,
    validate_action_id,
)

validate_tool_name("hello-world_greet")        # ok
validate_action_id("scene.get_info")           # ok

validate_tool_name("bad/name")                 # raises ValueError
validate_action_id("Scene.Get")                # raises ValueError (uppercase)
```

The regex constants (`TOOL_NAME_RE`, `ACTION_ID_RE`) are exported for
downstream tooling — schema generators, lint rules, docs — that need to
reference the pattern without calling into Rust. **The validator remains the
authoritative check**: prefer `validate_tool_name()` over re-implementing
the regex in your own code.

## Cheatsheet

### Valid tool names

```
create_sphere
geometry_create_sphere
scene_object_transform
hello-world_greet
CamelCaseTool          # MCP allows mixed case
_leading               # leading `_` is accepted
-leading               # leading `-` is accepted
0              # single ASCII alphanumeric is legal
```

### Invalid tool names

| Input | Reason |
|-------|--------|
| `""` | empty |
| `tool.name` / `.tool` | `.` is not part of the common client-safe alphabet |
| `tool/call` | `/` is reserved for gateway prefixes |
| `tool name` / `tool,other` / `tool@host` / `tool+v2` | punctuation outside `[_-]` |
| `a * 65` | exceeds `MAX_TOOL_NAME_LEN = 64` |
| `工具` / `tôol` | non-ASCII |

### Valid action ids

```
scene
create_sphere
scene.get_info
maya.geometry.create_sphere
v2.create
```

### Invalid action ids

| Input | Reason |
|-------|--------|
| `""` | empty |
| `Scene.get` / `scene.Get` | uppercase |
| `1scene.get` | leading digit |
| `scene..get` / `.scene` / `scene.` | empty `.`-separated segment |
| `scene-get` | `-` is not allowed in action ids (use `_`) |
| `scene/get` | `/` is not allowed |

## Caps and rationale

* **`MAX_TOOL_NAME_LEN = 64`** — matches the common client-safe MCP tool name
  limit used by desktop clients.
* **Stricter action-id grammar** — keeps hand-typed identifiers consistent
  with Python attribute conventions (lowercase, snake_case, dot-separated
  namespaces) and eliminates ambiguity when action ids are serialised in
  audit logs, telemetry and IPC payloads.

## When to call the validators

* **Host authors** — call `validate_action_id` **at registration time**,
  not at dispatch time. A registry that accepts bad ids is a bug magnet.
* **Server authors** — call `validate_tool_name` before publishing a tool
  in `tools/list`, including skill-derived tools where the tool name is
  composed from the skill slug + tool slug.
* **Skill authors** — no explicit call needed; the library validates your
  skill's tool names when loading the skill. Invalid names cause the skill
  to fail loading with a human-readable error.

## Migration from bespoke rules

Earlier code paths occasionally re-invented these rules (substring checks,
ad-hoc regexes). When you touch such code, replace it with the validator:

```diff
- if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
-     return Err("bad tool name");
- }
+ dcc_mcp_naming::validate_tool_name(name)?;
```

The goal is **one rule, one implementation** — no `name.len() > 100` in
random files, no "I think it should allow hyphens" disagreement between
crates.
