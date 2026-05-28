# Gateway

Gateway（`McpHttpConfig::gateway_port > 0`）是一个 first-wins HTTP
门面，将所有在线的 DCC 实例呈现在一个 MCP 端点下。
单个客户端可以通过同一个 `/mcp` URL 与 Maya、Blender 和 Houdini
通信；Gateway 通过 `FileRegistry` 发现在线后端，将自身 MCP
`tools/list` 固定为四个规范工作流原语，按需索引后端能力，用 MCP
`search` / `describe` / `load_skill` / `call` 完成发现、加载与执行，并把 REST `/v1/*` 调用路由到正确的后端，
同时将服务器推送的通知多路复用回原始客户端会话。

可以在每个候选进程上设置 `gateway_name`、`--gateway-name` 或
`DCC_MCP_GATEWAY_NAME` 来显式声明身份。赢得选举的进程会把这个标签写入
`__gateway__` sentinel，并暴露在 `/admin/api/health.gateway.current`；
challenger 会以 `gateway_role=challenger` 写入同类标签，因此排障时能同时
看到当前网关和正在尝试接班的下一个候选。

生产环境推荐机器级独立 gateway：

```bash
dcc-mcp-server gateway --port 9765 --name studio-gateway
```

Per-DCC sidecar 现在会在 `GET /health` 不可达时自动拉起这个进程。它们会
在 registry 目录里使用单飞 `gateway-launch.lock`，因此三个 DCC 同时启动也
最多只会 spawn 一个 gateway。使用 `dcc-mcp-server sidecar --no-ensure-gateway`
可以关闭自动拉起；使用 `--legacy-gateway-election` 可以恢复旧的 per-DCC
first-wins 选举。

## 独立 gateway 守护进程（#1358）

`dcc-mcp-server gateway` 子命令把 gateway **作为独立进程**运行，与任何
per-DCC server 解耦。它只承载 gateway 平面 —— 发现、聚合、路由、动态
能力、resources / prompts fan-out、只读 admin UI、审计 —— 自身**永远
不执行工具**；每个 `tools/call` 都通过 HTTP 转发给真正拥有的 DCC backend。

```bash
# 前台运行，带可读的 owner 标签
dcc-mcp-server gateway --host 127.0.0.1 --port 9765 --name studio-gateway

# 同时监听 LAN，让同子网内其他主机加入
dcc-mcp-server gateway --remote-host 0.0.0.0 --remote-port 59765
```

常用 flag（也接受对应的 `DCC_MCP_*` 环境变量）：

| Flag | 环境变量 | 默认 |
|------|----------|------|
| `--host` | `DCC_MCP_GATEWAY_HOST` | `127.0.0.1` |
| `--port` | `DCC_MCP_GATEWAY_PORT` | `9765` |
| `--name` | `DCC_MCP_GATEWAY_NAME` | `gateway-<host>-pid<n>` |
| `--remote-host` | `DCC_MCP_GATEWAY_REMOTE_HOST` | `0.0.0.0` |
| `--remote-port` | `DCC_MCP_GATEWAY_REMOTE_PORT` | `59765`（`0` 关闭） |
| `--registry-dir` | `DCC_MCP_REGISTRY_DIR` | OS 默认 |
| `--no-admin` | `DCC_MCP_NO_ADMIN` | admin 默认开启 |
| `--admin-path` | `DCC_MCP_ADMIN_PATH` | `/admin` |
| `--stale-timeout-secs` | `DCC_MCP_STALE_TIMEOUT` | `30` |

附加环境变量：

- `DCC_MCP_GATEWAY_ADMIN_DB` —— admin SQLite 路径（默认在 workspace 锚定位置）。
- `DCC_MCP_GATEWAY_ADMIN_RETENTION_DAYS` —— admin SQLite 保留天数，
  自动 clamp 到 `[1, 3650]`，默认 `30`。

### 守护进程模式保证

独立 daemon 路径会把 gateway 的 `adapter_dcc` 标为 `"gateway"`，让 peer
在 election tiebreak 时识别（参见 `version.rs` —— 真实 DCC 会抢占
generic standalone）。运行时满足：

- **不执行 DCC 工具。** `dcc-mcp-gateway` 仅从 `dcc-mcp-actions` 引用
  `EventBus` / `EventEnvelope` 等 wire 类型；从不持有 `ToolDispatcher`，
  也不会在进程内 inline 调用工具。
- **无 PyO3 / Python host bridge。** `cargo tree -p dcc-mcp-gateway`
  里完全找不到 `pyo3`、`dcc-mcp-pybridge`、`dcc-mcp-host`、
  `dcc-mcp-sandbox` 或 `dcc-mcp-capture`。
- **无 DCC backend 也能跑。** `GET /health` 在空 registry 下返回
  `200 OK`，回归用例在 `crates/dcc-mcp-server/src/gateway_daemon.rs`
  的 `gateway_daemon::tests::standalone_daemon_serves_health_without_any_backend`。
- **与 auto-gateway 共存。** 使用 `dcc-mcp-http` 默认 feature 的 per-DCC
  server 在没有 daemon 时仍会自我选举（#1357 把 auto-gateway 路径放到
  默认开启的 cargo feature 后面，关闭该 feature 即可让 binary 完全跳过
  gateway 运行时）。

### 何时用哪种模式

| 场景 | 推荐模式 |
|------|----------|
| 单艺术家本机，单 DCC | per-DCC server 默认自带的 auto-gateway |
| 工作站多 DCC | 任意；auto-gateway 让最先启动的 DCC 当选 |
| 渲染机 / 共享主机 / CI | `dcc-mcp-server gateway` daemon，sidecar 拉起 DCC |
| 无任何 DCC 安装的 headless agent | `dcc-mcp-server gateway` daemon —— DCC 通过 `FileRegistry` / HTTP 注册接入 |

## 拓扑

```
              ┌──────────────── gateway ────────────────┐
  client_A ──▶│  POST /mcp  (tools/list, tools/call)    │───▶ backend (maya)
              │  GET  /mcp  (SSE — MCP 2025-03-26)      │───▶ backend (blender)
  client_B ──▶│  subscribers: per-client broadcast sink │
              │  backend SSE sub: one per backend URL   │
              └────────────────────────────────────────┘
```

## 拓扑配方（issue #1366）

四种命名配方覆盖支持的所有部署形态。每个都是完整可复制粘贴的命令集；按
你的约束挑一种，并参考迁移指南了解配方之间的切换路径
（[`docs/zh/guide/migration/from-embedded-to-daemon.md`](migration/from-embedded-to-daemon.md)）。

### 配方 1 —— 单工作站（内嵌自动网关）

默认零配置流程。一个 DCC 适配器赢得网关端口，为整台机器服务。

```bash
# Maya 插件宿主：
dcc-mcp-server --app maya

# 同一工作站第二个 DCC —— 作为后端加入已选举的网关：
dcc-mcp-server --app blender
```

对 `http://127.0.0.1:9765/mcp` 调用 `tools/list` 暴露的是网关的有界发现
原语；路由会扇出到两个 DCC。

### 配方 2 —— 多工作站 LAN + 守护进程网关 + HTTP 注册

守护进程在选定主机上占有网关端口；每台工作站的每个 DCC 适配器通过
HTTP API 注册（#1361）。

```bash
# 主机 A —— 只跑网关：
dcc-mcp-server gateway --host 0.0.0.0 --port 9765 --registry-dir /var/lib/dcc-mcp

# 主机 B —— 跑 DCC，永远不抢网关端口：
dcc-mcp-server serve --no-auto-gateway --app maya \
    --register-url http://host-a.lan:9765/v1/instances/register \
    --heartbeat-secs 5

# 主机 C —— 不同的 DCC，同一个注册目标：
dcc-mcp-server serve --no-auto-gateway --app photoshop \
    --register-url http://host-a.lan:9765/v1/instances/register
```

主机 A 上的 `gateway://instances` 会列出 B 和 C，`source: "http"`。

### 配方 3 —— LAN + mDNS 自动发现

适合不方便给每台主机配 `--register-url` 的场景。需要 `--features mdns`
构建；网关浏览 `_dcc-mcp._tcp.local`、探测每个发现的端点，幸存者以
`source: "mdns"` 出现（#1362）。

```bash
# 主机 A —— 守护进程监听 LAN 上广播的 DCC sidecar：
dcc-mcp-server gateway --host 0.0.0.0 --port 9765 --discover-mdns

# 主机 B（或 C、D…）—— 每个 DCC sidecar 广播自己：
dcc-mcp-server serve --no-auto-gateway --app blender --advertise-mdns

dcc-mcp-server serve --no-auto-gateway --app houdini --advertise-mdns
```

安全立场：mDNS 只用于*地址发现*。发现的端点仍然必须通过网关的 auth chain
才能路由调用。

### 配方 4 —— 经 tunnel relay 暴露到公网

DCC 位于 NAT / 防火墙后。`dcc-mcp-tunnel-agent` 通过 WSS 回连到公网可达
的 `dcc-mcp-tunnel-relay`；网关轮询 relay 的 admin API，把健康的隧道以
`source: "relay"` 暴露出来（#1363）。

```bash
# 公网 relay（例如 fly.io、k8s ingress 等）：
dcc-mcp-tunnel-relay \
    --agent-bind 0.0.0.0:9090 \
    --frontend-bind 0.0.0.0:9091 \
    --admin-bind 127.0.0.1:9092

# NAT 后的 DCC 主机：
dcc-mcp-tunnel-agent \
    --relay-url wss://relay.example.com:9090 \
    --jwt $TUNNEL_JWT \
    --dcc photoshop \
    --local-target http://127.0.0.1:8765/mcp

# 网关指向 relay 的 admin 端点：
dcc-mcp-server gateway --host 0.0.0.0 --port 9765 \
    --relay-source http://relay.example.com:9092=https://relay.example.com:9091
```

Auth 契约：agent 段使用 tunnel JWT；gateway 段使用网关自己的 auth chain。
两者都通过，调用才能端到端路由。

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

客户端使用此事件通过 `jobs_get_status` 重新查询进行中的作业。

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
   `crates/dcc-mcp-gateway/src/gateway/sentinel.rs` 中的
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

## 实例与诊断发现

Gateway 将实时 DCC 注册表暴露为 Gateway 原生 MCP resource（也见
`docs/zh/api/http.md`）：

```json
{"jsonrpc":"2.0","id":1,"method":"resources/read",
 "params":{"uri":"gateway://instances"}}
```

payload 会包含 live、stale 与 unhealthy 行，方便客户端决定是路由、
重连，还是提示用户重启 DCC 实例。每条记录已经携带 `mcp_url`，因此
读取这个 resource 后即可直连。可选 URI 查询参数
（`?include_stale=false`、`?include_dead=true`）对应旧实例发现工具的
过滤意图。`tools/list` 会在每次调用时基于当前注册表组装，因此
Gateway 启动后新注册的实例不需要重启即可被发现。

## 动态能力索引与有界工具暴露 (#652-#657)

在大型多 DCC 部署中，Gateway **永远不会**把每个后端 action 直接发布到
`tools/list`。已移除的 `GatewayToolExposure` 枚举、
`McpHttpConfig.gateway_tool_exposure`、`publishes_backend_tools` 与
`--gateway-tool-exposure` 都是 0.15 之前的概念。现在只有一个无条件表面：


| 表面 | `tools/list` 中出现什么 | Agent 工作流 |
|------|--------------------------|--------------|
| Gateway MCP | 固定工作流原语：`search`、`describe`、`load_skill`、`call`。实例注册表通过 `gateway://instances` MCP resource 暴露（用 `resources/read` 读取），而不是工具 — 见 #813 phase 1 | `resources/read uri=gateway://instances`（或跳过它，直接 `search` → `describe`），必要时按 `next_step.arguments` 调 `load_skill`，然后用 `call` 执行单个 `tool_slug` 或有序 `calls` 批处理 |

| Gateway REST | `/v1/search`、`/v1/load_skill`、`/v1/unload_skill`、`/v1/describe`、`/v1/call`、`/v1/call_batch`、`/v1/instances` | `POST /v1/search` → 必要时用 `next_step.arguments` 调 `/v1/load_skill` → `/v1/describe` → `/v1/call` |
| 直连 per-DCC MCP | 单个 DCC 服务的 skills 与已加载工具 | `search_skills` → `load_skill` → 调用工具 |

Gateway capability index 使用 `<dcc>.<id8>.<tool>` 作为紧凑记录键，并按需刷新。
因此启动后或 `load_skill` 后的第一次 agent 查询就能看到最新能力，不需要等待轮询。
固定 MCP 工作流工具是 cursor-safe 且稳定的；隐藏兼容 wrapper 仍可被已固定的旧客户端调用，但不再广告：

| 工具 | 用途 |
|------|------|
| `search` | 按 query、DCC 类型、tag、实例、scene hint、分页参数搜索紧凑能力记录；`kind=skill` 搜索 skills |
| `describe` | 获取选中 `tool_slug` 的完整 schema、annotations 与路由记录，或按 `skill_name` 获取 skill 详情 |
| `load_skill` | 在目标后端加载已发现的 skill，或激活/停用一个渐进式工具组 |
| `call` | 调用单个 `tool_slug`，或执行有序 `{calls:[...]}` 批处理；批处理沿用 `/v1/call_batch` 的最大 25 条 guardrail |

Agent 连接到 Gateway 时使用这条四工具动态能力流程；直接连接某个 DCC 服务时使用
per-DCC Skills-First 流程（`search_skills` → `load_skill` → 调用工具）。
当 `/v1/search` 或 MCP `search` 返回未加载 skill 命中时，结果会带
`load_state`、已知的 `available_groups`，以及同时包含 MCP/REST 形态的
`next_step`。Gateway `load_skill` 默认使用惰性 group 激活
（未显式传入时等价于 `activate_groups=false`），只让默认/core group 自动可用；
更重或有破坏性的 group 应通过显式 `tool_group` 激活。

## 代码指针

| 组件 | 文件 |
|------|------|
| 订阅管理器、重连循环 | `crates/dcc-mcp-gateway/src/gateway/sse_subscriber.rs` |
| 每会话 SSE 管道 | `crates/dcc-mcp-gateway/src/gateway/handlers/` (`handle_gateway_get`) |
| `tools/call` 关联钩子 | `crates/dcc-mcp-gateway/src/gateway/aggregator.rs` (`route_tools_call`) |
| 订阅观察者和运行时任务 | `crates/dcc-mcp-gateway/src/gateway/tasks.rs` |

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
调用者可以重新通过 SSE 连接或持续轮询 `jobs_get_status`
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
