# CLI 参考

仓库提供四个面向运维的二进制文件。本页是所有旗标、所有环境变量、以及五个
典型部署场景的**唯一信息源**。每个二进制的旗标都一对一映射到一个
`DCC_MCP_*` 环境变量，所以任何部署清单都能驱动同一套配置面板。

`dcc-mcp-cli` 与 `dcc-mcp-server` 会在每个 Release 作为原生 GitHub
Release 资产发布。CLI 可以通过 URL 直接安装：

```bash
curl -fsSL https://raw.githubusercontent.com/dcc-mcp/dcc-mcp-core/main/scripts/install-cli.sh | bash

# Windows PowerShell
powershell -c "irm https://raw.githubusercontent.com/dcc-mcp/dcc-mcp-core/main/scripts/install-cli.ps1 | iex"
```

需要固定版本时，设置 `DCC_MCP_VERSION=v0.17.17`，或给安装脚本传
`--version v0.17.17`。

| 二进制 | 角色 | 源码位置 |
|---|---|---|
| [`dcc-mcp-cli`](#dcc-mcp-cli) | 面向用户/CI 的控制面 CLI，用来访问本地或远程 DCC-MCP REST 端点。 | `crates/dcc-mcp-cli/` |
| [`dcc-mcp-server`](#dcc-mcp-server) | per-DCC MCP + REST 服务器，内置自动网关。 | `crates/dcc-mcp-server/` |
| [`dcc-mcp-tunnel-relay`](#dcc-mcp-tunnel-relay) | 面向公网的 WebSocket 隧道中继（零配置远程访问，#504）。 | `crates/dcc-mcp-tunnel-relay/` |
| [`dcc-mcp-tunnel-agent`](#dcc-mcp-tunnel-agent) | 在工作站上注册到中继、转发 MCP 流量的本地 sidecar。 | `crates/dcc-mcp-tunnel-agent/` |

开发辅助二进制（`stub_gen`）在
[`AGENTS.md`](https://github.com/dcc-mcp/dcc-mcp-core/blob/main/AGENTS.md) 里。

---

## `dcc-mcp-cli`

DCC-MCP 的客户端控制面。它不托管 skills，也不替代 `dcc-mcp-server`；
它负责访问本地或远程 gateway / per-DCC REST 端点，并生成可审计的安装计划。

默认端点是 `http://127.0.0.1:9765`，可用 `--base-url` 或
`DCC_MCP_BASE_URL` 覆盖。

```bash
dcc-mcp-cli list
dcc-mcp-cli health
dcc-mcp-cli search --query sphere --dcc-type maya --instance-id abc12345
dcc-mcp-cli describe maya.abc12345.create_sphere
dcc-mcp-cli call maya.abc12345.create_sphere --json '{"radius":2}'
dcc-mcp-cli call maya_scene__get_session_info --dcc-type maya --instance-id abc12345 --json '{}'
dcc-mcp-cli wait-ready --dcc-type maya --instance-id abc12345 --require skill_catalog,host_execution_bridge
dcc-mcp-cli stop-instance --dcc-type maya --instance-id abc12345 --expected-owner release-smoke-test
dcc-mcp-cli install --dcc-type maya --version 2026
dcc-mcp-cli lint path/to/skills
```

### 命令

| 命令 | REST/API 契约 | 说明 |
|---|---|---|
| `health` | `GET /v1/healthz` | 检查配置的端点。 |
| `list` | `GET /v1/instances` | 从 gateway 列出在线 DCC 实例。 |
| `search [--instance-id <id>]` | `POST /v1/search` | 搜索可调用能力，可限定完整 UUID 或唯一前缀。 |
| `describe <tool-slug>` | `POST /v1/describe` | 调用前检查能力 schema。 |
| `call <tool-slug> --json <object>` | `POST /v1/call` | 调用一个能力。 |
| `call <backend-tool> --dcc-type <dcc> --instance-id <id> --json <object>` | `POST /v1/dcc/{dcc}/instances/{id}/call` | 不手工拼 dotted gateway slug，直接调用指定实例上的 backend tool。 |
| `wait-ready [--dcc-type <dcc>] [--instance-id <id>] [--require <bits>]` | `GET /v1/instances` + per-instance `/v1/readyz` | 等待 release smoke test 所需 readiness bit，例如 `skill_catalog` 或 `host_execution_bridge`。 |
| `stop-instance --dcc-type <dcc> --instance-id <id>` | `POST /v1/dcc/{dcc}/instances/{id}/stop` | 对声明了 `safe_stop_url` 的实例发起带保护条件的 safe-stop 请求。 |
| `install --dcc-type <dcc> [--version <v>]` | catalog-backed local plan | 解析匹配的 adapter 并输出可审计安装计划。 |
| `lint [PATH ...]` | local filesystem validator | 默认递归校验每个路径下两层内的 SKILL.md 包。 |

`install` 目前是规划契约：它解析 catalog entry，并列出 runtime、adapter、
验证步骤，不会静默修改 DCC 插件目录。DCC-specific installer 后续可增量接入
这份契约。

`lint` 复用生产 `dcc-mcp-skills` validator，因此本地检查与运行时加载会因同一类
结构问题失败。CI 也通过 `just lint-skills` 显式传入仓库 skill roots，跑同一条
`dcc-mcp-cli lint <PATH...>` 路径。

### CLI 安装资产

安装脚本会下载以下 GitHub Release 资产之一：

| 平台 | 资产 |
|---|---|
| Linux x86_64 | `dcc-mcp-cli-linux-x86_64` |
| Windows x86_64 | `dcc-mcp-cli-windows-x86_64.exe` |
| macOS universal2 | `dcc-mcp-cli-macos-universal2` |

默认安装位置：Linux/macOS 为 `~/.local/bin`，Windows 为
`%LOCALAPPDATA%\dcc-mcp\bin`。可用 `DCC_MCP_INSTALL_DIR` 或
`--install-dir` 覆盖。

---

## `dcc-mcp-server`

独立服务器入口，显式区分 per-DCC MCP server 与整机 gateway daemon。
不带子命令调用 `dcc-mcp-server` 仍保持向后兼容：行为等同于
`dcc-mcp-server auto`，会确保本机 gateway daemon 已启动，注册当前
per-DCC server 为 backend，并在 backend 存活期间保留轻量 guardian。

### 运行模式

| 命令 | 角色 | 网关行为 |
|---|---|---|
| `dcc-mcp-server` | 向后兼容的隐式 `auto`。 | 确保独立 gateway daemon，然后注册为 backend。 |
| `dcc-mcp-server auto` | 默认行为的显式写法。 | 与无子命令路径相同。 |
| `dcc-mcp-server serve` | per-DCC MCP server。 | 确保独立 gateway daemon，然后注册为 backend。 |
| `dcc-mcp-server serve --no-auto-gateway` | 仅运行 per-DCC MCP server。 | 提供 MCP 工具，但绝不尝试绑定 gateway port。 |
| `dcc-mcp-server auto --legacy-gateway-election` | 旧的嵌入式 gateway 模式。 | per-DCC 进程直接竞争 gateway port。 |
| `dcc-mcp-server sidecar` | per-DCC sidecar worker。 | 确保独立 gateway daemon，注册 `per-dcc-sidecar` 行，并通过 host RPC 派发。运行时由 `dcc-mcp-sidecar` 实现。 |
| `dcc-mcp-server gateway` | 整机 gateway daemon。 | 只托管 discovery、routing、resources/prompts、admin 与 audit，不内联执行 DCC tool。 |

`auto` 与 `serve` 共享下面的 server 旗标。`gateway` 有更小的独立旗标面，
会拒绝 `--app` 这类 server-only 旗标。

### 核心旗标

| 旗标 | 环境变量 | 默认值 | 说明 |
|---|---|---|---|
| `--mcp-port` | `DCC_MCP_MCP_PORT` | `0` | MCP Streamable HTTP 端口。`0` = OS 分配。 |
| `--ws-port` | `DCC_MCP_WS_PORT` | `9001` | 给非 Python DCC 插件用的 WebSocket 桥端口。 |
| `--app` | `DCC_MCP_APP` | `""` | 应用标签（`"maya"`、`"blender"`、`"photoshop"` …）。驱动 skill 发现 + 注册表行。 |
| `--skill-paths` | — | `[]` | 附加的 skill 搜索路径（可重复）。 |
| `--server-name` | `DCC_MCP_SERVER_NAME` | `"dcc-mcp-server"` | 通告给 MCP 客户端的服务器名。 |
| `--no-bridge` | — | `false` | 关闭 WebSocket 桥；仅 MCP HTTP。 |
| `--host` | — | `127.0.0.1` | 绑定主机。 |
| `--pid-file` | — | — | 运行期间把服务器 PID 写入此文件。 |
| `--force` | — | `false` | 覆盖指向活进程的 PID 文件。 |
| `--shutdown-timeout-secs` | `DCC_MCP_SHUTDOWN_TIMEOUT_SECS` | `10` | 优雅关闭时限。 |

### 自动网关旗标（`auto` / `serve`）

| 旗标 | 环境变量 | 默认值 | 说明 |
|---|---|---|---|
| `--gateway-port` | `DCC_MCP_GATEWAY_PORT` | `9765` | 要争的公认端口。`0` 完全关闭网关角色，因此也关闭 admin。 |
| `--no-admin` | `DCC_MCP_NO_ADMIN` | `false` | 关闭获选网关上的只读 Admin UI。默认获选网关会开启 admin。 |
| `--admin-path` | `DCC_MCP_ADMIN_PATH` | `/admin` | Admin UI 与其 JSON API 的 URL 前缀。 |
| `--registry-dir` | `DCC_MCP_REGISTRY_DIR` | 平台 temp | 共享 `FileRegistry` 目录。 |
| `--stale-timeout-secs` | `DCC_MCP_STALE_TIMEOUT` | `30` | 没心跳后多少秒实例被判为过期。 |
| `--app-version` | `DCC_MCP_APP_VERSION` | — | 应用版本（如 `"2024.2"`）；记入注册表。 |
| `--scene` | `DCC_MCP_SCENE` | — | 当前打开的场景 / 文档；记入注册表，多实例 disambiguation 使用。 |
| `--heartbeat-secs` | `DCC_MCP_HEARTBEAT_INTERVAL` | `5` | 心跳周期。`0` 关闭。 |

Admin 审计/trace 持久化只通过环境变量配置：设置 `DCC_MCP_GATEWAY_AUDIT_DIR` 为可写目录后，`/admin/api/calls` 行会写入 `audit.jsonl`，dispatch traces 会写入 `traces.jsonl`；`DCC_MCP_GATEWAY_AUDIT_MAX_ROWS`（默认 `5000`）限制每个文件保留行数。

> **已移除** —— `--gateway-tool-exposure` /
> `DCC_MCP_GATEWAY_TOOL_EXPOSURE` 已删除。网关表面现在无条件最小化，详见
> `docs/zh/guide/rest-api-surface.md`。
>
> **已移除** —— `--gateway-cursor-safe-tool-names` /
> `DCC_MCP_GATEWAY_CURSOR_SAFE_TOOL_NAMES`。聚合网关的 `prompts/list` 始终使用
> cursor-safe 的 `i_<id8>__<escaped>` 线格式（#656）。

### 独立 gateway 旗标（`gateway`）

| 旗标 | 环境变量 | 默认值 | 说明 |
|---|---|---|---|
| `--daemon` | `DCC_MCP_DAEMON` | `false` | 重新执行当前可执行文件，启动 detached gateway child，然后父进程退出。Unix child 会进入新 session；Windows child 使用 detached process flags。respawn 失败会在父进程退出前报错。 |
| `--pidfile PATH` | `DCC_MCP_PIDFILE` | — | 隐式开启 daemon mode。pidfile 记录 detached child PID，并在 child 正常退出时移除。pidfile 写入失败会在父进程退出前报错。 |
| `--gateway-persist` | `DCC_MCP_GATEWAY_PERSIST` | `false` | 即使没有已注册 backend，也保持 gateway daemon 存活。 |
| `--gateway-idle-timeout-secs` | `DCC_MCP_GATEWAY_IDLE_TIMEOUT_SECS` | `30` | 最后一个 backend 消失后等待多少秒再关闭。`0` 关闭 idle shutdown。 |

### 文件日志旗标

| 旗标 | 环境变量 | 默认值 | 说明 |
|---|---|---|---|
| `--no-log-file` | `DCC_MCP_NO_LOG_FILE` | `false` | 关闭文件日志（stderr 日志仍开）。 |
| `--log-dir` | `DCC_MCP_LOG_DIR` | 平台默认 | 日志目录。 |
| `--log-max-size` | `DCC_MCP_LOG_MAX_SIZE` | 10 MiB | 单文件达到该大小触发滚动。 |
| `--log-max-files` | `DCC_MCP_LOG_MAX_FILES` | `7` | 保留多少个滚动文件。 |
| `--log-rotation` | `DCC_MCP_LOG_ROTATION` | `"both"` | 滚动策略：`size`、`daily`、`both`。 |
| `--log-file-prefix` | `DCC_MCP_LOG_FILE_PREFIX` | `"dcc-mcp"` | 文件名前缀。完整名：`<prefix>.<pid>.<YYYYMMDD>.log`。 |
| `--log-retention-days` | `DCC_MCP_LOG_RETENTION_DAYS` | `7` | 按天保留。`0` 关闭。 |
| `--log-max-total-size-mb` | `DCC_MCP_LOG_MAX_TOTAL_SIZE_MB` | `100` | 总目录容量上限（MiB）。`0` 关闭。 |

### Capture replay/diff

`dcc-mcp-server capture` 处理由
`DCC_MCP_TRAFFIC_CAPTURE=jsonl:<path>` 或 `DCC_MCP_TRAFFIC_CONFIG=<yaml>`
产生的离线 traffic capture 文件。它不会自动开启 capture；只有 `replay`
模式需要一个在线 gateway。

如果 YAML 配置包含 `admin_live` sink，可以从 `/admin/api/traffic/export`
（或稳定镜像 `/v1/debug/traffic/export`）把保留在内存里的窗口下载为 JSONL，
然后像其他 capture 文件一样交给 `capture replay` 或 `capture diff`。

```bash
# 把记录下来的 client -> gateway 请求重放到在线 gateway MCP endpoint。
dcc-mcp-server capture replay ./captures/run.sqlite \
    --target http://127.0.0.1:9765/mcp \
    --session sess_01HQX \
    --assert outputs-compatible

# 逐帧比较两份 capture。
dcc-mcp-server capture diff ./captures/before.sqlite ./captures/after.sqlite \
    --before-session sess_before \
    --after-session sess_after
```

Replay 断言模式：

| Mode | 契约 |
|---|---|
| `outputs-compatible` | HTTP status 与 JSON-RPC result/error 形状必须和记录响应一致。 |
| `outputs-equal` | HTTP status 与响应 JSON 必须完全一致。 |
| `outputs-ignored` | 只发送并计数请求，不比较响应 body。 |

当文件扩展名不足以判断格式时，使用 `--format jsonl` 或
`--format sqlite`。`--rebind-instance-id <id>` 会重写捕获到的 gateway tool
slug（例如 `maya.old.tool`）以及 `instance_id` 字段，方便把旧记录重放到当前
在线实例。

### 典型调用

```bash
# 1) 向后兼容的 auto 模式（等同于：dcc-mcp-server auto --app maya）。
dcc-mcp-server --app maya

# 2) 仅 per-DCC server，不竞争共享 gateway port。
dcc-mcp-server serve --no-auto-gateway --app maya

# 3) 工作站上多 DCC 的网关赢家。
#    第一个终端赢网关端口，后续的注册成普通实例。
dcc-mcp-server auto --app maya --server-name maya-shotgun-alpha \
               --scene /shots/ep101/sh0200/shot.ma \
               --log-dir /var/log/dcc-mcp

# 4) 整台工作站的 gateway daemon。
dcc-mcp-server gateway --host 127.0.0.1 --port 9765 \
                       --registry-dir /var/lib/dcc-mcp

# 4b) 同一个 gateway，以显式 detached daemon 方式运行。
dcc-mcp-server gateway --host 127.0.0.1 --port 9765 \
                       --registry-dir /var/lib/dcc-mcp \
                       --daemon --pidfile /var/run/dcc-mcp-gateway.pid
```

---

## `dcc-mcp-tunnel-relay`

面向公网的 WebSocket 中继，接收本地隧道 agent 的注册，把远端 AI
助手的多路 MCP 会话转发到正确的工作站。

编译：`cargo build --bin dcc-mcp-tunnel-relay --features bin`。

| 旗标 | 环境变量 | 默认值 | 说明 |
|---|---|---|---|
| `--jwt-secret-file` | `DCC_MCP_TUNNEL_RELAY_JWT_SECRET_FILE` | **必需** | HS256 JWT 密钥文件路径。生产环境 ≥32 字节（`openssl rand -base64 48`）。密钥从文件读取，不会出现在 `ps` 里。 |
| `--public-host` | `DCC_MCP_TUNNEL_RELAY_PUBLIC_HOST` | `localhost` | 公网主机名（写入 JWT `iss` 声明）。 |
| `--base-url` | `DCC_MCP_TUNNEL_RELAY_BASE_URL` | `ws://localhost:9870` | WebSocket 基础 URL；拼接到 `RegisterAck.public_url`。 |
| `--agent-bind` | `DCC_MCP_TUNNEL_RELAY_AGENT_BIND` | `0.0.0.0:9870` | agent 控制面绑定。 |
| `--frontend-bind` | `DCC_MCP_TUNNEL_RELAY_FRONTEND_BIND` | `0.0.0.0:9871` | 远端客户端 TCP 前端绑定。 |
| `--ws-frontend-bind` | `DCC_MCP_TUNNEL_RELAY_WS_FRONTEND_BIND` | — | 可选 WebSocket 前端绑定（`/tunnel/<id>` 升级）。不填即关闭。 |
| `--admin-bind` | `DCC_MCP_TUNNEL_RELAY_ADMIN_BIND` | — | 可选只读管理端点（`GET /tunnels`、`GET /healthz`）。不填即关闭。 |
| `--stale-timeout-secs` | `DCC_MCP_TUNNEL_RELAY_STALE_TIMEOUT_SECS` | `30` | 无心跳后多少秒隧道被剔除。 |
| `--max-tunnels` | `DCC_MCP_TUNNEL_RELAY_MAX_TUNNELS` | `0` | 并发隧道硬上限。`0` 不限。 |

关停：SIGINT / SIGTERM（Windows 下 Ctrl+C）触发 accept loops 的排空，
活动 session 自行关闭。

```bash
dcc-mcp-tunnel-relay \
    --jwt-secret-file /etc/dcc-mcp/tunnel-secret \
    --public-host relay.example.com \
    --base-url wss://relay.example.com \
    --agent-bind 0.0.0.0:9870 \
    --frontend-bind 0.0.0.0:9871 \
    --ws-frontend-bind 0.0.0.0:9880 \
    --admin-bind 127.0.0.1:9877
```

---

## `dcc-mcp-tunnel-agent`

本地 sidecar，向中继注册并把每会话流量桥接到本地 DCC MCP 服务器。根据
配置的重连策略，在瞬态故障下维持连接。

编译：`cargo build --bin dcc-mcp-tunnel-agent --features bin`。

| 旗标 | 环境变量 | 默认值 | 说明 |
|---|---|---|---|
| `--relay-url` | `DCC_MCP_TUNNEL_AGENT_RELAY_URL` | **必需** | 中继 WebSocket URL（`wss://relay.example.com`）。 |
| `--token-file` | `DCC_MCP_TUNNEL_AGENT_TOKEN_FILE` | **必需** | JWT bearer token 文件路径（`dcc_mcp_tunnel_protocol::auth::issue` 铸造）。 |
| `--dcc` | `DCC_MCP_TUNNEL_AGENT_DCC` | **必需** | agent 标识的 DCC 标签；必须在 JWT `allowed_dcc` 列表里。 |
| `--local-target` | `DCC_MCP_TUNNEL_AGENT_LOCAL_TARGET` | **必需** | 要桥接的本地 MCP HTTP 服务器地址（`host:port`）。 |
| `--heartbeat-secs` | `DCC_MCP_TUNNEL_AGENT_HEARTBEAT_SECS` | `10` | 心跳周期。留足余量，远小于中继的 `--stale-timeout-secs`。 |
| `--reconnect-policy` | `DCC_MCP_TUNNEL_AGENT_RECONNECT_POLICY` | `exponential` | `constant` 或 `exponential`。 |
| `--reconnect-initial-secs` | `DCC_MCP_TUNNEL_AGENT_RECONNECT_INITIAL_SECS` | `2` | 指数退避：首次重试等待秒数。 |
| `--reconnect-max-secs` | `DCC_MCP_TUNNEL_AGENT_RECONNECT_MAX_SECS` | `60` | 指数退避：重试延迟硬上限。 |
| `--reconnect-constant-secs` | `DCC_MCP_TUNNEL_AGENT_RECONNECT_CONSTANT_SECS` | `5` | 固定退避的延迟。 |
| `--capabilities` | `DCC_MCP_TUNNEL_AGENT_CAPABILITIES` | `[]` | 逗号分隔能力标签，通过 `/tunnels` 展示给远端客户端。 |

不可重试的 `Rejected` 错误（JWT 不对、DCC 类型不匹配）以非零退出码终止，
避免 supervisor 无限重启循环。

```bash
dcc-mcp-tunnel-agent \
    --relay-url wss://relay.example.com \
    --token-file ~/.config/dcc-mcp/tunnel.jwt \
    --dcc maya \
    --local-target 127.0.0.1:8765 \
    --heartbeat-secs 10 \
    --reconnect-policy exponential \
    --reconnect-initial-secs 2 \
    --reconnect-max-secs 60
```

---

## 部署场景

### 场景 1 —— 嵌入 DCC 宿主

Maya / Blender / Houdini 插件把 `dcc_mcp_core` 加载到宿主的 Python
解释器里，直接调 `create_skill_server()`。不涉及任何外部二进制。大多数
终端用户场景都长这样。

参考 `examples/host_adapter_template.py`。

### 场景 2 —— 独立 per-DCC 服务

工作站上一个 `dcc-mcp-server` 进程，由 DCC supervisor 或用户 autostart
拉起。适合跑 `mayapy` 批处理、Python-only 渲染器等仍想通过 MCP + REST
对外暴露能力的场景。

```bash
dcc-mcp-server --app maya --scene /shots/ep101/sh0200/shot.ma
```

### 场景 3 —— 网关汇聚多个 DCC 服务

同一工作站上多个 `dcc-mcp-server`。先起的绑定网关端口 `9765` 成为
赢家，并索引其余实例。客户端只连 `127.0.0.1:9765/mcp`（或 `/v1/*`），
用 MCP `search` / `describe` 做发现，再通过 REST `/v1/call` 或
`/v1/call_batch` 访问任意 DCC。

示例清单：`examples/compose/gateway-ha/` 与 `examples/k8s/gateway-ha/`。

### 场景 4 —— 远程中继 + 隧道 agent

中继跑在运维方的公网主机上；每台艺术家工作站跑一个 agent 向中继注册。
SaaS AI 客户端（企业防火墙后的 Claude.ai、Cursor 桌面版等）连中继前端，
被转发到工作站本地的 MCP 服务。

```bash
# 中继主机（公网）：
dcc-mcp-tunnel-relay \
    --jwt-secret-file /etc/dcc-mcp/tunnel-secret \
    --public-host relay.example.com \
    --base-url wss://relay.example.com

# 艺术家工作站：
dcc-mcp-tunnel-agent \
    --relay-url wss://relay.example.com \
    --token-file ~/.config/dcc-mcp/tunnel.jwt \
    --dcc maya --local-target 127.0.0.1:8765
```

用 `dcc_mcp_tunnel_protocol::auth::issue` 铸造 JWT；通过 `allowed_dcc`
声明按艺术家 / 按 DCC 限定作用域。

### 场景 5 —— CI / 测试夹具

集成测试在进程内拉起 `McpHttpServer` 并直接打它的 `/v1/*` 端点。不涉及
外部二进制、不涉及网关。

参考模式：`crates/dcc-mcp-skill-rest/src/tests.rs`、
`crates/dcc-mcp-http/tests/http/`。

---

## 相关阅读

- [REST API 面板](rest-api-surface.md) —— `/v1/*` 契约。
- [网关争用与调试](gateway-diagnostics.md) —— 多实例竞争网关时怎么读日志 + 指标。
