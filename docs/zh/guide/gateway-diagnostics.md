# 网关争用与调试

在同一工作站上跑多个 `dcc-mcp-server`（或把网关部署跨多 pod 扩展）时，它们
会竞争网关角色、维护共享服务注册表、互相探活、清理死节点。这一页是运维
playbook：每个机制如何在日志 / 指标 / MCP 工具里暴露，以及如何调试五种
常见故障模式。

---

## 状态矩阵

网关按 `ServiceStatus` 对实例分类。运维通过 `gateway://instances` MCP 资源、
`GET /v1/readyz`、`/metrics` Prometheus 导出看到这些值。

| 状态 | 含义 | 谁设置 | 如何恢复 |
|---|---|---|---|
| `Available` / `ok` | 就绪位全绿；实例可路由。 | per-DCC `ReadinessProbe` 返回 `ready`。 | — |
| `Booting` | DCC 主机在线但至少一个就绪位红（进程上、派发器未就绪、或 DCC 未就绪）。 | `probe_mcp_readiness` 解码 `GET /v1/readyz → 503`。 | 等待；瞬态。网关**保留**注册表行避免抖动，但不路由流量。 |
| `Unreachable` | 网关的 TCP 探针连 `/v1/readyz` 和 `/health` 都拿不到响应。 | 网关健康检查循环，连续 2 次 miss 后。 | 检查 DCC 进程；连续 3 次 miss 后该行会被自动剔除。 |
| `ShuttingDown` | 实例已调 `deregister`，正在收尾活动 session。 | 优雅关闭流程。 | 等它消失。 |
| `stale`（仅展示态） | `last_heartbeat` 早于 `stale_timeout`。 | 剔除扫描器。 | 下一轮扫描会移除；如果长期 stale，进程可能崩了但没 deregister。只有明白原因再调 `DCC_MCP_STALE_TIMEOUT`。 |
| `ghost`（内部） | 没有任何进程持有 sentinel lock / PID 文件。 | 每次读取时的 `FileRegistry::read_alive`。 | 自动剔除，无需动作。 |

---

## 流量捕获

本地调试 agent / skill 时，可以用 quick JSONL 模式启动网关：

```bash
DCC_MCP_TRAFFIC_CAPTURE=jsonl:./capture.jsonl dcc-mcp-server ...
```

JSONL 文件会收到 `traffic.frame` EventBus envelope，覆盖 `tools/call` 经过
网关时的 client-to-gateway、gateway-to-client，以及 gateway-to-adapter
`/v1/call` 转发帧。该功能默认关闭；当 `DCC_MCP_PROD_PROFILE=1` 时，除非再
设置 `DCC_MCP_FORCE_TRAFFIC_CAPTURE=1`，否则网关会拒绝启用捕获。

需要后续 replay / diff 的调试会话，用 YAML 配置：

```yaml
enabled: true
sinks:
  - kind: sqlite
    path: ./captures/run-${TIMESTAMP}.db
  - kind: admin_live
    ring_buffer: 500
filters:
  include:
    - mcp.method: tools/call
  exclude:
    - http.url: "*/v1/readyz"
redact:
  - body.data.params.arguments.api_key: "[REDACTED]"
```

通过 `DCC_MCP_TRAFFIC_CONFIG=./traffic_capture.yaml` 启动。include 规则按
OR 处理，exclude 优先于 include，字符串字段支持简单 `*` 通配符。redact 路径
是 frame attributes 下的精确 JSON path，并且会在 JSONL / SQLite 写入前应用；
被改写的路径记录在 `attributes.body.redacted_paths`。

可选的 `admin_live` sink 会保留一个有界内存环形缓冲区，供 Admin Traffic
面板和稳定 debug API 查看。通过 `GET /admin/api/traffic` 或
`GET /v1/debug/traffic` 检查，使用 `/admin/api/traffic/export` 或
`/v1/debug/traffic/export` 将保留窗口导出为 JSONL。

捕获内容可能包含 prompt、工具参数、场景路径和结果 payload；把它当成本地调试
产物，而不是生产审计日志。

修改 skill、prompt 或 routing policy 之后，可以把记录的 session 重放到在线
gateway：

```bash
dcc-mcp-server capture replay ./captures/run.sqlite \
    --target http://127.0.0.1:9765/mcp \
    --session sess_01HQX \
    --assert outputs-compatible
```

验证 prompt 或 skill 改动是否改变了可观测 traffic 时，可以比较两份 capture：

```bash
dcc-mcp-server capture diff ./captures/before.sqlite ./captures/after.sqlite \
    --before-session sess_before \
    --after-session sess_after
```

只有完全确定性的 fixture 才建议用 `outputs-equal`。真实 DCC 运行通常用
`outputs-compatible`，只要求 status 与 JSON-RPC result/error 形状一致。

---

## 选举（三级比较）

同一时刻只有一个进程能绑定网关端口，其他等待。当更新/更好的适配器出现时，
当前网关合作式让位（#718）。比较按顺序走三级：

1. **`crate_version`** —— 二进制编译时的 `dcc_mcp_core` 版本。0.14.28 的
   挑战者击败 0.14.17 的在任。
2. **`adapter_version`** —— 一级并列时的次级 tiebreaker。真实 DCC 适配器
   （`dcc_mcp_maya 0.3.0`）击败没有 adapter_version 的在任。
3. **`adapter_dcc`** —— 二级并列时的末级 tiebreaker。真实 DCC
   （`adapter_dcc = "maya"`）击败通用独立服务（`adapter_dcc = "unknown"`
   或未设）。

字段都在 `FileRegistry` 的 `__gateway__` sentinel 行上。读取
`gateway://instances` 即可查看：

```jsonc
{
  "dcc_type": "__gateway__",     // sentinel 行，永不可路由
  "version": "0.14.28",          // crate_version
  "adapter_version": "0.3.0",    // adapter_version
  "adapter_dcc": "maya",         // adapter_dcc
  "host": "127.0.0.1",
  "port": 9765
}
```

### 日志会看到什么

| 事件 | 模板 | 级别 |
|---|---|---|
| 赢家绑定端口 | `Won gateway election`（带 `version`） | `INFO` |
| 挑战者等待 | `Challenger: port still taken (attempt N/M)` | `DEBUG` |
| 让位接受 | `Cooperative yield accepted — waiting for port to free up` | `INFO` |
| 可选让位 fallback | `Cooperative yield optional capability unavailable (...) — polling for port` | `DEBUG` |
| 同版本/旧版本挑战者跳过让位探测 | `Skipping cooperative yield probe because challenger is not newer than the current gateway` | `DEBUG` |
| 检测到更新的 sentinel | `Gateway: newer-version sentinel detected — initiating voluntary yield` | `INFO` |

---

## 心跳、过期、ghost 剔除

三种互补的活跃检测机制：

1. **心跳**（`--heartbeat-secs`，默认 5）—— 每个实例按间隔 `touch` 自己
   的行。`flush_to_file` 用原子 temp-file + rename，并发读者永远看不到
   写到一半的行（#554）。Windows 下用 `LockFileEx` 保护读写重叠。

2. **过期扫描**（`--stale-timeout-secs`，默认 30）—— `last_heartbeat` 早于
   超时的行以 `status: "stale"` 展示，下一轮扫描剔除。

3. **Ghost 剔除**（#748 + #719）—— 每次 `read_alive()` 都探活拥有者进程：
   要么 sentinel lock 文件可被获取（意味着上个持有者已死），要么 `sysinfo`
   报告 `pid` 不再运行。没有 `pid` 字段的行保持存活（失败开放契约 #227）。

### 日志会看到什么

| 模板 | 级别 | 何时 |
|---|---|---|
| `registering service`（带 `dcc_type`、`instance_id`、`host`、`port`） | `INFO` | 实例注册。 |
| `deregistered service` | `INFO` | 优雅关闭。 |
| `removed stale service` | `INFO` | 过期扫描剔除。 |
| `removed ghost entry (owner sentinel/PID is dead)` | `INFO` | 拥有者进程崩溃未 deregister。 |
| `FileRegistry hot-reloaded from disk` | `TRACE` | mtime 触发的重载。 |
| `Gateway: evicted N stale instance(s)` | `INFO` | 周期扫描。 |
| `Gateway: reaped N ghost entry/entries` | `INFO` | 周期扫描。 |
| `Gateway: pre-subscribe dead-PID sweep reaped ...` | `INFO` | 启动期清理（#556）。 |

---

## TCP 探针循环

网关每 30 秒用 `GET /v1/readyz`（5 秒超时）探活每个活实例，pre-#660 后端
回退到 `GET /health`。结果映射到 `ProbeOutcome::{Ready, Booting, Unreachable}`。

失败升级路径：

- **1 次失败** —— WARN `Health check failed`，`consecutive_failures=1`。
- **2 次失败** —— 该行标 `Unreachable` 并从扇出中过滤。
- **3 次失败** —— 自动剔除；INFO `Auto-deregistered after 3 consecutive health-check failures`。

启动探针：网关订阅后端之前，先以 3 秒超时 TCP 连每个注册过的实例，把
不可达的剔掉（避免在重启后残留的注册行上浪费重连预算）。

---

## Prometheus 指标

`cargo build --features prometheus` 构建后挂载 `GET /metrics`。指标每 5 秒
刷新：

- `dcc_mcp_instances_total{status="active"}` —— `Available` 行计数。
- `dcc_mcp_instances_total{status="stale"}` —— 过期行计数。
- `dcc_mcp_tools_total{dcc_type="maya"}` —— 每 DCC 可见工具计数。
- `dcc_mcp_request_duration_seconds` —— 请求延迟直方图。
- `dcc_mcp_requests_failed_total{method, error}` —— 按方法的失败计数。

---

## 故障排查 recipes

### 场景 1 —— "一个 DCC 服务从 `tools/list` 里消失了"

记住：网关 `tools/list` 只含只读发现基础工具。per-tool 工具在 MCP
`search` / `describe` 与 REST `/v1/call` 后面。消失的大概率是**实例**，
不是它的工具。

```bash
# 通过网关原生 MCP 资源（任何 MCP 客户端都能跑）
# → 返回每一行及其状态；每条记录已携带 `mcp_url`。
resources/read uri=gateway://instances
# 可选 URI 查询：gateway://instances?include_dead=true 可以看到
# 拥有进程已退出的行。

# 通过网关 REST
curl -s http://127.0.0.1:9765/v1/context | jq .
```

按状态诊断：

- `stale` → 心跳早于 `stale_timeout`。大概率进程死了。
- `booting` → 该实例的 `GET /v1/readyz` 返回 503。DCC 主机还在启动。
- `unreachable` → 探针失败。查实例自己的日志；连续 3 次 miss 后自动剔除。
- 根本不在列表里 → 进程从未注册。检查 `DCC_MCP_REGISTRY_DIR` 和
  `FileRegistry` 权限。

### 场景 2 —— Ghost 行始终不 deregister

```bash
# 列出所有行，包括网关过滤掉的：
resources/read uri=gateway://instances?include_dead=true
```

如果看到 `pid` 指向一个早已死掉的进程，sentinel lock 文件应该在进程退出时
释放，下一次 `read_alive` 会自动剔除。强制一次：重启网关（启动探针会调
`read_alive`）。如果还不剔，查 `DCC_MCP_REGISTRY_DIR` 下的 `locks/` 目录 ——
残留的 `<dcc_type>-<instance_id>.lock`（拥有者已死但没能解锁）通常是
Windows 句柄卡了；手动删除 lock 文件 + `services.json` 里的行是安全的。

### 场景 3 —— `tools/call` 返回 "Unknown gateway tool"

**PR A 后**网关不再通过 `tools/list` 暴露 per-tool 工具。网关不识别的
任何工具名 —— 包括 backend-qualified `<skill>__<action>` /
`i_<id8>__<escaped>` / `<id8>__<tool>` 形式 —— 现在都返回重定向消息：

> Unknown gateway tool 'X'. The gateway MCP surface is intentionally
> minimal — it only exposes search, describe, load_skill, and call. Use
> `search` to find backend capabilities and `describe` to get a schema,
> then invoke one by slug through MCP `call` or REST `POST /v1/call`.

修复：调用方改用新流程 —— MCP `search` → `describe` → `call` +
`tool_slug`。

### 场景 4 —— 网关把我的服务剔了但它还在跑

TCP 探针连 miss 3 次。根因按概率排序：

1. **防火墙** —— 网关主机真的能到那个实例的 `mcp_port` 吗？从网关主机
   `curl -s http://<host>:<port>/v1/readyz`。
2. **探针超时太紧** —— 默认 5 秒。场景打开时 HTTP 线程阻塞就会 miss。
   要么让 `/v1/readyz` 保持便宜、非阻塞（默认就这样），要么调大探针间隔。
3. **端点不对** —— pre-#660 服务只响应 `GET /health`。网关自动回退；
   如果你把 health 路径改到别处，改回来。

根因解决后，实例下一次心跳周期会自动重新注册（无需手工介入）。

### 场景 5 —— 选举抖动 / 两个实例认领同一个 DCC

发生在两个进程注册了相同的 `dcc_type` 但 `instance_id` 不同。网关保持
它们独立（tool slug 的 `<id8>` 前缀消除歧义）—— 这是设计，不是 bug。
**不是**设计的是同一 `(host, port)` 有两行 —— 这意味着两个进程绑定了
同一端口，不该发生。检查：

- 崩溃后重启的进程，旧行成了 ghost —— 等 `read_alive` 剔除（通常 30 秒内）。
- autostart 配错导致同一 DCC 启了两遍。

选举本身是合作式：当前网关在更新 sentinel 出现时让位，不抢。如果看到
`__gateway__` sentinel 行的 version 字段在抖动，查系统时钟漂移（两台机器
互相声称比对方"更新"几乎都是时间同步问题）。

---

## 调试 recipes 速查表

```bash
# 列出所有已知实例，活的死的都有。
curl -s http://127.0.0.1:9765/mcp \
     -H 'content-type: application/json' \
     -d '{"jsonrpc":"2.0","id":1,"method":"resources/read","params":{"uri":"gateway://instances?include_dead=true"}}' \
  | jq .

# 手动探一个实例。
curl -v http://127.0.0.1:18812/v1/readyz

# 查网关自己的指标（需要 prometheus 特性）。
curl -s http://127.0.0.1:9765/metrics | grep dcc_mcp_

# 查磁盘上的注册表。
ls -la "$DCC_MCP_REGISTRY_DIR"
cat "$DCC_MCP_REGISTRY_DIR/services.json" | jq .

# 跟踪网关日志。
tail -F "$DCC_MCP_LOG_DIR/dcc-mcp.*.log" | grep -E 'Gateway|ghost|stale|Health'
```

---

## 相关阅读

- [REST API 面板](rest-api-surface.md) —— `/v1/readyz`、错误 kind、envelope 对齐。
- [CLI 参考](cli-reference.md) —— `dcc-mcp-server` 的每个旗标和环境变量。
- [AGENTS.md](https://github.com/dcc-mcp/dcc-mcp-core/blob/main/AGENTS.md) —— 公共 API 完整决策表。
