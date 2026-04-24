# Elicitation — 工具执行期间向用户请求输入

> 源码：[`python/dcc_mcp_core/elicitation.py`](https://github.com/loonghao/dcc-mcp-core/blob/main/python/dcc_mcp_core/elicitation.py) · Issue [#407](https://github.com/loonghao/dcc-mcp-core/issues/407) · [MCP 2025-11-25 Elicitation 规范](https://modelcontextprotocol.io/specification/2025-11-25/client/elicitation)
>
> **[English](../../api/elicitation.md)**

Elicitation 允许工具处理器在执行过程中**暂停**，向终端用户索取输入——可以是基于 JSON Schema 渲染的表单，也可以是浏览器 URL 流（OAuth、支付、凭据采集）。

**典型场景**

- **破坏性操作确认** — "删除 127 个镜头相机？此操作不可撤销。"
- **缺失必填参数** — Agent 调用时遗漏 `render_layer`，弹出下拉选择。
- **认证流程** — 把用户引导到 `/oauth/authorize` 并等待回调。

没有 elicitation 就需要把场景弹回 Agent——浪费 Token 且常常打断交互流。

## 导入

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

## 类型

### `ElicitationMode`（枚举）

| 值 | 含义 |
|----|------|
| `ElicitationMode.FORM` | 客户端渲染 JSON-Schema 表单 |
| `ElicitationMode.URL` | 客户端打开浏览器 URL 并等待完成 |

### `FormElicitation`

| 字段 | 类型 | 说明 |
|------|------|------|
| `message` | `str` | 表单上方的提示词 |
| `schema` | `dict` | JSON Schema（`type: object`、`properties`、`required`） |
| `title` | `str \| None` | 可选对话框标题 |

### `UrlElicitation`

| 字段 | 类型 | 说明 |
|------|------|------|
| `message` | `str` | 简短描述 |
| `url` | `str` | 浏览器 URL |
| `description` | `str \| None` | 长描述 |

### `ElicitationRequest`

组合 `mode` 与 `FormElicitation` 或 `UrlElicitation`。

### `ElicitationResponse`

| 字段 | 类型 | 说明 |
|------|------|------|
| `accepted` | `bool` | 提交为 `True`，取消/不支持为 `False` |
| `data` | `dict \| None` | 用户填写的值（仅 form 模式） |
| `message` | `str \| None` | 状态或错误信息 |

## 辅助函数

### `await elicit_form(message, schema, *, title=None) -> ElicitationResponse`

`async def` 技能处理器使用的异步表单请求。

```python
async def delete_objects(objects: list[str], **kwargs):
    resp = await elicit_form(
        message=f"删除 {len(objects)} 个对象？此操作不可撤销。",
        schema={
            "type": "object",
            "properties": {"confirm": {"type": "boolean", "title": "确认删除"}},
            "required": ["confirm"],
        },
    )
    if not resp.accepted or not resp.data.get("confirm"):
        return {"success": False, "message": "用户取消"}
    # ... 继续删除 ...
```

### `await elicit_url(message, url, *, description=None) -> ElicitationResponse`

异步 URL 请求（OAuth、支付、凭据流）。

### `elicit_form_sync(message, schema, *, title=None, fallback_values=None) -> ElicitationResponse`

DCC 主线程处理器（Maya、Houdini）中无法使用 `async` 的阻塞变体。Rust 传输支持 elicitation 后会阻塞直到用户响应；在此之前若提供 `fallback_values`，则返回 `accepted=True, message="fallback_values_used"`。

## 当前状态——桩实现 + 优雅降级

Rust 级 MCP HTTP 层对 `notifications/elicitation/request` 转发与 `notifications/elicitation/response` 回调的支持跟踪于 issue [#407](https://github.com/loonghao/dcc-mcp-core/issues/407)。在此之前，三个 helper 均会：

- 记录警告日志（`"MCP Elicitation is not yet wired to the HTTP transport"`）；
- 返回 `ElicitationResponse(accepted=False, message="elicitation_not_supported")`。

**今天**按此 API 编写的处理器，在 Rust 层落地后会自动获得真实的 elicitation 行为——无需改代码。请立刻为所有破坏性工具接入 `elicit_form`，并在 `accepted=False` 分支做降级处理。

## 参见

- [远程服务器指南](../guide/remote-server.md)
- [`ToolAnnotations.destructive_hint`](./actions.md) — 每个 `destructive_hint=True` 的工具都应该搭配 elicitation 确认
- [MCP 规范 2025-11-25 § Elicitation](https://modelcontextprotocol.io/specification/2025-11-25/client/elicitation)
