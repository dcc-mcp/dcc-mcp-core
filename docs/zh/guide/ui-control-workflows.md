# UI Control 工作流

UI Control 只用于类型化 DCC 工具无法完成的应用界面操作，不替代 adapter API。

## 路由顺序

1. 优先调用类型化 DCC-MCP 工具。
2. 浏览器或 webview 内容优先使用 `chrome`/`edge` CDP 后端。
3. 原生应用界面使用独立的 `dcc-mcp-cua` 后端。

CDP 的 DOM 语义和 selector 更稳定，也能在无需前台可见时工作。CUA 用于浏览器
外框、原生对话框、纯 Canvas 内容和非浏览器软件。

## 独立 CUA 配置

单独安装 `dcc-mcp-cua` 并加入 `PATH`，或把 `DCC_MCP_CUA_BINARY` 设置为绝对
可执行文件路径，然后配置：

```text
DCC_MCP_UI_CONTROL_BACKEND=cua
DCC_MCP_UI_CONTROL_PROCESS_ID=<pid>
DCC_MCP_UI_CONTROL_WINDOW_HANDLE=<native-handle>
```

原生鼠标键盘还必须显式设置：

```text
DCC_MCP_CUA_ALLOW_RAW_INPUT=true
```

Core 会校验 `dcc-mcp-cua manifest`、确保共享 Host，并保持持久 JSONL bridge。
带原生扩展的 Core 优先共享内存截图；Python 3.7 pure wheel 使用有界二进制附件。
目标边框/banner/agent 鼠标、平台无障碍、输入队列和 Escape 广播都由 CUA Host
负责。

## 观察与操作

1. 调用 `ui_control__snapshot`。
2. 有语义控件时调用 `ui_control__find`。
3. 使用最新 `snapshot_id` 调用一次 `ui_control__act`。
4. 调用 `ui_control__wait_for` 或重新截图。
5. 完成后调用 `ui_control__stop_computer_use`。

每次动作都由 CUA observation 与 accessibility state 双重 fence。任何修改后都要
重新截图。优先使用语义 element token；坐标输入只作为自绘界面的受控回退。

多个 agent 可以并行控制不同应用。session grant、window capability、observation、
录制状态和清理互相隔离；共享 Host 串行化原生输入，Escape 对所有活动 session
广播中断。

## 录制

录制应包围真实动作，而不是同步等待固定时长：

```text
ui_control__recording_start(output_dir=<绝对路径>, record_video=true)
ui_control__act(...)
ui_control__recording_state()
ui_control__recording_stop()
dcc-mcp-cua recording render <input-dir> <output.mp4>
```

录制格式和渲染器由独立 CUA runtime 负责，Core 不重复实现。

## 安全与证据

- 每个 session 必须绑定准确进程/窗口。
- 不自动处理凭据、认证提示或 secure desktop。
- `user_interrupted`、`permission_denied`、`policy_disabled` 都是硬停止。
- 截图作为证据时必须保留 `capture_provenance`。
- 审计日志会脱敏输入文本和敏感动作参数。

所有权边界见 [ADR-020](../../adr/020-external-cua-runtime.md)。
