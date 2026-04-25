# 桥接 API

> **[English](../api/bridge.md)**

WebSocket JSON-RPC 2.0 桥接，用于非 Python DCC 应用程序。`DccBridge` 启动 WebSocket 服务器，DCC 插件连接后通过同步 RPC 调用 DCC 功能。同时包含桥接注册表系统用于管理命名的协议桥接。

**导出符号：** `DccBridge`, `BridgeError`, `BridgeConnectionError`, `BridgeTimeoutError`, `BridgeRpcError`, `BridgeRegistry`, `BridgeContext`, `register_bridge`, `get_bridge_context`

## DccBridge

WebSocket 桥接服务器，用于与非 Python DCC（如通过 C++ 插件）通信。

- `DccBridge(host="localhost", port=9001, timeout=30.0)` — 创建桥接实例
- `.connect(wait_for_dcc=False)` — 启动 WebSocket 服务器；可选阻塞等待 DCC 插件连接
- `.call(method, **params) -> Any` — 同步 RPC 调用 DCC 插件（线程安全）
- `.disconnect()` — 关闭 WebSocket 服务器
- `.is_connected() -> bool`, `.endpoint -> str`（如 `"ws://localhost:9001"`）
- 上下文管理器：`with DccBridge(port=9001) as bridge: ...`

## 错误类型

- `BridgeError` — 所有 DccBridge 错误的基类
- `BridgeConnectionError(BridgeError)` — DCC 插件未连接/连接丢失
- `BridgeTimeoutError(BridgeError)` — 调用超时
- `BridgeRpcError(BridgeError)` — DCC 插件返回 JSON-RPC 错误；含 `.code`、`.message`、`.data` 属性

## 桥接注册表

- `BridgeRegistry` — 管理命名的协议桥接（RPyC ↔ MCP、HTTP ↔ IPC）
- `BridgeContext` — 桥接上下文，含名称、描述、元数据
- `register_bridge(name, ctx)` — 注册命名桥接
- `get_bridge_context(name) -> Optional[BridgeContext]` — 按名称检索桥接

详见 [English API 参考](../api/bridge.md)。
