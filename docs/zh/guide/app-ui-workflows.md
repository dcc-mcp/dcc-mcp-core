# 应用 UI 工作流

本文档概述 dcc-mcp-core 中的应用层 UI 界面：内置管理控制面板（Admin
Dashboard）、`ui_control` 自动化工具套件以及与 UI 功能交互的 CLI 子命令。
每个部分都链接到详细参考文档以便深入阅读。

## 管理控制面板

网关（gateway）内置了一个 `/admin` Web 管理面板（React/Vite 源码位于
`admin-ui/`）。运行时它以单一 HTML 负载形式从二进制文件中提供；贡献者编辑
`admin-ui/` 下的源码，构建系统会在 Cargo 编译过程中将构建产物嵌入。

该面板在当选网关上默认启用，提供以下功能：

- **实例（Instances）** — 已连接 DCC 适配器概览及版本诊断
- **工具（Tools）** — 已注册工具/动作目录，支持搜索和描述
- **调用（Calls）** — 实时和历史工具调用日志
- **追踪（Traces）** — 分布式追踪查看器
- **统计（Stats）** — 网关级指标和吞吐量
- **工作者（Workers）** — 后台工作者池状态
- **合并日志（Merged Logs）** — 跨所有网关日志文件的聚合日志查看器
- **健康检查（Health）** — 就绪与存活探针

详见 [admin-ui.md](../../guide/admin-ui.md) 了解激活标志、环境变量、
Python/Rust API 配置及完整功能说明。

### 启用与禁用

```bash
# 默认：在 :9765 加入网关选举；当选进程提供 /admin
dcc-mcp-server --app maya

# 完全禁用网关（同时禁用管理面板）
dcc-mcp-server --gateway-port 0

# 保留网关但禁用管理面板
dcc-mcp-server --no-admin

# 将管理面板挂载到其他路径前缀下
dcc-mcp-server --admin-path /dcc-admin
```

等效环境变量：

| 环境变量 | 默认值 | 说明 |
|---------|--------|------|
| `DCC_MCP_GATEWAY_PORT` | `9765` | 网关选举端口。`0` 禁用网关/管理面板。 |
| `DCC_MCP_NO_ADMIN` | `false` | 在当选网关上禁用管理面板。 |
| `DCC_MCP_ADMIN_PATH` | `/admin` | 管理面板 URL 前缀。 |
| `DCC_MCP_GATEWAY_AUDIT_DIR` | 未设置 | 可选的 JSONL 目录，用于审计和追踪持久化。 |
| `DCC_MCP_GATEWAY_AUDIT_MAX_ROWS` | `5000` | 每个持久化文件保留的最大 JSONL 行数。 |
| `DCC_MCP_GATEWAY_AUDIT_MAX_BYTES` | `52428800` | 每个持久化 JSONL 文件约 50 MiB 的字节上限。 |
| `DCC_MCP_LOG_DIR` | 平台日志目录 | `/admin/api/logs` 扫描 `*.log` 文件的目录。 |

### 分析仪表盘

独立分析仪表盘提供 KPI 时序、热力图、工具排名及 CSV/JSONL 导出。
详见 [analytics-dashboard.md](../../guide/analytics-dashboard.md)。

## UI 控制自动化

当 DCC 仅在窗口、模态对话框、webview、启动器或设置面板中暴露状态（而非通
过类型化 API）时，`ui_control` 工具套件提供了用于界面自动化的有界回退方案。

标准工作流遵循以下循环：

1. **`ui_control__snapshot`** — 捕获有界 DCC 窗口，返回 `snapshot_id`
2. **`ui_control__find`** — 通过标签、文本、角色或名称解析控件
3. **`ui_control__act`** — 执行一个操作（点击、设置文本、拖拽等）
4. **`ui_control__wait_for`** — 在一次工具调用内轮询直到 UI 达到预期状态
5. **`ui_control__snapshot`** — 验证最终状态
6. **`ui_control__stop_computer_use`** — 释放原生输入，移除视觉效果

始终将第 6 步视为 `finally` 代码块——在成功、失败、取消或放弃时都需调用。

详见 [ui-control-workflows.md](../../guide/ui-control-workflows.md) 查阅完整参考：
决策规则、证据溯源、系统配置操作、恢复模式及验证要求。

### CLI 接口

`dcc-mcp-cli ui-control` 子命令通过 Shell 暴露相同的能力：

```bash
dcc-mcp-cli ui-control snapshot --instance-id <id> --json '{"session_id":"ui","process_id":1234}'
dcc-mcp-cli ui-control act --instance-id <id> --json '{"session_id":"ui","control_id":"ok","action":"click","snapshot_id":"<snapshot_id>"}'
dcc-mcp-cli ui-control record-clip --instance-id <id> --json '{"session_id":"pv","process_id":1234,"duration_ms":5000}'
dcc-mcp-cli ui-control stop --instance-id <id> --json '{"session_id":"ui"}'
dcc-mcp-cli ui-control system-operation --instance-id <id> --json '{"operation_id":"enable-remote-control"}'
```

## 相关文档

| 文档 | 用途 |
|------|------|
| [admin-ui.md](../../guide/admin-ui.md) | 管理面板完整参考：激活、面板、审计、健康检查 |
| [analytics-dashboard.md](../../guide/analytics-dashboard.md) | KPI 仪表盘、时序、热力图、CSV/JSONL 导出 |
| [ui-control-workflows.md](../../guide/ui-control-workflows.md) | UI 控制自动化：完整工作流参考、恢复、验证 |
| [gateway.md](../../guide/gateway.md) | 多 DCC 网关：聚合、工具路由、选举 |
| [gateway-diagnostics.md](../../guide/gateway-diagnostics.md) | 网关健康、就绪、争用、故障诊断 |
| [cli-reference.md](../../guide/cli-reference.md) | 规范 CLI 命令、标志、配置 |
| [observability-usage.md](../../guide/observability-usage.md) | Agent、CLI 和管理面板的可观测性用法 |
