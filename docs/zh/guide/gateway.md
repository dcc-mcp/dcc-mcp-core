# Gateway

Gateway（`McpHttpConfig::gateway_port > 0`）是一个 first-wins HTTP
门面，将所有在线的 DCC 实例呈现在一个 MCP 端点下。
单个客户端可以通过同一个 `/mcp` URL 与 Maya、Blender 和 Houdini
通信；Gateway 通过 `FileRegistry` 发现在线后端，聚合它们的
`tools/list`，将每个 `tools/call` 路由到正确的后端，并将
服务器推送的通知多路复用回原始客户端会话。

## 拓扑

```
              ┌──────────────── gateway ────────────────┐
  client_A ──▶│  POST /mcp  (tools/list, tools/call)    │───▶ backend (maya)
              │  GET  /mcp  (SSE — MCP 2025-03-26)      │───▶ backend (blender)
  client_B ──▶│  subscribers: per-client broadcast sink │
              │  backend SSE sub: one per backend URL   │
              └────────────────────────────────────────┘
```

## SSE 多路复用 (#320)

当 Gateway 检测到新后端时，它会打开一个持久的 SSE 连接到
`<backend>/mcp`（客户端对 Gateway 使用的同一个 Streamable HTTP
传输）。后端发出的通知被解析为 JSON-RPC 消息并路由到正确的客户端：

| MCP 方法 | 关联键 | 来源 |
|----------|--------|------|
| `notifications/progress` | `params.progressToken` | 当外发 `tools/call` 携带了 `_meta.progressToken` 时由 Gateway 设置 |
| `notifications/$/dcc.jobUpdated` | `params.job_id` | 从后端回复的 `_meta.dcc.jobId` / `structuredContent.job_id` 设置 |
| `notifications/$/dcc.workflowUpdated` | `params.job_id` | 同上 |

### 挂起缓冲区

在关联已知之前到达的通知（后端 SSE 推送与 `tools/call` HTTP
回复之间的竞争）被保存在一个有界的每后端队列中：**256 个事件**
或 **30 秒**，以先到者为准。当映射出现时缓冲区被排空；
过期条目会以 `warn!` 日志丢弃。

### 重连 + 合成 `$/dcc.gatewayReconnect`

每个后端订阅器拥有一个带 jitter 的指数退避重连循环
（100 ms → 10 s，±25% jitter）。当断开的流重新连接时，
Gateway 会向每个在该后端上有进行中的作业的客户端发出一个合成的
`notifications/$/dcc.gatewayReconnect` 通知：

```json
{
  "jsonrpc": "2.0",
  "method": "notifications/$/dcc.gatewayReconnect",
  "params": { "backend_url": "http://127.0.0.1:18812/mcp" }
}
```

客户端使用此事件通过 `jobs.get_status` 重新查询进行中的作业。

### 会话生命周期

每客户端 SSE sink 以 `Mcp-Session-Id` 为键。当 `GET /mcp`
响应体被丢弃时（客户端断开连接），一个 `SessionCleanup` RAII
守卫会运行：从订阅管理器中移除该客户端的 sink，并清除绑定到
该会话的任何 `job_routes` / `progress_token_routes`。后端订阅保持
活动状态 — 另一个客户端可能仍然依赖它们。

### 自环防护与订阅前卫生检查（#419）

当一个 DCC 进程（Maya、Blender、Houdini…）赢得 Gateway 选举时，
`FileRegistry` 中会同时保留 *两行*：`__gateway__` 哨兵行以及它
自身的普通 `"maya"` / `"blender"` / … 行。如果不做过滤，后端 SSE
订阅器会连接到自己的 `/mcp` 端点 —— 这是一个经典的自环，每次
facade 抖动都会浪费 socket 并塞满重连日志。

两条不变量防止这种情况：

1. **所有 fan-out 路径都排除自身。** `GatewayState::live_instances`
   会跳过 `(host, port)` 等于 Gateway 自身绑定地址的行，使用
   `crates/dcc-mcp-http/src/gateway/sentinel.rs` 中的
   `is_own_instance` 辅助函数。该函数将 localhost 别名
   （`localhost` / `::1` / `0.0.0.0` / `[::]`）规范化为
   `127.0.0.1`，这样即便适配器把 host 写成 `"localhost"`，当
   Gateway 绑定到 `127.0.0.1` 时也能被正确过滤。
   `backend_sub_handle` 订阅循环和 `compute_tools_fingerprint_with_own`
   监听器都复用同一过滤规则。
2. **启动订阅循环前执行同步卫生检查。** 在 `start_gateway_tasks`
   内部，`backend_sub_handle` spawn **之前**会同步执行一次
   `prune_dead_pids()` + `cleanup_stale()`。周期性清理任务每 15
   秒才触发一次；没有这次同步前置扫描的话，上一次崩溃残留的幽灵
   行会在 Gateway 启动后的前 ~15 秒内耗尽完整的指数退避重连预算。

## 代码指针

| 组件 | 文件 |
|------|------|
| 订阅管理器、重连循环 | `crates/dcc-mcp-http/src/gateway/sse_subscriber.rs` |
| 每会话 SSE 管道 | `crates/dcc-mcp-http/src/gateway/handlers.rs` (`handle_gateway_get`) |
| `tools/call` 关联钩子 | `crates/dcc-mcp-http/src/gateway/aggregator.rs` (`route_tools_call`) |
| 订阅观察者 | `crates/dcc-mcp-http/src/gateway/mod.rs` (`backend_sub_handle`) |

## 从 Gateway 等待终止结果 (#321)

Gateway 对出站 `tools/call` 应用两套独立的请求预算：

| 情况 | 超时 | 来源 |
|------|------|------|
| 同步调用（无 `_meta.dcc.async`，无 `progressToken`） | `backend_timeout_ms`（默认 10 s） | `McpHttpConfig` |
| 异步 opt-in 调用（`_meta.dcc.async=true` 或 `_meta.progressToken`） | `gateway_async_dispatch_timeout_ms`（默认 60 s） | `McpHttpConfig` |
| 异步 opt-in **并且** `_meta.dcc.wait_for_terminal=true` | `gateway_wait_terminal_timeout_ms`（默认 10 min）用于等待，`gateway_async_dispatch_timeout_ms` 用于初始排队步骤 | `McpHttpConfig` |

**为什么需要两个超时？** 异步分派的工具在作业在后端排队后立即
回复 `{status:"pending", job_id:"…"}`。在冷启动条件下
（Maya 重新导入重型模块、Blender 启动新的 Python 解释器）
即使是这个排队步骤也可能合法地需要 >10 s，因此短的同步超时
会在后端仍在启动工作时表面一个虚假的传输错误。

### 响应缝合（opt-in）

无法消费 SSE 的客户端（纯 `curl`、批处理脚本、CI runner）
仍然可以通过在 `_meta.dcc.async = true` 的同时设置
`_meta.dcc.wait_for_terminal = true` 来在单个 `tools/call`
响应中获取最终结果：

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "tools/call",
  "params": {
    "name": "maya__bake_simulation",
    "arguments": {...},
    "_meta": {
      "dcc": {"async": true, "wait_for_terminal": true}
    }
  }
}
```

Gateway 现在：

1. 以更长的 `gateway_async_dispatch_timeout_ms` 预算转发调用到后端。
2. 接收 `{pending, job_id}` 信封并订阅 SSE 订阅管理器拥有的
   每作业广播总线。
3. 阻塞 HTTP 响应，直到通过后端 SSE 流到达一个状态属于
   `{completed, failed, cancelled, interrupted}` 的
   `notifications/$/dcc.jobUpdated` 帧，或者直到
   `gateway_wait_terminal_timeout_ms` 超时。
4. 将终止状态、`result` 和 `error` 合并到原始 pending 信封的
   `structuredContent` 中，并返回生成的 `CallToolResult`。
   任何非 `completed` 状态都会设置 `isError`。

### 超时语义

如果 `gateway_wait_terminal_timeout_ms` 超时前未到达终止事件，
Gateway 会返回**最后观察到的**作业信封，并标注
`_meta.dcc.timed_out = true`，同时让作业继续在后端运行。
调用者可以重新通过 SSE 连接或持续轮询 `jobs.get_status`
来收集最终结果。

### 后端断开

如果后端 SSE 流在等待者阻塞时断开，Gateway 返回一个
JSON-RPC `-32000` 错误，标识后端和 `job_id`。作业本身不会被
取消 — 后端的后续重启可能将其表面为 `interrupted`
（issue #328），当持久化作业存储重新水合时。

## 作业到后端路由缓存 (#322)

为了将客户端的 `notifications/cancelled { requestId }` 转发到
实际拥有该作业的后端，Gateway 维护一个小缓存：

```rust
pub struct JobRoute {
    pub client_session_id: ClientSessionId,
    pub backend_id: BackendId,            // 例如 http://127.0.0.1:8001/mcp
    pub tool: String,                     // 用于日志 + cancel payload
    pub created_at: DateTime<Utc>,        // GC 锚点
    pub parent_job_id: Option<String>,    // #318 级联
}
// DashMap<Uuid, JobRoute>
```

在后端对 `tools/call` 的回复携带 `job_id` 时填充。被以下功能消费：

- `notifications/cancelled { requestId }` — Gateway 解析
  `requestId → job_id → JobRoute` 并向 `backend_id` POST cancel。
- 父作业级联 — 如果被取消的作业有 `parent_job_id`，或者
  *它自己就是*父作业，Gateway 会遍历 `children_of` 索引并将
  cancel 扇出到每个不同的 `backend_id`（这可能与发起后端不同 —
  `#318` 仅覆盖单服务器级联，Gateway 将其扩展到跨后端）。

### 生命周期

- **插入** — `aggregator::route_tools_call` → `SubscriberManager::bind_job_route`。
- **自动驱逐** — `deliver()` 在观察到带终止状态（`completed`、`failed`、
  `cancelled`、`interrupted`）的 `$/dcc.jobUpdated` 时立即移除路由。
- **TTL GC** — 后台任务每 60 秒扫描一次超过
  `gateway_route_ttl_secs`（默认 24 小时）的路由，因此一个从未发出
  终止事件的后端崩溃不会泄漏路由。
- **每会话上限** — `gateway_max_routes_per_session`（默认 1000）。
  当会话已持有 `cap` 个活跃路由时，新的分派会被拒绝，返回
  JSON-RPC `-32005 too_many_in_flight_jobs`。

### Python 配置

```python
from dcc_mcp_core import McpHttpConfig

cfg = McpHttpConfig(
    port=0,
    gateway_route_ttl_secs=3600,              # 1 小时
    gateway_max_routes_per_session=500,
)
```

两个字段在返回的 `McpHttpConfig` 实例上也可作为 getter/setter 访问。

## 非目标

HTTP/2 多路复用调优以及路由缓存的多后端故障转移
（路由是粘性的）对 #320 / #321 / #322 来说超出范围。
