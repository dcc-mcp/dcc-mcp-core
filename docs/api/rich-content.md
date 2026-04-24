# Rich Content — MCP Apps Inline UI

> Source: [`python/dcc_mcp_core/rich_content.py`](https://github.com/loonghao/dcc-mcp-core/blob/main/python/dcc_mcp_core/rich_content.py) · Issue [#409](https://github.com/loonghao/dcc-mcp-core/issues/409) · [MCP Apps overview](https://modelcontextprotocol.io/extensions/apps/overview)
>
> **[中文版](../zh/api/rich-content.md)**

MCP Apps is the first official MCP protocol extension. A tool can return
an interactive interface — chart, form, dashboard, image, table —
rendered inline in the chat interface, **without hitting the model
context**. Servers that return rich content see meaningfully higher
adoption than text-only servers.

**When rich content pays off for DCC tools**

| Tool | Rich return | Value |
|------|-------------|-------|
| `render_frames` | Thumbnail gallery + stats table | Visual verification without leaving chat |
| `get_scene_hierarchy` | Interactive tree | Browse 10k-node scene |
| `diagnostics__screenshot` | Inline screenshot | More useful than a file path |
| `analyze_keyframes` | Vega-Lite curve chart | Visual timing debug |
| `get_render_stats` | Bar chart per layer | Faster than raw JSON |
| `list_materials` | Material swatch grid | Visual selection |

## Imports

```python
from dcc_mcp_core import (
    RichContent,
    RichContentKind,
    attach_rich_content,
    skill_success_with_chart,
    skill_success_with_image,
    skill_success_with_table,
)
```

## `RichContentKind` (enum)

| Value | Rendered as |
|-------|-------------|
| `"chart"` | Vega-Lite / Chart.js spec |
| `"form"` | Interactive JSON-Schema form |
| `"dashboard"` | Composite layout of multiple components |
| `"image"` | Inline PNG / JPEG / WebP (base64) |
| `"table"` | Headers + rows grid |

## `RichContent` (dataclass)

Prefer the class-method constructors over the raw dataclass.

### `RichContent.chart(spec) -> RichContent`

Vega-Lite v5 or Chart.js specification dict.

```python
RichContent.chart({
    "$schema": "https://vega.github.io/schema/vega-lite/v5.json",
    "data": {"values": [{"x": 1, "y": 2}]},
    "mark": "line",
    "encoding": {"x": {"field": "x"}, "y": {"field": "y"}},
})
```

### `RichContent.form(schema, *, title=None) -> RichContent`

Interactive form rendered from a JSON Schema. Distinct from
[Elicitation](./elicitation.md) — this `form` is part of the **tool
result** (one-shot display), whereas elicitation *pauses* the tool call
for user input.

### `RichContent.image(data, mime="image/png", *, alt=None) -> RichContent`

Raw image bytes encoded to base64.

### `RichContent.image_from_file(path, mime=None, *, alt=None) -> RichContent`

Convenience loader. MIME type auto-detected from extension (`.png`,
`.jpg`, `.jpeg`, `.webp`, `.gif`).

### `RichContent.table(headers, rows, *, title=None) -> RichContent`

Grid with `headers: list[str]` and `rows: list[list]`. Every inner row
list must have the same length as `headers`.

### `RichContent.dashboard(components) -> RichContent`

Composite layout containing an ordered list of other `RichContent`
items.

### `.to_dict() -> dict`

Flattens to `{"kind": <value>, **payload}` — safe for JSON
serialization.

## `attach_rich_content(result, content) -> dict`

Stash a `RichContent` on an existing skill result dict. Stored under
`result["context"]["__rich__"]` — MCP-Apps-aware clients render it;
plain clients ignore it gracefully (backward-compatible).

```python
result = skill_success("Render complete", total_frames=250)
return attach_rich_content(result, RichContent.chart({...}))
```

## Skill-script helpers

These return ready-to-use skill result dicts. Additional keyword
arguments are forwarded into the `context` dict.

### `skill_success_with_chart(message, chart_spec, **context) -> dict`

```python
return skill_success_with_chart(
    "Render complete",
    chart_spec={
        "$schema": "https://vega.github.io/schema/vega-lite/v5.json",
        "data": {"values": render_stats},
        "mark": "bar",
        "encoding": {
            "x": {"field": "layer"},
            "y": {"field": "time_secs"},
        },
    },
    total_frames=250,
)
```

### `skill_success_with_table(message, headers, rows, *, title=None, **context) -> dict`

```python
return skill_success_with_table(
    "Scene objects",
    headers=["Name", "Type", "Vertices"],
    rows=[["pCube1", "mesh", 8], ["nurbsSphere1", "nurbs", 0]],
)
```

### `skill_success_with_image(message, image_data=None, image_path=None, mime="image/png", *, alt=None, **context) -> dict`

Provide either `image_data` (raw bytes) or `image_path` (file). Raises
`ValueError` if neither is given.

```python
return skill_success_with_image(
    "Viewport captured",
    image_data=capture_viewport(),
    alt="Maya viewport",
)
```

## Current status — context-side storage, Rust-side wiring pending

Rich content is stored today in `result.context["__rich__"]` as a
JSON-serialisable dict. Full wiring into `tools/call` responses using
the MCP Apps canonical envelope is tracked in issue
[#409](https://github.com/loonghao/dcc-mcp-core/issues/409).

Skills written against these helpers today will automatically surface
rich content to MCP-Apps clients once the Rust layer ships.

## See also

- [Remote Server guide](../guide/remote-server.md)
- [Elicitation](./elicitation.md) — *pauses* a tool for input; this doc covers *one-shot* display
- [Vega-Lite v5 docs](https://vega.github.io/vega-lite/) — chart schema reference
- [MCP Apps extension](https://modelcontextprotocol.io/extensions/apps/overview)
