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

DCC-MCP 的客户端控制面。它是主要用户/agent 入口；它不托管 skills，也不替代
底层 runtime binary `dcc-mcp-server`。

`dcc-mcp-cli` 有两种 gateway 模式：

- `local`（默认）：直接读取 core 默认 FileRegistry，并用选中 DCC instance
  暴露的 MCP HTTP endpoint 执行 `search`、`describe`、`load-skill`、`call`、
  `wait-ready` 和受保护的 `stop-instance`。
- 命名远程 profile：同一套控制流程通过选中的远程 gateway base URL 执行。

注册和选择远程 profile：

```bash
dcc-mcp-cli gateway register https://workstation.example:19293 --name pcA
dcc-mcp-cli gateway list
dcc-mcp-cli gateway set pcA
dcc-mcp-cli gateway set local
```

`--gateway <name>` 可为单次命令覆盖当前 profile。`--base-url` 与
`DCC_MCP_BASE_URL` 继续作为旧脚本和 smoke check 的直接 endpoint override。

默认 `local` profile 下，agent-control 命令会先确保 machine-wide loopback
gateway 健康，然后本地 `list` 读取 FileRegistry；本地 `search`、
`describe`、`load-skill`、`call`、`wait-ready` 和 `stop-instance` 会从
registry 解析目标实例，并直连该实例声明的 `mcp_url` / `readyz` /
`safe_stop_url`。gateway daemon 仍会保持可用，用于 Admin、health、update
和跨实例控制面路由。当前 profile 是远程，或传了 `--gateway pcA` /
`--base-url ...` 时，同一组命令走 gateway `/v1/*`。

`list` 是 inventory 和诊断命令：它会保留仍然 live 的 `booting` 行，以及
`dispatch_status=unavailable` 的 sidecar 行，方便看到启动失败原因。本地
`search`、`describe`、`load-skill`、`call` 和 `reload-skills` 只会路由到
已经可被本地 CLI 直控的 direct MCP 实例（`status=available` 或 `busy`，
且如果上报了 `dispatch_status`，必须是 `ready`）。per-DCC sidecar 行在
`dispatch_status=ready` 后也可以被本地路由；在 ready 之前仍保留为启动诊断，
不会用于 tool call。如果 `list` 里能看到实例但还不能被本地 CLI 直控，用
`wait-ready` 或 `doctor` 判断它卡在哪个 readiness 阶段。每个本地 `list`
行都会带 `direct_control.recommended_next_action`，agent 可以据此区分
“可通过 local MCP 路由”或“继续等待 sidecar dispatch readiness”。本地行还会带
`direct_control.diagnostics`，集中暴露 sidecar 的 `failure_stage`、
`failure_reason`、`host_rpc_*`、gateway guardian/recovery 字段，以及 DCC
supervisor 写入 registry 的 stdout/stderr 日志路径。`doctor` 会把不可直控的本地行
汇总到 `local.inventory.direct_control.not_ready_instances`。

Agent 控制命令（`list`、`search`、`describe`、`load-skill`、`call`、
`wait-ready`、`reload-skills`、`stop-instance`）以及仍需要本机 gateway 的
endpoint 级命令（`health`、`update`，以及未显式传 `--url` 的 `smoke`）只会
对 loopback HTTP 目标（`http://127.0.0.1:<port>` 或
`http://localhost:<port>`）执行 auto-ensure。单次禁用可传
`--no-auto-gateway`。只操作本地文件的命令（`install`、`marketplace`、
`lint`）、显式生命周期命令（`gateway ...`），以及带显式 `--url` 的 smoke
check 不会自动启动 gateway。
启动状态不清楚时，先运行 `dcc-mcp-cli doctor`。它会输出当前 profile
配置、选中的模式、registry 目录和 inventory、direct-control readiness 汇总、
本机 gateway daemon 状态、以及 server binary 的路径/来源/版本，而且不会启动
或下载任何服务。

```bash
dcc-mcp-cli list
dcc-mcp-cli list --gateway pcA
dcc-mcp-cli doctor
dcc-mcp-cli health
dcc-mcp-cli --no-auto-gateway health
dcc-mcp-cli gateway register https://workstation.example:19293 --name pcA
dcc-mcp-cli gateway list
dcc-mcp-cli gateway set pcA
dcc-mcp-cli gateway set local
dcc-mcp-cli search --query sphere --dcc-type maya --instance-id abc12345
dcc-mcp-cli describe maya.abc12345.create_sphere
dcc-mcp-cli load-skill workflow --dcc-type 3dsmax --instance-id 80321760
dcc-mcp-cli call maya.abc12345.create_sphere --json '{"radius":2}'
dcc-mcp-cli call maya_scene__get_session_info --dcc-type maya --instance-id abc12345 --json '{}'
dcc-mcp-cli wait-ready --dcc-type maya --instance-id abc12345 --require skill_catalog,host_execution_bridge
dcc-mcp-cli stop-instance --dcc-type maya --instance-id abc12345 --expected-owner release-smoke-test
dcc-mcp-cli install --dcc-type maya --version 2026
dcc-mcp-cli install --dcc-type maya --version 2026 --python "C:/Program Files/Autodesk/Maya2026/bin/mayapy.exe"
dcc-mcp-cli install --dcc-type maya --version 2026 --python "C:/Program Files/Autodesk/Maya2026/bin/mayapy.exe" --execute
dcc-mcp-cli marketplace add dcc-mcp/marketplace
dcc-mcp-cli marketplace search --query hunyuan --dcc maya
dcc-mcp-cli marketplace inspect dcc-asset-hunyuan-download
dcc-mcp-cli marketplace install dcc-asset-hunyuan-download --dcc maya
dcc-mcp-cli reload-skills --dcc-type maya
dcc-mcp-cli marketplace list-installed --dcc maya
dcc-mcp-cli marketplace outdated --dcc maya
dcc-mcp-cli marketplace update dcc-mcp-maya-skills --dcc maya
dcc-mcp-cli reload-skills --dcc-type maya
dcc-mcp-cli marketplace update --all
dcc-mcp-cli update check
dcc-mcp-cli update check --binary dcc-mcp-server --current-version 0.18.16
dcc-mcp-cli update apply
dcc-mcp-cli gateway daemon start
dcc-mcp-cli gateway daemon restart
dcc-mcp-cli gateway daemon stop
dcc-mcp-cli gateway daemon status
dcc-mcp-cli lint path/to/skills
```

### 命令

| 命令 | REST/API 契约 | 说明 |
|---|---|---|
| `health` | `GET /v1/healthz` | 检查配置的端点。 |
| `doctor [--registry-dir <path>] [--gateway-port <port>]` | local filesystem + gateway probe | 不启动或下载服务，输出 profile 配置/当前选择、本地 registry path/inventory、direct-control readiness 汇总和 not-ready 诊断、gateway daemon 状态和 server binary 诊断。 |
| `list [--gateway <profile>]` | local FileRegistry 或 `GET /v1/instances` | 列出在线 DCC 实例。默认先确保 loopback gateway，再读取本机 FileRegistry；远程 profile 走选中的 gateway。 |
| `search [--instance-id <id>]` | 本地 MCP `search_tools` 或远程 `POST /v1/search` | 搜索可调用能力，可限定完整 UUID 或唯一前缀。 |
| `describe <tool-slug>` | 本地 MCP `tools/list` 或远程 `POST /v1/describe` | 调用前检查能力 schema。 |
| `load-skill <skill-name> [--dcc-type <dcc>] [--instance-id <id>]` | 本地 MCP `tools/call load_skill` 或远程 `POST /v1/load_skill` | 激活 progressive skill 并输出已注册工具。 |
| `call <tool-slug> --json <object>` | 本地 MCP `tools/call` 或远程 `POST /v1/call` | 调用一个能力。 |
| `call <backend-tool> --dcc-type <dcc> --instance-id <id> --json <object>` | 本地 MCP `tools/call` 或远程 `POST /v1/dcc/{dcc}/instances/{id}/call` | 不手工拼 dotted gateway slug，直接调用指定实例上的 backend tool。 |
| `wait-ready [--dcc-type <dcc>] [--instance-id <id>] [--require <bits>]` | 本地 registry + per-instance `/v1/readyz`，或远程 gateway inventory + `/v1/readyz` | 等待 release smoke test 所需 readiness bit，例如 `skill_catalog` 或 `host_execution_bridge`。 |
| `reload-skills [--dcc-type <dcc>] [--instance-id <id>]` | 本地 MCP `tools/call dcc_admin__reload_skills`，或远程 `POST /v1/dcc/{dcc}/instances/{id}/call` | marketplace 安装或 skill path 变更后，让正在运行的 adapter 重新扫描 skill 搜索路径。 |
| `stop-instance --dcc-type <dcc> --instance-id <id>` | 本地 `safe_stop_url` 或远程 `POST /v1/dcc/{dcc}/instances/{id}/stop` | 对声明了 `safe_stop_url` 的实例发起带保护条件的 safe-stop 请求。 |
| `install --dcc-type <dcc> [--version <v>] [--python <path>] [--execute]` | catalog-backed local plan / executor | 解析匹配的 adapter 并输出可审计安装计划；加 `--execute` 后会在确认后执行 package 安装步骤、失败回滚并做 package/path 验证。Live DCC 检查保留在返回的 `next_steps` 中。 |
| `marketplace search/install/update/...` | marketplace catalog + local installed state | 搜索、安装、卸载和更新本地 marketplace skill 包。 |
| `update check [--binary <name>] [--current-version <version>]` | `GET /v1/update/check` | 检查 gateway update manifest。默认检查 CLI 自身；检查 Admin 面板里的实例版本时，传 `--binary dcc-mcp-server` 和对应 server 版本。 |
| `update apply` | `GET /v1/update/check` + download URL | 下载并暂存 CLI binary，下一次 CLI 启动时应用。它不会更新正在运行的 server 实例；server 请用 Admin 实例页升级按钮，或在 server 环境里运行 `dcc-mcp-server update apply`。 |
| `gateway register <url> --name <profile>` | local profile config | 保存命名远程 gateway profile。 |
| `gateway list` | local profile config | 显示已配置的远程 profile 和当前 local/remote 选择。 |
| `gateway set <profile\|local>` | local profile config | 选择当前 gateway profile。 |
| `gateway daemon start/restart/stop/status` | local process | 显式管理本机 machine-wide gateway daemon 生命周期；`start` 和 `restart` 的启动阶段默认传 `--gateway-idle-timeout-secs 0`，无 backend 时也保持存活；`status` 会输出 registry dir、PID file、health URL 和 CLI version 等诊断字段。 |
| `gateway ensure/start/stop/status` | local process | 旧脚本兼容 alias；面向用户文档优先使用 `gateway daemon ...`。 |
| `lint [PATH ...]` | local filesystem validator | 默认递归校验每个路径下两层内的 SKILL.md 包。 |

`gateway daemon start` 和 `gateway daemon restart` 是持久 operator 路径。默认
`--gateway-idle-timeout-secs 0` 会关闭 idle shutdown；只有脚本明确想要短生命周期
daemon 时才传非零 timeout。本机 loopback auto-ensure 覆盖 agent-control path
和 endpoint 命令；单次不想启动可传 `--no-auto-gateway`。

`install` 默认仍是规划契约：它解析 catalog entry，并列出 adapter package、
host plugin 和验证步骤，不会静默修改 DCC 插件目录。JSON plan 还会包含
机器可读的 `next_steps`：当 catalog 或 GitHub repo URL 提供安装说明时，第一步是
指向 adapter 仓库 raw `install.md` 的 `read-install-instructions`，随后是覆盖
`doctor`、`list`、`wait-ready`、`search`、marketplace skill
`search` / `inspect` / `install`、`reload-skills` 的命令数组，并包含手动启动/启用 DCC host plugin 的步骤。pip adapter 需要安装进特定 DCC 解释器时，
传 `--python`（或设置 `DCC_MCP_INSTALL_PYTHON`），例如 `mayapy`、`hython` 或
Blender 自带 Python。传 `--execute` 后才会请求确认并执行
可执行 package 安装步骤。执行时如果后续步骤失败，会按相反顺序回滚已完成步骤；
pip 安装使用 `<python> -m pip`，并用 `pip show` 验证；git/zip/path 安装会检查目标路径
确实存在，且目标目录不是空目录。DCC 只有在 host plugin / sidecar 启动、保持存活、
并出现在 `dcc-mcp-cli list` 中后才算在线；CLI install 不会伪造 gateway 注册。

如果工作室有专门的 Pipeline 部署流程，可以设置
`DCC_MCP_INSTALL_DISABLED=1` 禁用自动执行安装。plan 仍会返回 adapter metadata 和
`next_steps`，但 `install_policy.auto_install_enabled` 为 `false`，`--execute` 会被跳过，
agent-facing 提示词来自 `DCC_MCP_INSTALL_DISABLED_PROMPT`（支持 `{adapter}`、
`{dcc_type}`、`{version}` 占位）。可用于类似 “Automatic install is unavailable;
contact Pipeline TD to deploy {adapter} for {dcc_type}.” 的内部提示。

`marketplace` 是面向 CLI 的官方/私有 skill 包发现入口。安装位置默认为
`~/.dcc-mcp/marketplace/<dcc>/<name>/`，可用
`DCC_MCP_MARKETPLACE_INSTALL_ROOT` 覆盖。DCC adapter 会把
`~/.dcc-mcp/marketplace/<dcc>` 加入 skill 搜索路径，因此新安装的 skill 会在
adapter 启动时，或下一次
`dcc-mcp-cli reload-skills --dcc-type <dcc>` 后被发现。刷新后，如果 adapter
没有自动加载该 skill，再运行
`dcc-mcp-cli load-skill <skill-name> --dcc-type <dcc> --instance-id <id>`。

`dcc-mcp-cli update` 面向由 gateway update manifest 暴露的二进制更新；
manifest 通过 `DCC_MCP_UPDATE_MANIFEST_URL`（或
`GatewayConfig.update_manifest_url`）配置。`update check` 只读取
`/v1/update/check`，适合人和 agent 使用；CLI 会在请求前默认确保本机 gateway
存在。`update apply` 只暂存 CLI binary。对于 server 实例，优先使用 Admin
实例页的升级按钮：它调用 `POST /admin/api/instances/{instance_id}/update`，
并以需要重启的状态暂存 `dcc-mcp-server`。如果你就在 server 所在环境操作，
则使用 `dcc-mcp-server update apply`。

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

适配器、sidecar、bridge 与整机 gateway daemon 使用的底层 runtime binary。
它仍然适合 CI 和运维脚本，但主要用户/agent UX 是 `dcc-mcp-cli`。

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
| `dcc-mcp-server update check/apply` | Server binary 更新助手。 | 读取 `127.0.0.1:<gateway-port>` 上的 gateway update manifest，并为下一次 server 启动暂存 `dcc-mcp-server`。 |

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
| `--no-admin` | `DCC_MCP_NO_ADMIN` | `false` | 关闭获选网关上的 Admin UI。默认获选网关会开启 admin。 |
| `--admin-path` | `DCC_MCP_ADMIN_PATH` | `/admin` | Admin UI 与其 JSON API 的 URL 前缀。 |
| `--registry-dir` | `DCC_MCP_REGISTRY_DIR` | `<temp>/dcc-mcp-registry` | CLI local mode、sidecar 与 gateway runner 共用的 `FileRegistry` 目录。 |
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

Daemon auto-ensure 路径默认传有界 idle timeout，除非设置
`DCC_MCP_GATEWAY_IDLE_TIMEOUT_SECS` 覆盖。面向用户的
`dcc-mcp-cli gateway daemon start` wrapper 默认传 `0`，因此显式管理的整机
daemon 不会只因为暂时没有 DCC 注册就退出。

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
