# 反馈 API

> **[English](../../api/feedback.md)**

代理反馈与决策理由机制。Gateway 提供 `POST /v1/feedback` 与 `dcc-mcp-cli feedback`，即使没有在线 DCC 也可提交；在线实例的 `dcc_feedback__report` 仅作为共享 Core 实现的 gateway 转发入口。另可提取和构建 `tools/call` 请求中的 `_meta.dcc.rationale` 决策理由。

**导出符号：** `register_feedback_tool`, `extract_rationale`, `make_rationale_meta`, `get_feedback_entries`, `clear_feedback`

## 主要函数

- `register_feedback_tool(server, *, dcc_name="dcc", gateway_endpoint=None, gateway_host=None, gateway_port=None, instance_id_provider=None)` — 注册 `dcc_feedback__report` MCP 工具，**在 `server.start()` 之前调用**；Core 会附加当前 DCC/instance，转发到 gateway，严格校验 `X-Request-ID` 与回执，并在 gateway 不可用或回执失配时 fail-closed，不会本地伪成功
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
dcc-mcp-cli feedback list --range 7d --dcc <dcc> --severity blocked --json
dcc-mcp-cli feedback export --range all --dcc <dcc> --json
```

Gateway 会把有界的 `feedback_reported` 记录写入
`resources://gateway/events` 并返回 `feedback_id`；它不依赖在线 DCC，也不会创建外部 issue。只提交经过脱敏的值，绝不能包含凭据、可复用令牌或原始敏感载荷。实例退出后直接使用 gateway CLI/REST，并携带已退出 instance 及最后的 request/job id；不要再依赖已消失的实例工具。gateway 路径失败时，
保留 CLI 返回的 `request_id`，获取 public-safe
`/v1/debug/issue-reports/<request_id>`；`?mode=raw` 必须本地人工审查，禁止自动上传。
Skill 缺陷归属对应 Skill，adapter/host runtime 缺陷归属 adapter 仓库，
CLI/gateway/protocol 共性缺陷归属 `dcc-mcp-core`。只有用户授权后才创建外部 issue。

Adapter 会把 Gateway 已接受的反馈同步写入共享 registry 下有界轮转的 JSONL。
`feedback list` 默认返回 100 条，`feedback export` 默认返回 endpoint 上限 1,000 条；
两者都调用 `GET /admin/api/feedback`，按时间倒序并按 feedback id 去重。响应中的
`skipped_invalid`、`deduplicated`、`files_scanned` 用于审计输入集合；目录/文件读取
失败或超过扫描边界时会显式失败，不会把不完整导出伪装成成功。

详见 [English API 参考](../../api/feedback.md)。
