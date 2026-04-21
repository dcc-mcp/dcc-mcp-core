# 生产环境部署

> **[English](../../guide/production-deployment)**

本指南介绍如何在生产环境中部署独立的 `dcc-mcp-server` 二进制程序：
直接运行二进制、Docker 容器化、systemd 托管，以及在负载均衡器后运行
多副本网关以实现高可用。

关于网关选举协议本身（单个进程如何抢占知名端口）请参阅
[网关选举机制](gateway-election.md)。本页专注于**运维层面**：
如何安全地运行 N 个这样的进程。

## 何时阅读本文

- 你正在把 `dcc-mcp-server` 打包成 Docker 镜像或系统服务。
- 你需要多个网关副本在 nginx / ALB 后实现高可用。
- 你需要有据可查的 TLS、防火墙、日志和升级流程。

如果只是想在开发机上跑起来，请先看
[快速开始](getting-started.md)。

---

## 1. 二进制部署

### 编译二进制

二进制来自 Rust bin crate
[`crates/dcc-mcp-server/`](https://github.com/loonghao/dcc-mcp-core/tree/main/crates/dcc-mcp-server)。
除平台 libc 以外它是静态链接的，不依赖 Python 运行时。

```bash
# Release 构建（生产环境推荐）
cargo build --release --bin dcc-mcp-server

# 产物
# Linux / macOS: target/release/dcc-mcp-server
# Windows:       target\release\dcc-mcp-server.exe
```

从任意平台交叉编译到 Linux：

```bash
cargo install cross --locked
cross build --release --bin dcc-mcp-server --target x86_64-unknown-linux-gnu
```

### 安装路径

每台机器上选定一个**唯一**位置：

| 平台 | 建议路径 |
|------|---------|
| Linux（系统级） | `/usr/local/bin/dcc-mcp-server` |
| Linux（用户级） | `~/.local/bin/dcc-mcp-server` |
| 容器镜像 | `/usr/local/bin/dcc-mcp-server` |
| Windows | `C:\Program Files\dcc-mcp\dcc-mcp-server.exe` |

### 环境变量

二进制完全通过 CLI 参数或环境变量配置（参数优先）。只有下列变量是稳定
公开的，完整列表请执行 `dcc-mcp-server --help`。

| 变量 | 默认值 | 用途 |
|------|--------|------|
| `DCC_MCP_MCP_PORT` | `0`（OS 分配） | 实例的 MCP HTTP 端口 |
| `DCC_MCP_GATEWAY_PORT` | `9765` | 网关抢占的知名端口；`0` 表示禁用网关 |
| `DCC_MCP_REGISTRY_DIR` | 平台默认值 | 共享 `FileRegistry` 目录 —— 需要互相发现的副本**必须**配置成相同路径 |
| `DCC_MCP_STALE_TIMEOUT` | `30` | 超过多少秒没有心跳则视为实例已死 |
| `DCC_MCP_HEARTBEAT_INTERVAL` | `5` | 心跳间隔（秒） |
| `DCC_MCP_SERVER_NAME` | `dcc-mcp-server` | 向 MCP 客户端广告的服务名 |
| `DCC_MCP_DCC` | *（空）* | DCC 类型提示：`maya`、`blender`、`photoshop`…… |
| `DCC_MCP_DCC_VERSION` | — | 写入注册表条目 |
| `DCC_MCP_SCENE` | — | 当前场景文件名 |
| `DCC_MCP_SKILL_PATHS` | — | `:` / `;` 分隔的额外 skill 目录 |
| `DCC_MCP_LOG_FILE` | `false` | 在 stderr 之外启用滚动文件日志 |
| `DCC_MCP_LOG_DIR` | 平台默认 | 滚动日志的输出目录 |

### 冒烟测试

```bash
# 终端 1
dcc-mcp-server --dcc generic --mcp-port 18812

# 终端 2
curl -sf http://127.0.0.1:9765/health   # → {"ok":true}
curl -s http://127.0.0.1:9765/instances # → 已注册实例的 JSON 列表
```

---

## 2. Docker

### 多阶段镜像

完整示例见
[`examples/compose/gateway-ha/Dockerfile`](https://github.com/loonghao/dcc-mcp-core/tree/main/examples/compose/gateway-ha)。
示意：

```Dockerfile
# 阶段 1 —— 构建
FROM rust:1.85-slim AS builder
WORKDIR /src
COPY . .
RUN cargo build --release --bin dcc-mcp-server

# 阶段 2 —— 运行
FROM debian:12-slim
RUN useradd --system --uid 10001 --home-dir /var/lib/dcc-mcp dcc
COPY --from=builder /src/target/release/dcc-mcp-server /usr/local/bin/
USER dcc
EXPOSE 9765
ENTRYPOINT ["/usr/local/bin/dcc-mcp-server"]
```

单次构建与运行：

```bash
docker build -t dcc-mcp-server:latest -f examples/compose/gateway-ha/Dockerfile .
docker run --rm -p 9765:9765 dcc-mcp-server:latest \
  --dcc generic --host 0.0.0.0
```

### docker-compose 高可用

[`examples/compose/gateway-ha/docker-compose.yml`](https://github.com/loonghao/dcc-mcp-core/tree/main/examples/compose/gateway-ha)
启动**两个网关候选**（都抢占 `9765`，一个胜出、另一个作为后备普通实例，
可在故障时接管）以及**两个 mock DCC 服务器**，共享同一个 registry
卷。

```bash
cd examples/compose/gateway-ha
docker compose up -d
curl http://localhost:9765/health
curl http://localhost:9765/instances
docker compose down
```

---

## 3. systemd

在裸机或长期存在的 VM 上使用 systemd，让操作系统维持
`dcc-mcp-server` 的生命周期。标准 unit 文件在
[`examples/systemd/dcc-mcp-gateway.service`](https://github.com/loonghao/dcc-mcp-core/tree/main/examples/systemd)。

该 unit 启用了下列加固选项：

- `DynamicUser=true` —— 自动创建的非特权用户运行。
- `ProtectSystem=strict` + `ProtectHome=true` —— 只读 `/`、无 `/home`。
- `NoNewPrivileges=true` —— 进程 `exec` 后无法提权。
- `PrivateTmp=true`、`PrivateDevices=true`、`ProtectKernelTunables=true`。
- `StateDirectory=dcc-mcp` —— 可写的 `/var/lib/dcc-mcp`，存放注册表。
- `CapabilityBoundingSet=` —— 完全移除所有 capability。

安装与启用：

```bash
sudo install -m0644 examples/systemd/dcc-mcp-gateway.service \
  /etc/systemd/system/dcc-mcp-gateway.service
sudo systemctl daemon-reload
sudo systemctl enable --now dcc-mcp-gateway.service
systemctl status dcc-mcp-gateway.service
journalctl -u dcc-mcp-gateway.service -f
```

用 drop-in 文件覆盖单机配置（端口、skill 路径等）：

```bash
sudo systemctl edit dcc-mcp-gateway.service
# [Service]
# Environment=DCC_MCP_GATEWAY_PORT=9765
# Environment=DCC_MCP_REGISTRY_DIR=/var/lib/dcc-mcp/registry
# Environment=DCC_MCP_SKILL_PATHS=/opt/skills:/etc/skills
```

---

## 4. 负载均衡

### MCP 会话粘滞

MCP Streamable HTTP 传输（规范
[2025-03-26](https://modelcontextprotocol.io/specification/2025-03-26)）
在 HTTP 头 `Mcp-Session-Id` 中携带会话 ID。初始化响应中由服务端写入，
之后每一次 `POST /mcp` 与长连接 `GET /mcp` SSE 流都会回带它。所有带
有相同 `Mcp-Session-Id` 的请求**必须**路由到同一个网关副本，否则
SSE 事件将无法送达。

按 `Mcp-Session-Id` 做 hash，而不是按客户端 IP —— NAT 后的一个办公室
可能以同一个源 IP 出现却承载大量独立会话。

### nginx

```nginx
upstream dcc_mcp_gateways {
    hash $http_mcp_session_id consistent;
    server 10.0.0.11:9765 max_fails=2 fail_timeout=5s;
    server 10.0.0.12:9765 max_fails=2 fail_timeout=5s;
    keepalive 16;
}

server {
    listen 443 ssl http2;
    server_name mcp.example.com;

    ssl_certificate     /etc/ssl/mcp.example.com.crt;
    ssl_certificate_key /etc/ssl/mcp.example.com.key;

    location /mcp {
        proxy_pass http://dcc_mcp_gateways;
        proxy_http_version 1.1;
        proxy_set_header Host $host;
        proxy_set_header Mcp-Session-Id $http_mcp_session_id;

        # SSE：关闭缓冲，保持长连接
        proxy_buffering  off;
        proxy_cache      off;
        proxy_read_timeout 1h;
        proxy_send_timeout 1h;
        chunked_transfer_encoding on;
    }

    location = /health {
        proxy_pass http://dcc_mcp_gateways;
    }
}
```

### AWS Application Load Balancer

ALB 本身不支持按 header 做 hash，因此使用**应用级 Cookie 粘滞**，
以会话头作为锚：

1. Target group → Attributes → **Stickiness: enabled**，类型
   `app_cookie`，Cookie 名 `Mcp-Session-Id`，持续时间 `3600s`。
2. 健康检查路径 `/health`，响应码 `200`，间隔 `10s`，阈值 `2`。
3. `:443` 监听器终止 TLS，转发 HTTP 到后端 `:9765`。
4. Idle timeout ≥ `3600s`，避免 SSE 流被切断。

可选：在前面再挂一层 CloudFront 做 DDoS 防护、并缓存 `/health` 10s。

---

## 5. 网关高可用拓扑（#327）

这是 issue #327 最初规划、现已并入 #330 的拓扑。这是在保留单一公开
入口的前提下横向扩展网关吞吐的**唯一**方式。

```
                ┌──────────────────┐
   客户端  ───▶ │  LB (nginx/ALB)  │
                └──────────────────┘
                   │          │
                   ▼          ▼
         ┌───────────┐  ┌───────────┐
         │ gateway-a │  │ gateway-b │    dcc-mcp-server 副本
         └───────────┘  └───────────┘    （无状态，主要只读）
                 \            /
                  \          /
                ┌──────────────────┐
                │    共享注册表     │    NFS / EFS / S3-mountpoint
                │  (FileRegistry)  │    挂载路径 = DCC_MCP_REGISTRY_DIR
                └──────────────────┘
                   ▲          ▲
                   │          │
         ┌───────────┐  ┌───────────┐
         │ dcc-maya-1│  │ dcc-blender-1 │ DCC 实例自行注册
         └───────────┘  └───────────┘
```

### 共享 FileRegistry

每个副本**必须**把 `DCC_MCP_REGISTRY_DIR` 指向同一个 POSIX 目录，
文件系统需要正确实现 `fsync`：

- Kubernetes：`ReadWriteMany` PVC（CephFS、EFS CSI、Longhorn RWX）。
- 裸机：NFSv4 或 GlusterFS。
- AWS：通过 NFSv4.1 挂载 EFS。
- 类 S3 的对象存储**不可直接使用** —— 注册表依赖原子 rename 与目录
  列举，需要 POSIX 层；S3 Mountpoint 可用但慢，仍优先选 EFS。

### 选举与重复工具抑制

所有副本运行同一份选举代码，细节见
[`gateway-election.md`](gateway-election.md)：

1. 每个副本尝试在自己的 pod/容器 IP 上绑定
   `DCC_MCP_GATEWAY_PORT`。在单机 compose 下只有一个能成功。
2. 在 LB 拓扑下每个副本拥有独立 IP，因此都能绑定成功、都自认为是
   "本 pod 的网关"，LB 对客户端隐藏了这一点。
3. 所有副本读同一个 `FileRegistry`，因此每个副本看到的
   **DCC 实例集合和工具集合是一致的**。
4. 工具以 `{instance_short_id}__{tool}` 命名空间化，short id 从
   注册条目确定性推导，因此两个副本发布同一个 DCC 实例产生的工具
   名**完全一致**。MCP 客户端按名称去重，`tools/list` 中不会有重复。
5. `tools/list_changed` 通知由 SSE 所在的副本触发，由注册表文件变更
   事件驱动。

### 故障转移 SLA

目标：**pod 死亡到 LB 摘除 < 5 秒**。

- LB 健康检查间隔 `2s`、不健康阈值 `2` → ≤ `4s` 停止路由到死掉的
  副本。
- 已有的 SSE 流因为副本死亡被切断；客户端感知到断开并重连，LB
  会用 `Mcp-Session-Id` cookie/hash 把它送到一个健康副本。
- 任何活着的网关副本会在 `DCC_MCP_STALE_TIMEOUT`（默认 `30s`）
  后清理真正死掉的 DCC 注册条目；该逻辑与 LB 故障转移相互独立。

在"积极故障转移"的部署里可把
`DCC_MCP_HEARTBEAT_INTERVAL=2`、`DCC_MCP_STALE_TIMEOUT=10` 调低。
心跳不要低于 `1s`，否则共享注册表目录会被打爆。

---

## 6. 安全

- **绑定到私网**。二进制默认绑 `127.0.0.1`。容器内设 `--host 0.0.0.0`
  时请确认容器网络是私有的。
- **仅在边缘终止 TLS**。TLS 在 LB（nginx / ALB）终止。
  `dcc-mcp-server` 在 loopback / pod 网络上讲明文 HTTP，追求简单与
  性能。
- **防火墙**。只暴露 LB 的公网端口（443）。`9765` 和每个副本的
  `--mcp-port` 都不要暴露到公网。
- **认证**。MCP 规范定义了 OAuth 2.1 Bearer Token —— 如果入口伸到
  VPC 之外，请在 LB 上强制校验。
- **systemd 加固**（见第 3 节）加上 `DynamicUser=true`，即使进程被
  攻破也波及不到主机上的其它资产。
- **不要把 secret 塞进 env**。用文件挂载：systemd `LoadCredential=`
  或 Kubernetes Secret。

---

## 7. 监控

### 日志

- **systemd** —— `journalctl -u dcc-mcp-gateway.service -f`。
- **Docker / compose** —— `docker compose logs -f gateway-a`。
- **Kubernetes** —— `kubectl logs -f deploy/dcc-mcp-gateway`。
- **滚动文件日志** —— 设置 `DCC_MCP_LOG_FILE=true` 和
  `DCC_MCP_LOG_DIR=/var/log/dcc-mcp`，再用 promtail / fluentbit 收集。

### 健康与就绪探针

网关暴露 `GET /health`，返回 `{"ok":true}`、状态码 `200`。
liveness 和 readiness 都可以用它。

```yaml
readinessProbe:
  httpGet: { path: /health, port: 9765 }
  initialDelaySeconds: 2
  periodSeconds: 5
  failureThreshold: 2
livenessProbe:
  httpGet: { path: /health, port: 9765 }
  initialDelaySeconds: 10
  periodSeconds: 10
  failureThreshold: 3
```

> **说明**：当前没有 `/mcp/healthz` 端点 —— LB 友好的路径是
> `/health`。同时检查注册表可达性的 `/readyz` 作为后续工作追踪。

### 指标

Prometheus 抓取支持作为独立任务跟踪。在此之前请使用遥测组件
（`DCC_MCP_LOG_FILE=true` 加日志衍生指标），或从 LB 访问日志导出
请求计数。

---

## 8. 升级与回滚

借助 LB 的 draining 实现零停机。两副本在同一个 LB 后的步骤：

```bash
# 1. 摘除副本 A
#    nginx：从 upstream 中移除，reload
#    ALB：deregister target，等待 "draining" 完成
#
# 2. 用新版本二进制升级并重启副本 A
#
# 3. 探活
curl -sf http://<副本 A IP>:9765/health

# 4. 把副本 A 放回轮询
# 5. 对副本 B 重复
```

回滚用相同流程换成旧版本二进制。磁盘上的 registry 使用防御式版本
化（未知字段会被忽略），因此相邻小版本间滚动是安全的；跨主版本请
查阅 release notes。

---

## 延伸阅读

- [网关选举机制](gateway-election.md) —— 知名端口如何被抢占。
- [传输层](transport.md) —— DCC 进程间 IPC。
- [MCP 2025-03-26 规范](https://modelcontextprotocol.io/specification/2025-03-26) —— Streamable HTTP、`Mcp-Session-Id`。
- 示例工件：[`examples/compose/gateway-ha/`](https://github.com/loonghao/dcc-mcp-core/tree/main/examples/compose/gateway-ha)、[`examples/k8s/gateway-ha/`](https://github.com/loonghao/dcc-mcp-core/tree/main/examples/k8s/gateway-ha)、[`examples/systemd/`](https://github.com/loonghao/dcc-mcp-core/tree/main/examples/systemd)。
