# Introspection API

Runtime namespace discovery tools for DCC host interpreters (issue #426).

Four read-only MCP tools that let AI agents inspect the live DCC Python namespace without burning tokens on web searches. All tools are registered with `read_only_hint=True, idempotent_hint=True` and hard-cap their output.

**Exported symbols:** `introspect_eval`, `introspect_list_module`, `introspect_search`, `introspect_signature`, `register_introspect_tools`

## register_introspect_tools

```python
register_introspect_tools(server, *, dcc_name="dcc") -> None
```

Register the four `dcc_introspect__*` tools on `server`. Call **before** `server.start()`.

Registered tools: `dcc_introspect__list_module`, `dcc_introspect__signature`, `dcc_introspect__search`, `dcc_introspect__eval`.

## introspect_list_module

```python
introspect_list_module(module_name: str, *, limit: int = 200) -> dict
```

Return exported names from `module_name`.

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| `module_name` | `str` | | Dotted module path (e.g. `"maya.cmds"`) |
| `limit` | `int` | 200 | Max names to return |

**Returns:** `{"names": [...], "count": N, "truncated": bool}`

## introspect_signature

```python
introspect_signature(qualname: str) -> dict
```

Return signature and docstring for `qualname` (e.g. `"maya.cmds.polyCube"`).

**Returns:** `{"signature": str, "doc": str, "source_file": str|None, "kind": str}`

## introspect_search

```python
introspect_search(pattern: str, module_name: str, *, limit: int = 50) -> dict
```

Regex-search exported names in `module_name` (case-insensitive).

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| `pattern` | `str` | | Case-insensitive regex |
| `module_name` | `str` | | Module to search |
| `limit` | `int` | 50 | Max hits to return |

**Returns:** `{"hits": [{"qualname": str, "summary": str}, ...], "count": int}`

## introspect_eval

```python
introspect_eval(expression: str) -> dict
```

Evaluate a read-only Python expression and return its repr. Only bare expressions allowed — no assignments, imports, or multi-statement code.

::: warning
`introspect_eval` has a lightweight guard against obvious statement patterns, but it evaluates code in the DCC interpreter. Use `SandboxPolicy` in production.
:::

**Returns:** `{"repr": str}` on success, or `{"success": False, "message": err}` on error.
