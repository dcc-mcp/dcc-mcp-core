# 反馈 API

> **[English](../../api/feedback.md)**

代理反馈与决策理由机制。Gateway 提供 `POST /v1/feedback` 与 `dcc-mcp-cli feedback`，即使没有在线 DCC 也可提交；在线实例的 `dcc_feedback__report` 仅作为共享 Core 实现的 gateway 转发入口。另可提取和构建 `tools/call` 请求中的 `_meta.dcc.rationale` 决策理由。

**导出符号：** `FINDING_V1_SCHEMA_VERSION`, `FindingRuntimeContext`, `FindingValidationError`, `build_finding_v1`, `finding_fingerprint`, `finding_v1_json_schema`, `register_feedback_tool`, `extract_rationale`, `make_rationale_meta`, `get_feedback_entries`, `clear_feedback`

## Finding v1 契约

规范化机器契约随 Python 包安装在
`dcc_mcp_core/schemas/feedback-finding-v1.schema.json`。Rust 侧通过
`dcc_mcp_models::FindingV1` 与 `FINDING_V1_JSON_SCHEMA` 使用相同 Schema；
Python 侧使用 `finding_v1_json_schema()` 与 `build_finding_v1(...)`。

Agent 提供 `phase`、`severity`、`intent`、`observed`、`expected`、唯一一种
`repro.argv`/`repro.steps`，并用 `tool_slug` 或 `evidence.error_kind` 标识主题。
Core 自动填充 DCC、adapter/core/host 版本、OS、instance、稳定 fingerprint，
并把 `redaction_status.mode` 设为 `needs-review`；这不表示内容已经适合公开。
重现列表最多 64 项、文本最多 4096 字符、标识符最多 256 字符、序列化 evidence
最多 32 KiB；未知字段或歧义结构会 fail-closed。

## 离线 Issue 路由

无需启动或连接 Gateway，即可把已校验的 Finding v1 文件解析到责任仓库：

```bash
dcc-mcp-cli feedback route finding.json --json
# 使用经过审查的自定义 catalog：
dcc-mcp-cli feedback route finding.json --catalog catalog.yml --json
```

该命令只读，只返回 `repo`、`issues_url` 和稳定的 `rationale`，不会创建
GitHub Issue。`install`、`startup`、`dispatch` 按 catalog 中精确的 adapter
包名路由；`evidence.error_kind` 属于 gateway、CLI 或 protocol 命名空间时，
优先路由到 `dcc-mcp-core`。没有这些共性错误类型的 `other` 会 fail-closed。

Skill Finding 不会默认继承 adapter 仓库，必须携带从 Skill
`metadata.dcc-mcp.links.repo` 和 `metadata.dcc-mcp.links.issues` 复制的有界证据：

```json
{
  "evidence": {
    "error_kind": "skill_contract_violation",
    "routing": {
      "source": "skill_metadata",
      "skill_name": "godot-export",
      "repo": "https://github.com/dcc-mcp/dcc-mcp-godot",
      "issues_url": "https://github.com/dcc-mcp/dcc-mcp-godot/issues"
    }
  }
}
```

归属信息缺失、重复、不规范或互相冲突时会显式失败，不会猜测其他仓库。
Finding 仍保持 `redaction_status.mode="needs-review"`；解析出路由不代表获准发布。

## Public-safe 反馈包

人工审查 Finding，并把 `redaction_status.mode` 设为 `public-safe`、所有排除标志
设为 true 后，可以组装有界诊断证据：

```bash
dcc-mcp-cli feedback bundle finding.json --json
# 包含 install --execute --json 输出的终态 JSON：
dcc-mcp-cli feedback bundle finding.json --install-report install-report.json --json
# Finding 中没有 PID 或需要指定日志根目录时：
dcc-mcp-cli feedback bundle finding.json --dcc-pid 4321 --log-dir /safe/log/root --json
```

`feedback bundle` 只读且不会自动启动 Gateway。它组合已审查 Finding、脱敏后的
`doctor` 快照、版本矩阵、`evidence.request_id` 对应的 public-safe issue report，
以及准确 `dcc-mcp-<dcc>.<pid>.host-errors.log` 常规文件的尾部。Host-error 输入最多
256 KiB，默认 50 条（`--host-error-lines` 最大 200）；输出会排除原始 message、
traceback、metadata、路径、token 与 DCC PID。PID 来自 `--dcc-pid` 或
`evidence.dcc_pid`；日志根目录按 `--log-dir`、`DCC_MCP_LOG_DIR`、平台默认目录解析。

`--install-report` 接受一个终态 Install SOP v1 执行报告，输入必须是最大 256 KiB 的
常规非符号链接文件。报告的 DCC 类型、Core 版本和 adapter 版本必须与 Finding 完全一致
（包括双方都记录为 `unknown` 的情况）。CLI 会在收集其余 bundle 证据前拒绝格式错误、
非终态、超限或身份不匹配的报告，并只通过同一 public-safe 路径/凭据投影输出已审查
字段。CLI 会先按已发布的 Install SOP v1 Draft 2020-12 schema 验证原始 JSON，
包括每个 next step 必须且只能包含 `command` 或 `file_edit`。公开输出会脱敏敏感命令
option/value 对、相对与绝对路径及所有 URL scheme；`file_edit.content` 与输入报告路径
绝不会输出。原始命令输出与异常文本不属于可接受输入。

结果契约为 `dcc-mcp.feedback-bundle.v1`。每个组件显式返回 `included`、
`not_applicable` 或 `unavailable`，缺失证据不会伪装为完成。不传 `--install-report`
会把该组件标为 `unavailable`；只有所有组件都已解析时才返回 `complete=true`。命令
不提供 raw bundle 模式；raw issue report 和 host log 只能留在本地人工审查，禁止
自动附加。

## 授权与去重后的 Issue 提交

使用已审查且 public-safe 的 Finding 生成只读提交计划，无需启动 Gateway：

```bash
dcc-mcp-cli feedback file finding.json --json
# 审查 next_step 并获得用户授权后，原样执行 next_step.argv；禁止手工删减或重建。
```

第一条命令只读。它先解析责任仓库并校验 fingerprint 与仓库绑定，再通过 `gh`
查询 open issue：先查 fingerprint digest，完全没有精确命中时才执行有界标题关键词
查询；完整 `sha256:` fingerprint 会在本地对返回的标题和正文严格匹配。唯一精确
命中建议评论，完全无候选建议新建；只有关键词命中、多个命中或结果截断时必须
人工选择，CLI 不会自动决定。

任何写入都必须同时提供 `--yes`、唯一决策，以及只读计划生成的完整授权绑定。
返回 argv 绑定规范 Finding 路径，并把 catalog 来源绑定为规范路径或精确的内置
catalog sentinel，同时绑定 Finding 内容 SHA-256、fingerprint、责任仓库和 catalog
SHA-256；CLI 在 tracker I/O 前及实际写入前立即重新捕获并校验，
因此路径、内容、工作目录、仓库或 catalog 漂移都会 fail-closed。CLI 还会在写入前
再次查询精确 fingerprint。所有 `gh` 操作固定到 `github.com`，最长运行 30 秒，并在
启动时纳入受控进程树；超时会终止并回收整棵进程树，所有 pipe worker 也受硬清理
时限约束。超过 65,536 个 Unicode 标量值的 Issue/comment 正文会在 tracker I/O 前
拒绝。通过校验的正文只投影经过审查的 Finding v1 字段，排除 request、job、
instance、原始证据及 extra 字段，并通过 stdin 传给 `gh`。当前命令尚不负责多
Finding 分组，也不应用组织级 Issue form 与 labels。

## 主要函数

- `register_feedback_tool(server, *, dcc_name="dcc", gateway_endpoint=None, gateway_host=None, gateway_port=None, instance_id_provider=None, finding_context_provider=None)` — 注册 `dcc_feedback__report` MCP 工具，**在 `server.start()` 之前调用**；优先接收 Finding v1 的 Agent 字段，兼容旧 `tool_name`/`blocker` 形式并规范化为 v1。Core 自动附加运行时身份，转发到 gateway，严格校验 `X-Request-ID`、schema version 与 fingerprint，并在身份缺失、gateway 不可用或回执失配时 fail-closed，不会本地伪成功
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
dcc-mcp-cli feedback route finding.json --json
dcc-mcp-cli feedback bundle reviewed-finding.json --json
dcc-mcp-cli feedback file reviewed-finding.json --json
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
