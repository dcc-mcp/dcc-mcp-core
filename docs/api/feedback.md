# Feedback API

Agent feedback and rationale utilities for DCC-MCP servers (issues #433, #434).

Two complementary features: **Rationale capture** — agents include `_meta.dcc.rationale` in `tools/call` requests to explain why they are invoking a tool. **Feedback tool** — `dcc_feedback__report` built-in MCP tool lets agents report when blocked or when a tool doesn't work as expected.

**Exported symbols:** `clear_feedback`, `extract_rationale`, `get_feedback_entries`, `make_rationale_meta`, `register_feedback_tool`

## register_feedback_tool

```python
register_feedback_tool(server, *, dcc_name="dcc") -> None
```

Register the `dcc_feedback__report` MCP tool on `server`. Call **before** `server.start()`.

The tool accepts: `tool_name`, `intent`, `blocker`, `severity` (`"blocked"` | `"workaround_found"` | `"suggestion"`), optional `attempt`.

## extract_rationale

```python
extract_rationale(params: dict | str) -> str | None
```

Extract `_meta.dcc.rationale` from a `tools/call` params dict.

```python
params = {
    "name": "create_sphere",
    "arguments": {"radius": 1.0},
    "_meta": {"dcc": {"rationale": "User wants a reference sphere."}},
}
rationale = extract_rationale(params)  # "User wants a reference sphere."
```

## make_rationale_meta

```python
make_rationale_meta(rationale: str) -> dict
```

Build the `_meta` fragment for a `tools/call` request with a rationale. Returns `{"_meta": {"dcc": {"rationale": "..."}}}`.

## get_feedback_entries

```python
get_feedback_entries(*, tool_name=None, severity=None, limit=50) -> list[dict]
```

Return recent feedback entries, newest first. Each entry has keys: `id`, `timestamp`, `tool_name`, `intent`, `attempt`, `blocker`, `severity`.

## clear_feedback

```python
clear_feedback() -> int
```

Clear all in-memory feedback entries. Returns the count cleared.
