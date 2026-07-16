# App UI 代理工作流

`app_ui` 是用于仅界面工作的有界回退。首先优先使用原生 DCC 技能：它们通常携带更强的架构、更好的撤销语义和宿主感知的分发。当您需要的状态仅存在于窗口、模态对话框、webview、启动器、许可证工具或设置面板中时，使用 `app_ui__*`。

## 决策规则

在以下情况下使用原生 DCC 工具：

- 宿主 API 直接暴露状态或操作。
- 操作会更改场景数据、文件、包、渲染或项目状态。
- 您需要可靠的批处理执行、撤销集成或主线程宿主语义。

在以下情况下使用 `app_ui`：

- 类型化 DCC 工具返回 `unsupported` 或 `capability_missing`，且唯一剩余控制路径是可见 UI。
- 需要处理没有类型化宿主能力的 DCC 自有模态框、向导、webview 或伴生进程控件。

不要将 `app_ui` 用作缺少类型化工具的快捷方式。如果工作流常见且稳定，请首先添加原生技能/API，并将 `app_ui` 保留为诊断或紧急路径。

策略拒绝、用户中断、锁屏/断连、身份验证和安全边界都是停止条件，不能作为尝试另一条 UI 路径的理由。

## 标准循环

每个工作流应保持相同的形状：

1. `app_ui__snapshot` 观察有界窗口并返回 `snapshot_id`。
2. `app_ui__find` 通过标签、文本、角色或对象名称解析控件 ID。
3. `app_ui__act` 执行一个操作。传递 `snapshot_id` 以便在操作前检测过时的控件。
4. `app_ui__wait_for` 在一次工具调用内轮询，直到 UI 达到预期状态或返回结构化的 `timeout`。
5. `app_ui__snapshot` 验证最终状态。
6. `app_ui__stop_computer_use` 释放原生输入所有权、移除可见边框和横幅，并使最终 observation 失效。

将第 6 步视为 `finally`。工作流成功、任一工具失败、代理或用户放弃工作流时都必须调用它。停止工具是幂等的，并且在桌面不可用时仍可安全调用。若返回 `cleanup_pending=true`，表示 Windows 尚未确认所有待释放按键或鼠标按钮；应重试清理且不得启动新会话。全局输入所有权会跨适配器进程保持占用，直到重连后释放事件全部排空。

对于网关客户端，在调用前发现和检查工具：

```json
{"name": "search_tools", "arguments": {"query": "app_ui snapshot", "dcc_type": "maya"}}
{"name": "describe_tool", "arguments": {"tool_slug": "<slug from search>"}}
```

REST 客户端通过 `/v1/search`、`/v1/describe` 和 `/v1/call` 使用相同的序列。

## 原生 Computer Use 回退

仅当类型化 DCC 工具返回 `unsupported` 或 `capability_missing` 时才进入 `app_ui`。始终将操作限定到准确的 DCC 窗口。执行 Windows UIA 写操作前，适配器或运维人员必须通过 `DCC_MCP_APP_UI_UIA_PROCESS_ID` 或 `DCC_MCP_APP_UI_UIA_WINDOW_HANDLE` 绑定目标；请求参数只能缩小该范围，不能扩大范围。即使执行语义 UIA，该绑定会话也会提供可见横幅、截图和用户中断监控。原生鼠标和键盘输入还有第二道门槛：运维人员必须同时设置 `DCC_MCP_COMPUTER_USE_ALLOW_RAW_INPUT=true`。

原生会话会把绑定的 PID/HWND 作为独立授权范围：没有绑定时拒绝创建，并在显示横幅、每次截图及每次动作前重新校验真实进程与窗口。仅标题或进程名的范围不能授权原生输入。

仅当语义 UIA 返回 `unsupported_action` 或 UIA 后端不可用时，才能改用原生坐标输入。遇到 `policy_disabled`、`permission_denied`、`invalid_target`、`missing_window`、`user_interrupted` 或 `desktop_unavailable` 时不得回退。

使用以下循环：

1. 对准确的 PID 或 HWND 调用 `app_ui__snapshot`，检查返回的截图和 `snapshot_id`。
2. 优先使用 `app_ui__find` 和语义控件操作。只有不存在稳定语义控件时才使用截图坐标。
3. 使用最新的 `snapshot_id` 执行且只执行一次 `app_ui__act`。
4. 操作后立即重新调用 `app_ui__snapshot`。绝不复用旧截图中的坐标、控件 ID 或 observation ID。
5. 每次只执行一个操作并重复观察；无论成功、失败、取消还是放弃，都必须调用 `app_ui__stop_computer_use`。

可见边框、控制横幅和鼠标效果属于适配器宿主的交互式 Windows 登录会话。如果用户按下 `Ctrl+Alt+Esc` 并收到 `user_interrupted`，请立即停止。普通 `Esc` 仍会发送给目标 DCC，用于取消工具或关闭对话框。不要重试、改变 `session_id` 或启动新会话。只有用户明确要求恢复后，才能设置 `resume_computer_use=true`。

### 锁屏、RDP 与显示变化

- 将 `desktop_unavailable` 视为暂停。此时 Windows 已锁定、远程会话已断开或正在显示安全桌面，因此不会运行 UIA 或原生输入。停止发送 UI 调用，请用户解锁或重新连接，且不得自动轮询。保留逻辑 `session_id`；仅在宿主仍就绪时继续使用非 UI 工具。
- 解锁或重新连接 RDP 后，丢弃此前所有 snapshot、observation、控件 ID 和坐标。操作前先对准确目标获取新快照；成功快照会恢复可见的 Computer Use 效果。
- Computer Use 在拥有 DCC 的交互式 Windows 登录会话和适配器宿主上运行。网关只负责路由调用。绝不复用在网关、其他宿主或其他 Windows 会话中捕获的坐标。
- 截图只覆盖有界目标窗口，绝不覆盖整个桌面。该窗口可以跨越具有负数虚拟桌面原点或不同 DPI 的显示器。显示器拓扑、分辨率、DPI/缩放、窗口位置或窗口大小的任何变化都会使 observation 失效，此时必须获取新快照。坐标相对于返回的 PNG，而不是全局桌面坐标。

绝不能将 LockApp、Windows 安全中心、凭据/身份验证/密码管理器窗口、Windows“运行”对话框、终端、PowerShell 或 `cmd` 作为目标。这些是后端强制执行的边界，不能改用其他 UI 自动化方式绕过。绑定 DCC 进程内部的脚本编辑器不属于终端目标。

`app_ui__act` 会向调用宿主声明 destructive 注解，由宿主执行自己的确认策略。不得新增或信任模型传入的 `confirmed=true` 参数，也不得把环境变量当成单次操作批准。若宿主策略要求确认但当前无法取得确认，应停止操作，不能切换到另一种自动化方式。未来可以加入由可信宿主签发的一次性批准能力，而无需削弱当前的目标范围和原生输入门槛。

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

键入的文本应在审计记录中脱敏，除非适配器策略明确允许记录敏感值。

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
