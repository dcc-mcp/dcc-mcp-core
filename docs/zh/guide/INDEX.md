# 文档指南索引

`docs/guide/` 目录的快速参考索引，帮助你找到所需文档，无需逐一扫描。

## AI Agent 快速路径

**如果你是 AI Agent**，请按以下顺序阅读：

| 优先级 | 文档 | 说明 |
|--------|------|------|
| 1 | [`AGENTS.md`](https://github.com/loonghao/dcc-mcp-core/blob/main/AGENTS.md) | 导航地图、响应规则、PR 规则及链接 |
| 2 | [`AI_AGENT_GUIDE.md`](https://github.com/loonghao/dcc-mcp-core/blob/main/AI_AGENT_GUIDE.md) | AI Agent 的技能优先使用工作流 |
| 3 | [agents-reference.md](agents-reference.md) | **关键** — 陷阱、注意事项、代码风格、常量 |
| 4 | [skills.md](skills.md) | 如何编写和注册技能 |
| 5 | [gateway.md](gateway.md) + [dcc-rest-skill-api.md](dcc-rest-skill-api.md) | 网关动态能力与 REST 工作流 |

## 快速上手

| 文档 | 用途 |
|------|------|
| [getting-started.md](getting-started.md) | 安装、第一个服务器、第一个工具 |
| [what-is-dcc-mcp-core.md](what-is-dcc-mcp-core.md) | 项目高层概述与背景 |
| [architecture.md](architecture.md) | Rust workspace 布局、crate 边界、PyO3 桥接 |

## AI Agent 与技能开发

| 文档 | 用途 |
|------|------|
| [agents-reference.md](agents-reference.md) | **关键** — 陷阱、注意事项、代码风格、完整示例 |
| [skills.md](skills.md) | 技能系统：扫描、加载、生命周期、持久化、语义搜索、Agent 记忆 |
| [thin-harness.md](thin-harness.md) | 薄封装层：`execute_python` + recipes 模式 |
| [mcp-skills-integration.md](mcp-skills-integration.md) | 技能与 MCP HTTP 服务器的集成方式 |
| [skill-scopes-policies.md](skill-scopes-policies.md) | SkillScope（信任级别）与 SkillPolicy |
| [context-bundles.md](context-bundles.md) | 通过解析的启动上下文按项目/任务/资产加载技能 |
| [rez-skill-packages.md](rez-skill-packages.md) | Rez 包布局与分发技能的环境变量约定 |

## MCP 服务器与 HTTP

| 文档 | 用途 |
|------|------|
| [remote-server.md](remote-server.md) | 云端托管 MCP Agent：认证、批处理、引导、富内容 |
| [gateway.md](gateway.md) | 多 DCC 网关：聚合、工具路由 |
| [gateway-election.md](gateway-election.md) | `DccGatewayElection` — 自动故障切换 |
| [dcc-rest-skill-api.md](dcc-rest-skill-api.md) | 每个 DCC 的 REST 技能 API 接口（#658 / #660） |
| [tunnel-relay.md](tunnel-relay.md) | 零配置远程 MCP 中继（`RelayServer` + tunnel agent） |
| [production-deployment.md](production-deployment.md) | 生产部署检查清单：日志、健康探针、监控 |
| [protocols.md](protocols.md) | MCP 协议类型与版本管理 |
| [middleware.md](middleware.md) | 可插拔 BeforeCall/AfterCall 中间件链：审计、限流、脱敏 |
| [admin-ui.md](admin-ui.md) | 内置零构建 `/admin` 仪表盘（实例、工具、调用、traces、stats、workers、日志、健康、JSONL 审计/trace 持久化） |
| [translate.md](translate.md) | `translate` 子命令：将任意 stdio MCP 服务器桥接到 HTTP/SSE |
| [openapi-mount.md](openapi-mount.md) | OpenAPI → MCP 挂载助手：将任意 REST API 暴露为 MCP 工具 |
| [catalog.md](catalog.md) | DCC-MCP 公共适配器目录：搜索与描述 |

## 核心子系统

| 文档 | 用途 |
|------|------|
| [actions.md](actions.md) | ToolRegistry、ToolDispatcher、ToolPipeline、VersionedRegistry |
| [custom-actions.md](custom-actions.md) | 添加自定义工具类型与验证策略 |
| [events.md](events.md) | EventBus 发布/订阅系统 |
| [naming.md](naming.md) | 客户端安全工具名称与 Action ID 验证规则 |
| [transport.md](transport.md) | IPC 传输：DccLinkFrame、IpcChannelAdapter、SocketServerAdapter |
| [process.md](process.md) | 进程管理：启动、监控、崩溃恢复 |
| [capture.md](capture.md) | 屏幕/窗口截图 API |
| [sandbox.md](sandbox.md) | SandboxPolicy、InputValidator、AuditLog |
| [shm.md](shm.md) | 共享内存与零拷贝场景数据 |
| [usd.md](usd.md) | OpenUSD 桥接：UsdStage、场景信息 JSON |
| [artefacts.md](artefacts.md) | FileRef + ArtefactStore — 跨工具文件交接 |
| [telemetry.md](telemetry.md) | ToolMetrics、ToolRecorder、RecordingGuard |
| [observability.md](observability.md) | OTLP exporter、agent workflow spans、网关事件日志（`resources://gateway/events`）、Prometheus 计数器 |
| [scheduler.md](scheduler.md) | ScheduleSpec、TriggerSpec、cron/webhook 调度 |
| [workflows.md](workflows.md) | WorkflowSpec 引擎：步骤类型、策略、持久化 |
| [job-persistence.md](job-persistence.md) | SQLite 支持的作业/工作流持久化与恢复 |
| [project-persistence.md](project-persistence.md) | 跨会话的项目级状态（场景、资产、活跃技能） |
| [prompts.md](prompts.md) | MCP Prompt 定义 |
| [capabilities.md](capabilities.md) | DccCapabilities 与功能检测 |
| [faq.md](faq.md) | 常见问题解答 |

## 线程安全与并发

| 文档 | 用途 |
|------|------|
| [dcc-thread-safety.md](dcc-thread-safety.md) | DCC 主线程分发、协作式取消 |
| [host-adapter.md](host-adapter.md) | `HostAdapter` 基类，用于每个 DCC 的分发器接线 |
| [cross-dcc-verification.md](cross-dcc-verification.md) | 生产者→文件→验证者往返的 `SceneStats` 契约 |

## 集成

| 文档 | 用途 |
|------|------|
| [mcp-skills-integration.md](mcp-skills-integration.md) | 在 MCP HTTP 服务器上注册技能 |
