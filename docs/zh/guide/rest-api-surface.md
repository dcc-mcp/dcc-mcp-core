# REST API 面板

每个 per-DCC 服务和多 DCC 网关都同时暴露 `/v1/*` REST API，和 MCP 端点并列。
这个页面是面向**传统调用方**（cURL、CI 流水线、片场自动化、非-MCP 工具）的
接入契约 —— 任何能讲 HTTP 的客户端都能用这些路由驱动 DCC，不需要碰 MCP 协议栈。

> **和 MCP 的关系** —— 网关 MCP 的 `call_tool` / `describe_tool` / `search_tools`
> 与 REST 端点走同一条 `call_service` 代码路径。选 MCP 还是 REST 是传输决定，
> 不是功能决定；envelope 完全一致。

---

## 端点一览

| 方法 | 路径 | 用途 |
|---|---|---|
| `GET` | `/v1/healthz` | 存活探针。只要 HTTP 处理器在跑就 `200 {"status": "ok"}`。 |
| `GET` | `/v1/readyz` | 三态就绪：`200 Ready` / `503 Booting` / 无响应 `Unreachable`。 |
| `GET` | `/v1/skills` | 已加载 action 的扁平清单，按字典序稳定排序。 |
| `POST` | `/v1/search` | 模糊 / 精确搜索 loaded + unloaded skills。 |
| `POST` | `/v1/describe` | 按 `tool_slug` 返回完整 input schema + 注解。 |
| `GET` | `/v1/tools/{slug}` | `/v1/describe` 的别名（只读 URL 查询）。 |
| `POST` | `/v1/call` | **按 slug 调用**一个工具。这是唯一规范的调用面。 |
| `GET` | `/v1/context` | 场景 / 文档快照（per-DCC 或网关汇聚）。 |
| `GET` | `/v1/openapi.json` | 自动生成的 OpenAPI 3.x 文档，可供代码生成。 |

网关也暴露相同的路径作为汇聚面板：网关上的 `POST /v1/call`
解析 `<dcc>.<id8>.<action>` 三段式 slug，转发到拥有者后端。

---

## `POST /v1/call` —— 调用契约

### 请求体

```json
{
  "tool_slug": "maya.a1b2c3d4.create_sphere",
  "params": { "radius": 2.0, "segments": 32 },
  "meta": { "progressToken": "session-42" }
}
```

| 字段 | 必需 | 说明 |
|---|---|---|
| `tool_slug` | ✅ | `<dcc>.<id8>.<action>` 三段。8-hex-char 的 `id8` 前缀用于在多实例环境下唯一定位一个 DCC。**不要**手写 slug —— 从 `POST /v1/search` 或 `GET /v1/skills` 里拿。 |
| `params` | ❌ | 工具特定的输入。默认 `{}` 让 cURL 无参调用保持简洁。服务端会用工具的 JSON-Schema 做派发前校验。 |
| `meta` | ❌ | MCP 风格的元信息侧车。认得这几个键：`progressToken`（进度事件绑定 session）、`dcc.async`（启用异步派发）、`dcc.wait_for_terminal`（阻塞直到终态）。 |

### 成功响应 — `200 OK`

```json
{
  "slug": "maya.a1b2c3d4.create_sphere",
  "output": { "sphere_id": "pSphere1" },
  "validation_skipped": false,
  "request_id": "req-7f3c..."
}
```

`slug` 字段原样回显调用方用过的 slug，便于批量 dispatch 时做关联。

### 错误响应 —— 结构化、kebab-case

```json
{
  "kind": "unknown-slug",
  "message": "no action registered for slug 'maya.a1b2c3d4.make_sphere'",
  "hint": "call /v1/search to list available tools",
  "request_id": "req-7f3c...",
  "candidates": ["maya.a1b2c3d4.create_sphere"]
}
```

错误 kind 词汇表（括号里是 HTTP 状态码）：

- `unknown-slug` (404) —— 找不到匹配的 action；`candidates` 可能带建议 slug。
- `ambiguous` (409) —— slug 匹配多个 action；`candidates` 列出全部。
- `skill-not-loaded` (409) —— slug 有效但拥有者 skill 未加载。先调 `load_skill`。
- `invalid-params` (400) —— JSON-Schema 校验 `params` 失败。
- `unauthorized` (401) —— `AuthGate` 拒绝。per-DCC 默认仅本机；远程访问需装 `BearerTokenGate`。
- `not-ready` (503) —— `/v1/readyz` 红灯；DCC 还在启动中。
- `affinity-violation` (409) —— 从 worker 线程调用了主线程独占的工具。
- `bad-request` (400) —— envelope 有误（缺 `tool_slug`、JSON 坏、等）。
- `backend-error` (502) —— 拥有者 DCC 进程响应了但工具失败。
- `instance-offline` (503) —— **仅网关** —— `<id8>` 对应的实例已不在线。
- `schema-unavailable` (502) —— **仅网关** —— 拥有者 DCC 在 discovery 和 call 之间失联。
- `internal` (500) —— REST 层自身失败；查服务端日志。

### Request ID

每个请求都有 `request_id`（客户端 `X-Request-Id` 头优先，否则服务端生成）。
它会流进审计日志、响应 envelope，以及网关 MCP 响应的 `_meta.request_id`，
让 MCP 和 REST 调用方可以追踪同一工作单元。

---

## 三态就绪（`GET /v1/readyz`）

| 状态 | HTTP | Body | 含义 |
|---|---|---|---|
| `Ready` | 200 | `{"status": "ready", "process": "ok", "dispatcher": "ok", "dcc": "ok"}` | 所有就绪位都绿了；`POST /v1/call` 会派发。 |
| `Booting` | 503 | `{"status": "booting", ...哪些位红}` | 服务在线但 DCC 主机 / 派发器还没完成初始化。网关保留该实例的注册表行但不路由流量。 |
| `Unreachable` | 无响应 | — | 5 秒内没应答。网关会回退试 `GET /health`（#660 前的后端兼容）；还没响应 → 算 probe 失败，连续 3 次后从注册表中剔除。 |

区分很重要：agent 看到"我的工具不见了"时，诊断路径和修复手段完全不同。
监控面板上把 `process` / `dispatcher` / `dcc` 分列展示。

---

## 与 MCP 的 envelope 对齐

| 关注点 | MCP `call_tool`（JSON-RPC） | REST `POST /v1/call` | 一致？ |
|---|---|---|---|
| 成功体 | `result.content[].text` (JSON 字符串) | `{slug, output, validation_skipped, request_id}` | ✅ 同一 `CallOutcome` |
| 失败体 | `result.content[].text` + `isError: true` | `{kind, message, hint?, request_id, candidates?}` | ✅ 同一 `ServiceError`；MCP 再包一层 `CallToolResult` |
| `request_id` | `_meta.request_id` | 顶层字段 | ✅ 相同值 |
| 取消 | `notifications/cancelled` | 关 HTTP 连接 | ✅ 都触发合作式取消 |
| 进度事件 | `notifications/progress` 通过 `_meta.progressToken` 绑定 | 长轮询 SSE（路线图 #604） | ⚠ 目前仅 MCP 支持 |

契约由 `crates/dcc-mcp-skill-rest/src/openapi.rs` 里的 OpenAPI snapshot
测试锁定（`call_request_schema_contract_is_stable` 与
`call_outcome_schema_contract_is_stable`）。任何对这两个测试的修改都标志着
下游可见的 envelope 破坏。

---

## 什么时候选 REST、什么时候选 MCP

| 你是 … | 选 |
|---|---|
| 写 **AI Agent**（Claude、Cursor、ChatGPT 桌面版、自研） | **MCP**。用网关的 `search_tools` / `describe_tool` / `call_tool`；拿到流式事件、渐进式发现、MCP 能力注册表。 |
| 写 **cURL 脚本** / cron / CI 流水线 | **REST**。纯 HTTP + JSON，不需要 MCP 库。 |
| 写对接多个 DCC 的**企业后端** | **网关上的 REST**。单一端点、跨 DCC 一致 envelope、OpenAPI 文档供代码生成。 |
| 写**宿主内插件**（Maya 插件、Blender add-on） | 两者都不是 —— 直接调 `DccServerBase.register_*`。REST / MCP 是给外部调用方的。 |
| 调试"我的工具为啥没跑？" | **优先 REST**：`curl /v1/healthz` → `/v1/readyz` → `/v1/search`。三个端点给你一条从"进程活着吗"到"我的工具能被发现吗"的直线。 |

---

## 可插拔 trait（给嵌入方）

REST 面板由 5 个 trait 组成，每一个都可替换。默认开箱即用（仅本机开发），
生产部署只替换自己关心的那个。

| Trait | 默认实现 | 常见替换 |
|---|---|---|
| `SkillCatalogSource` | 真实 `SkillCatalog` | 测试夹具；对远程 registry 的只读缓存。 |
| `ToolInvoker` | 基于 `ToolDispatcher` 的 `DispatcherInvoker` | 排队投递到 DCC 主线程的队列型 invoker。 |
| `AuthGate` | `AllowLocalhostGate` | 远程访问用 `BearerTokenGate::new(vec![token])`；企业用 SSO gate。 |
| `AuditSink` | `NoopSink` | 追加 JSONL 的 `FileAuditSink`；中央审计的 Kafka producer。 |
| `ReadinessProbe` | 静态 `Ready` | 与 DCC 主机的真实就绪信号挂钩，场景加载中变红。 |

这就是 REST 面板的 DIP 实现：handler 依赖 trait 而非具体类型，插入自定义
auth / audit / invocation 永远不用碰 `handle_call`。

---

## 相关阅读

- [网关争用与调试](gateway-diagnostics.md) —— 如何读懂竞争、探针、选举、ghost 剔除。
- [CLI 参考](cli-reference.md) —— 启动 per-DCC 服务、tunnel relay + agent。
- [AGENTS.md](https://github.com/loonghao/dcc-mcp-core/blob/main/AGENTS.md) —— AI Agent 接入规则。
