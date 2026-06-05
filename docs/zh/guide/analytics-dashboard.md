# 分析仪表盘

网关分析仪表盘提供网关流量、性能和 Token 使用的聚合可见性，支持可配置的时间范围。
它作为内置面板在 `/admin` Web UI 中提供，也可通过 `/admin/api/analytics/*` 的
REST API 端点访问。

## 启用

分析仪表盘在启用了 `admin` 特性（默认）的网关上可用。无需单独的配置标志——
如果 `/admin` 已启用，分析面板即可使用。

```bash
# 默认：包含分析功能
dcc-mcp-server --app maya

# 禁用管理 UI（也会禁用分析）
dcc-mcp-server --no-admin
```

## REST API 端点

所有端点都需要 `range` 查询参数来指定回溯窗口。支持的值：
`7d`、`30d`（默认）、`90d`、`180d`、`365d`。

### `GET /admin/api/analytics/overview`

所选时间范围内的聚合 KPI。

```json
{
  "range_days": 30,
  "calls_total": 15420,
  "calls_success": 14803,
  "calls_failed": 617,
  "success_rate": 0.96,
  "tokens_input": 28500000,
  "tokens_output": 4200000,
  "tokens_saved": 12300000,
  "duration_ms_avg": 842,
  "duration_ms_min": 12,
  "duration_ms_max": 45000,
  "llm_prompt_tokens": 15000000,
  "llm_completion_tokens": 3800000,
  "llm_total_tokens": 18800000,
  "unique_instances": 12,
  "unique_agents": 8,
  "daily_series": [
    { "date": "2026-05-06", "calls": 512, "failed": 18, "success": 494,
      "tokens_input": 950000, "tokens_output": 140000, "tokens_saved": 410000 },
    ...
  ],
  "top_tools": [
    { "name": "maya_scene__get_session_info", "calls": 3200, "failed": 2,
      "success": 3198, "success_rate": 0.999, "duration_ms_avg": 45 },
    ...
  ]
}
```

### `GET /admin/api/analytics/timeseries`

按 DCC 类型细分的时间序列。支持 `granularity=day`（默认）或
`granularity=hour`。

```json
{
  "range_days": 30,
  "granularity": "day",
  "points": [
    { "timestamp": "2026-05-06T00:00:00Z", "calls": 512,
      "dcc_breakdown": { "maya": 320, "blender": 192 } },
    ...
  ]
}
```

### `GET /admin/api/analytics/heatmap`

小时 × 星期热力图数据。

```json
{
  "range_days": 30,
  "cells": [
    { "weekday": 1, "hour": 9, "calls": 142, "failed": 3 },
    { "weekday": 1, "hour": 10, "calls": 189, "failed": 1 },
    ...
  ],
  "max_calls": 234
}
```

### `GET /admin/api/analytics/export`

批量数据导出。接受 `format=csv`（默认）或 `format=jsonl`。

```text
GET /admin/api/analytics/export?range=30d&format=csv
Content-Disposition: attachment; filename="analytics-export-30d.csv"
```

## Web UI 面板

`/admin` 中的分析面板会渲染：

- **时间范围选择器**：7天 / 30天 / 90天 / 180天 / 365天
- **KPI 卡片网格**：总调用数、成功率、失败数、输入/输出/节省 Token、平均耗时、
  LLM Token 细分、唯一实例数、唯一 Agent 数
- **每日趋势迷你图**：每日调用量的柱状图；失败日以红色高亮
- **热力图**：7（星期）× 24（小时）网格，蓝色渐变色阶；悬停提示显示
  调用/失败计数
- **Top 工具表格**：按调用量排序，含失败数、成功率、平均耗时
- **导出链接**：CSV 和 JSONL 下载

## 数据源

分析数据从存储在以下位置的 `AdminAuditRecord` 条目计算：

1. **SQLite**（`~/.dcc-mcp/gateway/admin/audit.db`）— 通过
   `DCC_MCP_GATEWAY_AUDIT_DIR` 配置持久化审计后可用
2. **内存环形缓冲区**（默认，约最后 5000 条记录）

要实现跨网关重启的持久化分析，请设置 `DCC_MCP_GATEWAY_AUDIT_DIR` 为
可写目录。

## 数据聚合

API 端点被调用时按需运行聚合。核心 `aggregate_audits()` 函数将审计记录
按 `(date, dcc_type, hour)` 分组到 `DayAggregate` 结构中，跟踪：

| 字段               | 来源                                |
|--------------------|-------------------------------------|
| `calls_total`      | 审计记录计数                        |
| `calls_success`    | `success == true` 的记录            |
| `calls_failed`     | `success == false` 的记录           |
| `tokens_input`     | `llm_usage.input_tokens` 的总和     |
| `tokens_output`    | `llm_usage.output_tokens` 的总和    |
| `tokens_saved`     | `context.tokens.total_saved` 的总和 |
| `llm_*`            | 直接 LLM Token 计数器               |
| `duration_ms_*`    | `duration_ms` 的最小/最大/总和      |
| `instance_ids`     | 唯一实例集合                        |
| `agent_ids`        | 唯一 Agent 集合                     |

## 限制

- 数据保留受审计存储容量限制（内存模式默认约 5000 条记录；SQLite 模式受
  `DCC_MCP_GATEWAY_AUDIT_MAX_ROWS` / `MAX_BYTES` 限制）
- 聚合在每次请求时计算；非常大的时间范围或高流量网关可能响应较慢
- 仪表盘每 5 秒通过 React Query 轮询刷新

## 参见

- [admin-ui.md](admin-ui.md) — 完整管理仪表盘文档
- [gateway.md](gateway.md) — 网关架构与配置
- [observability.md](observability.md) — 指标、追踪与 Prometheus 导出
