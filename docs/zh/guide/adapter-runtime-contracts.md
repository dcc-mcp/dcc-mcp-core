# 适配器运行时契约

核心为代理在工具和作业运行期间需要的运行时材料暴露了小型、与 DCC 无关的契约。适配器保留宿主特定的收集和安全策略；核心标准化形状和资源移交路径。

## 会话事件

使用 `SessionEventBuffer` 处理有界的 stdout/stderr/log/progress/checkpoint 事件。将其注册为 MCP 资源：

```python
from dcc_mcp_core import SessionEventBuffer

events = SessionEventBuffer("maya-001", maxlen=1000, max_message_bytes=4096)
server.resources().register_session_event_buffer(events)
events.append("python", "stdout", "Created rig control", tool_call_id="req-1")
```

客户端读取 `events://session/maya-001?cursor=N&limit=100`。响应包含 `next_cursor`，因此客户端无需实时订阅即可避免重复事件。`drain=true` 可用于需要读取时消耗行为的客户端。

## 工件引用

对于大型或二进制输出，使用现有的 `FileRef` / `ArtefactStore` 路径：

- `artefact://sha256/<hex>` 永远不会暴露适配器文件系统路径。
- 伴生进程携带 MIME、大小、摘要、显示名称、会话/工具/作业/关联字段、过期时间和适配器元数据。
- 有界存储可以强制执行最大载荷字节数、最大保留条目数、最大总字节数和默认 TTL。

工具结果应在上下文中返回小型 `FileRef` 对象，并让客户端通过 `resources/read` 获取字节。

## 调试描述符

使用 `DebugSessionDescriptor` 发布可选的附加元数据，而无需向核心添加硬调试器依赖。描述符支持 `unavailable`、`available`、`listening`、`client_connected` 和 `error` 状态，以及主机/端口、运行时/进程标识、路径映射、日志 URI、设置说明和适配器元数据。

Python 适配器可以使用：

```python
from dcc_mcp_core import DebugSessionDescriptor

descriptor = DebugSessionDescriptor.listening("debugpy", "127.0.0.1", 5678)
```

通过文档/自定义资源或适配器拥有的可选工具发布结果 `descriptor.to_dict()`。

## App UI 自动化契约

`app_ui` 契约是架构和工作流，不是通用点击机器人。适配器可以使用 Qt、原生可访问性 API、webview 或 DCC 特定的 UI API 实现它。公共工具名称使用 `app_ui__*`，因为该能力有意比 DCC 专用的 UI 命名空间更广泛：相同的契约可以描述 DCC 偏好设置对话框、外部启动器、许可证实用程序或其他适配器拥有的应用程序窗口。

Rust 架构位于 `dcc-mcp-app-ui` crate 中，因此 UI 自动化契约可以独立于 HTTP 服务器层演进。Python 适配器继续从 `dcc_mcp_core.adapter_contracts` 导入匹配的数据类。

核心形状包括：

- `UiControlNode` 和 `UiSnapshot` 用于有界的 UI 树。
- `UiFindRequest` 用于通过查询、角色、标签或对象名称定位控件。
- `UiActionRequest` 用于一个有界操作，如单击、设置文本、切换、设置选中、选择选项或聚焦。
- `UiWaitCondition` 和 `UiWaitResult` 用于工具内轮询，如"等待状态文本等于 Applied"或"等待模态框消失"。
- `UiActionResult` 包含结构化错误，如 `stale_control`、`denied`、`unsupported_action` 和可选的截图/工件引用。
- `AppUiPolicy` 和 `AppUiAuditRecord` 用于作用域操作控制和隐私保护的审计输出。

当控件过时时，适配器必须返回结构化错误而不是挂起，适配器端安全策略仍决定允许哪些操作。

首选代理循环：

1. `app_ui__snapshot` 观察一个作用域应用程序窗口并返回 `snapshot_id`。
2. `app_ui__find` 通过查询、角色、标签或对象名称解析稳定的控件 ID。
3. `app_ui__act` 对该控件 ID 执行一个操作。在可用时传递 `snapshot_id`，以便过时的控件以 `stale_control` 失败，而不是对错误目标执行操作。
4. `app_ui__wait_for` 在一次调用内轮询，直到预期的 UI 状态为真，或返回带有结构化详细信息的 `timeout`。
5. `app_ui__snapshot` 验证最终状态。

首先使用原生 DCC 技能或 API。仅当行为在应用程序 UI 中可见但未通过可靠的宿主 API 暴露时，才使用 `app_ui__*`。工作流示例和恢复模式位于 [app-ui-workflows.md](app-ui-workflows.md)。

安全期望：

- 快照/查找工具是只读的，当后端支持时可以在任何线程上运行。
- 变异操作应声明保守的安全注释，宿主要求时的主线程亲和性，以及反映 UI 轮询的超时。
- 在 `tools.yaml` 中声明 MCP `annotations`、`execution`、`affinity` 和 `timeout_hint_secs`。网关 `search_tools` / `/v1/search` 携带紧凑的安全提示，`describe_tool` / `/v1/describe` 暴露完整架构加上 `_meta.dcc` 亲和性、执行、超时和风险提示。
- 网关实例行包括 `diagnostics.app_ui.status`：当 `app_ui__*` 能力被索引时为 `available`，当不存在时为 `unavailable`，或当适配器注册表元数据发布 `app_ui.status=disabled` 时为 `disabled_by_policy`（可选带 `app_ui.reason`）。
- 策略应默认禁用全桌面访问。限定到适配器拥有的进程、窗口或显式允许列表。除非用户明确选择特定后端的全桌面回退，否则保持 `AppUiPolicy.require_scoped_window` 启用。
- 原始坐标单击和键盘快捷键是高风险的。除非适配器明确选择加入并记录回退，否则保持禁用。
- 审计记录应包括操作类型、目标控件 ID/角色/标签（安全时）、前后焦点 ID、成功/失败和结构化错误代码。敏感的键入文本和截图字节应被编辑或仅作为工件/资源引用返回。

捆绑的 `app-ui` 技能默认为测试和适配器创作的确定性模拟后端。设置 `DCC_MCP_APP_UI_BACKEND=chrome` 以使用实验性 CDP 后端，并通过相同的 `app_ui__snapshot`、`app_ui__find`、`app_ui__act` 和 `app_ui__wait_for` 工具驱动浏览器或 webview 搜索。CDP 后端支持预设：`reuse` 首先附加到现有 DevTools 端点以便可以重用当前浏览器令牌，`isolated` 启动临时 Chrome 配置文件，`auroraview` 使用 `DCC_MCP_APP_UI_AURORAVIEW_CDP_PORT`、`AURORAVIEW_CDP_PORT`、`DCC_MCP_APP_UI_CDP_PORT` 或端口 `9222` 附加到 AuroraView 的 CDP 端点。相同的运行时还支持 `edge` 用于 Microsoft Edge CDP 和 `agent-browser` 用于 Vercel 的 `agent-browser` CLI，该 CLI 通过 `agent-browser get cdp-url` 公开其 DevTools URL，并可以在 CI 中通过 `agent-browser install` 配置。

在 Windows 上设置 `DCC_MCP_APP_UI_BACKEND=windows-uia` 以使用参考 Windows UI Automation 后端。它通过 PowerShell 辅助程序使用 OS UIAutomationClient API，并保持在相同的契约之后。后端拒绝无作用域的全桌面访问：通过调用策略或 `DCC_MCP_APP_UI_UIA_*` 环境变量提供允许的窗口标题、进程 ID 或进程名称。它将 UIA 控件类型映射到规范化的 app_ui 角色，并返回结构化的 `missing_window`、`not_found`、`unsupported_action`、`policy_disabled`、`stale_control` 和 `timeout` 错误。
