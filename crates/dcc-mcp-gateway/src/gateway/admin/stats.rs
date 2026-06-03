//! Phase 3 — Statistics aggregator for the admin UI `/api/stats` endpoint.
//!
//! Computes on-demand aggregations from the [`TraceLog`] ring buffer:
//!
//! - Overall call rate, success rate, total call count
//! - Latency percentiles (p50 / p95 / p99) in milliseconds
//! - Top-N tools by call count
//! - Top-N instances by call count
//! - Hour-of-day call distribution (24 buckets, UTC)
//!
//! All computations are pure (no background tasks, no write side), so Phase 3
//! adds zero to the `tools/call` hot path.  The `GET /admin/api/stats` handler
//! calls `StatsAggregator::compute()` on the current ring-buffer snapshot;
//! the whole operation takes O(N) time and memory where N = ring-buffer size
//! (default 200).

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use super::sqlite_lane::AdminSqliteReader;
use super::trace::{AgentContext, DispatchTrace, TraceLog};

/// How far back to consider when computing statistics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatsRange {
    /// Last 1 hour.
    Hour1,
    /// Last 24 hours.
    Hour24,
    /// Last 7 days.
    Day7,
    /// All data in the ring buffer (default).
    All,
}

impl StatsRange {
    /// Parse the `range` query string parameter from the admin UI.
    ///
    /// Recognises `"1h"`, `"24h"`, `"7d"`. Any other value (including
    /// `"all"`, the empty string, or unknown strings) maps to
    /// [`StatsRange::All`] — the handler intentionally falls back rather
    /// than 400 so a typo in the UI does not break the page.
    ///
    /// Intentionally does not implement [`std::str::FromStr`]: the
    /// "invalid input falls through to All" contract is incompatible
    /// with `FromStr`'s fallible shape.
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Self {
        match s {
            "1h" => Self::Hour1,
            "24h" => Self::Hour24,
            "7d" => Self::Day7,
            _ => Self::All,
        }
    }

    fn cutoff(&self) -> Option<SystemTime> {
        match self {
            Self::Hour1 => Some(SystemTime::now() - Duration::from_secs(3600)),
            Self::Hour24 => Some(SystemTime::now() - Duration::from_secs(86_400)),
            Self::Day7 => Some(SystemTime::now() - Duration::from_secs(7 * 86_400)),
            Self::All => None,
        }
    }
}

/// Snapshot of aggregate statistics for the admin Stats tab.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayStats {
    /// Time range these stats cover (e.g. `"1h"`, `"24h"`, `"7d"`, `"all"`).
    pub range: String,
    /// Total number of calls in the ring buffer that fall within `range`.
    pub total_calls: usize,
    /// Number of successful calls.
    pub successful_calls: usize,
    /// Number of failed calls.
    pub failed_calls: usize,
    /// Success rate as a fraction [0.0, 1.0].
    pub success_rate: f64,
    /// Sum of input payload token estimates in the selected range.
    pub total_input_tokens: u64,
    /// Sum of output payload token estimates in the selected range.
    pub total_output_tokens: u64,
    /// Sum of all payload token estimates in the selected range.
    pub total_tokens: u64,
    /// Average input token estimate per call.
    pub avg_input_tokens_per_call: f64,
    /// Average output token estimate per call.
    pub avg_output_tokens_per_call: f64,
    /// Average combined token estimate per call.
    pub avg_total_tokens_per_call: f64,
    /// Latency statistics in milliseconds.
    pub latency_ms: LatencyStats,
    /// Top DCC app types by call count (up to 10).
    pub top_app_types: Vec<TopEntry>,
    /// Top tools by call count (up to 10).
    pub top_tools: Vec<TopEntry>,
    /// Top DCC instances by call count (up to 10).
    pub top_instances: Vec<TopEntry>,
    /// Top client-supplied agents/callers by call count (up to 10).
    pub top_agents: Vec<TopEntry>,
    /// Top human/service actors by request count and health.
    pub top_actors: Vec<AttributionFacet>,
    /// Top client platforms by request count and health.
    pub top_client_platforms: Vec<AttributionFacet>,
    /// Top server-derived source IPs by request count and health.
    pub top_source_ips: Vec<AttributionFacet>,
    /// Payload request/response token estimates and coverage.
    pub payload_token_usage: PayloadTokenUsageStats,
    /// Token accounting totals and savings breakdowns.
    pub token_usage: TokenUsageStats,
    /// Call distribution across the 24 hours of the day (UTC, index 0 = midnight).
    ///
    /// Each element is the number of calls that started in that hour window
    /// within the selected `range`.
    pub hourly_distribution: Vec<u32>,
}

/// Latency percentile summary.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LatencyStats {
    /// Minimum observed latency in milliseconds.
    pub min_ms: u64,
    /// Maximum observed latency in milliseconds.
    pub max_ms: u64,
    /// Mean latency in milliseconds.
    pub mean_ms: f64,
    /// Median (p50) latency in milliseconds.
    pub p50_ms: u64,
    /// 95th-percentile latency in milliseconds.
    pub p95_ms: u64,
    /// 99th-percentile latency in milliseconds.
    pub p99_ms: u64,
}

/// A (name, count) pair for top-N rankings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TopEntry {
    pub name: String,
    pub count: usize,
}

/// Aggregated caller-attribution health for an operator-facing facet.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttributionFacet {
    pub name: String,
    pub count: usize,
    pub failed: usize,
    pub failure_rate: f64,
    pub mean_latency_ms: f64,
    pub p95_latency_ms: u64,
}

/// Aggregate payload token estimates for captured request/response previews.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PayloadTokenUsageStats {
    pub token_estimator: String,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub total_tokens: u64,
    pub calls_with_input_tokens: usize,
    pub calls_with_output_tokens: usize,
    pub calls_with_any_payload_tokens: usize,
    pub calls_missing_payload_tokens: usize,
    pub avg_input_tokens_per_call: f64,
    pub avg_output_tokens_per_call: f64,
    pub avg_total_tokens_per_call: f64,
    pub avg_total_tokens_per_recorded_call: f64,
}

impl PayloadTokenUsageStats {
    pub fn empty(total_calls: usize) -> Self {
        Self {
            token_estimator: crate::gateway::response_codec::TOKEN_ESTIMATOR.to_string(),
            total_input_tokens: 0,
            total_output_tokens: 0,
            total_tokens: 0,
            calls_with_input_tokens: 0,
            calls_with_output_tokens: 0,
            calls_with_any_payload_tokens: 0,
            calls_missing_payload_tokens: total_calls,
            avg_input_tokens_per_call: 0.0,
            avg_output_tokens_per_call: 0.0,
            avg_total_tokens_per_call: 0.0,
            avg_total_tokens_per_recorded_call: 0.0,
        }
    }
}

#[derive(Default)]
struct AttributionAccumulator {
    count: usize,
    failed: usize,
    latency_ms: Vec<u64>,
}

/// Aggregate token accounting for the selected stats range.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TokenUsageStats {
    pub total_original_bytes: usize,
    pub total_returned_bytes: usize,
    pub total_original_tokens: usize,
    pub total_returned_tokens: usize,
    pub total_saved_tokens: usize,
    pub average_savings_pct: f64,
    pub by_tool: Vec<TokenBreakdownEntry>,
    pub by_instance: Vec<TokenBreakdownEntry>,
    pub by_agent: Vec<TokenBreakdownEntry>,
    pub by_transport: Vec<TokenBreakdownEntry>,
    pub by_response_format: Vec<TokenBreakdownEntry>,
}

/// Token savings grouped by a stable dimension.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TokenBreakdownEntry {
    pub name: String,
    pub calls: usize,
    pub returned_tokens: usize,
    pub saved_tokens: usize,
    pub savings_pct: f64,
}

/// Computes on-demand statistics from the [`TraceLog`] ring buffer.
pub struct StatsAggregator {
    trace_log: Arc<TraceLog>,
    sqlite_reader: Option<AdminSqliteReader>,
}

impl StatsAggregator {
    pub fn new(trace_log: Arc<TraceLog>) -> Self {
        Self {
            trace_log,
            sqlite_reader: None,
        }
    }

    pub fn with_sqlite_reader(mut self, reader: AdminSqliteReader) -> Self {
        self.sqlite_reader = Some(reader);
        self
    }

    /// Compute statistics for the given range.
    ///
    /// Reads the ring buffer once (O(N)), performs a single pass, and returns
    /// a fully-materialised [`GatewayStats`] struct.
    pub fn compute(&self, range: StatsRange) -> GatewayStats {
        let cutoff = range.cutoff();
        let mut by_id: HashMap<String, DispatchTrace> = HashMap::new();
        if let Some(db) = &self.sqlite_reader {
            for t in db.list_traces_since(cutoff, 500_000) {
                by_id.insert(t.request_id.clone(), t);
            }
        }
        for t in self.trace_log.recent(usize::MAX) {
            by_id.insert(t.request_id.clone(), t);
        }
        let mut traces: Vec<DispatchTrace> = by_id
            .into_values()
            .filter(|t| cutoff.map(|c| t.started_at >= c).unwrap_or(true))
            .collect();
        traces.sort_by(|a, b| {
            let ta = a.started_at.duration_since(UNIX_EPOCH).ok();
            let tb = b.started_at.duration_since(UNIX_EPOCH).ok();
            tb.cmp(&ta)
        });
        compute_from_traces(&traces, range)
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn compute_from_traces(in_range: &[DispatchTrace], range: StatsRange) -> GatewayStats {
    let total_calls = in_range.len();
    if total_calls == 0 {
        return GatewayStats {
            range: range_label(range),
            total_calls: 0,
            successful_calls: 0,
            failed_calls: 0,
            success_rate: 0.0,
            total_input_tokens: 0,
            total_output_tokens: 0,
            total_tokens: 0,
            avg_input_tokens_per_call: 0.0,
            avg_output_tokens_per_call: 0.0,
            avg_total_tokens_per_call: 0.0,
            latency_ms: LatencyStats::default(),
            top_app_types: vec![],
            top_tools: vec![],
            top_instances: vec![],
            top_agents: vec![],
            top_actors: vec![],
            top_client_platforms: vec![],
            top_source_ips: vec![],
            payload_token_usage: PayloadTokenUsageStats::empty(0),
            token_usage: TokenUsageStats::default(),
            hourly_distribution: vec![0u32; 24],
        };
    }

    let successful_calls = in_range.iter().filter(|t| t.ok).count();
    let failed_calls = total_calls - successful_calls;
    let success_rate = successful_calls as f64 / total_calls as f64;
    let payload_token_usage = compute_payload_token_usage(in_range);
    let total_input_tokens = payload_token_usage.total_input_tokens;
    let total_output_tokens = payload_token_usage.total_output_tokens;
    let total_tokens = payload_token_usage.total_tokens;
    let avg_input_tokens_per_call = payload_token_usage.avg_input_tokens_per_call;
    let avg_output_tokens_per_call = payload_token_usage.avg_output_tokens_per_call;
    let avg_total_tokens_per_call = payload_token_usage.avg_total_tokens_per_call;

    let mut latencies: Vec<u64> = in_range.iter().map(|t| t.total_ms).collect();
    latencies.sort_unstable();
    let latency_ms = compute_latency_stats(&latencies);

    let mut app_type_counts: HashMap<String, usize> = HashMap::new();
    for t in in_range {
        let key = t.dcc_type.clone().unwrap_or_else(|| "unknown".to_string());
        *app_type_counts.entry(key).or_insert(0) += 1;
    }
    let top_app_types = top_n(app_type_counts, 10);

    let mut tool_counts: HashMap<String, usize> = HashMap::new();
    for t in in_range {
        if let Some(slug) = &t.tool_slug {
            *tool_counts.entry(slug.clone()).or_insert(0) += 1;
        }
    }
    let top_tools = top_n(tool_counts, 10);

    let mut instance_counts: HashMap<String, usize> = HashMap::new();
    for t in in_range {
        let key = t
            .instance_id
            .clone()
            .or_else(|| t.dcc_type.clone())
            .unwrap_or_else(|| "unknown".to_string());
        *instance_counts.entry(key).or_insert(0) += 1;
    }
    let top_instances = top_n(instance_counts, 10);

    let mut agent_counts: HashMap<String, usize> = HashMap::new();
    for t in in_range {
        if let Some(name) = t.agent_context.as_ref().and_then(agent_label) {
            *agent_counts.entry(name.to_string()).or_insert(0) += 1;
        }
    }
    let top_agents = top_n(agent_counts, 10);
    let mut actor_facets: HashMap<String, AttributionAccumulator> = HashMap::new();
    let mut platform_facets: HashMap<String, AttributionAccumulator> = HashMap::new();
    let mut source_ip_facets: HashMap<String, AttributionAccumulator> = HashMap::new();
    for t in in_range {
        let Some(ctx) = t.agent_context.as_ref() else {
            continue;
        };
        add_attribution_facet(&mut actor_facets, actor_label(ctx), t);
        add_attribution_facet(&mut platform_facets, ctx.client_platform.clone(), t);
        add_attribution_facet(&mut source_ip_facets, ctx.source_ip.clone(), t);
    }
    let top_actors = top_attribution_facets(actor_facets, 10);
    let top_client_platforms = top_attribution_facets(platform_facets, 10);
    let top_source_ips = top_attribution_facets(source_ip_facets, 10);
    let token_usage = compute_token_usage(in_range);

    let mut hourly = vec![0u32; 24];
    for t in in_range {
        let hour = t
            .started_at
            .duration_since(UNIX_EPOCH)
            .map(|d| ((d.as_secs() % 86_400) / 3600) as usize)
            .unwrap_or(0);
        if hour < 24 {
            hourly[hour] += 1;
        }
    }

    GatewayStats {
        range: range_label(range),
        total_calls,
        successful_calls,
        failed_calls,
        success_rate,
        total_input_tokens,
        total_output_tokens,
        total_tokens,
        avg_input_tokens_per_call,
        avg_output_tokens_per_call,
        avg_total_tokens_per_call,
        latency_ms,
        top_app_types,
        top_tools,
        top_instances,
        top_agents,
        top_actors,
        top_client_platforms,
        top_source_ips,
        payload_token_usage,
        token_usage,
        hourly_distribution: hourly,
    }
}

fn compute_payload_token_usage(traces: &[DispatchTrace]) -> PayloadTokenUsageStats {
    if traces.is_empty() {
        return PayloadTokenUsageStats::empty(0);
    }

    let mut stats = PayloadTokenUsageStats::empty(traces.len());
    stats.calls_missing_payload_tokens = 0;
    for trace in traces {
        let input = trace.input_tokens();
        let output = trace.output_tokens();
        if let Some(tokens) = input {
            stats.total_input_tokens += tokens as u64;
            stats.calls_with_input_tokens += 1;
        }
        if let Some(tokens) = output {
            stats.total_output_tokens += tokens as u64;
            stats.calls_with_output_tokens += 1;
        }
        if input.is_some() || output.is_some() {
            stats.calls_with_any_payload_tokens += 1;
        } else {
            stats.calls_missing_payload_tokens += 1;
        }
    }
    stats.total_tokens = stats
        .total_input_tokens
        .saturating_add(stats.total_output_tokens);
    let total_calls = traces.len() as f64;
    stats.avg_input_tokens_per_call = stats.total_input_tokens as f64 / total_calls;
    stats.avg_output_tokens_per_call = stats.total_output_tokens as f64 / total_calls;
    stats.avg_total_tokens_per_call = stats.total_tokens as f64 / total_calls;
    if stats.calls_with_any_payload_tokens > 0 {
        stats.avg_total_tokens_per_recorded_call =
            stats.total_tokens as f64 / stats.calls_with_any_payload_tokens as f64;
    }
    stats
}

fn actor_label(ctx: &AgentContext) -> Option<String> {
    ctx.actor_name
        .clone()
        .or_else(|| ctx.actor_id.clone())
        .or_else(|| ctx.auth_subject.clone())
        .or_else(|| ctx.actor_email_hash.clone())
}

fn agent_label(ctx: &AgentContext) -> Option<String> {
    let name = ctx
        .agent_name
        .clone()
        .or_else(|| ctx.agent_id.clone())
        .or_else(|| ctx.agent_kind.clone())
        .or_else(|| ctx.model.clone())
        .or_else(|| ctx.model_version.clone())?;
    if let Some(version) = ctx
        .agent_version
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        Some(format!("{name}@{version}"))
    } else {
        Some(name)
    }
}

fn add_attribution_facet(
    facets: &mut HashMap<String, AttributionAccumulator>,
    name: Option<String>,
    trace: &DispatchTrace,
) {
    let Some(name) = name.filter(|value| !value.trim().is_empty()) else {
        return;
    };
    let facet = facets.entry(name).or_default();
    facet.count += 1;
    if !trace.ok {
        facet.failed += 1;
    }
    facet.latency_ms.push(trace.total_ms);
}

fn top_attribution_facets(
    facets: HashMap<String, AttributionAccumulator>,
    limit: usize,
) -> Vec<AttributionFacet> {
    let mut rows: Vec<AttributionFacet> = facets
        .into_iter()
        .map(|(name, mut facet)| {
            facet.latency_ms.sort_unstable();
            let latency = compute_latency_stats(&facet.latency_ms);
            AttributionFacet {
                name,
                count: facet.count,
                failed: facet.failed,
                failure_rate: if facet.count == 0 {
                    0.0
                } else {
                    facet.failed as f64 / facet.count as f64
                },
                mean_latency_ms: latency.mean_ms,
                p95_latency_ms: latency.p95_ms,
            }
        })
        .collect();
    rows.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.name.cmp(&b.name)));
    rows.truncate(limit);
    rows
}

fn compute_token_usage(traces: &[DispatchTrace]) -> TokenUsageStats {
    let mut stats = TokenUsageStats::default();
    let mut by_tool = HashMap::<String, TokenAccumulator>::new();
    let mut by_instance = HashMap::<String, TokenAccumulator>::new();
    let mut by_agent = HashMap::<String, TokenAccumulator>::new();
    let mut by_transport = HashMap::<String, TokenAccumulator>::new();
    let mut by_response_format = HashMap::<String, TokenAccumulator>::new();

    for trace in traces {
        let Some(tokens) = trace.token_accounting.as_ref() else {
            continue;
        };
        stats.total_original_bytes += tokens.original_bytes;
        stats.total_returned_bytes += tokens.returned_bytes;
        stats.total_original_tokens += tokens.original_tokens;
        stats.total_returned_tokens += tokens.returned_tokens;
        stats.total_saved_tokens += tokens.saved_tokens;

        add_token_group(
            &mut by_tool,
            trace
                .tool_slug
                .clone()
                .unwrap_or_else(|| trace.method.clone()),
            tokens,
        );
        add_token_group(
            &mut by_instance,
            trace
                .instance_id
                .clone()
                .or_else(|| trace.dcc_type.clone())
                .unwrap_or_else(|| "unknown".to_string()),
            tokens,
        );
        add_token_group(
            &mut by_agent,
            trace
                .agent_context
                .as_ref()
                .and_then(agent_label)
                .unwrap_or_else(|| "unknown".to_string()),
            tokens,
        );
        add_token_group(
            &mut by_transport,
            trace
                .transport
                .clone()
                .unwrap_or_else(|| "unknown".to_string()),
            tokens,
        );
        add_token_group(
            &mut by_response_format,
            tokens.response_format.clone(),
            tokens,
        );
    }

    stats.average_savings_pct = savings_pct(stats.total_saved_tokens, stats.total_original_tokens);
    stats.by_tool = token_top_n(by_tool, 10);
    stats.by_instance = token_top_n(by_instance, 10);
    stats.by_agent = token_top_n(by_agent, 10);
    stats.by_transport = token_top_n(by_transport, 10);
    stats.by_response_format = token_top_n(by_response_format, 10);
    stats
}

#[derive(Default)]
struct TokenAccumulator {
    calls: usize,
    original_tokens: usize,
    returned_tokens: usize,
    saved_tokens: usize,
}

fn add_token_group(
    map: &mut HashMap<String, TokenAccumulator>,
    key: String,
    tokens: &super::trace::TokenTelemetry,
) {
    let entry = map.entry(key).or_default();
    entry.calls += 1;
    entry.original_tokens += tokens.original_tokens;
    entry.returned_tokens += tokens.returned_tokens;
    entry.saved_tokens += tokens.saved_tokens;
}

fn token_top_n(map: HashMap<String, TokenAccumulator>, n: usize) -> Vec<TokenBreakdownEntry> {
    let mut entries: Vec<_> = map
        .into_iter()
        .map(|(name, acc)| TokenBreakdownEntry {
            name,
            calls: acc.calls,
            returned_tokens: acc.returned_tokens,
            saved_tokens: acc.saved_tokens,
            savings_pct: savings_pct(acc.saved_tokens, acc.original_tokens),
        })
        .collect();
    entries.sort_by(|a, b| {
        b.saved_tokens
            .cmp(&a.saved_tokens)
            .then_with(|| b.returned_tokens.cmp(&a.returned_tokens))
            .then_with(|| a.name.cmp(&b.name))
    });
    entries.truncate(n);
    entries
}

fn savings_pct(saved_tokens: usize, original_tokens: usize) -> f64 {
    if original_tokens == 0 {
        0.0
    } else {
        (((saved_tokens as f64 / original_tokens as f64) * 100.0) * 100.0).round() / 100.0
    }
}

fn range_label(r: StatsRange) -> String {
    match r {
        StatsRange::Hour1 => "1h".into(),
        StatsRange::Hour24 => "24h".into(),
        StatsRange::Day7 => "7d".into(),
        StatsRange::All => "all".into(),
    }
}

fn compute_latency_stats(sorted: &[u64]) -> LatencyStats {
    if sorted.is_empty() {
        return LatencyStats::default();
    }
    let n = sorted.len();
    let min_ms = sorted[0];
    let max_ms = sorted[n - 1];
    let sum: u64 = sorted.iter().sum();
    let mean_ms = sum as f64 / n as f64;
    let p50_ms = sorted[(n * 50 / 100).min(n - 1)];
    let p95_ms = sorted[(n * 95 / 100).min(n - 1)];
    let p99_ms = sorted[(n * 99 / 100).min(n - 1)];
    LatencyStats {
        min_ms,
        max_ms,
        mean_ms,
        p50_ms,
        p95_ms,
        p99_ms,
    }
}

fn top_n(counts: HashMap<String, usize>, n: usize) -> Vec<TopEntry> {
    let mut v: Vec<_> = counts
        .into_iter()
        .map(|(name, count)| TopEntry { name, count })
        .collect();
    v.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.name.cmp(&b.name)));
    v.truncate(n);
    v
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::time::SystemTime;

    use super::*;
    use crate::gateway::admin::trace::{
        AgentContext, DispatchTrace, TokenTelemetry, TraceLog, TracePayload,
    };
    use serde_json::json;

    fn make_trace(ok: bool, total_ms: u64, tool: &str, instance: &str) -> DispatchTrace {
        DispatchTrace {
            request_id: uuid::Uuid::new_v4().to_string(),
            trace_id: uuid::Uuid::new_v4().simple().to_string(),
            span_id: None,
            parent_span_id: None,
            parent_request_id: None,
            trace_flags: None,
            trace_state: None,
            method: "tools/call".into(),
            tool_slug: Some(tool.to_string()),
            instance_id: Some(instance.to_string()),
            session_id: None,
            dcc_type: Some("maya".into()),
            transport: None,
            agent_context: None,
            started_at: SystemTime::now(),
            total_ms,
            ok,
            spans: vec![],
            input: None,
            output: None,
            token_accounting: None,
            llm_usage: None,
        }
    }

    fn token_telemetry(
        response_format: &str,
        original_tokens: usize,
        returned_tokens: usize,
    ) -> TokenTelemetry {
        let saved_tokens = original_tokens.saturating_sub(returned_tokens);
        TokenTelemetry {
            response_format: response_format.to_string(),
            token_estimator: "dcc-mcp-byte4-v1".to_string(),
            original_bytes: original_tokens * 4,
            returned_bytes: returned_tokens * 4,
            original_tokens,
            returned_tokens,
            saved_tokens,
            savings_pct: savings_pct(saved_tokens, original_tokens),
        }
    }

    #[test]
    fn empty_log_returns_zero_stats() {
        let log = Arc::new(TraceLog::new(100));
        let agg = StatsAggregator::new(log);
        let s = agg.compute(StatsRange::All);
        assert_eq!(s.total_calls, 0);
        assert_eq!(s.successful_calls, 0);
        assert_eq!(s.success_rate, 0.0);
        assert_eq!(s.token_usage.total_saved_tokens, 0);
        assert_eq!(s.hourly_distribution.len(), 24);
    }

    #[test]
    fn success_rate_is_correct() {
        let log = Arc::new(TraceLog::new(100));
        log.push(make_trace(true, 100, "maya.create_sphere", "inst-1"));
        log.push(make_trace(true, 200, "maya.create_sphere", "inst-1"));
        log.push(make_trace(false, 50, "maya.open_file", "inst-2"));

        let agg = StatsAggregator::new(log);
        let s = agg.compute(StatsRange::All);
        assert_eq!(s.total_calls, 3);
        assert_eq!(s.successful_calls, 2);
        assert_eq!(s.failed_calls, 1);
        assert!((s.success_rate - 2.0 / 3.0).abs() < 1e-6);
    }

    #[test]
    fn latency_percentiles_are_monotone() {
        let log = Arc::new(TraceLog::new(100));
        for ms in [10, 20, 30, 40, 50, 60, 70, 80, 90, 100u64] {
            log.push(make_trace(true, ms, "maya.t", "inst-1"));
        }
        let agg = StatsAggregator::new(log);
        let s = agg.compute(StatsRange::All);
        let l = &s.latency_ms;
        assert!(l.p50_ms <= l.p95_ms, "p50 <= p95");
        assert!(l.p95_ms <= l.p99_ms, "p95 <= p99");
        assert!(l.min_ms <= l.p50_ms, "min <= p50");
        assert!(l.p99_ms <= l.max_ms, "p99 <= max");
        assert_eq!(l.min_ms, 10);
        assert_eq!(l.max_ms, 100);
    }

    #[test]
    fn top_tools_sorted_by_count_descending() {
        let log = Arc::new(TraceLog::new(100));
        for _ in 0..5 {
            log.push(make_trace(true, 10, "maya.popular", "inst-1"));
        }
        for _ in 0..2 {
            log.push(make_trace(true, 10, "maya.rare", "inst-1"));
        }
        let agg = StatsAggregator::new(log);
        let s = agg.compute(StatsRange::All);
        assert_eq!(s.top_tools[0].name, "maya.popular");
        assert_eq!(s.top_tools[0].count, 5);
        assert_eq!(s.top_tools[1].name, "maya.rare");
        assert_eq!(s.top_tools[1].count, 2);
    }

    #[test]
    fn hourly_distribution_has_24_buckets() {
        let log = Arc::new(TraceLog::new(100));
        log.push(make_trace(true, 10, "maya.t", "inst-1"));
        let agg = StatsAggregator::new(log);
        let s = agg.compute(StatsRange::All);
        assert_eq!(s.hourly_distribution.len(), 24);
        // The single trace should land in exactly one bucket.
        assert_eq!(s.hourly_distribution.iter().sum::<u32>(), 1);
    }

    #[test]
    fn range_filter_excludes_old_traces() {
        let log = Arc::new(TraceLog::new(100));
        // Add a trace with a very old timestamp (well outside 1h).
        let mut old = make_trace(true, 10, "maya.old", "inst-1");
        old.started_at = UNIX_EPOCH; // epoch = Jan 1 1970
        log.push(old);
        // Add a recent trace.
        log.push(make_trace(true, 10, "maya.new", "inst-1"));

        let agg = StatsAggregator::new(log);
        let s_all = agg.compute(StatsRange::All);
        let s_1h = agg.compute(StatsRange::Hour1);
        assert_eq!(s_all.total_calls, 2);
        assert_eq!(s_1h.total_calls, 1);
    }

    #[test]
    fn token_usage_aggregates_totals_and_breakdowns() {
        let log = Arc::new(TraceLog::new(100));
        let mut rest_compact = make_trace(true, 10, "maya.create_sphere", "inst-1");
        rest_compact.transport = Some("rest".into());
        rest_compact.agent_context = Some(AgentContext {
            agent_name: Some("Planner".into()),
            actor_id: Some("artist-1".into()),
            client_platform: Some("cursor".into()),
            source_ip: Some("192.0.2.44".into()),
            ..AgentContext::default()
        });
        rest_compact.token_accounting = Some(token_telemetry("toon", 100, 40));
        log.push(rest_compact);

        let mut rest_json = make_trace(true, 12, "maya.create_sphere", "inst-1");
        rest_json.transport = Some("rest".into());
        rest_json.agent_context = Some(AgentContext {
            agent_name: Some("Planner".into()),
            actor_id: Some("artist-1".into()),
            client_platform: Some("cursor".into()),
            source_ip: Some("192.0.2.44".into()),
            ..AgentContext::default()
        });
        rest_json.token_accounting = Some(token_telemetry("json", 80, 80));
        log.push(rest_json);

        let mut mcp_compact = make_trace(true, 14, "photoshop.apply_filter", "inst-2");
        mcp_compact.dcc_type = Some("photoshop".into());
        mcp_compact.transport = Some("mcp".into());
        mcp_compact.agent_context = Some(AgentContext {
            agent_name: Some("Reviewer".into()),
            actor_id: Some("artist-2".into()),
            client_platform: Some("claude-desktop".into()),
            source_ip: Some("192.0.2.45".into()),
            ..AgentContext::default()
        });
        mcp_compact.token_accounting = Some(token_telemetry("toon", 120, 60));
        log.push(mcp_compact);

        let stats = StatsAggregator::new(log).compute(StatsRange::All);
        assert_eq!(stats.top_app_types[0].name, "maya");
        assert_eq!(stats.top_app_types[0].count, 2);
        assert!(
            stats
                .top_app_types
                .iter()
                .any(|entry| entry.name == "photoshop" && entry.count == 1)
        );
        assert_eq!(stats.token_usage.total_original_tokens, 300);
        assert_eq!(stats.token_usage.total_returned_tokens, 180);
        assert_eq!(stats.token_usage.total_saved_tokens, 120);
        assert_eq!(stats.token_usage.average_savings_pct, 40.0);
        assert_eq!(stats.payload_token_usage.total_tokens, 0);
        assert_eq!(stats.payload_token_usage.calls_with_any_payload_tokens, 0);
        assert_eq!(stats.payload_token_usage.calls_missing_payload_tokens, 3);
        assert_eq!(stats.token_usage.by_response_format[0].name, "toon");
        assert_eq!(stats.token_usage.by_response_format[0].calls, 2);
        assert_eq!(stats.token_usage.by_response_format[0].saved_tokens, 120);
        assert!(
            stats
                .token_usage
                .by_response_format
                .iter()
                .any(|entry| entry.name == "json" && entry.calls == 1 && entry.saved_tokens == 0)
        );
        assert!(
            stats
                .token_usage
                .by_transport
                .iter()
                .any(|entry| entry.name == "rest" && entry.saved_tokens == 60)
        );
        assert!(
            stats
                .token_usage
                .by_agent
                .iter()
                .any(|entry| entry.name == "Planner" && entry.calls == 2)
        );
        assert_eq!(stats.top_actors[0].name, "artist-1");
        assert_eq!(stats.top_actors[0].count, 2);
        assert_eq!(stats.top_actors[0].failed, 0);
        assert!(
            stats
                .top_client_platforms
                .iter()
                .any(|entry| entry.name == "cursor" && entry.count == 2)
        );
        assert!(
            stats
                .top_source_ips
                .iter()
                .any(|entry| entry.name == "192.0.2.44" && entry.count == 2)
        );
    }

    #[test]
    fn token_stats_are_aggregated() {
        let log = Arc::new(TraceLog::new(10));
        let mut with_input = make_trace(true, 10, "maya.t", "inst-1");
        with_input.input = Some(TracePayload::from_value(
            &json!({"prompt": "a lightweight prompt"}),
            1024,
        ));
        let mut with_output = make_trace(false, 12, "maya.t", "inst-1");
        with_output.output = Some(TracePayload::from_value(&json!({"result": "ok"}), 1024));
        log.push(with_input);
        log.push(with_output);

        let agg = StatsAggregator::new(log);
        let s = agg.compute(StatsRange::All);
        assert_eq!(s.total_calls, 2);
        assert!(s.total_input_tokens > 0);
        assert!(s.total_output_tokens > 0);
        assert_eq!(s.total_tokens, s.total_input_tokens + s.total_output_tokens);
        assert!(s.avg_input_tokens_per_call > 0.0);
        assert!(s.avg_output_tokens_per_call > 0.0);
        assert!(s.avg_total_tokens_per_call > 0.0);
        assert_eq!(s.payload_token_usage.calls_with_input_tokens, 1);
        assert_eq!(s.payload_token_usage.calls_with_output_tokens, 1);
        assert_eq!(s.payload_token_usage.calls_with_any_payload_tokens, 2);
        assert_eq!(s.payload_token_usage.calls_missing_payload_tokens, 0);
        assert_eq!(
            s.payload_token_usage.total_tokens,
            s.payload_token_usage.total_input_tokens + s.payload_token_usage.total_output_tokens
        );
    }

    #[test]
    fn payload_token_usage_distinguishes_missing_payloads_from_response_accounting() {
        let log = Arc::new(TraceLog::new(10));
        let mut response_only = make_trace(true, 10, "maya.response_only", "inst-1");
        response_only.token_accounting = Some(token_telemetry("toon", 100, 40));
        log.push(response_only);

        let mut with_payloads = make_trace(true, 12, "maya.with_payloads", "inst-1");
        with_payloads.input = Some(TracePayload::from_value(
            &json!({"prompt": "create a turntable preview"}),
            1024,
        ));
        with_payloads.output = Some(TracePayload::from_value(
            &json!({"ok": true, "frames": 24}),
            1024,
        ));
        with_payloads.token_accounting = Some(token_telemetry("json", 50, 50));
        log.push(with_payloads);

        let stats = StatsAggregator::new(log).compute(StatsRange::All);
        assert_eq!(stats.total_calls, 2);
        assert_eq!(stats.payload_token_usage.calls_with_any_payload_tokens, 1);
        assert_eq!(stats.payload_token_usage.calls_missing_payload_tokens, 1);
        assert!(stats.payload_token_usage.total_input_tokens > 0);
        assert!(stats.payload_token_usage.total_output_tokens > 0);
        assert!(stats.payload_token_usage.avg_total_tokens_per_call > 0.0);
        assert!(stats.payload_token_usage.avg_total_tokens_per_recorded_call > 0.0);
        assert_eq!(stats.token_usage.total_original_tokens, 150);
        assert_eq!(stats.token_usage.total_returned_tokens, 90);
        assert_eq!(stats.token_usage.total_saved_tokens, 60);
    }

    // ── Property-based tests (#846) ────────────────────────────────────────
    //
    // These verify the invariants ("laws") of pure helpers used by the
    // stats aggregator.  Adding proptest as a dev-dependency seeds the
    // toolchain so future LSP / trait-law tests can plug in cheaply
    // (#846 acceptance gate).

    use proptest::prelude::*;

    proptest! {
        /// Latency percentiles must be weakly monotone:
        ///   min ≤ p50 ≤ p95 ≤ p99 ≤ max
        /// for any non-empty input. The mean must lie in [min, max].
        #[test]
        fn prop_latency_percentiles_are_monotone(
            mut samples in proptest::collection::vec(0u64..1_000_000, 1..256)
        ) {
            samples.sort_unstable();
            let stats = compute_latency_stats(&samples);
            prop_assert!(stats.min_ms <= stats.p50_ms);
            prop_assert!(stats.p50_ms <= stats.p95_ms);
            prop_assert!(stats.p95_ms <= stats.p99_ms);
            prop_assert!(stats.p99_ms <= stats.max_ms);
            prop_assert!(stats.mean_ms >= stats.min_ms as f64);
            prop_assert!(stats.mean_ms <= stats.max_ms as f64);
        }

        /// `top_n` must:
        ///   1. Return at most `n` entries.
        ///   2. Be sorted by count descending (with name ascending as tiebreaker).
        ///   3. Never invent entries — every returned name must exist in the input.
        #[test]
        fn prop_top_n_is_sorted_and_truncated(
            entries in proptest::collection::hash_map("[a-z]{1,8}", 0usize..100, 0..32),
            n in 0usize..16,
        ) {
            let input = entries.clone();
            let result = top_n(entries, n);
            prop_assert!(result.len() <= n);
            for w in result.windows(2) {
                let a = &w[0];
                let b = &w[1];
                // count desc, name asc on tie
                if a.count == b.count {
                    prop_assert!(a.name <= b.name);
                } else {
                    prop_assert!(a.count > b.count);
                }
            }
            for entry in &result {
                prop_assert_eq!(input.get(&entry.name), Some(&entry.count));
            }
        }

        /// `top_n` is idempotent on length: feeding the result back in does
        /// not grow it.
        #[test]
        fn prop_top_n_is_idempotent_on_length(
            entries in proptest::collection::hash_map("[a-z]{1,4}", 0usize..50, 0..16),
            n in 1usize..16,
        ) {
            let first = top_n(entries.clone(), n);
            let second_input: HashMap<String, usize> = first
                .iter()
                .map(|e| (e.name.clone(), e.count))
                .collect();
            let second = top_n(second_input, n);
            prop_assert_eq!(first.len(), second.len());
        }
    }
}
