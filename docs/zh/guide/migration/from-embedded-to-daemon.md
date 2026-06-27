# 从 legacy 内嵌自动网关迁移到独立守护进程

> **[English](../../../guide/migration/from-embedded-to-daemon)**

本指南面向仍在使用内嵌 first-wins gateway 的旧部署，或显式带
`--legacy-gateway-election` 启动的适配器。当前 `dcc-mcp-server` 的
零配置启动已经是 daemon-backed：per-DCC 进程会确保机器级
`dcc-mcp-server gateway` daemon 已启动，然后把自己注册为 backend。只有在
淘汰 legacy 内嵌拓扑，或迁到独立托管 daemon / 多机发现源时，才需要下面的
步骤；这些拓扑来自 epic [#1367][epic]。

[epic]: https://github.com/dcc-mcp/dcc-mcp-core/issues/1367

## 什么时候迁移？

只有满足下列条件时，才建议继续使用 legacy 内嵌自动网关：

- 只在一台工作站上跑 DCC 工具，并且
- 所有 DCC 进程（Maya、Blender、Houdini 等）以同一个 OS 用户启动并共享本机
  `FileRegistry`，并且
- 你为了兼容旧 supervisor 主动传入 `--legacy-gateway-election`。

满足下列任意一条时迁移到独立网关守护进程：

- 多台工作站需要共用同一个网关 URL。
- 发起 MCP 调用的主机（CI runner、headless agent、农场调度器）本身**不**跑
  DCC。
- 某个 DCC 在 NAT / 防火墙后面，必须从 LAN 外部访问 —— 这需要 Phase 3 的
  relay-source 路径。
- 你希望网关在线时间独立于任意一个 DCC 的生命周期（Maya 重启不应该让正在
  使用网关的 agent 掉线）。

## 第 0 步 —— 留一份 legacy 基线

改动 legacy 部署前，先把内嵌模式跑一次，记录现状以便对比 / 回滚：

```bash
# 用 legacy 内嵌选举路径启动任意 DCC 适配器（例如 Maya）。
dcc-mcp-server --app maya --legacy-gateway-election

# 另一个终端列出网关看见的实例：
curl -s http://127.0.0.1:9765/v1/instances | jq '.by_source'
```

保存输出。迁移后 `by_source` 计数应该仍然合理（`file` 可能转为
`http` / `relay` / `mdns`，取决于你选择的拓扑）。

## 第 1 步 —— 让 legacy 适配器停止竞争网关端口

同一工作站部署最简单的迁移方式，是去掉 `--legacy-gateway-election`，直接
使用当前默认 `auto` 路径。每个 DCC 进程都会确保同一个独立 daemon 并注册
为 backend：

```bash
dcc-mcp-server --app maya
dcc-mcp-server --app blender
```

如果外部 supervisor 负责 daemon 生命周期，而 DCC 进程只需要发布
FileRegistry 行，可以禁用 adapter 侧的 daemon 拉起/guardian：

```bash
dcc-mcp-server --app maya --no-ensure-gateway
dcc-mcp-server --app blender --no-ensure-gateway
```

`serve --no-auto-gateway` 仍可用于 per-DCC-only 启动：它会设置
`gateway_port=0`，因此进程不会 ensure、guardian 或绑定网关端口。在启用
`gateway-auto` feature 的构建里，它仍会写 FileRegistry service row，独立
托管的同机 daemon 可以读取。

只有重新加上 `--legacy-gateway-election`，才会回滚到旧内嵌拓扑。

## 第 2 步 —— 启动独立网关守护进程

把网关作为独立进程跑。它的功能由 Cargo feature 控制（参见 #1359），
二进制体积可以精简：

```bash
dcc-mcp-server gateway \
    --host 127.0.0.1 \
    --port 9765 \
    --registry-dir /var/lib/dcc-mcp
```

守护进程**只**承载网关平面 —— 发现、路由、本地 admin UI、审计。它从不
inline 执行工具；每个 `tools/call` 都会被转发到拥有该工具的 DCC 后端。
完整契约见 [`gateway.md` § Standalone gateway daemon][standalone]。

[standalone]: ../gateway.md#standalone-gateway-daemon-1358

回滚：停掉守护进程，并用 `--legacy-gateway-election` 重启适配器。具体观察
方式见 [网关选举 → 故障转移诊断][failover-diag]。

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

[1365]: https://github.com/dcc-mcp/dcc-mcp-core/issues/1365

## 第 5 步 —— 验证并保留回滚方案

```bash
# 1. 守护进程是网关端口的唯一监听者。
curl -s http://127.0.0.1:9765/v1/health

# 2. 之前可见的所有 DCC 仍然在列表里，可能 source 字段变了。
curl -s http://127.0.0.1:9765/v1/instances | jq '.by_source'

# 3. Gateway metadata 上报预期模式：
#    默认 auto-launch 是 "daemon-backed"；
#    --no-ensure-gateway 是 "failover_disabled_by_adapter"；
#    --no-auto-gateway / gateway_port=0 是 "gateway_port_not_configured"。
```

一键回滚：

```bash
# 停掉守护进程。
pkill -f "dcc-mcp-server gateway"

# 用 legacy 选举 flag 重启每个 DCC 适配器。
dcc-mcp-server --app maya --legacy-gateway-election
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
