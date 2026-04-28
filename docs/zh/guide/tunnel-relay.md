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

## 前端客户端线协议

在 HTTP / WSS 前端落地前（PR 4 of #504），远端客户端使用普通 TCP，
首个有效载荷必须是 2 字节大端长度前缀的 tunnel id：

```text
[u16 BE: tunnel_id_len][tunnel_id_bytes][... 后续 MCP 流量 ...]
```

测试 / SDK 可直接调用：

```rust
use dcc_mcp_tunnel_relay::data::write_select_tunnel;
write_select_tunnel(&mut tcp_stream, &tunnel_id).await?;
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

## MVP 之外的能力

| 能力 | 状态 |
|---|---|
| 纯 TCP 传输（agent + 前端） | ✅ MVP |
| JWT 鉴权 + DCC 作用域 | ✅ MVP |
| 单 tunnel 1:N 会话多路复用 | ✅ MVP |
| 周期性失活剔除 | ✅ MVP |
| WebSocket Secure 传输（浏览器友好） | 后续 |
| HTTP+SSE 前端 + `/dcc/<name>/<tunnel_id>` 路由 | 后续 |
| `/tunnels` 列表端点 + 管理指标 | 后续 |
| Agent 退避重连 | 后续 |

MVP 已能端到端跑通协议，并满足 #504 的验收条件 1-5。

## 相关

- `crates/dcc-mcp-tunnel-relay/tests/e2e.rs` —— 可运行示例，覆盖
  成功的端到端往返与 token 拒绝路径。
- [`docs/guide/agents-reference.md`](agents-reference.md) —— 陷阱清单
  与约定。
