//! Pure analytics projections over admin audit records.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Deserialize;
use serde_json::{Value, json};

use crate::{AdminAuditRecord, LlmUsage, TokenTelemetry};

/// Analytics query parameters shared by the admin and compatibility routes.
#[derive(Debug, Deserialize)]
pub struct AnalyticsQuery {
    /// Time range: `7d`, `30d`, `90d`, `180d`, or `365d`.
    #[serde(default = "default_range")]
    pub range: String,
    /// Aggregation granularity: `day` or `hour`.
    #[serde(default = "default_granularity")]
    pub granularity: String,
    /// Export format: `json` or `csv`.
    #[serde(default)]
    pub format: String,
}

fn default_range() -> String {
    "30d".into()
}

fn default_granularity() -> String {
    "day".into()
}

/// Parse a supported analytics range, falling back to 30 days.
#[must_use]
pub fn analytics_range_duration(range: &str) -> Duration {
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

#[derive(Debug, Clone)]
struct DayAggregate {
    date: String,
    dcc_type: String,
    hour: Option<u32>,
    calls_total: u64,
    calls_success: u64,
    calls_failed: u64,
    tokens_input: u64,
    tokens_output: u64,
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
        tokens: &TokenTelemetry,
        llm: Option<&LlmUsage>,
        instance_id: Option<&str>,
        agent_id: Option<&str>,
    ) {
        self.calls_total += 1;
        if success {
            self.calls_success += 1;
        } else {
            self.calls_failed += 1;
        }

        let duration_ms = duration_ms.unwrap_or(0);
        self.duration_ms_sum += duration_ms;
        self.duration_ms_min = self.duration_ms_min.min(duration_ms);
        self.duration_ms_max = self.duration_ms_max.max(duration_ms);
        self.tokens_input += tokens.original_tokens as u64;
        self.tokens_output += tokens.returned_tokens as u64;
        self.tokens_saved += tokens.saved_tokens as u64;

        if let Some(llm) = llm {
            self.llm_prompt += llm.prompt_tokens.unwrap_or(0);
            self.llm_completion += llm.completion_tokens.unwrap_or(0);
            self.llm_total += llm.total_tokens.unwrap_or(0);
        }

        push_unique(&mut self.instance_ids, instance_id);
        push_unique(&mut self.agent_ids, agent_id);
    }

    fn avg_duration_ms(&self) -> f64 {
        if self.calls_total == 0 {
            0.0
        } else {
            self.duration_ms_sum as f64 / self.calls_total as f64
        }
    }
}

fn push_unique(values: &mut Vec<String>, candidate: Option<&str>) {
    if let Some(candidate) = candidate
        && !candidate.is_empty()
        && !values.iter().any(|value| value == candidate)
    {
        values.push(candidate.to_string());
    }
}

fn days_to_ymd(mut days: i64) -> (i64, u32, u32) {
    days += 719_468;
    let era = if days >= 0 { days } else { days - 146_096 } / 146_097;
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * month_prime + 2) / 5 + 1) as u32;
    let month = if month_prime < 10 {
        month_prime + 3
    } else {
        month_prime - 9
    } as u32;
    (if month <= 2 { year + 1 } else { year }, month, day)
}

fn format_day(time: SystemTime) -> String {
    let seconds = time
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    let (year, month, day) = days_to_ymd((seconds / 86_400) as i64);
    format!("{year:04}-{month:02}-{day:02}")
}

fn format_day_ms(timestamp_ms: u64) -> String {
    let (year, month, day) = days_to_ymd((timestamp_ms / 1000 / 86_400) as i64);
    format!("{year:04}-{month:02}-{day:02}")
}

fn hour_from_ms(timestamp_ms: i64) -> u32 {
    ((timestamp_ms / 1000 % 86_400) / 3600) as u32
}

fn weekday_from_ms(timestamp_ms: i64) -> u32 {
    ((timestamp_ms / 1000 / 86_400 + 4) % 7) as u32
}

fn aggregate_audits(
    audits: &[AdminAuditRecord],
) -> HashMap<(String, String, Option<u32>), DayAggregate> {
    let mut aggregates = HashMap::new();
    for audit in audits {
        let timestamp_ms = audit
            .timestamp
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis() as i64)
            .unwrap_or(0);
        let date = format_day(audit.timestamp);
        let dcc_type = audit
            .dcc_type
            .as_deref()
            .filter(|value| !value.is_empty())
            .unwrap_or("unknown")
            .to_string();
        let hour = Some(hour_from_ms(timestamp_ms));
        let aggregate = aggregates
            .entry((date.clone(), dcc_type.clone(), hour))
            .or_insert_with(|| DayAggregate::new(date, dcc_type, hour));
        let default_tokens = TokenTelemetry {
            response_format: String::new(),
            token_estimator: String::new(),
            original_bytes: 0,
            returned_bytes: 0,
            original_tokens: 0,
            returned_tokens: 0,
            saved_tokens: 0,
            savings_pct: 0.0,
        };
        aggregate.ingest(
            audit.success,
            audit.duration_ms,
            audit.token_accounting.as_ref().unwrap_or(&default_tokens),
            audit.llm_usage.as_ref(),
            audit.instance_id.as_deref(),
            audit.agent_id.as_deref(),
        );
    }
    aggregates
}

fn merge_daily(
    aggregates: &HashMap<(String, String, Option<u32>), DayAggregate>,
) -> Vec<DayAggregate> {
    let mut by_day = HashMap::new();
    for aggregate in aggregates.values() {
        let entry = by_day
            .entry(aggregate.date.clone())
            .or_insert_with(|| DayAggregate::new(aggregate.date.clone(), "all".to_string(), None));
        entry.calls_total += aggregate.calls_total;
        entry.calls_success += aggregate.calls_success;
        entry.calls_failed += aggregate.calls_failed;
        entry.tokens_input += aggregate.tokens_input;
        entry.tokens_output += aggregate.tokens_output;
        entry.tokens_saved += aggregate.tokens_saved;
        entry.llm_prompt += aggregate.llm_prompt;
        entry.llm_completion += aggregate.llm_completion;
        entry.llm_total += aggregate.llm_total;
        entry.duration_ms_sum += aggregate.duration_ms_sum;
        entry.duration_ms_min = entry.duration_ms_min.min(aggregate.duration_ms_min);
        entry.duration_ms_max = entry.duration_ms_max.max(aggregate.duration_ms_max);
    }
    let mut result: Vec<_> = by_day.into_values().collect();
    result.sort_by(|left, right| left.date.cmp(&right.date));
    result
}

#[derive(Debug, Clone, serde::Serialize)]
struct HeatmapCell {
    weekday: u32,
    hour: u32,
    calls: u64,
    failures: u64,
    avg_duration_ms: f64,
    tokens_total: u64,
}

fn compute_heatmap(audits: &[AdminAuditRecord]) -> Vec<HeatmapCell> {
    let mut cells = HashMap::new();
    for audit in audits {
        let timestamp_ms = audit
            .timestamp
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis() as i64)
            .unwrap_or(0);
        let weekday = weekday_from_ms(timestamp_ms);
        let hour = hour_from_ms(timestamp_ms);
        let cell = cells.entry((weekday, hour)).or_insert(HeatmapCell {
            weekday,
            hour,
            calls: 0,
            failures: 0,
            avg_duration_ms: 0.0,
            tokens_total: 0,
        });
        cell.calls += 1;
        if !audit.success {
            cell.failures += 1;
        }
        let duration_ms = audit.duration_ms.unwrap_or(0) as f64;
        cell.avg_duration_ms =
            (cell.avg_duration_ms * (cell.calls - 1) as f64 + duration_ms) / cell.calls as f64;
        if let Some(tokens) = &audit.token_accounting {
            cell.tokens_total += tokens.original_tokens as u64 + tokens.returned_tokens as u64;
        }
        if let Some(llm) = &audit.llm_usage {
            cell.tokens_total += llm.total_tokens.unwrap_or(0);
        }
    }
    let mut result: Vec<_> = cells.into_values().collect();
    result.sort_by(|left, right| {
        left.weekday
            .cmp(&right.weekday)
            .then(left.hour.cmp(&right.hour))
    });
    result
}

#[derive(Debug, Clone, serde::Serialize)]
struct TopEntry {
    name: String,
    calls: u64,
    failures: u64,
    success_rate_pct: f64,
    avg_duration_ms: f64,
}

fn compute_top_tools(audits: &[AdminAuditRecord], top_n: usize) -> Vec<TopEntry> {
    let mut tools = HashMap::new();
    for audit in audits {
        let entry = tools.entry(audit.action.clone()).or_insert((0, 0, 0));
        entry.0 += 1;
        if !audit.success {
            entry.1 += 1;
        }
        entry.2 += audit.duration_ms.unwrap_or(0);
    }
    let mut entries: Vec<_> = tools
        .into_iter()
        .map(|(name, (calls, failures, duration_sum))| TopEntry {
            name,
            calls,
            failures,
            success_rate_pct: if calls > 0 {
                (calls - failures) as f64 / calls as f64 * 100.0
            } else {
                100.0
            },
            avg_duration_ms: if calls > 0 {
                duration_sum as f64 / calls as f64
            } else {
                0.0
            },
        })
        .collect();
    entries.sort_by_key(|entry| std::cmp::Reverse(entry.calls));
    entries.truncate(top_n);
    entries
}

/// Build the overview response body for a bounded audit slice.
#[must_use]
pub fn analytics_overview_payload(
    audits: &[AdminAuditRecord],
    range: &str,
    now: SystemTime,
) -> Value {
    let daily = merge_daily(&aggregate_audits(audits));
    let total_calls: u64 = daily.iter().map(|day| day.calls_total).sum();
    let total_failed: u64 = daily.iter().map(|day| day.calls_failed).sum();
    let total_input: u64 = daily.iter().map(|day| day.tokens_input).sum();
    let total_output: u64 = daily.iter().map(|day| day.tokens_output).sum();
    let total_saved: u64 = daily.iter().map(|day| day.tokens_saved).sum();
    let total_llm: u64 = daily.iter().map(|day| day.llm_total).sum();
    let total_duration_ms: u64 = daily.iter().map(|day| day.duration_ms_sum).sum();
    let unique_instances = count_unique(
        audits
            .iter()
            .filter_map(|audit| audit.instance_id.as_deref()),
    );
    let unique_agents = count_unique(audits.iter().filter_map(|audit| audit.agent_id.as_deref()));
    let period_start = now
        .checked_sub(analytics_range_duration(range))
        .map(|cutoff| {
            format_day_ms(
                cutoff
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64,
            )
        })
        .unwrap_or_default();

    json!({
        "range": range,
        "period_start": period_start,
        "period_end": format_day(now),
        "kpi": {
            "calls_total": total_calls,
            "calls_failed": total_failed,
            "failure_rate_pct": format_2dp(if total_calls > 0 { total_failed as f64 / total_calls as f64 * 100.0 } else { 0.0 }),
            "success_rate_pct": format_2dp(if total_calls > 0 { (total_calls - total_failed) as f64 / total_calls as f64 * 100.0 } else { 100.0 }),
            "tokens_input_total": total_input,
            "tokens_output_total": total_output,
            "tokens_response_saved": total_saved,
            "tokens_total": total_input + total_output,
            "llm_tokens_total": total_llm,
            "avg_duration_ms": format!("{:.1}", if total_calls > 0 { total_duration_ms as f64 / total_calls as f64 } else { 0.0 }),
            "avg_tokens_per_call": format!("{:.1}", if total_calls > 0 { (total_input + total_output) as f64 / total_calls as f64 } else { 0.0 }),
            "unique_instances": unique_instances,
            "unique_agents": unique_agents,
        },
        "top_tools": compute_top_tools(audits, 10),
        "daily_series": daily.iter().map(|day| json!({
            "date": day.date,
            "dcc_type": day.dcc_type,
            "calls": day.calls_total,
            "failures": day.calls_failed,
            "tokens_input": day.tokens_input,
            "tokens_output": day.tokens_output,
            "avg_duration_ms": format!("{:.1}", day.avg_duration_ms()),
            "max_duration_ms": day.duration_ms_max,
        })).collect::<Vec<_>>(),
    })
}

/// Build the hourly or daily time-series response body.
#[must_use]
pub fn analytics_timeseries_payload(
    audits: &[AdminAuditRecord],
    range: &str,
    granularity: &str,
) -> Value {
    let aggregates = aggregate_audits(audits);
    if granularity == "hour" {
        let mut series: Vec<Value> = aggregates
            .values()
            .map(|aggregate| {
                json!({
                    "date": aggregate.date,
                    "hour": aggregate.hour,
                    "dcc_type": aggregate.dcc_type,
                    "calls": aggregate.calls_total,
                    "failures": aggregate.calls_failed,
                    "tokens_input": aggregate.tokens_input,
                    "tokens_output": aggregate.tokens_output,
                    "avg_duration_ms": format!("{:.1}", aggregate.avg_duration_ms()),
                    "max_duration_ms": aggregate.duration_ms_max,
                })
            })
            .collect();
        series.sort_by_key(|row| {
            format!(
                "{}|{:02}|{}",
                row["date"].as_str().unwrap_or(""),
                row["hour"].as_u64().unwrap_or(0),
                row["dcc_type"].as_str().unwrap_or("")
            )
        });
        json!({ "range": range, "granularity": "hour", "series": series })
    } else {
        let daily = merge_daily(&aggregates);
        let series: Vec<Value> = daily
            .iter()
            .map(|day| {
                json!({
                    "date": day.date,
                    "calls": day.calls_total,
                    "failures": day.calls_failed,
                    "tokens_input": day.tokens_input,
                    "tokens_output": day.tokens_output,
                    "avg_duration_ms": format!("{:.1}", day.avg_duration_ms()),
                    "max_duration_ms": day.duration_ms_max,
                })
            })
            .collect();
        json!({ "range": range, "granularity": "day", "series": series })
    }
}

/// Build the weekday-by-hour heatmap response body.
#[must_use]
pub fn analytics_heatmap_payload(audits: &[AdminAuditRecord], range: &str) -> Value {
    json!({ "range": range, "heatmap": compute_heatmap(audits) })
}

/// Build the metadata-only JSON Lines analytics export.
#[must_use]
pub fn analytics_jsonl_export(audits: &[AdminAuditRecord]) -> String {
    let mut output = String::new();
    for audit in audits {
        output.push_str(
            &json!({
                "request_id": audit.request_id,
                "timestamp": audit.timestamp.duration_since(UNIX_EPOCH).map(|duration| duration.as_secs()).unwrap_or(0),
                "action": audit.action,
                "dcc_type": audit.dcc_type,
                "success": audit.success,
                "duration_ms": audit.duration_ms,
                "instance_id": audit.instance_id,
                "agent_id": audit.agent_id,
                "agent_name": audit.agent_name,
                "agent_model": audit.agent_model,
            })
            .to_string(),
        );
        output.push('\n');
    }
    output
}

/// Build the spreadsheet-safe CSV analytics export.
#[must_use]
pub fn analytics_csv_export(audits: &[AdminAuditRecord]) -> String {
    let mut output = String::from(
        "request_id,timestamp,action,dcc_type,success,duration_ms,instance_id,agent_id,agent_name,tokens_input,tokens_output,tokens_saved,llm_prompt,llm_completion,llm_total\n",
    );
    for audit in audits {
        let timestamp = audit
            .timestamp
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_secs())
            .unwrap_or(0);
        let (tokens_input, tokens_output, tokens_saved) = audit
            .token_accounting
            .as_ref()
            .map(|tokens| {
                (
                    tokens.original_tokens as u64,
                    tokens.returned_tokens as u64,
                    tokens.saved_tokens as u64,
                )
            })
            .unwrap_or_default();
        let (llm_prompt, llm_completion, llm_total) = audit
            .llm_usage
            .as_ref()
            .map(|llm| {
                (
                    llm.prompt_tokens.unwrap_or(0),
                    llm.completion_tokens.unwrap_or(0),
                    llm.total_tokens.unwrap_or(0),
                )
            })
            .unwrap_or_default();
        output.push_str(&csv_row(&[
            audit.request_id.as_str(),
            &timestamp.to_string(),
            audit.action.as_str(),
            audit.dcc_type.as_deref().unwrap_or(""),
            &(audit.success as u8).to_string(),
            &audit.duration_ms.unwrap_or(0).to_string(),
            audit.instance_id.as_deref().unwrap_or(""),
            audit.agent_id.as_deref().unwrap_or(""),
            audit.agent_name.as_deref().unwrap_or(""),
            &tokens_input.to_string(),
            &tokens_output.to_string(),
            &tokens_saved.to_string(),
            &llm_prompt.to_string(),
            &llm_completion.to_string(),
            &llm_total.to_string(),
        ]));
        output.push('\n');
    }
    output
}

fn format_2dp(value: f64) -> String {
    format!("{value:.2}")
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

#[cfg(test)]
mod tests {
    use super::*;

    fn audit_record(request_id: &str, timestamp: SystemTime, success: bool) -> AdminAuditRecord {
        AdminAuditRecord {
            timestamp,
            request_id: request_id.to_string(),
            trace_id: Some(format!("trace-{request_id}")),
            span_id: None,
            parent_span_id: None,
            method: Some("tools/call".to_string()),
            instance_id: Some("maya-instance".to_string()),
            session_id: None,
            transport: Some("rest".to_string()),
            agent_id: Some("agent-1".to_string()),
            agent_name: Some("Scene Agent".to_string()),
            agent_model: None,
            actor_id: None,
            actor_name: None,
            actor_email_hash: None,
            client_platform: None,
            client_os: None,
            client_host: None,
            auth_subject: None,
            source_ip: None,
            attribution_trust: None,
            parent_request_id: None,
            action: "maya.scene__info".to_string(),
            dcc_type: Some("maya".to_string()),
            success,
            error: (!success).then(|| "boom".to_string()),
            duration_ms: Some(40),
            token_accounting: None,
            llm_usage: None,
        }
    }

    #[test]
    fn range_parser_falls_back_to_thirty_days() {
        assert_eq!(
            analytics_range_duration("7d"),
            Duration::from_secs(7 * 86_400)
        );
        assert_eq!(
            analytics_range_duration("invalid"),
            Duration::from_secs(30 * 86_400)
        );
    }

    #[test]
    fn csv_cells_block_spreadsheet_formulas_and_quote_newlines() {
        assert_eq!(csv_cell("@agent"), "'@agent");
        assert_eq!(csv_cell("=cmd,tool\nline"), "\"'=cmd,tool\nline\"");
    }

    #[test]
    fn timeseries_and_heatmap_share_the_same_audit_aggregation() {
        let timestamp = UNIX_EPOCH + Duration::from_secs(4 * 86_400 + 2 * 3_600);
        let audits = [
            audit_record("req-ok", timestamp, true),
            audit_record("req-failed", timestamp, false),
        ];

        let timeseries = analytics_timeseries_payload(&audits, "7d", "hour");
        assert_eq!(timeseries["series"][0]["calls"], 2);
        assert_eq!(timeseries["series"][0]["failures"], 1);
        assert_eq!(timeseries["series"][0]["hour"], 2);

        let heatmap = analytics_heatmap_payload(&audits, "7d");
        assert_eq!(heatmap["heatmap"][0]["calls"], 2);
        assert_eq!(heatmap["heatmap"][0]["failures"], 1);
        assert_eq!(heatmap["heatmap"][0]["hour"], 2);
    }
}
