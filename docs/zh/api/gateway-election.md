# 网关选举 API

> **[English](../api/gateway-election.md)**

自动网关故障转移选举机制。基于 first-wins socket 选举策略，当主网关不可用时自动提升备用实例。适用于多实例 DCC 服务器的高可用部署场景。

**导出符号：** `DccGatewayElection`

## DccGatewayElection

自动网关故障转移选举器，通过 socket 抢占实现 first-wins 策略。

- `DccGatewayElection(dcc_name, server, gateway_host="127.0.0.1", gateway_port=9765, ..., on_promote=None)` — 创建选举器
- `.start()` — 启动后台选举线程
- `.stop()` — 优雅停止选举线程
- `.is_running -> bool` — 是否正在运行
- `.consecutive_failures -> int` — 连续失败次数
- `.get_status() -> dict` — 返回 `{running, consecutive_failures, gateway_host, gateway_port}`

## 环境变量

- `DCC_MCP_GATEWAY_PROBE_INTERVAL` — 探测间隔（默认 5 秒）
- `DCC_MCP_GATEWAY_PROBE_TIMEOUT` — 探测超时（默认 2 秒）
- `DCC_MCP_GATEWAY_PROBE_FAILURES` — 连续失败阈值（默认 3 次）

详见 [English API 参考](../api/gateway-election.md)。
