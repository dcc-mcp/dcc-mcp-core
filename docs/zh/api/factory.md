# 服务器工厂 API

> **[English](../api/factory.md)**

DCC MCP 服务器单例工厂模式。零样板代码创建和管理 DCC 服务器的单例实例，确保同一 DCC 应用仅运行一个 MCP 服务器。提供 `make_start_stop` 便捷函数生成启动/停止函数对。

**导出符号：** `create_dcc_server`, `make_start_stop`, `get_server_instance`

## create_dcc_server

创建或返回 DCC MCP 服务器的单例实例。

- `create_dcc_server(*, instance_holder, lock, server_class, port=8765, register_builtins=True, extra_skill_paths=None, include_bundled=True, enable_hot_reload=False, hot_reload_env_var=None, **server_kwargs) -> McpServerHandle`
  - `instance_holder` — 存放单例实例的可变容器（通常为模块级 dict 或 list）
  - `lock` — 线程锁，确保单例创建的线程安全
  - `server_class` — 服务器类（如 `DccServerBase` 的子类）
  - 首次调用创建实例并启动，后续调用返回已有实例

## make_start_stop

生成 `(start_server, stop_server)` 函数对，适用于 DCC 插件的启动/停止钩子。

- `make_start_stop(server_class, hot_reload_env_var=None) -> (start_fn, stop_fn)`

## get_server_instance

返回当前的单例服务器实例。

- `get_server_instance(instance_holder) -> server | None`

---

## 嵌入式宿主接线（issues #521、#525）

嵌入式 DCC 插件（Maya、Houdini、Unreal Python、Blender）通常需要在裸服务器之外
再补两块胶水：

1. **callable 调度器**：把 skill 脚本路由到宿主 UI / 主线程；
2. **声明式 skill 列表**：让启动期加载在每次会话之间可复现。

`DccServerBase` 为此暴露了 `register_inprocess_executor()` 和
`register_builtin_actions(minimal_mode=...)`：

```python
from dcc_mcp_core import (
    DccServerBase, McpHttpConfig,
    InProcessCallableDispatcher, build_inprocess_executor,
    MinimalModeConfig,
)

class MayaDccServer(DccServerBase):
    @classmethod
    def dcc_name(cls) -> str: return "maya"

server = MayaDccServer(McpHttpConfig(port=8765))

# 1) 在注册 builtins 之前接入 in-process executor。
dispatcher = InProcessCallableDispatcher()    # 或者你的 Maya 主线程子类
server.register_inprocess_executor(build_inprocess_executor(dispatcher))

# 2) 用声明式方式锁定启动期 skill 集合。
server.register_builtin_actions(minimal_mode=MinimalModeConfig(
    skills=("scene_inspector", "render_queue"),
    deactivate_groups={"render_queue": ("submit",)},
    env_var_minimal="DCC_MCP_MAYA_MINIMAL",
))

server.start()
```

完整 `BaseDccCallableDispatcher` / `BaseDccCallableDispatcherFull` / `BaseDccPump`
契约和 `MinimalModeConfig` 解析顺序见 [可调用对象调度器 API](./dispatcher.md)。

详见 [English API 参考](../api/factory.md)。
