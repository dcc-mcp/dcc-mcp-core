# 网关选举与多实例支持

> **[English](../../guide/gateway-election)**

## 什么是网关？

**网关**是一个单一的 Rust HTTP 服务器（默认运行在 `localhost:9765`），负责：

- 发现所有运行中的 DCC 实例（Maya、Blender、Houdini、Photoshop 等）
- 将所有活跃后端的工具聚合到统一的 `/mcp` 端点（按 `{instance_short}__{name}` 命名空间化）
- 对 skill 管理调用做扇出（`search_skills`、`list_skills`）或按实例路由（`load_skill`）
- 当 skill 加载 / 卸载或实例进出时，通过 SSE 推送 `tools/list_changed` 和 `resources/list_changed`

**每台机器只有一个网关**。当第一个 DCC 实例注册时自动启动。

## 问题：先到先得

没有版本感知时，最旧的 DCC 赢得网关角色：

```
Maya v0.12.6 启动 → 绑定端口 9999 → 成为网关
Maya v0.12.29 启动 → 端口 9999 已占用 → 成为从属
❌ 旧版本控制路由；新功能被忽略
```

## 我们的解决方案：版本感知选举

```
Maya v0.12.6（网关）              Maya v0.12.29（新实例）
         │                                │
         │                   端口 9999 已占用
         │                                │
         │         读取 __gateway__ 哨兵
         │         自己版本 0.12.29 > 网关 0.12.6
         │                                │
         │ ←── POST /gateway/yield {"challenger_version": "0.12.29"}
         │
         │（支持 yield → 优雅关闭）
         │ yield_tx.send(true)
         │ 释放端口 9999
         │
                          每 10 秒重试
                          端口空闲 → 绑定
                          注册新哨兵
                          ✅ v0.12.29 现在是网关
```

### 工作原理

**1. `__gateway__` 哨兵**

网关启动时，在 FileRegistry 中写入一个特殊条目：
```json
{"dcc_type": "__gateway__", "version": "0.12.29"}
```

新实例读取此条目以了解当前网关及其版本。

**2. 语义版本比较**

版本按数值比较（非字母序）：
```
0.12.6  对比  0.12.29
↓              ↓
[0, 12, 6]  [0, 12, 29]
                 29 > 6 → v0.12.29 更新 ✓
```

**3. 主动让位**

清理任务（每 15 秒）检查是否有更新的挑战者。如果发现，立即优雅关闭。

**4. 挑战者重试循环**

新实例每 10 秒轮询端口，最多 120 秒。一旦端口空闲，立即接管。

## 多实例注册

同一类型的多个 DCC 实例可以同时存在：

> **v0.14（issue #251）** 已移除 `TransportManager`。多实例信息请通过：
> ① `create_skill_server(..., gateway_port=9765)` 启动 DCC 适配器并让它自动注册；
> ② gateway HTTP API（`GET /instances`）查询；
> ③ 或在底层直接使用 `dcc_mcp_transport::discovery::FileRegistry` + `ServiceEntry`。
>
> PyO3 嵌入式宿主（Maya 等）下的监听器生命周期变化见后文 spawn_mode 与 issue #303 说明。

```python
from dcc_mcp_core import create_skill_server, McpHttpConfig

# 启动带 Gateway 的服务器（自动注册）
server = create_skill_server("maya", McpHttpConfig(port=8765))
handle = server.start()
```

## 文档追踪

对于多文档 DCC（Photoshop、After Effects），网关通过 `McpHttpConfig.scene` 追踪活跃文档：

```python
from dcc_mcp_core import create_skill_server, McpHttpConfig

config = McpHttpConfig(port=0, server_name="photoshop")
config.gateway_port = 9765
config.dcc_type = "photoshop"
config.scene = "logo.psd"  # 当前活跃文档

server = create_skill_server("photoshop", config)
handle = server.start()
```

文档切换时更新 `config.scene` 即可反映到网关路由中。

## 会话隔离

每个 AI 会话**绑定到一个实例**。通过聚合式网关，多个实例的工具都会出现在同一份 `tools/list` 中，通过 8 字符前缀区分，agent 可定向调用任一实例：

```
a1b2c3d4__set_keyframe   ← maya-animation
e5f6g7h8__mirror_joints  ← maya-rigging
```

## 实例健康检查

网关通过心跳自动检测实例健康状态（`stale_timeout_secs` 和 `heartbeat_secs` 在 `McpHttpConfig` 中配置）。实例退出时 `McpServerHandle` 被 drop，自动从网关注销。

## 向后兼容性

不支持 `/gateway/yield` 的旧版 DCC 会返回 404——这没问题。挑战者进入轮询重试循环，等待端口自然释放（当旧 DCC 退出或崩溃时）。无硬性失败，优雅降级。

## DccGatewayElection（Python API）

`DccGatewayElection` 是一个纯 Python 类，为非网关 DCC 实例提供**自动网关故障转移**。当当前网关不可达时，选举线程会自动尝试接管。

### 工作原理

1. 后台守护线程定期探测网关的 `/health` 端点
2. 统计连续探测失败次数
3. 当失败次数超过阈值时，尝试首次获胜的 TCP 端口检查
4. 如果端口空闲，通知服务器升级为网关模式

### 构造函数

```python
from dcc_mcp_core import DccGatewayElection

election = DccGatewayElection(
    dcc_name="blender",           # 日志中的 DCC 标识
    server=blender_server,        # DCC 服务器实例（需暴露 is_gateway、is_running、_handle）
    gateway_host="127.0.0.1",     # 网关绑定地址
    gateway_port=9765,            # 竞争的网关端口
    probe_interval=5,             # 健康探测间隔（秒）
    probe_timeout=2.0,            # 每次探测超时（秒）
    probe_failures=3,             # 触发选举前的连续失败次数
    on_promote=None,              # 可选回调：() -> bool，覆盖 server._upgrade_to_gateway()
)
```

### 环境变量配置

| 变量 | 默认值 | 说明 |
|------|--------|------|
| `DCC_MCP_GATEWAY_PROBE_INTERVAL` | `5` | 健康探测间隔（秒） |
| `DCC_MCP_GATEWAY_PROBE_TIMEOUT` | `2` | 每次探测超时（秒） |
| `DCC_MCP_GATEWAY_PROBE_FAILURES` | `3` | 触发选举前的连续失败次数 |

### 属性

| 属性 | 类型 | 说明 |
|------|------|------|
| `is_running` | `bool` | 选举线程是否活跃 |
| `consecutive_failures` | `int` | 当前连续网关探测失败次数 |

### 方法

| 方法 | 返回值 | 说明 |
|------|--------|------|
| `start()` | `None` | 启动后台选举线程（幂等） |
| `stop()` | `None` | 优雅停止线程（最多等待 5 秒） |
| `get_status()` | `dict` | 返回 `{running, consecutive_failures, gateway_host, gateway_port}` |

### 提升路径

选举获胜（端口空闲）时，按以下顺序解析提升路径：

1. 传给 `__init__` 的 `on_promote` 可调用对象（如有）
2. 绑定服务器上的 `server._upgrade_to_gateway()` 方法（如有）
3. 回退：记录警告并返回 `False`

### 与 DccServerBase 配合使用

`DccServerBase` 已自动集成 `DccGatewayElection`：

```python
from dcc_mcp_core import DccServerBase

class BlenderMcpServer(DccServerBase):
    def __init__(self, **kwargs):
        super().__init__(dcc_name="blender", builtin_skills_dir=..., **kwargs)

server = BlenderMcpServer(gateway_port=9765)
server.register_builtin_actions()
handle = server.start()        # 选举线程自动启动
print(server._election.get_status())  # 检查选举状态
```

### 独立使用

```python
from dcc_mcp_core import DccGatewayElection

# 使用自定义提升回调
def promote():
    # 用网关端口重启 MCP 服务器
    return True

election = DccGatewayElection(
    dcc_name="blender",
    server=my_server,
    gateway_port=9765,
    on_promote=promote,
)
election.start()

# 稍后...
status = election.get_status()
# {"running": True, "consecutive_failures": 0, "gateway_host": "127.0.0.1", "gateway_port": 9765}

election.stop()
```
