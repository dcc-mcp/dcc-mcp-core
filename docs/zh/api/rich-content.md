# Rich Content — MCP Apps 内联 UI

> 源码：[`python/dcc_mcp_core/rich_content.py`](https://github.com/loonghao/dcc-mcp-core/blob/main/python/dcc_mcp_core/rich_content.py) · Issue [#409](https://github.com/loonghao/dcc-mcp-core/issues/409) · [MCP Apps 总览](https://modelcontextprotocol.io/extensions/apps/overview)
>
> **[English](../../api/rich-content.md)**

MCP Apps 是 MCP 协议首个官方扩展。工具可以返回交互式界面——图表、表单、仪表盘、图像、表格——直接在对话窗口内联渲染，**完全不占用模型上下文**。返回富内容的服务器比纯文本服务器有明显更高的接受度。

**DCC 工具中富内容的典型收益**

| 工具 | 富内容 | 价值 |
|------|--------|------|
| `render_frames` | 缩略图画廊 + 统计表 | 不离开对话即可可视化验证 |
| `get_scene_hierarchy` | 交互式树视图 | 浏览 1 万节点的场景 |
| `diagnostics__screenshot` | 内联截图 | 比文件路径好用得多 |
| `analyze_keyframes` | 动画曲线图 | 可视化时序调试 |
| `get_render_stats` | 各层柱状图 | 比原始 JSON 数组更快 |
| `list_materials` | 材质色板网格 | 可视化选择 |

## 导入

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

## `RichContentKind`（枚举）

| 值 | 渲染方式 |
|----|----------|
| `"chart"` | Vega-Lite / Chart.js 规范 |
| `"form"` | 交互式 JSON-Schema 表单 |
| `"dashboard"` | 多个组件的组合布局 |
| `"image"` | 内联 PNG / JPEG / WebP（base64） |
| `"table"` | 表头 + 行的网格 |

## `RichContent`（dataclass）

推荐使用类方法构造器而非原始构造函数。

### `RichContent.chart(spec) -> RichContent`

Vega-Lite v5 或 Chart.js 规范字典。

```python
RichContent.chart({
    "$schema": "https://vega.github.io/schema/vega-lite/v5.json",
    "data": {"values": [{"x": 1, "y": 2}]},
    "mark": "line",
    "encoding": {"x": {"field": "x"}, "y": {"field": "y"}},
})
```

### `RichContent.form(schema, *, title=None) -> RichContent`

从 JSON Schema 渲染的交互表单。注意和 [Elicitation](./elicitation.md) 的区别：这里的 `form` 是**工具结果的一部分**（一次性展示），而 elicitation 会**暂停**工具调用等待用户输入。

### `RichContent.image(data, mime="image/png", *, alt=None) -> RichContent`

原始图像字节（内部 base64 编码）。

### `RichContent.image_from_file(path, mime=None, *, alt=None) -> RichContent`

便捷读取器。根据扩展名自动推断 MIME（`.png`、`.jpg/.jpeg`、`.webp`、`.gif`）。

### `RichContent.table(headers, rows, *, title=None) -> RichContent`

`headers: list[str]` 与 `rows: list[list]`。每行长度必须与 `headers` 一致。

### `RichContent.dashboard(components) -> RichContent`

有序的 `RichContent` 组合布局。

### `.to_dict() -> dict`

扁平化为 `{"kind": <value>, **payload}`，可 JSON 序列化。

## `attach_rich_content(result, content) -> dict`

将 `RichContent` 附到已存在的技能结果字典上。存储于 `result["context"]["__rich__"]`——支持 MCP Apps 的客户端会渲染；纯客户端优雅忽略。

## 技能脚本辅助函数

返回可直接使用的技能结果字典。附加关键字参数进入 `context` 字典。

### `skill_success_with_chart(message, chart_spec, **context) -> dict`

### `skill_success_with_table(message, headers, rows, *, title=None, **context) -> dict`

### `skill_success_with_image(message, image_data=None, image_path=None, mime="image/png", *, alt=None, **context) -> dict`

必须提供 `image_data` 或 `image_path` 之一，否则抛 `ValueError`。

```python
return skill_success_with_image(
    "视口已捕获",
    image_data=capture_viewport(),
    alt="Maya viewport",
)
```

## 当前状态——上下文存储就绪，Rust 层对接中

目前富内容以 JSON 可序列化字典形式存储在 `result.context["__rich__"]`。完整对接到 `tools/call` 响应的 MCP Apps 标准信封跟踪于 issue [#409](https://github.com/loonghao/dcc-mcp-core/issues/409)。

**今天**基于这些 helper 编写的技能，在 Rust 层落地后会自动向支持 MCP Apps 的客户端暴露富内容。

## 参见

- [远程服务器指南](../guide/remote-server.md)
- [Elicitation](./elicitation.md) — *暂停*工具等待输入；本文档讲的是*一次性*展示
- [Vega-Lite v5 文档](https://vega.github.io/vega-lite/)
- [MCP Apps 扩展](https://modelcontextprotocol.io/extensions/apps/overview)
