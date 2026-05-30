# App UI 代理工作流

`app_ui` 是用于仅界面工作的有界回退。首先优先使用原生 DCC 技能：它们通常携带更强的架构、更好的撤销语义和宿主感知的分发。当您需要的状态仅存在于窗口、模态对话框、webview、启动器、许可证工具或设置面板中时，使用 `app_ui__*`。

## 决策规则

在以下情况下使用原生 DCC 工具：

- 宿主 API 直接暴露状态或操作。
- 操作会更改场景数据、文件、包、渲染或项目状态。
- 您需要可靠的批处理执行、撤销集成或主线程宿主语义。

在以下情况下使用 `app_ui`：

- 唯一可用的控制路径是可见的 UI 表面。
- 您需要解除模态对话框、向导、浏览器视图、安装程序提示或适配器拥有的伴生进程应用的阻止。
- 原生工具已报告需要手动 UI 确认。

不要将 `app_ui` 用作缺少类型化工具的快捷方式。如果工作流常见且稳定，请首先添加原生技能/API，并将 `app_ui` 保留为诊断或紧急路径。

## 标准循环

每个工作流应保持相同的形状：

1. `app_ui__snapshot` 观察有界窗口并返回 `snapshot_id`。
2. `app_ui__find` 通过标签、文本、角色或对象名称解析控件 ID。
3. `app_ui__act` 执行一个操作。传递 `snapshot_id` 以便在操作前检测过时的控件。
4. `app_ui__wait_for` 在一次工具调用内轮询，直到 UI 达到预期状态或返回结构化的 `timeout`。
5. `app_ui__snapshot` 验证最终状态。

对于网关客户端，在调用前发现和检查工具：

```json
{"name": "search_tools", "arguments": {"query": "app_ui snapshot", "dcc_type": "maya"}}
{"name": "describe_tool", "arguments": {"tool_slug": "<slug from search>"}}
```

REST 客户端通过 `/v1/search`、`/v1/describe` 和 `/v1/call` 使用相同的序列。

## 示例：模态对话框

当 DCC 原生操作打开了没有宿主 API 等效项的确认对话框时，使用此方法。

调用 `app_ui__snapshot` 并验证根窗口是预期的对话框。然后找到确认按钮：

```json
{"session_id": "maya-confirm-export", "label": "Overwrite", "role": "button"}
```

仅对解析的控件 ID 和当前快照执行操作：

```json
{
  "session_id": "maya-confirm-export",
  "control_id": "overwrite",
  "action": "click",
  "snapshot_id": "<snapshot_id>"
}
```

等待模态框消失或状态文本更改：

```json
{
  "session_id": "maya-confirm-export",
  "condition": {
    "kind": "control_missing",
    "control_id": "overwrite",
    "timeout_ms": 5000,
    "interval_ms": 100
  }
}
```

最后在存在时使用原生 DCC 验证工具。例如，通过类型化技能验证导出的文件或场景状态已更改，而不仅仅是通过 UI。

## 示例：设置面板

当设置仅存在于首选项面板或 webview 中时，使用此方法。

1. 快照有界的应用程序窗口。
2. 通过可见标签而不是索引查找设置。
3. 设置文本、复选框或选择。
4. 单击面板的 apply/save 控件。
5. 等待稳定的状态消息。
6. 再次快照并验证设置值。

模拟后端载荷镜像预期的真实工作流：

```json
{"session_id": "settings-demo", "label": "Project name"}
```

```json
{
  "session_id": "settings-demo",
  "control_id": "project-name",
  "action": "set_text",
  "text": "Hero",
  "snapshot_id": "<snapshot_id>"
}
```

```json
{
  "session_id": "settings-demo",
  "condition": {
    "kind": "value_equals",
    "control_id": "project-name",
    "value": "Hero",
    "timeout_ms": 1000,
    "interval_ms": 50
  }
}
```

键入的文本应在审计记录中被编辑，除非适配器策略明确允许敏感值。

## 示例：等待 UI 状态

优先使用 `app_ui__wait_for` 而不是代理端轮询循环。它将重试保持在后端附近，避免重复的 MCP 往返，并在状态永不出现时返回一个结构化的超时信封。

良好的等待条件是稳定的和语义化的：

- 在状态标签（如 `Applied` 或 `Complete`）上使用 `text_equals`。
- 在编辑后在文本字段上使用 `value_equals`。
- 在复选框上使用 `checked_equals`。
- 对于模态框生命周期使用 `control_exists` 或 `control_missing`。
- 对于工作后变得可操作的控件使用 `enabled` 或 `disabled`。

避免等待屏幕坐标、像素颜色或视觉顺序，除非后端没有可访问性树且适配器明确记录了该回退。

## 恢复示例

`stale_control`：从 `app_ui__snapshot` 重新开始，然后使用新的 `snapshot_id` 重复 `find` 和 `act`。永远不要盲目重试相同的过时控件 ID。

`missing_window`：验证预期的 DCC/应用程序进程是否仍在运行，以及后端是否限定到正确的窗口标题或进程 ID。如果窗口因工作流完成而消失，请切换到原生验证工具。

`policy_disabled`：停止 UI 操作。优先使用原生技能，或要求用户进行更窄的策略更改，如允许一个窗口的文本输入。不要静默扩展到全桌面访问。

`timeout`：获取新的快照并检查最后观察到的 UI 状态。如果状态仍在进行中，请使用合理的超时再调用 `wait_for` 一次。如果状态被阻止，请向用户显示当前控件/状态文本或切换到宿主诊断技能。

## 验证

对于涉及 `app_ui` 的代码更改，请包含至少一个可执行路径：

- 用于契约映射和结构化错误的单元测试。
- 用于 snapshot -> find -> act -> wait -> verify 的模拟后端工作流测试。
- 当涉及网关 `/v1/*` 路由或 REST 信封时的 VRS 跟踪。

VRS 跟踪 `tests/vrs/traces/core-1134-app-ui-mock-workflow.jsonl` 固定了模拟后端工作流和恢复信封，用于实时网关运行。当没有注册 `app_ui__snapshot` 能力时，它会干净地跳过。
