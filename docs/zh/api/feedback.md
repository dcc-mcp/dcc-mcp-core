# 反馈 API

> **[English](../../api/feedback.md)**

代理反馈与决策理由机制。Gateway 提供 `POST /v1/feedback` 与 `dcc-mcp-cli feedback`，即使没有在线 DCC 也可提交；`dcc_feedback__report` 继续作为在线实例兼容工具。另可提取和构建 `tools/call` 请求中的 `_meta.dcc.rationale` 决策理由。

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
dcc-mcp-cli feedback \
  --tool-name tool_that_failed \
  --intent "goal" \
  --attempt "sanitized attempt" \
  --blocker "observed failure" \
  --severity blocked \
  --dcc-type <dcc> \
  --instance-id <live-or-dead-instance-id> \
  --request-id <request-id> \
  --job-id <job-id>
```

Gateway 会把有界的 `feedback_reported` 记录写入
`resources://gateway/events` 并返回 `feedback_id`；它不依赖在线 DCC，也不会创建外部 issue。只提交经过脱敏的值，绝不能包含凭据、可复用令牌或原始敏感载荷。实例退出后不要再依赖实例级 `dcc_feedback__report`。gateway 路径失败时，
保留 CLI 返回的 `request_id`，获取 public-safe
`/v1/debug/issue-reports/<request_id>`；`?mode=raw` 必须本地人工审查，禁止自动上传。
Skill 缺陷归属对应 Skill，adapter/host runtime 缺陷归属 adapter 仓库，
CLI/gateway/protocol 共性缺陷归属 `dcc-mcp-core`。只有用户授权后才创建外部 issue。

详见 [English API 参考](../../api/feedback.md)。
