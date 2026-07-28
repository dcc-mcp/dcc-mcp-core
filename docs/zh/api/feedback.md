# 反馈 API

> **[English](../../api/feedback.md)**

代理反馈与决策理由机制。注册 `dcc_feedback__report` MCP 工具供代理提交反馈，提取和构建 `tools/call` 请求中的 `_meta.dcc.rationale` 决策理由。

**导出符号：** `register_feedback_tool`, `extract_rationale`, `make_rationale_meta`, `get_feedback_entries`, `clear_feedback`

## 主要函数

- `register_feedback_tool(server, *, dcc_name="dcc")` — 注册 `dcc_feedback__report` MCP 工具，**在 `server.start()` 之前调用**
- `extract_rationale(params) -> str | None` — 从 `tools/call` 参数中提取 `_meta.dcc.rationale`
- `make_rationale_meta(rationale) -> dict` — 构建包含 rationale 的 `_meta` 片段
- `get_feedback_entries(*, tool_name=None, severity=None, limit=50) -> list[dict]` — 获取最近的反馈条目（最新在前）
- `clear_feedback() -> int` — 清空内存中的反馈条目，返回清除数量

## Agent 错误上报工作流

具备 shell 能力的 Agent 使用公开 `dcc-mcp` Skill 和现有 CLI 接口：

```bash
dcc-mcp-cli doctor
dcc-mcp-cli stats --range 24h --status failure --session-id <session-id>
dcc-mcp-cli search --query "report feedback" --dcc-type <dcc>
dcc-mcp-cli describe <returned-feedback-tool-slug>
dcc-mcp-cli call <returned-feedback-tool-slug> --json \
  '{"tool_name":"tool_that_failed","intent":"goal","attempt":"sanitized attempt","blocker":"observed failure","severity":"blocked"}'
```

feedback 调用只记录结构化 runtime 信号，不会创建外部 issue。gateway 路径失败时，
保留 CLI 返回的 `request_id`，获取 public-safe
`/v1/debug/issue-reports/<request_id>`；`?mode=raw` 必须本地人工审查，禁止自动上传。
Skill 缺陷归属对应 Skill，adapter/host runtime 缺陷归属 adapter 仓库，
CLI/gateway/protocol 共性缺陷归属 `dcc-mcp-core`。只有用户授权后才创建外部 issue。

详见 [English API 参考](../../api/feedback.md)。
