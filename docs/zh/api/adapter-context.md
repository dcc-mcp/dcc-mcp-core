# 适配器上下文和策略辅助工具

此 API 为 DCC 适配器提供共享契约，用于简洁指令、工具后快照、可视反馈、响应整形、工具集配置文件和可搜索的 API 文档。

## 指令资源

```python
from dcc_mcp_core import AdapterInstructionSet, register_adapter_instruction_resources

register_adapter_instruction_resources(
    server,
    AdapterInstructionSet(
        dcc="maya",
        instructions="Use screenshots after visual scene changes.",
        capabilities={"screenshots": True, "in_process_execution": True},
        troubleshooting="If execution stalls, check for modal dialogs.",
        adapter_version="0.3.0",
    ),
)
```

这将注册：

- `docs://adapter/<dcc>/instructions`
- `docs://adapter/<dcc>/capabilities`
- `docs://adapter/<dcc>/troubleshooting`（提供时）

`DccServerBase.register_adapter_instructions(...)` 是嵌入式适配器的薄包装器。

## 上下文快照

适配器可以在变异工具后附加一个小的、有界的场景/文档快照：

```python
from dcc_mcp_core import DccContextSnapshot, append_context_snapshot

result = append_context_snapshot(
    {"success": True, "message": "Layer created"},
    DccContextSnapshot(
        dcc="photoshop",
        document={"name": "hero.psd"},
        active_layer={"name": "Glow"},
        counts={"layers": 12},
    ),
)
```

`DccServerBase.set_context_snapshot_provider(callable)` 存储提供者，`DccServerBase.append_context_snapshot(result)` 应用它。

## 响应整形

`ResponseShapePolicy` 截断大型列表、字典和字符串，并添加带有省略计数和 `next_query` 提示的 `_meta["dcc.response_shape"]`。

```python
from dcc_mcp_core import ResponseShapePolicy, shape_response

payload = shape_response(scene_graph, ResponseShapePolicy(max_items=200, max_bytes=256_000))
```

## 可视反馈

`VisualFeedbackPolicy` 标准化资源支持的预览：

```python
from dcc_mcp_core import VisualFeedbackPolicy, build_visual_feedback_context

context = build_visual_feedback_context(
    resource="output://viewport.png",
    width=1280,
    height=720,
    policy=VisualFeedbackPolicy(mode="after_mutation", max_size=800),
)
```

## 工具集配置文件

`DccToolsetProfile` 和 `ToolsetProfileRegistry` 在技能组之上提供高级层，用于适配器模式，如 `modeling-basic`、`rendering` 或 `photoshop-layer-editing`。
