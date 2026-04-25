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

详见 [English API 参考](../api/factory.md)。
