//! Analytics aggregation for the executive dashboard (PIP-494 P1).
//!
//! Computes daily call aggregates from persisted audit rows and returns
//! overview KPI, time series, and weekday×hour heatmap data.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::extract::{Query, State};
use axum::http::HeaderValue;
use axum::http::{StatusCode, header};
use axum::response::IntoResponse;
use serde::Deserialize;
use serde_json::{Value, json};

use super::state::AdminState;

/// Analytics query parameters.
#[derive(Deserialize)]
pub struct AnalyticsQuery {
    /// Time range: "7d", "30d", "90d", "180d", "365d". Default "30d".
    #[serde(default = "default_range")]
    pub range: String,
    /// Aggregation granularity: "day" or "hour". Default "day".
    #[serde(default = "default_granularity")]
    pub granularity: String,
    /// Export format: "json" or "csv". Only valid for /export.
    #[serde(default)]
    pub format: String,
}

fn default_range() -> String {
    "30d".into()
}

fn default_granularity() -> String {
    "day".into()
}

/// Parse a range string into a Duration.
fn parse_range_duration(range: &str) -> Duration {
    let days: u64 = match range.trim_end_matches('d') {
        "7" => 7,
        "30" => 30,
        "90" => 90,
        "180" => 180,
        "365" => 365,
        _ => 30,
    };
    Duration::from_secs(days * 86_400)
}

/// One aggregated data point keyed by (date, dcc_type, hour).
#[derive(Debug, Clone)]
struct DayAggregate {
    date: String,
    dcc_type: String,
    hour: Option<u32>,
    calls_total: u64,
    calls_success: u64,
    calls_failed: u64,
    tokens_input: u64,  // original_tokens (before compaction)
    tokens_output: u64, // returned_tokens (after compaction)
    tokens_saved: u64,
    llm_prompt: u64,
    llm_completion: u64,
    llm_total: u64,
    duration_ms_sum: u64,
    duration_ms_min: u64,
    duration_ms_max: u64,
    instance_ids: Vec<String>,
    agent_ids: Vec<String>,
}

impl DayAggregate {
    fn new(date: String, dcc_type: String, hour: Option<u32>) -> Self {
        Self {
            date,
            dcc_type,
            hour,
            calls_total: 0,
            calls_success: 0,
            calls_failed: 0,
            tokens_input: 0,
            tokens_output: 0,
            tokens_saved: 0,
            llm_prompt: 0,
            llm_completion: 0,
            llm_total: 0,
            duration_ms_sum: 0,
            duration_ms_min: u64::MAX,
            duration_ms_max: 0,
            instance_ids: Vec::new(),
            agent_ids: Vec::new(),
        }
    }

    fn ingest(
        &mut self,
        success: bool,
        duration_ms: Option<u64>,
        tokens: &super::trace::TokenTelemetry,
        llm: Option<&super::trace::LlmUsage>,
        instance_id: Option<&str>,
        agent_id: Option<&str>,
    ) {
        self.calls_total += 1;
        if success {
            self.calls_success += 1;
        } else {
            self.calls_failed += 1;
        }

        let dms = duration_ms.unwrap_or(0);
        self.duration_ms_sum += dms;
        self.duration_ms_min = self.duration_ms_min.min(dms);
        self.duration_ms_max = self.duration_ms_max.max(dms);

        self.tokens_input += tokens.original_tokens as u64;
        self.tokens_output += tokens.returned_tokens as u64;
        self.tokens_saved += tokens.saved_tokens as u64;

        if let Some(llm) = llm {
            self.llm_prompt += llm.prompt_tokens.unwrap_or(0);
            self.llm_completion += llm.completion_tokens.unwrap_or(0);
            self.llm_total += llm.total_tokens.unwrap_or(0);
        }

        if let Some(id) = instance_id
            && !id.is_empty()
            && !self.instance_ids.iter().any(|i| i == id)
        {
            self.instance_ids.push(id.to_string());
        }
        if let Some(id) = agent_id
            && !id.is_empty()
            && !self.agent_ids.iter().any(|i| i == id)
        {
            self.agent_ids.push(id.to_string());
        }
    }

    fn avg_duration_ms(&self) -> f64 {
        if self.calls_total == 0 {
            0.0
        } else {
            self.duration_ms_sum as f64 / self.calls_total as f64
        }
    }
}

/// Compute the "all dcc" rollup key from a dcc_type field.
fn dcc_rollup_key(dcc: &Option<String>) -> String {
    dcc.as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or("unknown")
        .to_string()
}

// ── Date helpers ──────────────────────────────────────────────────────────

fn days_to_ymd(mut days: i64) -> (i64, u32, u32) {
    days += 719_468;
    let era = if days >= 0 { days } else { days - 146_096 } / 146_097;
    let doe = days - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn format_day(t: SystemTime) -> String {
    let secs = t
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days_since_epoch = secs / 86_400;
    let (y, m, d) = days_to_ymd(days_since_epoch as i64);
    format!("{y:04}-{m:02}-{d:02}")
}

fn format_day_ts(ts_ms: u64) -> String {
    let secs = ts_ms / 1000;
    let days_since_epoch = secs / 86_400;
    let (y, m, d) = days_to_ymd(days_since_epoch as i64);
    format!("{y:04}-{m:02}-{d:02}")
}

fn hour_from_ms(ts_ms: i64) -> u32 {
    let secs = ts_ms / 1000;
    ((secs % 86_400) / 3600) as u32
}

fn weekday_from_ms(ts_ms: i64) -> u32 {
    let secs = ts_ms / 1000;
    let days = secs / 86_400;
    ((days + 4) % 7) as u32
}

/// Aggregate audits into daily buckets.
fn aggregate_audits(
    audits: &[super::state::AdminAuditRecord],
) -> HashMap<(String, String, Option<u32>), DayAggregate> {
    let mut map: HashMap<(String, String, Option<u32>), DayAggregate> = HashMap::new();

    for a in audits {
        let ts_ms = a
            .timestamp
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        let day = format_day(a.timestamp);
        let dcc = dcc_rollup_key(&a.dcc_type);
        let hour = Some(hour_from_ms(ts_ms));

        let key = (day.clone(), dcc.clone(), hour);
        let entry = map
            .entry(key)
            .or_insert_with(|| DayAggregate::new(day, dcc, hour));

        // Default empty token telemetry when not present
        let default_tokens = super::trace::TokenTelemetry {
            response_format: String::new(),
            token_estimator: String::new(),
            original_bytes: 0,
            returned_bytes: 0,
            original_tokens: 0,
            returned_tokens: 0,
            saved_tokens: 0,
            savings_pct: 0.0,
        };
        let tokens = a.token_accounting.as_ref().unwrap_or(&default_tokens);

        entry.ingest(
            a.success,
            a.duration_ms,
            tokens,
            a.llm_usage.as_ref(),
            a.instance_id.as_deref(),
            a.agent_id.as_deref(),
        );
    }

    map
}

// ── Heatmap aggregation ───────────────────────────────────────────────────

#[derive(Debug, Clone, serde::Serialize)]
struct HeatmapCell {
    weekday: u32,
    hour: u32,
    calls: u64,
    failures: u64,
    avg_duration_ms: f64,
    tokens_total: u64,
}

fn compute_heatmap(audits: &[super::state::AdminAuditRecord]) -> Vec<HeatmapCell> {
    let mut cells: HashMap<(u32, u32), HeatmapCell> = HashMap::new();

    for a in audits {
        let ts_ms = a
            .timestamp
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        let wd = weekday_from_ms(ts_ms);
        let h = hour_from_ms(ts_ms);

        let cell = cells.entry((wd, h)).or_insert(HeatmapCell {
            weekday: wd,
            hour: h,
            calls: 0,
            failures: 0,
            avg_duration_ms: 0.0,
            tokens_total: 0,
        });

        cell.calls += 1;
        if !a.success {
            cell.failures += 1;
        }
        let dms = a.duration_ms.unwrap_or(0) as f64;
        cell.avg_duration_ms =
            (cell.avg_duration_ms * (cell.calls - 1) as f64 + dms) / cell.calls as f64;

        if let Some(t) = &a.token_accounting {
            cell.tokens_total += t.original_tokens as u64;
            cell.tokens_total += t.returned_tokens as u64;
        }
        if let Some(llm) = &a.llm_usage {
            cell.tokens_total += llm.total_tokens.unwrap_or(0);
        }
    }

    let mut result: Vec<_> = cells.into_values().collect();
    result.sort_by(|a, b| a.weekday.cmp(&b.weekday).then(a.hour.cmp(&b.hour)));
    result
}

// ── Top-N helpers ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, serde::Serialize)]
struct TopEntry {
    name: String,
    calls: u64,
    failures: u64,
    success_rate_pct: f64,
    avg_duration_ms: f64,
}

fn compute_top_tools(audits: &[super::state::AdminAuditRecord], top_n: usize) -> Vec<TopEntry> {
    let mut map: HashMap<String, (u64, u64, u64)> = HashMap::new();
    for a in audits {
        let key = a.action.clone();
        let entry = map.entry(key).or_insert((0, 0, 0));
        entry.0 += 1;
        if !a.success {
            entry.1 += 1;
        }
        entry.2 += a.duration_ms.unwrap_or(0);
    }
    let mut entries: Vec<_> = map
        .into_iter()
        .map(|(name, (calls, failures, dsum))| TopEntry {
            name,
            calls,
            failures,
            success_rate_pct: if calls > 0 {
                ((calls - failures) as f64 / calls as f64) * 100.0
            } else {
                100.0
            },
            avg_duration_ms: if calls > 0 {
                dsum as f64 / calls as f64
            } else {
                0.0
            },
        })
        .collect();
    entries.sort_by_key(|b| std::cmp::Reverse(b.calls));
    entries.truncate(top_n);
    entries
}

// ── Helper: merge aggregates into daily rollup ────────────────────────────

fn merge_daily(
    aggregates: &HashMap<(String, String, Option<u32>), DayAggregate>,
) -> Vec<DayAggregate> {
    let mut day_map: HashMap<String, DayAggregate> = HashMap::new();
    for agg in aggregates.values() {
        let entry = day_map
            .entry(agg.date.clone())
            .or_insert_with(|| DayAggregate::new(agg.date.clone(), "all".to_string(), None));
        entry.calls_total += agg.calls_total;
        entry.calls_success += agg.calls_success;
        entry.calls_failed += agg.calls_failed;
        entry.tokens_input += agg.tokens_input;
        entry.tokens_output += agg.tokens_output;
        entry.tokens_saved += agg.tokens_saved;
        entry.llm_prompt += agg.llm_prompt;
        entry.llm_completion += agg.llm_completion;
        entry.llm_total += agg.llm_total;
        entry.duration_ms_sum += agg.duration_ms_sum;
        entry.duration_ms_min = entry.duration_ms_min.min(agg.duration_ms_min);
        entry.duration_ms_max = entry.duration_ms_max.max(agg.duration_ms_max);
    }
    let mut result: Vec<_> = day_map.into_values().collect();
    result.sort_by(|a, b| a.date.cmp(&b.date));
    result
}

// ── Handler: overview ─────────────────────────────────────────────────────

pub async fn handle_admin_analytics_overview(
    State(s): State<AdminState>,
    Query(params): Query<AnalyticsQuery>,
) -> impl IntoResponse {
    let range_duration = parse_range_duration(&params.range);
    let cutoff = SystemTime::now().checked_sub(range_duration);

    let audits = fetch_audits(&s, cutoff);

    let aggregates = aggregate_audits(&audits);
    let daily = merge_daily(&aggregates);

    let total_calls: u64 = daily.iter().map(|d| d.calls_total).sum();
    let total_failed: u64 = daily.iter().map(|d| d.calls_failed).sum();
    let total_input: u64 = daily.iter().map(|d| d.tokens_input).sum();
    let total_output: u64 = daily.iter().map(|d| d.tokens_output).sum();
    let total_saved: u64 = daily.iter().map(|d| d.tokens_saved).sum();
    let total_llm: u64 = daily.iter().map(|d| d.llm_total).sum();
    let total_dur_ms: u64 = daily.iter().map(|d| d.duration_ms_sum).sum();
    let unique_instances = count_unique(audits.iter().filter_map(|a| a.instance_id.as_deref()));
    let unique_agents = count_unique(audits.iter().filter_map(|a| a.agent_id.as_deref()));

    let top_tools = compute_top_tools(&audits, 10);
    let period_start = cutoff
        .map(|c| format_day_ts(c.duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64))
        .unwrap_or_default();
    let period_end = format_day(SystemTime::now());

    let body = json!({
        "range": params.range,
        "period_start": period_start,
        "period_end": period_end,
        "kpi": {
            "calls_total": total_calls,
            "calls_failed": total_failed,
            "failure_rate_pct": format_2dp(if total_calls > 0 { (total_failed as f64 / total_calls as f64) * 100.0 } else { 0.0 }),
            "success_rate_pct": format_2dp(if total_calls > 0 { ((total_calls - total_failed) as f64 / total_calls as f64) * 100.0 } else { 100.0 }),
            "tokens_input_total": total_input,
            "tokens_output_total": total_output,
            "tokens_response_saved": total_saved,
            "tokens_total": total_input + total_output,
            "llm_tokens_total": total_llm,
            "avg_duration_ms": format!("{:.1}", if total_calls > 0 { total_dur_ms as f64 / total_calls as f64 } else { 0.0 }),
            "avg_tokens_per_call": format!("{:.1}", if total_calls > 0 { (total_input + total_output) as f64 / total_calls as f64 } else { 0.0 }),
            "unique_instances": unique_instances,
            "unique_agents": unique_agents,
        },
        "top_tools": top_tools,
        "daily_series": daily.iter().map(|d| json!({
            "date": d.date,
            "dcc_type": d.dcc_type,
            "calls": d.calls_total,
            "failures": d.calls_failed,
            "tokens_input": d.tokens_input,
            "tokens_output": d.tokens_output,
            "avg_duration_ms": format!("{:.1}", d.avg_duration_ms()),
            "max_duration_ms": d.duration_ms_max,
        })).collect::<Vec<_>>(),
    });

    (StatusCode::OK, axum::Json(body)).into_response()
}

// ── Handler: timeseries ───────────────────────────────────────────────────

pub async fn handle_admin_analytics_timeseries(
    State(s): State<AdminState>,
    Query(params): Query<AnalyticsQuery>,
) -> impl IntoResponse {
    let range_duration = parse_range_duration(&params.range);
    let cutoff = SystemTime::now().checked_sub(range_duration);

    let audits = fetch_audits(&s, cutoff);

    if params.granularity == "hour" {
        let aggregates = aggregate_audits(&audits);
        let mut series: Vec<Value> = aggregates
            .values()
            .map(|agg| {
                json!({
                    "date": agg.date,
                    "hour": agg.hour,
                    "dcc_type": agg.dcc_type,
                    "calls": agg.calls_total,
                    "failures": agg.calls_failed,
                    "tokens_input": agg.tokens_input,
                    "tokens_output": agg.tokens_output,
                    "avg_duration_ms": format!("{:.1}", agg.avg_duration_ms()),
                    "max_duration_ms": agg.duration_ms_max,
                })
            })
            .collect();
        series.sort_by(|a, b| {
            let ak = format!(
                "{}|{:02}|{}",
                a["date"].as_str().unwrap_or(""),
                a["hour"].as_u64().unwrap_or(0),
                a["dcc_type"].as_str().unwrap_or("")
            );
            let bk = format!(
                "{}|{:02}|{}",
                b["date"].as_str().unwrap_or(""),
                b["hour"].as_u64().unwrap_or(0),
                b["dcc_type"].as_str().unwrap_or("")
            );
            ak.cmp(&bk)
        });

        let body = json!({ "range": params.range, "granularity": "hour", "series": series });
        (StatusCode::OK, axum::Json(body)).into_response()
    } else {
        let aggregates = aggregate_audits(&audits);
        let daily = merge_daily(&aggregates);
        let mut series: Vec<Value> = daily
            .iter()
            .map(|d| {
                json!({
                    "date": d.date,
                    "calls": d.calls_total,
                    "failures": d.calls_failed,
                    "tokens_input": d.tokens_input,
                    "tokens_output": d.tokens_output,
                    "avg_duration_ms": format!("{:.1}", d.avg_duration_ms()),
                    "max_duration_ms": d.duration_ms_max,
                })
            })
            .collect();
        series.sort_by(|a, b| {
            a["date"]
                .as_str()
                .unwrap_or("")
                .cmp(b["date"].as_str().unwrap_or(""))
        });

        let body = json!({ "range": params.range, "granularity": "day", "series": series });
        (StatusCode::OK, axum::Json(body)).into_response()
    }
}

// ── Handler: heatmap ──────────────────────────────────────────────────────

pub async fn handle_admin_analytics_heatmap(
    State(s): State<AdminState>,
    Query(params): Query<AnalyticsQuery>,
) -> impl IntoResponse {
    let range_duration = parse_range_duration(&params.range);
    let cutoff = SystemTime::now().checked_sub(range_duration);

    let audits = fetch_audits(&s, cutoff);
    let heatmap = compute_heatmap(&audits);

    let body = json!({ "range": params.range, "heatmap": heatmap });
    (StatusCode::OK, axum::Json(body)).into_response()
}

// ── Handler: export ───────────────────────────────────────────────────────

pub async fn handle_admin_analytics_export(
    State(s): State<AdminState>,
    Query(params): Query<AnalyticsQuery>,
) -> impl IntoResponse {
    let range_duration = parse_range_duration(&params.range);
    let cutoff = SystemTime::now().checked_sub(range_duration);

    let audits = fetch_audits(&s, cutoff);

    let fmt = if params.format.is_empty() {
        "json"
    } else {
        &params.format
    };

    if fmt == "csv" {
        let mut csv = String::from(
            "request_id,timestamp,action,dcc_type,success,duration_ms,instance_id,agent_id,agent_name,tokens_input,tokens_output,tokens_saved,llm_prompt,llm_completion,llm_total\n",
        );
        for a in &audits {
            let ts = a
                .timestamp
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let (ti, to, tsaved) = if let Some(t) = &a.token_accounting {
                (
                    t.original_tokens as u64,
                    t.returned_tokens as u64,
                    t.saved_tokens as u64,
                )
            } else {
                (0, 0, 0)
            };
            let (lp, lc, lt) = if let Some(llm) = &a.llm_usage {
                (
                    llm.prompt_tokens.unwrap_or(0),
                    llm.completion_tokens.unwrap_or(0),
                    llm.total_tokens.unwrap_or(0),
                )
            } else {
                (0, 0, 0)
            };

            csv.push_str(&csv_row(&[
                a.request_id.as_str(),
                &ts.to_string(),
                a.action.as_str(),
                a.dcc_type.as_deref().unwrap_or(""),
                &(a.success as u8).to_string(),
                &a.duration_ms.unwrap_or(0).to_string(),
                a.instance_id.as_deref().unwrap_or(""),
                a.agent_id.as_deref().unwrap_or(""),
                a.agent_name.as_deref().unwrap_or(""),
                &ti.to_string(),
                &to.to_string(),
                &tsaved.to_string(),
                &lp.to_string(),
                &lc.to_string(),
                &lt.to_string(),
            ]));
            csv.push('\n');
        }

        let filename = format!("dcc-mcp-analytics-export-{}.csv", params.range);
        let mut response = axum::response::Response::new(axum::body::Body::from(csv));
        response.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("text/csv; charset=utf-8"),
        );
        response.headers_mut().insert(
            header::CONTENT_DISPOSITION,
            HeaderValue::from_str(&format!("attachment; filename=\"{filename}\""))
                .unwrap_or_else(|_| HeaderValue::from_static("attachment")),
        );
        response
    } else {
        let mut jsonl = String::new();
        for a in &audits {
            let obj = json!({
                "request_id": a.request_id,
                "timestamp": a.timestamp.duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0),
                "action": a.action,
                "dcc_type": a.dcc_type,
                "success": a.success,
                "duration_ms": a.duration_ms,
                "instance_id": a.instance_id,
                "agent_id": a.agent_id,
                "agent_name": a.agent_name,
                "agent_model": a.agent_model,
            });
            jsonl.push_str(&obj.to_string());
            jsonl.push('\n');
        }

        let filename = format!("dcc-mcp-analytics-export-{}.jsonl", params.range);
        let mut response = axum::response::Response::new(axum::body::Body::from(jsonl));
        response.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/x-ndjson; charset=utf-8"),
        );
        response.headers_mut().insert(
            header::CONTENT_DISPOSITION,
            HeaderValue::from_str(&format!("attachment; filename=\"{filename}\""))
                .unwrap_or_else(|_| HeaderValue::from_static("attachment")),
        );
        response
    }
}

// ── helpers ───────────────────────────────────────────────────────────────

fn format_2dp(v: f64) -> String {
    format!("{:.2}", v)
}

fn count_unique<'a>(values: impl Iterator<Item = &'a str>) -> usize {
    values
        .filter(|value| !value.trim().is_empty())
        .collect::<HashSet<_>>()
        .len()
}

fn csv_row(cells: &[&str]) -> String {
    cells
        .iter()
        .map(|cell| csv_cell(cell))
        .collect::<Vec<_>>()
        .join(",")
}

fn csv_cell(raw: &str) -> String {
    let mut value = raw.to_string();
    if value
        .chars()
        .next()
        .is_some_and(|first| matches!(first, '=' | '+' | '-' | '@' | '\t' | '\r'))
    {
        value.insert(0, '\'');
    }

    if value.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value
    }
}

/// Fetch audit records from SQLite or in-memory ring buffer.
fn fetch_audits(s: &AdminState, cutoff: Option<SystemTime>) -> Vec<super::state::AdminAuditRecord> {
    if let Some(ref lane) = s.admin_sqlite_lane {
        let reader = lane.reader();
        reader.list_audits_since(cutoff, 50_000)
    } else if let Some(ref log) = s.audit_log {
        log.lock()
            .iter()
            .filter(|a| cutoff.is_none_or(|c| a.timestamp >= c))
            .cloned()
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    }
}
