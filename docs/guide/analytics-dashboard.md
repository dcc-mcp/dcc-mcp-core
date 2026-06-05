# Analytics Dashboard

The gateway analytics dashboard provides aggregate visibility into gateway
traffic, performance, and token usage over configurable time ranges. It is
available as a built-in panel in the `/admin` web UI and through REST API
endpoints under `/admin/api/analytics/*`.

## Activation

The analytics dashboard is available on any gateway built with the `admin`
feature (default). There is no separate configuration flag — if `/admin` is
enabled, the analytics panel is available.

```bash
# Default: analytics included
dcc-mcp-server --app maya

# Disable admin (also disables analytics)
dcc-mcp-server --no-admin
```

## REST API Endpoints

All endpoints require a `range` query parameter indicating the look-back
window. Supported values: `7d`, `30d` (default), `90d`, `180d`, `365d`.

### `GET /admin/api/analytics/overview`

Aggregate KPIs for the selected time range.

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

Time-series broken down by DCC type. Supports `granularity=day` (default) or
`granularity=hour`.

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

Hour-of-day × day-of-week heatmap data.

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

Bulk data export. Accepts `format=csv` (default) or `format=jsonl`.

```text
GET /admin/api/analytics/export?range=30d&format=csv
Content-Disposition: attachment; filename="analytics-export-30d.csv"
```

## Web UI Panel

The analytics panel in `/admin` renders:

- **Range selector**: 7d / 30d / 90d / 180d / 365d
- **KPI card grid**: total calls, success rate, failure count, input/output/saved
  tokens, average duration, LLM token breakdown, unique instances, unique agents
- **Daily trend mini-chart**: bar chart of calls per day; failed days highlighted
  in red
- **Heatmap**: 7 (weekday) × 24 (hour) grid with blue intensity gradient; hover
  for tooltip with call/failure counts
- **Top tools table**: ranked by call volume with failure count, success rate,
  and average duration
- **Export links**: CSV and JSONL download

## Data Source

Analytics are computed from `AdminAuditRecord` entries stored in:

1. **SQLite** (`~/.dcc-mcp/gateway/admin/audit.db`) when durable audit is
   configured via `DCC_MCP_GATEWAY_AUDIT_DIR`
2. **In-memory ring buffer** (default, last ~5000 records)

For persistent analytics across gateway restarts, set `DCC_MCP_GATEWAY_AUDIT_DIR`
to a writable directory.

## Data Aggregation

Aggregation runs on-demand when API endpoints are called. The core
`aggregate_audits()` function groups audit records by `(date, dcc_type, hour)`
into `DayAggregate` structs tracking:

| Field               | Source                                      |
|---------------------|---------------------------------------------|
| `calls_total`       | Count of audit records                      |
| `calls_success`     | Records with `success == true`              |
| `calls_failed`      | Records with `success == false`             |
| `tokens_input`      | Sum of `llm_usage.input_tokens`             |
| `tokens_output`     | Sum of `llm_usage.output_tokens`            |
| `tokens_saved`      | Sum of `context.tokens.total_saved`         |
| `llm_*`             | Direct LLM token counters                   |
| `duration_ms_*`     | Min/max/sum of `duration_ms`                |
| `instance_ids`      | Unique instance set                         |
| `agent_ids`         | Unique agent set                            |

## Limitations

- Data retention is bounded by the audit store capacity (default ~5000 in-memory
  records; for SQLite, by `DCC_MCP_GATEWAY_AUDIT_MAX_ROWS` / `MAX_BYTES`)
- Aggregation is computed on every request; very large time ranges or high-traffic
  gateways may see slower response times
- The dashboard refreshes every 5 seconds via React Query polling

## See Also

- [admin-ui.md](admin-ui.md) — full admin dashboard documentation
- [gateway.md](gateway.md) — gateway architecture and configuration
- [observability.md](observability.md) — metrics, tracing, and Prometheus export
