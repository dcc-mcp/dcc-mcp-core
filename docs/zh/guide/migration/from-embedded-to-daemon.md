# 从内嵌自动网关迁移到独立守护进程

> **[English](../../../guide/migration/from-embedded-to-daemon)**

本指南介绍如何把"零配置单工作站（内嵌自动网关）"迁移到 epic
[#1367][epic] 引入的多进程 / 多机拓扑之一。默认零配置流程**不会**被移除，
本文里每一步都是可选的。

[epic]: https://github.com/loonghao/dcc-mcp-core/issues/1367

## 什么时候迁移？

继续用内嵌自动网关：

- 只在一台工作站上跑 DCC 工具，并且
- 所有 DCC 进程（Maya、Blender、Houdini 等）以同一个 OS 用户启动并共享本机
  `FileRegistry`。

满足下列任意一条时迁移到独立网关守护进程：

- 多台工作站需要共用同一个网关 URL。
- 发起 MCP 调用的主机（CI runner、headless agent、农场调度器）本身**不**跑
  DCC。
- 某个 DCC 在 NAT / 防火墙后面，必须从 LAN 外部访问 —— 这需要 Phase 3 的
  relay-source 路径。
- 你希望网关在线时间独立于任意一个 DCC 的生命周期（Maya 重启不应该让正在
  使用网关的 agent 掉线）。

## 第 0 步 —— 留一份基线

改动前先把内嵌模式跑一次，记录现状以便对比 / 回滚：

```bash
# 用正常方式启动任意 DCC 适配器（例如 Maya），让内嵌自动网关绑定标准端口。
dcc-mcp-server --app maya

# 另一个终端列出网关看见的实例：
curl -s http://127.0.0.1:9765/v1/instances | jq '.by_source'
```

保存输出。迁移后 `by_source` 计数应该仍然合理（`file` 可能转为
`http` / `relay` / `mdns`，取决于你选择的拓扑）。

## 第 1 步 —— 让所有内嵌适配器停止竞争网关端口

第一个可逆改动：告诉每个 DCC 适配器**永远不要**抢网关端口。这样你就可以
带外管理网关守护进程，同时保留原本的 registry、scene、skill paths。

```bash
# 每个 DCC sidecar / 插件宿主的启动命令加新 flag：
dcc-mcp-server serve --no-auto-gateway --app maya
dcc-mcp-server serve --no-auto-gateway --app blender
```

`serve --no-auto-gateway` 是 `auto` 的严格子集 —— 它永远不会试图绑定
`--gateway-port`。DCC 仍然会向当前可达的网关注册（无论那个网关是内嵌在
其他适配器里、还是独立守护进程）。

回滚：去掉 `--no-auto-gateway`，适配器重新加入 first-wins 选举。

## 第 2 步 —— 启动独立网关守护进程

把网关作为独立进程跑。它的功能由 Cargo feature 控制（参见 #1359），
二进制体积可以精简：

```bash
dcc-mcp-server gateway \
    --host 127.0.0.1 \
    --port 9765 \
    --registry-dir /var/lib/dcc-mcp
```

守护进程**只**承载网关平面 —— 发现、路由、只读 admin UI、审计。它从不
inline 执行工具；每个 `tools/call` 都会被转发到拥有该工具的 DCC 后端。
完整契约见 [`gateway.md` § Standalone gateway daemon][standalone]。

[standalone]: ../gateway.md#standalone-gateway-daemon-1358

回滚：停掉守护进程。内嵌适配器会在下一轮选举 tick 探测到 sentinel 不
存在，**如果**它们没有 `--no-auto-gateway` 就会接管。具体观察方式见
[网关选举 → 故障转移诊断][failover-diag]。

[failover-diag]: ../gateway-election.md#故障转移诊断-issue-1355

## 第 3 步 —— 选择一种发现源

守护进程提供四种发现源，它们都汇聚到同一份 `gateway://instances`：

| 源 | 适用场景 | 配置位置 |
|----|----------|----------|
| `file` | 守护进程和 DCC 在同一台机器、同一个用户。 | 无需配置 —— `FileRegistry` 自动 |
| `http` | DCC 在另一台机器，能通过 HTTPS 访问守护进程。 | DCC sidecar: `POST /v1/instances/register` + heartbeat |
| `mdns` | 同一 LAN，零共享配置。 | `serve --advertise-mdns` + `gateway --discover-mdns`（需 `--features mdns` 构建） |
| `relay` | DCC 在 NAT / 防火墙后。 | `tunnel-agent` → `tunnel-relay`，再 `gateway --relay-source ADMIN=PUBLIC` |

冲突优先级（#1364）：`http > relay > mdns > file`。同一个 `instance_id`
最近一次断言的源胜出。

## 第 4 步 —— 给守护进程加锁

后端可以从本机信任域之外加入之后，就必须加 auth。基于 token 的注册和
`allowed_dcc` scope 强制由 [#1365][1365] 跟踪；落地之后，守护进程会要求
跨主机的每个源都带 bearer token。在那之前，守护进程**只能**部署在受信
LAN 上，或者放在能在前面终结 auth 的反向代理后面。

[1365]: https://github.com/loonghao/dcc-mcp-core/issues/1365

## 第 5 步 —— 验证并保留回滚方案

```bash
# 1. 守护进程是网关端口的唯一监听者。
curl -s http://127.0.0.1:9765/v1/health

# 2. 之前可见的所有 DCC 仍然在列表里，可能 source 字段变了。
curl -s http://127.0.0.1:9765/v1/instances | jq '.by_source'

# 3. 每个内嵌适配器的故障转移诊断上报
#    "failover_disabled_by_adapter"（因为加了 --no-auto-gateway）或
#    "gateway_port_not_configured" —— 都是稳定预期状态。
```

一键回滚：

```bash
# 停掉守护进程。
pkill -f "dcc-mcp-server gateway"

# 重启每个 DCC 适配器但去掉 --no-auto-gateway。第一个起来的实例赢得
# 网关端口，拓扑回到"单工作站内嵌自动网关"。
dcc-mcp-server --app maya
```

## 延伸阅读

- [`docs/guide/gateway.md`](../gateway.md) —— 运行模式参考、拓扑图、
  发现 payload 形状。
- [`docs/guide/gateway-election.md`](../gateway-election.md) ——
  故障转移状态机和诊断工具。
- [`docs/guide/tunnel-relay.md`](../tunnel-relay.md) —— NAT / 跨子网
  拓扑的 relay-source 配置。
- [`docs/guide/cli-reference.md`](../cli-reference.md) —— `auto` /
  `serve` / `gateway` 子命令 flag 详解。
