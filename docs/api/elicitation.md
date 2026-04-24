# Elicitation — Mid-tool-call User Input

> Source: [`python/dcc_mcp_core/elicitation.py`](https://github.com/loonghao/dcc-mcp-core/blob/main/python/dcc_mcp_core/elicitation.py) · Issue [#407](https://github.com/loonghao/dcc-mcp-core/issues/407) · [MCP 2025-11-25 Elicitation spec](https://modelcontextprotocol.io/specification/2025-11-25/client/elicitation)
>
> **[中文版](../zh/api/elicitation.md)**

Elicitation lets a tool handler **pause** mid-execution to ask the end user
for input — either a form rendered from JSON Schema, or a browser URL
flow (OAuth, payment, credential collection).

**When to use**

- **Destructive confirmations** — "Delete 127 shot cameras? This cannot be undone."
- **Missing required parameter** — Agent invoked without `render_layer`; pop a dropdown.
- **Auth flows** — Send the user to `/oauth/authorize` and wait for callback.

Without elicitation these scenarios require bouncing through the agent
again — costing tokens and often breaking flow.

## Imports

```python
from dcc_mcp_core import (
    ElicitationMode,
    ElicitationRequest,
    ElicitationResponse,
    FormElicitation,
    UrlElicitation,
    elicit_form,
    elicit_form_sync,
    elicit_url,
)
```

## Types

### `ElicitationMode` (enum)

| Value | Meaning |
|-------|---------|
| `ElicitationMode.FORM` | Client renders a JSON-Schema form |
| `ElicitationMode.URL` | Client opens a browser URL and awaits completion |

### `FormElicitation`

| Field | Type | Notes |
|-------|------|-------|
| `message` | `str` | Prompt above the form |
| `schema` | `dict` | JSON Schema (`type: object`, `properties`, `required`) |
| `title` | `str \| None` | Optional dialog title |

### `UrlElicitation`

| Field | Type | Notes |
|-------|------|-------|
| `message` | `str` | Short description |
| `url` | `str` | Browser URL |
| `description` | `str \| None` | Longer explanation |

### `ElicitationRequest`

Wraps `mode` + a `FormElicitation` or `UrlElicitation`.

### `ElicitationResponse`

| Field | Type | Notes |
|-------|------|-------|
| `accepted` | `bool` | `True` on submit, `False` on cancel / unsupported client |
| `data` | `dict \| None` | User-supplied values (form mode only) |
| `message` | `str \| None` | Status / error message |

## Helpers

### `await elicit_form(message, schema, *, title=None) -> ElicitationResponse`

Async form elicitation for `async def` skill handlers.

```python
async def delete_objects(objects: list[str], **kwargs):
    resp = await elicit_form(
        message=f"Delete {len(objects)} objects? This cannot be undone.",
        schema={
            "type": "object",
            "properties": {"confirm": {"type": "boolean", "title": "Confirm deletion"}},
            "required": ["confirm"],
        },
    )
    if not resp.accepted or not resp.data.get("confirm"):
        return {"success": False, "message": "Cancelled by user"}
    # ... proceed ...
```

### `await elicit_url(message, url, *, description=None) -> ElicitationResponse`

Async URL elicitation (OAuth, payment, credential flow). Opens the URL
in the user's browser and waits for the client to report completion.

### `elicit_form_sync(message, schema, *, title=None, fallback_values=None) -> ElicitationResponse`

Blocking variant for DCC main-thread handlers that cannot be `async`
(Maya, Houdini, …). When the Rust transport supports elicitation this
blocks the calling thread; without it, `fallback_values` (if provided)
is returned with `accepted=True, message="fallback_values_used"`.

## Current status — stub + graceful fallback

The Rust-level MCP HTTP layer support for forwarding
`notifications/elicitation/request` and awaiting
`notifications/elicitation/response` is planned in issue
[#407](https://github.com/loonghao/dcc-mcp-core/issues/407). Until that
lands, all three helpers:

- log a warning (`"MCP Elicitation is not yet wired to the HTTP transport"`),
- return `ElicitationResponse(accepted=False, message="elicitation_not_supported")`.

Skill handlers written against this API **today** will automatically gain
real elicitation behaviour once the Rust layer is released — no code
changes required. Design your destructive tools now to call `elicit_form`
and rely on the `accepted=False` fallback path meanwhile.

## See also

- [Remote Server guide](../guide/remote-server.md)
- [`ToolAnnotations.destructive_hint`](./actions.md) — pair every `destructive_hint=True` tool with an elicitation gate
- [MCP spec 2025-11-25 § Elicitation](https://modelcontextprotocol.io/specification/2025-11-25/client/elicitation)
