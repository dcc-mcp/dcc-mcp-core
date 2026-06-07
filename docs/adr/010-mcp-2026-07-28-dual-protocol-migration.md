# ADR-010: MCP 2026-07-28 双协议迁移策略

- **Status:** Proposed
- **Date:** 2026-06-07
- **Issue:** [PIP-786](https://github.com/dcc-mcp/dcc-mcp-core/issues/786)
- **Parent ADR:** [ADR-009 (RMCP Migration)](./009-rmcp-migration.md)
- **Target release:** dcc-mcp-core 0.19.0 (2026-07-15)
- **Sunset deadline:** 2026-12-31

## Context

MCP `2026-07-28` RC 已于 2026-05-21 锁定，正式 spec 2026-07-28 发布。这是自 MCP 发布以来最大的一次协议修订，核心变更围绕三个方向：

### Breaking Changes in 2026-07-28

| Change | SEP | Status |
|--------|-----|--------|
| 移除 `initialize`/`initialized` 握手 | SEP-2575 | Breaking |
| 移除 `Mcp-Session-Id` header 及协议层 session | SEP-2567 | Breaking |
| `_meta` 对象携带版本/客户端信息 (每个请求自包含) | SEP-2575 | Breaking |
| 新增 `server/discover` 方法替代 initialize 协商 | SEP-2575 | New |
| SSE streaming 替换为 `InputRequiredResult` 多轮往返 | SEP-2260, SEP-2322 | Breaking |
| Tasks 扩展从实验性变为一等公民 | SEP-2663 | New |
| `MCP-Protocol-Version` / `Mcp-Method` / `Mcp-Name` headers 规范化 | SEP-2243 | New |
| 缓存 (`ttlMs` / `cacheScope`) | SEP-2549 | New |
| W3C Trace Context 追踪 | SEP-414 | New |
| MCP Apps 服务端 UI 渲染 | SEP-1865 | New |
| 授权加固 (OAuth 2.1 / OIDC) | 6 SEPs | Breaking |
| `roots` / `sampling` / `logging` 弃用 (12个月窗口) | SEP-2596 | Deprecated |
| `tasks/list` 移除 | SEP-2663 | Breaking |

### Current State of dcc-mcp-core

- **协议版本:** 默认 `2025-06-18`，兼容 `2025-03-26` (`dcc-mcp-jsonrpc/src/lib.rs:60-64`)
- **传输层:** rmcp SDK v1.6, Streamable HTTP with session (`transport-streamable-http-server`)
- **Session 管理:** `McpSession` 持有 `Mcp-Session-Id`、`initialized` 状态、SSE broadcast channel、TTL 驱逐 (`dcc-mcp-http-server/src/session.rs`)
- **初始化:** `rmcp_initialize.rs` 实现 `initialize` 协商 (协议版本、delta tools、roots)
- **SSE:** 用于 server→client 通知推送 (`sse_subscriber/` 模块)
- **Session store:** `DashMap<String, McpSession>` 内存存储
- **Gateway:** 依赖 session 做路由亲和性 (`gateway/` 模块)

### 为什么需要双协议窗口

1. **客户端生态迁移需要时间** — Tier 1 SDK (Python/TypeScript) 在 RC 窗口期内发布支持，但下游客户端 (Claude Desktop、Cursor、Copilot 等) 的升级节奏不可控
2. **DCC 宿主环境升级缓慢** — Maya/Houdini/Blender 插件的用户不会立即升级，需要向后兼容旧客户端
3. **MCP deprecation policy (SEP-2596)** 要求最少 12 个月弃用窗口，我们作为服务端应当同等承诺
4. **避免 flag day** — 不允许出现"某天升级后所有旧客户端断连"的情况

## Decision

采用**三阶段渐进式迁移**，按请求头 `MCP-Protocol-Version` 分流：

```
Phase 1 (0.19.0, 2026-07-15): 兼容层 — 新旧协议并存
Phase 2 (0.21.0, 2026-09-30): 默认新协议 — 新客户端默认 2026-07-28
Phase 3 (0.23.0, 2026-12-15): 移除旧协议 — 仅支持 2026-07-28
```

### 协议分流判断逻辑

```rust
fn select_protocol_mode(req: &HttpRequest) -> ProtocolMode {
    match req.headers().get("MCP-Protocol-Version") {
        Some(v) if v == "2026-07-28" => ProtocolMode::Stateless,
        Some(v) if SUPPORTED_LEGACY.contains(&v) => ProtocolMode::Session,
        None if req.headers().contains_key("Mcp-Session-Id") => ProtocolMode::Session,
        None => ProtocolMode::default(), // Phase 1: Session; Phase 2: Stateless
        _ => ProtocolMode::Session, // unknown version → fallback to session
    }
}
```

### Phase 1: 兼容层 (0.19.0, target 2026-07-15)

**目标:** 新旧客户端都可连接，新协议功能可用但非默认。

- `dcc-mcp-jsonrpc` 新增 `SUPPORTED_PROTOCOL_VERSIONS` 条目 `"2026-07-28"`
- 新增 `ProtocolMode` 枚举: `Session` (旧) / `Stateless` (新)，由请求头判定
- Session 路径保持不变 (rmcp `StreamableHttpService`)
- Stateless 路径新增 `StatelessMcpService`:
  - 不创建 session，不读取 `Mcp-Session-Id`
  - 每个请求从 `_meta` 解析 `ProtocolVersion`、`ClientInfo`、`ClientCapabilities`
  - `server/discover` 替代 `initialize` 返回 `ServerCapabilities`
  - 工具调用直接路由到现有 `ServerState`（ToolRegistry / Dispatcher / Catalog 不变）
- Gateway 层面:
  - `/mcp` 端点同时接受新旧协议，由 header 分流
  - 移除 stateless 请求的 sticky session 要求
  - SSE 模块保留给旧协议；新协议使用 `InputRequiredResult` 多轮往返
- **不引入 rmcp 新版本依赖**（rmcp 对 2026-07-28 的支持时机不确定），stateless 路径先自研，后续可替换为 rmcp 实现
- 特性开关: `mcp-2026-07-28` feature flag, off by default
- 无状态路径的 session store / TTL 驱逐逻辑完全跳过

**交付物:**
- `crates/dcc-mcp-http-server/src/stateless/` — 无状态服务实现
- `dcc-mcp-jsonrpc` 新增 `server/discover` 类型
- Gateway header 分流中间件
- 集成测试: 新旧客户端并行连接

### Phase 2: 默认新协议 (0.21.0, target 2026-09-30)

**目标:** 新客户端默认走 2026-07-28，旧客户端继续兼容。

- `MCP_PROTOCOL_VERSION` 默认值改为 `"2026-07-28"`
- `mcp-2026-07-28` feature 默认开启
- 无 `MCP-Protocol-Version` header 且无 `Mcp-Session-Id` 的请求默认走 stateless 路径
- Gateway 负载均衡不再依赖 session affinity
- SSE subscriber 模块标记 `#[deprecated]`，计划 Phase 3 移除
- Session 相关代码路径标记 `#[deprecated]`
- `ServerCapabilities` 广告中移除 `experimental.tasks` → Tasks 声明为正式 capabilities

**交付物:**
- 默认协议版本切换
- SSE 模块 deprecation annotation
- Gateway session affinity 可选化
- 升级指南文档

### Phase 3: 移除旧协议 (0.23.0, target 2026-12-15)

**目标:** 仅支持 2026-07-28，完成 Sunset。

- 移除 `McpSession`、`SessionManager`、TTL 驱逐
- 移除 SSE subscriber 模块
- 移除 `initialize`/`initialized` 握手逻辑
- 移除 `Mcp-Session-Id` header 处理
- 移除 `rmcp_initialize.rs`
- `SUPPORTED_PROTOCOL_VERSIONS` 仅保留 `"2026-07-28"`
- `mcp-2026-07-28` feature 移除 (成为唯一路径)
- 清理所有 `#[deprecated]` 代码
- 更新 `dcc-mcp-jsonrpc` 文档注释中的 spec 引用

**交付物:**
- Session 系统完全移除
- 代码净减少估计 ~3K lines

## Breaking Changes Impact Analysis

### 对 dcc-mcp-core 内部的影响

| 模块 | 影响 | 处理方式 |
|------|------|----------|
| `dcc-mcp-jsonrpc` | 协议常量、类型定义需扩展 | Phase 1 新增，Phase 3 移除旧常量 |
| `dcc-mcp-http-server/session.rs` | Session 系统将被移除 | Phase 3 删除 |
| `dcc-mcp-http-server/rmcp_initialize.rs` | `initialize` 握手将被移除 | Phase 3 删除 |
| `dcc-mcp-http-server/rmcp_handler.rs` | `ServerHandler` trait 可能需要适配 stateless | 增加 stateless handler |
| `dcc-mcp-gateway/sse_subscriber/` | SSE 推送将被移除 | Phase 2 deprecate, Phase 3 删除 |
| `dcc-mcp-gateway/handlers/sse_impl.rs` | SSE 端点处理 | Phase 2 deprecate, Phase 3 删除 |
| `dcc-mcp-http/session_events.rs` | Session 事件系统 | Phase 3 用 stateless events 替代 |
| Tool Registry / Dispatcher / Catalog | **不受影响** — 工具注册/调度/目录逻辑不变 | 零改动 |

### 对外部用户的影响

| 用户类型 | 影响 | 缓解措施 |
|----------|------|----------|
| 使用 2026-07-28 SDK 的新客户端 | 无影响，直接可用 | 开箱即用 |
| 使用 2025-03-26 / 2025-06-18 的旧客户端 | Phase 1-2 继续工作，Phase 3 需升级 | 6+ 个月升级窗口 |
| DCC 适配器开发者 | Python API 不变 | PyO3 公共接口保持稳定 |
| Skill 作者 | 工具注册/调用 API 不变 | 零改动 |
| Gateway 运维 | 负载均衡简化 (无需 sticky sessions) | Phase 2 起受益 |

### 与 Tasks 扩展的接口约定

```
┌─────────────────────────────────────────────────┐
│ Client                                          │
│   tools/call ────────────────► Stateless Service│
│   ◄──── task { id, status }                     │
│   tasks/get { id } ─────────► Stateless Service │
│   ◄──── task { id, status, result? }            │
│   tasks/cancel { id } ──────► Stateless Service │
└─────────────────────────────────────────────────┘
```

- Task 创建由服务端决定: 工具的 `@chunked_job` 装饰器 (`dcc-mcp-http/src/executor.rs`) 自动触发 task 化
- `taskId` 映射到现有 `dcc-mcp-job` crate 的 job ID
- `tasks/get` 轮询不需要落回同一实例 — stateless 天然支持
- Task 结果存储复用 `dcc-mcp-job` 的 SQLite 持久化
- 不实现 `tasks/list` (spec 已移除)

### 与 Auth 模块的接口约定

- 授权从 session-scoped 迁移到 request-scoped
- 每个请求的 `_meta` 携带 `Authorization` 上下文
- 复用 `dcc-mcp-gateway/src/gateway/security.rs` 的 token 验证逻辑
- OAuth 2.1 / OIDC 支持延后到 Phase 2 (非 Phase 1 必需)

## Client Protocol Negotiation

### 客户端如何协商协议版本

```
Client → Server:  POST /mcp
                  MCP-Protocol-Version: 2026-07-28    ← 客户端声明版本
                  Mcp-Method: tools/call
                  Mcp-Name: create_sphere

Server → Client:  200 OK
                  MCP-Protocol-Version: 2026-07-28    ← 服务端确认版本
                  Content-Type: application/json
                  { "jsonrpc": "2.0", "id": 1, "result": {...} }
```

### 版本降级流程

```
1. Client sends MCP-Protocol-Version: 2026-07-28
2. If server supports → use 2026-07-28 stateless
3. If server doesn't support → respond with supported versions in error
4. Client retries with highest mutually supported version
5. Phase 1-2: unknown versions fall back to session mode (2025-06-18)
6. Phase 3: unknown versions receive 400 with supported versions list
```

### `server/discover` 响应 (替代 `initialize`)

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "protocolVersion": "2026-07-28",
    "serverInfo": { "name": "dcc-mcp-core", "version": "0.19.0" },
    "capabilities": {
      "tools": { "listChanged": true },
      "tasks": {},
      "resources": { "subscribe": true, "listChanged": true },
      "prompts": { "listChanged": true }
    },
    "instructions": "Direct DCC workflow: search_tools(query) → load_skill → tools/call"
  }
}
```

## Sunset Date

**Sunset: 2026-12-15** (Phase 3 release, 在 2026-12-31 截止日前)

- 符合 MCP SEP-2596 12 个月最低弃用窗口 (从 2025-11-25 旧 spec 算起)
- 给客户端生态 4.5 个月从 2026-07-28 正式发布到我们移除旧协议的窗口
- 如果 `roots`/`sampling`/`logging` 的下游依赖未清理完，可延长 Phase 2 但不超过 2026-12-31

## Consequences

### Positive

- 负载均衡不再需要 sticky sessions / Redis session store
- 水平扩展更简单，任何实例可处理任何请求
- 移除 ~3K lines session/SSE 维护代码 (Phase 3)
- Tasks 扩展支持长任务 (Maya batch render 等)，天然适配 stateless 模型
- `server/discover` 比 `initialize` 更轻量: 无状态，可 CDN 缓存

### Negative

- Phase 1-2 期间维护两套代码路径，增加复杂度
- rmcp SDK 对 2026-07-28 的支持时间线不确定，stateless 路径需自研
- 现有 SSE 依赖 (gateway notification push) 需重构为 polling-based tasks 或 webhook
- Elicitation (用户确认弹窗) 从 SSE push 改为 `InputRequiredResult` 多轮往返，交互模式变化
- `roots`/`sampling`/`logging` 弃用可能影响依赖这些特性的下游

### Neutral

- Tool Registry、Skill Catalog、DCC Executor 不变 — 核心业务逻辑零改动
- Python 公共 API 保持不变 — adapter/skill 开发者无感
- 迁移对终端 DCC 用户透明

## Alternatives Considered

### A. Big Bang 切换 (单次发布直接切 2026-07-28)

*Rejected.* DCC 生态升级缓慢，Maya/Houdini 插件用户不会同步升级。flag day 式的硬切换会导致大量旧客户端断连。MCP deprecation policy 明确要求 12 个月窗口。

### B. 仅靠 rmcp SDK 升级，不做自研 stateless

*Rejected.* rmcp v1.6 当前仅支持到 2025-11-25，2026-07-28 RC 的支持时间线不确定。等待 rmcp 可能阻塞我们的发布节奏。Phase 1 自研 stateless 路径，后续替换为 rmcp 实现 (如果 rmcp 及时支持)。

### C. 仅支持 2026-07-28，不提供兼容窗口

*Rejected.* 违反 MCP 协议精神，且我们的 DCC 用户群决定了无法要求客户端同步升级。必须提供兼容窗口。

### D. 永久维护双协议

*Rejected.* 维护成本过高。session 管理、SSE、双份测试的长期负担不值得。Sunset date 给了明确的退出时间。

## Implementation Plan

| Milestone | Deliverable | Owner | Date |
|-----------|------------|-------|------|
| PRD-1 | ADR accepted, Phase 1 design review | loonghao | 2026-06-15 |
| PRD-2 | `dcc-mcp-jsonrpc` types + `server/discover` | 后端开发 | 2026-06-30 |
| PRD-3 | Stateless service + gateway header routing | 后端开发 | 2026-07-10 |
| PRD-4 | Integration tests (old + new clients) | 代码测试 | 2026-07-12 |
| 0.19.0 | Phase 1 release | — | 2026-07-15 |
| 0.21.0 | Phase 2 release (default new protocol) | — | 2026-09-30 |
| 0.23.0 | Phase 3 release (remove old protocol) | — | 2026-12-15 |

## References

- [MCP 2026-07-28 Release Candidate](https://blog.modelcontextprotocol.io/posts/2026-07-28-release-candidate/)
- [ADR-009: RMCP Migration](./009-rmcp-migration.md)
- [MCP Spec Deprecation Policy (SEP-2596)](https://spec.modelcontextprotocol.io/)
- `dcc-mcp-jsonrpc/src/lib.rs` — current protocol version constants
- `dcc-mcp-http-server/src/session.rs` — current session management
- `dcc-mcp-http-server/src/rmcp_initialize.rs` — current initialize negotiation
- `dcc-mcp-gateway/src/gateway/sse_subscriber/` — current SSE notification system
