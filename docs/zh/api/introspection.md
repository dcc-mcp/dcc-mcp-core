# 内省 API

> **[English](../api/introspection.md)**

运行时 DCC 命名空间内省工具。代理可在运行时发现模块中的可用函数、获取签名和文档字符串、搜索符号，无需查阅外部文档。

**导出符号：** `register_introspect_tools`, `introspect_list_module`, `introspect_signature`, `introspect_search`, `introspect_eval`

## 主要函数

- `register_introspect_tools(server, *, dcc_name="dcc")` — 注册四个 `dcc_introspect__*` 工具，**在 `server.start()` 之前调用**
- `introspect_list_module(module_name, *, limit=200) -> dict` — 列出模块中的导出名称
- `introspect_signature(qualname) -> dict` — 获取可调用对象的签名和文档字符串（如 `"maya.cmds.polyCube"`）
- `introspect_search(pattern, module_name, *, limit=50) -> dict` — 在模块中正则搜索名称
- `introspect_eval(expression) -> dict` — 求值短小的只读表达式并返回 repr

详见 [English API 参考](../api/introspection.md)。
