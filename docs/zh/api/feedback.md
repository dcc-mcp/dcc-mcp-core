# 反馈 API

> **[English](../api/feedback.md)**

代理反馈与决策理由机制。注册 `dcc_feedback__report` MCP 工具供代理提交反馈，提取和构建 `tools/call` 请求中的 `_meta.dcc.rationale` 决策理由。

**导出符号：** `register_feedback_tool`, `extract_rationale`, `make_rationale_meta`, `get_feedback_entries`, `clear_feedback`

## 主要函数

- `register_feedback_tool(server, *, dcc_name="dcc")` — 注册 `dcc_feedback__report` MCP 工具，**在 `server.start()` 之前调用**
- `extract_rationale(params) -> str | None` — 从 `tools/call` 参数中提取 `_meta.dcc.rationale`
- `make_rationale_meta(rationale) -> dict` — 构建包含 rationale 的 `_meta` 片段
- `get_feedback_entries(*, tool_name=None, severity=None, limit=50) -> list[dict]` — 获取最近的反馈条目（最新在前）
- `clear_feedback() -> int` — 清空内存中的反馈条目，返回清除数量

详见 [English API 参考](../api/feedback.md)。
