# 文档资源 API

> **[English](../api/docs-resources.md)**

面向 AI 代理的 `docs://` MCP 资源服务。代理按需获取输出格式规范、skill 编写指南等文档，无需一次性加载全部内容。

**导出符号：** `get_builtin_docs_uris`, `get_docs_content`, `register_docs_resource`, `register_docs_resources_from_dir`, `register_docs_server`

## 内置 URI

- `docs://output-format/call-action` — 调用操作输出格式
- `docs://output-format/list-actions` — 列出操作输出格式
- `docs://skill-authoring/tools-yaml` — tools.yaml 编写指南
- `docs://skill-authoring/annotations` — 注解编写指南
- `docs://skill-authoring/sibling-files` — 同级文件模式
- `docs://skill-authoring/thin-harness` — 薄线束模式

## 主要函数

- `get_builtin_docs_uris() -> list[str]` — 列出内置 `docs://` URI
- `get_docs_content(uri) -> dict | None` — 获取 URI 对应的内容
- `register_docs_resource(server, *, uri, name, description, content, mime="text/markdown")` — 注册单个资源
- `register_docs_resources_from_dir(server, *, directory, uri_prefix="docs://custom", glob="**/*.md") -> list[str]` — 批量注册 Markdown 文件
- `register_docs_server(server)` — 注册所有内置资源，**在 `server.start()` 之前调用**

详见 [English API 参考](../api/docs-resources.md)。
