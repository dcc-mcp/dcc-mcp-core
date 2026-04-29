# 远程 MCP 隧道中继

零配置打通工作站本地的 DCC MCP 服务到外部网络，无需在工作站上开放
入站防火墙端口。对应 issue #504。

## 何时使用

当需要把本地 DCC（Maya / Blender / Houdini …）的 MCP 服务暴露给云端
LLM、跨主机的编排器或 Web 客户端时使用。同一局域网内仍优先用
`McpHttpConfig(host="0.0.0.0", port=8765)`，少一跳更省事。

## 架构

```
┌──────────────┐      WSS / TCP       ┌────────────┐      TCP      ┌─────────────┐
│ 本地 Agent   │ ───── 注册 ────────► │ 中继       │ ◄─── 选择隧道 │ 远端 MCP    │
│ （DCC 内）   │ ◄─── OpenSession ─── │ （公网）   │     id        │ 客户端      │
│              │ ◄─── Data ─────────► │ 注册表+清扫 │ ────────────► │             │
│              │ ──── 心跳 ──────────► │            │               │             │
└──────┬───────┘                       └────────────┘               └─────────────┘
       │ TCP
       ▼
┌──────────────┐
│ DCC 本地 MCP │
│ HTTP 服务    │
└──────────────┘
```

三个 crate 协作：

| Crate | 角色 |
|---|---|
| `dcc-mcp-tunnel-protocol` | 帧格式（msgpack）、JWT 鉴权、编解码 |
| `dcc-mcp-tunnel-relay` | 公网入口：agent + 前端监听器、注册表、清扫器 |
| `dcc-mcp-tunnel-agent` | 本地边车：注册并把每会话字节多路复用到本地 DCC |

## 端到端最小示例（Rust）

```rust
use std::time::Duration;
use dcc_mcp_tunnel_protocol::{auth, TunnelClaims};
use dcc_mcp_tunnel_relay::{RelayConfig, RelayServer};
use dcc_mcp_tunnel_agent::{AgentConfig, run_once};

# async fn demo() -> anyhow::Result<()> {
let secret = b"swap-me-with-a-real-32B-secret-please";

// 1. 运维在公网 VM 上拉起中继。
let relay = RelayServer::start(
    RelayConfig {
        jwt_secret: secret.to_vec(),
        public_host: "relay.example.com".into(),
        base_url: "wss://relay.example.com".into(),
        stale_timeout: Duration::from_secs(45),
        max_tunnels: 0,
    },
    "0.0.0.0:9001".parse()?, // agent 监听端口
    "0.0.0.0:9002".parse()?, // 前端监听端口
).await?;

// 2. 运维签发 per-DCC JWT 并下发到工作站。
let token = auth::issue(&TunnelClaims {
    sub: "studio-bob".into(), iat: 0, exp: u64::MAX,
    iss: "studio-issuer".into(),
    allowed_dcc: vec!["maya".into()],
}, secret)?;

// 3. Agent 在 DCC 进程（或同侧 sidecar）运行。
let cfg = AgentConfig::new(
    "relay.example.com:9001",
    &token,
    "maya",
    "127.0.0.1:8765", // 本地 DCC MCP HTTP 服务
);
let registered = run_once(cfg).await?;
println!("公网 URL: {:?}", registered.public_url);
# Ok(()) }
```

## 前端传输

远端客户端有两种方式接入已注册的隧道。

### 1. 纯 TCP（原始字节流）

连接中继的前端端口，首个有效载荷必须是 2 字节大端长度前缀的
tunnel id，之后开始交换原始 MCP 流量：

```text
[u16 BE: tunnel_id_len][tunnel_id_bytes][... 后续 MCP 流量 ...]
```

测试 / SDK 可直接调用：

```rust
use dcc_mcp_tunnel_relay::data::write_select_tunnel;
write_select_tunnel(&mut tcp_stream, &tunnel_id).await?;
```

### 2. WebSocket（浏览器与代理友好）

在中继上填入
[`OptionalBinds::ws_frontend`](https://docs.rs/dcc-mcp-tunnel-relay)
即可启用 WS 前端，连接到：

```text
ws://<host>:<ws_port>/tunnel/<tunnel_id>
```

每个二进制 WS 消息映射为一份 MCP 载荷（双向）。Text 帧会被忽略
（线协议是二进制）。如需 TLS，请在反向代理（`nginx` / `caddy` / 云
负载均衡）上终结 —— 中继本身只跑明文 HTTP/1.1，跟 `dcc-mcp-http` 的
部署模式一致。

## 管理端点 (`/tunnels`)

填入 [`OptionalBinds::admin`](https://docs.rs/dcc-mcp-tunnel-relay)
后中继会在独立端口上暴露只读 HTTP：

| 路径 | 返回 |
|---|---|
| `GET /tunnels` | JSON 数组，每条对应一条活跃隧道（[`TunnelSummary`]） |
| `GET /healthz` | 进程存活时返回 `200 OK` `"ok"` |

示例：

```bash
curl -s http://relay.example.com:9003/tunnels | jq
# [{
#   "tunnel_id": "01J…",
#   "dcc": "maya",
#   "capabilities": ["scene.read"],
#   "agent_version": "dcc-mcp-tunnel-agent/0.14",
#   "registered_at_ms_ago": 31204,
#   "last_heartbeat_ms_ago": 1450,
#   "session_count": 2
# }]
```

接口完全只读；由于会泄露活跃 tunnel id 列表，请用防火墙限制到
运维内网。

## Agent 重连与退避

长连接 agent 请用
[`run_with_reconnect`](https://docs.rs/dcc-mcp-tunnel-agent) 替代
[`run_once`]，它会遵循
[`AgentConfig::reconnect`](https://docs.rs/dcc-mcp-tunnel-agent)
策略（默认指数退避：1 s → 60 s 翻倍）。注册一次成功就把延迟重置回
`initial`；若中继返回 `RegisterAck { ok: false }`，会立即返回
[`ReconnectExit::Fatal`] 不再重试，从而让 JWT 配错的情况快速失败。

```rust
use tokio::sync::watch;
let (shutdown_tx, shutdown_rx) = watch::channel(false);
let task = tokio::spawn(dcc_mcp_tunnel_agent::run_with_reconnect(cfg, shutdown_rx));
// ... 之后 ...
shutdown_tx.send(true)?;
let _ = task.await;
```

## 鉴权与作用域

每个 agent 注册都携带一份用 `RelayConfig::jwt_secret` 签发的 JWT。
Token 中的 `allowed_dcc` 字段**限定**该 agent 可以注册的 DCC 标签 ——
不匹配时中继返回 `ErrorCode::DccNotAllowed`。
JWT 应短期签发（分钟到小时级），轮换由运维处理。

## 失活隧道剔除

中继默认每 15 秒扫描一次注册表。任何上次心跳早于
`RelayConfig::stale_timeout` 的隧道都会被丢弃 —— 出站队列随之关闭，
per-tunnel 任务被回收。被剔除隧道上的活跃会话会感受到 TCP RST。

## 能力矩阵

| 能力 | 状态 |
|---|---|
| 纯 TCP 传输（agent + 前端） | ✅ 已上线 |
| JWT 鉴权 + DCC 作用域 | ✅ 已上线 |
| 单 tunnel 1:N 会话多路复用 | ✅ 已上线 |
| 周期性失活剔除 | ✅ 已上线 |
| WebSocket 前端（`ws://` —— 配合反代 TLS 即可得到 `wss://`） | ✅ 已上线 |
| `/tunnels` + `/healthz` 管理端点 | ✅ 已上线 |
| Agent 退避重连 | ✅ 已上线 |
| HTTP+SSE 前端 + `/dcc/<name>/<tunnel_id>` 路由 | 后续 |
| 中继自身内置 TLS 终结 | 后续 —— 让运维处理 |
| 生产级基准（与直连 TCP 的 p50/p99 对比） | 后续 |

## 相关

- `crates/dcc-mcp-tunnel-relay/tests/e2e.rs` —— 可运行示例，覆盖
  成功的端到端往返与 token 拒绝路径。
- [`docs/guide/agents-reference.md`](agents-reference.md) —— 陷阱清单
  与约定。
