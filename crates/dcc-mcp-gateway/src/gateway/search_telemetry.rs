//! Search-quality telemetry for gateway discovery workflows.
//!
//! The store keeps bounded, prompt-safe search records and correlates later
//! `describe`, `load_skill`, and `call` operations through `meta.search_id`.

use std::collections::{HashMap, HashSet, VecDeque};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::gateway::admin::trace::{AgentContext, TraceContext};

pub use dcc_mcp_gateway_core::capability::RANKER_VERSION;

const DEFAULT_CAPACITY: usize = 1_000;
const MAX_QUERY_PREVIEW_CHARS: usize = 120;
const MAX_FOLLOWUPS_PER_SEARCH: usize = 32;
const REFORMULATION_WINDOW: Duration = Duration::from_secs(10 * 60);

/// One lightweight hit captured from a search response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchTelemetryHit {
    pub tool_slug: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skill_name: Option<String>,
    pub dcc_type: String,
    pub rank: u32,
    pub score: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub match_reasons: Vec<String>,
    pub loaded: bool,
}

/// One follow-up operation correlated with a prior search.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchFollowupTelemetry {
    pub kind: String,
    pub timestamp_ms: u64,
    pub request_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trace_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_slug: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skill_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_rank: Option<u32>,
    pub success: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub elapsed_ms: Option<u64>,
}

/// Search event stored in the bounded in-memory ring.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchTelemetryRecord {
    pub search_id: String,
    pub timestamp_ms: u64,
    pub transport: String,
    pub kind: String,
    pub ranker_version: String,
    pub index_generation: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trace_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_context: Option<AgentContext>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query_preview: Option<String>,
    pub query_hash: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dcc_type: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dcc_types: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instance_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    pub total: usize,
    pub zero_results: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reformulation_of: Option<String>,
    pub hits: Vec<SearchTelemetryHit>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub followups: Vec<SearchFollowupTelemetry>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags_any: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_success_ms: Option<u64>,
}

/// Aggregated search-quality metrics suitable for admin/debug APIs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchQualityStats {
    pub total_searches: usize,
    pub zero_result_count: usize,
    pub zero_result_rate: f64,
    pub selected_rank: Option<f64>,
    pub selected_count: usize,
    pub top1_hit_rate: f64,
    pub top3_hit_rate: f64,
    pub top5_hit_rate: f64,
    pub describe_after_search_rate: f64,
    pub load_after_search_rate: f64,
    pub call_after_search_rate: f64,
    pub success_after_search_rate: f64,
    pub query_reformulation_count: usize,
    pub query_reformulation_rate: f64,
    pub time_to_first_success: Option<f64>,
    pub time_to_first_success_ms: Option<f64>,
}

/// Admin/debug response envelope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchTelemetrySnapshot {
    pub stats: SearchQualityStats,
    pub total: usize,
    pub recent: Vec<SearchTelemetryRecord>,
}

#[derive(Debug, Clone)]
pub struct SearchTelemetryInput {
    pub search_id: String,
    pub transport: String,
    pub kind: String,
    pub query: String,
    pub dcc_type: Option<String>,
    pub dcc_types: Vec<String>,
    pub instance_id: Option<String>,
    pub limit: Option<u32>,
    pub total: usize,
    pub ranker_version: String,
    pub index_generation: String,
    pub hits: Vec<SearchTelemetryHit>,
    pub trace_context: Option<TraceContext>,
    pub session_id: Option<String>,
    pub agent_context: Option<AgentContext>,
    pub tags_any: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct SearchFollowupInput {
    pub search_id: String,
    pub kind: String,
    pub tool_slug: Option<String>,
    pub skill_name: Option<String>,
    pub success: bool,
    pub trace_context: Option<TraceContext>,
}

#[derive(Debug, Clone)]
struct LastSearch {
    search_id: String,
    query_norm: String,
    timestamp: SystemTime,
    first_success: bool,
}

#[derive(Debug, Default)]
struct SearchTelemetryInner {
    records: VecDeque<SearchTelemetryRecord>,
    last_by_correlation: HashMap<String, LastSearch>,
    query_reformulation_count: usize,
}

/// Bounded in-memory search telemetry store.
#[derive(Debug)]
pub struct SearchTelemetryStore {
    inner: Mutex<SearchTelemetryInner>,
    capacity: usize,
}

impl SearchTelemetryStore {
    pub fn new() -> Self {
        Self::with_capacity(DEFAULT_CAPACITY)
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            inner: Mutex::new(SearchTelemetryInner::default()),
            capacity: capacity.max(1),
        }
    }

    pub fn new_search_id() -> String {
        uuid::Uuid::new_v4().to_string()
    }

    pub fn record_search(&self, input: SearchTelemetryInput) -> String {
        let mut inner = self.inner.lock();
        let now = SystemTime::now();
        let query_norm = normalise_query(&input.query);
        let agent_context = input.agent_context.clone();
        let session_id = input.session_id.or_else(|| {
            agent_context
                .as_ref()
                .and_then(|ctx| ctx.session_id.clone())
        });
        let correlation_key = correlation_key(
            input.trace_context.as_ref(),
            session_id.as_deref(),
            input.dcc_type.as_deref(),
            agent_context.as_ref(),
        );
        let reformulation_of = correlation_key.as_ref().and_then(|key| {
            let previous = inner.last_by_correlation.get(key)?;
            let recent = now
                .duration_since(previous.timestamp)
                .map(|age| age <= REFORMULATION_WINDOW)
                .unwrap_or(false);
            if recent
                && !previous.first_success
                && !query_norm.is_empty()
                && previous.query_norm != query_norm
            {
                Some(previous.search_id.clone())
            } else {
                None
            }
        });
        if reformulation_of.is_some() {
            inner.query_reformulation_count += 1;
            crate::gateway::metrics::record_gateway_search_reformulation();
        }

        let zero_results = input.total == 0;
        crate::gateway::metrics::record_gateway_search(if zero_results {
            "zero"
        } else {
            "nonzero"
        });

        let record = SearchTelemetryRecord {
            search_id: input.search_id.clone(),
            timestamp_ms: timestamp_ms(now),
            transport: bound_label(input.transport, "unknown"),
            kind: bound_label(input.kind, "tool"),
            ranker_version: input.ranker_version,
            index_generation: input.index_generation,
            request_id: input
                .trace_context
                .as_ref()
                .map(|ctx| ctx.request_id.clone()),
            trace_id: input.trace_context.as_ref().map(|ctx| ctx.trace_id.clone()),
            session_id,
            agent_context,
            query_preview: query_preview(&input.query),
            query_hash: hash_query(&query_norm),
            dcc_type: input.dcc_type,
            dcc_types: bounded_list(
                input
                    .dcc_types
                    .iter()
                    .map(|d| d.trim().to_ascii_lowercase())
                    .filter(|d| !d.is_empty()),
                10,
            ),
            instance_id: input.instance_id,
            limit: input.limit,
            total: input.total,
            zero_results,
            reformulation_of,
            hits: input.hits,
            followups: Vec::new(),
            tags_any: bounded_list(
                input
                    .tags_any
                    .iter()
                    .map(|t| t.trim().to_ascii_lowercase())
                    .filter(|t| !t.is_empty()),
                20,
            ),
            first_success_ms: None,
        };

        if let Some(key) = correlation_key {
            inner.last_by_correlation.insert(
                key,
                LastSearch {
                    search_id: input.search_id.clone(),
                    query_norm,
                    timestamp: now,
                    first_success: false,
                },
            );
        }

        while inner.records.len() >= self.capacity {
            inner.records.pop_front();
        }
        inner.records.push_back(record);
        input.search_id
    }

    pub fn record_followup(&self, input: SearchFollowupInput) -> bool {
        let mut inner = self.inner.lock();
        let mut mark_success_for: Option<(String, String)> = None;
        {
            let Some(record) = inner
                .records
                .iter_mut()
                .rev()
                .find(|record| record.search_id == input.search_id)
            else {
                return false;
            };
            let now = SystemTime::now();
            let selected_rank = selected_rank(
                record,
                input.tool_slug.as_deref(),
                input.skill_name.as_deref(),
            );
            let elapsed_ms = now
                .duration_since(UNIX_EPOCH + Duration::from_millis(record.timestamp_ms))
                .ok()
                .map(|duration| duration.as_millis() as u64);
            let followup = SearchFollowupTelemetry {
                kind: bound_label(input.kind, "unknown"),
                timestamp_ms: timestamp_ms(now),
                request_id: input
                    .trace_context
                    .as_ref()
                    .map(|ctx| ctx.request_id.clone()),
                trace_id: input.trace_context.as_ref().map(|ctx| ctx.trace_id.clone()),
                tool_slug: input.tool_slug,
                skill_name: input.skill_name,
                selected_rank,
                success: input.success,
                elapsed_ms,
            };
            let rank_bucket = rank_bucket(selected_rank);
            crate::gateway::metrics::record_gateway_search_followup(&followup.kind, rank_bucket);
            if followup.kind == "call" && followup.success && record.first_success_ms.is_none() {
                record.first_success_ms = elapsed_ms;
                if let Some(ms) = elapsed_ms {
                    crate::gateway::metrics::observe_gateway_search_time_to_first_success(
                        Duration::from_millis(ms),
                    );
                }
            }
            record.followups.push(followup);
            if record.followups.len() > MAX_FOLLOWUPS_PER_SEARCH {
                record.followups.remove(0);
            }

            if record.first_success_ms.is_some()
                && let Some(key) = correlation_key(
                    input.trace_context.as_ref(),
                    record.session_id.as_deref(),
                    record.dcc_type.as_deref(),
                    record.agent_context.as_ref(),
                )
            {
                mark_success_for = Some((key, record.search_id.clone()));
            }
        }
        if let Some((key, search_id)) = mark_success_for
            && let Some(last) = inner.last_by_correlation.get_mut(&key)
            && last.search_id == search_id
        {
            last.first_success = true;
        }
        true
    }

    pub fn selected_hit(
        &self,
        search_id: &str,
        tool_slug: Option<&str>,
        skill_name: Option<&str>,
    ) -> Option<SearchTelemetryHit> {
        let inner = self.inner.lock();
        let record = inner
            .records
            .iter()
            .rev()
            .find(|record| record.search_id == search_id)?;
        selected_hit(record, tool_slug, skill_name).cloned()
    }

    pub fn snapshot(&self, limit: usize) -> SearchTelemetrySnapshot {
        let inner = self.inner.lock();
        let recent: Vec<SearchTelemetryRecord> = inner
            .records
            .iter()
            .rev()
            .take(limit.max(1))
            .cloned()
            .collect();
        SearchTelemetrySnapshot {
            stats: compute_stats(&inner),
            total: recent.len(),
            recent,
        }
    }
}

impl Default for SearchTelemetryStore {
    fn default() -> Self {
        Self::new()
    }
}

pub fn search_id_from_payload(payload: &Value) -> Option<String> {
    payload
        .get("meta")
        .or_else(|| payload.get("_meta"))
        .and_then(search_id_from_meta)
        .or_else(|| {
            payload
                .get("search_id")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .and_then(normalise_search_id)
}

pub fn search_id_from_meta(meta: &Value) -> Option<String> {
    meta.get("search_id")
        .or_else(|| meta.get("searchId"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .and_then(normalise_search_id)
}

fn selected_rank(
    record: &SearchTelemetryRecord,
    tool_slug: Option<&str>,
    skill_name: Option<&str>,
) -> Option<u32> {
    selected_hit(record, tool_slug, skill_name).map(|hit| hit.rank)
}

fn selected_hit<'a>(
    record: &'a SearchTelemetryRecord,
    tool_slug: Option<&str>,
    skill_name: Option<&str>,
) -> Option<&'a SearchTelemetryHit> {
    if let Some(slug) = tool_slug
        && let Some(hit) = record.hits.iter().find(|hit| hit.tool_slug == slug)
    {
        return Some(hit);
    }
    let skill = skill_name?.to_ascii_lowercase();
    record.hits.iter().find(|hit| {
        hit.skill_name
            .as_deref()
            .is_some_and(|candidate| candidate.eq_ignore_ascii_case(&skill))
    })
}

fn compute_stats(inner: &SearchTelemetryInner) -> SearchQualityStats {
    let total = inner.records.len();
    if total == 0 {
        return SearchQualityStats {
            total_searches: 0,
            zero_result_count: 0,
            zero_result_rate: 0.0,
            selected_rank: None,
            selected_count: 0,
            top1_hit_rate: 0.0,
            top3_hit_rate: 0.0,
            top5_hit_rate: 0.0,
            describe_after_search_rate: 0.0,
            load_after_search_rate: 0.0,
            call_after_search_rate: 0.0,
            success_after_search_rate: 0.0,
            query_reformulation_count: inner.query_reformulation_count,
            query_reformulation_rate: 0.0,
            time_to_first_success: None,
            time_to_first_success_ms: None,
        };
    }

    let zero_result_count = inner
        .records
        .iter()
        .filter(|record| record.zero_results)
        .count();
    let mut selected_ranks = Vec::new();
    let mut top1 = 0usize;
    let mut top3 = 0usize;
    let mut top5 = 0usize;
    let mut describe = 0usize;
    let mut load = 0usize;
    let mut call = 0usize;
    let mut success = 0usize;
    let mut first_success_ms = Vec::new();

    for record in &inner.records {
        let kinds: HashSet<&str> = record
            .followups
            .iter()
            .map(|followup| followup.kind.as_str())
            .collect();
        if kinds.contains("describe") {
            describe += 1;
        }
        if kinds.contains("load_skill") {
            load += 1;
        }
        if kinds.contains("call") {
            call += 1;
        }
        if record.first_success_ms.is_some() {
            success += 1;
        }
        if let Some(ms) = record.first_success_ms {
            first_success_ms.push(ms);
        }
        if let Some(rank) = record
            .followups
            .iter()
            .find_map(|followup| followup.selected_rank)
        {
            selected_ranks.push(rank);
            if rank <= 1 {
                top1 += 1;
            }
            if rank <= 3 {
                top3 += 1;
            }
            if rank <= 5 {
                top5 += 1;
            }
        }
    }

    let selected_count = selected_ranks.len();
    let selected_rank = (!selected_ranks.is_empty()).then(|| {
        selected_ranks.iter().map(|rank| *rank as f64).sum::<f64>() / selected_ranks.len() as f64
    });
    let avg_success_ms = (!first_success_ms.is_empty()).then(|| {
        first_success_ms.iter().map(|ms| *ms as f64).sum::<f64>() / first_success_ms.len() as f64
    });

    SearchQualityStats {
        total_searches: total,
        zero_result_count,
        zero_result_rate: rate(zero_result_count, total),
        selected_rank,
        selected_count,
        top1_hit_rate: rate(top1, total),
        top3_hit_rate: rate(top3, total),
        top5_hit_rate: rate(top5, total),
        describe_after_search_rate: rate(describe, total),
        load_after_search_rate: rate(load, total),
        call_after_search_rate: rate(call, total),
        success_after_search_rate: rate(success, total),
        query_reformulation_count: inner.query_reformulation_count,
        query_reformulation_rate: rate(inner.query_reformulation_count, total),
        time_to_first_success: avg_success_ms.map(|ms| ms / 1_000.0),
        time_to_first_success_ms: avg_success_ms,
    }
}

fn correlation_key(
    trace_context: Option<&TraceContext>,
    session_id: Option<&str>,
    dcc_type: Option<&str>,
    agent_context: Option<&AgentContext>,
) -> Option<String> {
    trace_context
        .map(|ctx| format!("trace:{}", ctx.trace_id))
        .or_else(|| session_id.map(|session| format!("session:{session}")))
        .or_else(|| {
            agent_context
                .and_then(|ctx| ctx.turn_id.as_deref())
                .map(|turn_id| format!("turn:{turn_id}"))
        })
        .or_else(|| dcc_type.map(|dcc| format!("dcc:{dcc}")))
}

fn query_preview(query: &str) -> Option<String> {
    let redacted = redact_query(query);
    let trimmed = redacted.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(take_chars(trimmed, MAX_QUERY_PREVIEW_CHARS))
}

fn redact_query(query: &str) -> String {
    query
        .split_whitespace()
        .map(|part| {
            let lowered = part.to_ascii_lowercase();
            if lowered.contains("token=")
                || lowered.contains("api_key")
                || lowered.contains("apikey")
                || lowered.contains("secret")
                || lowered.contains("password")
            {
                "[redacted]"
            } else {
                part
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn normalise_query(query: &str) -> String {
    query
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

fn hash_query(query: &str) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in query.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

fn normalise_search_id(raw: String) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.len() > 128 {
        return None;
    }
    if trimmed
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | ':' | '.'))
    {
        Some(trimmed.to_string())
    } else {
        None
    }
}

fn timestamp_ms(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_millis() as u64
}

fn bound_label(value: String, fallback: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        fallback.to_string()
    } else {
        take_chars(trimmed, 40)
    }
}

fn take_chars(value: &str, max: usize) -> String {
    value.chars().take(max).collect()
}

fn bounded_list<I>(items: I, max: usize) -> Vec<String>
where
    I: Iterator<Item = String>,
{
    items.take(max).collect()
}

fn rank_bucket(rank: Option<u32>) -> &'static str {
    match rank {
        Some(1) => "top1",
        Some(2 | 3) => "top3",
        Some(4 | 5) => "top5",
        Some(_) => "beyond5",
        None => "unknown",
    }
}

fn rate(count: usize, total: usize) -> f64 {
    if total == 0 {
        0.0
    } else {
        count as f64 / total as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trace(id: &str) -> TraceContext {
        TraceContext {
            trace_id: "4bf92f3577b34da6a3ce929d0e0e4736".to_string(),
            request_id: id.to_string(),
            span_id: Some("00f067aa0ba902b7".to_string()),
            parent_span_id: None,
            parent_request_id: None,
            trace_flags: Some("01".to_string()),
            trace_state: None,
        }
    }

    fn hit(slug: &str, skill: &str, rank: u32) -> SearchTelemetryHit {
        SearchTelemetryHit {
            tool_slug: slug.to_string(),
            skill_name: Some(skill.to_string()),
            dcc_type: "maya".to_string(),
            rank,
            score: 100 - rank,
            match_reasons: vec!["tool_lexical".to_string()],
            loaded: true,
        }
    }

    #[test]
    fn store_correlates_describe_load_call_and_batch_followups() {
        let store = SearchTelemetryStore::with_capacity(10);
        let search_id = SearchTelemetryStore::new_search_id();
        store.record_search(SearchTelemetryInput {
            search_id: search_id.clone(),
            transport: "rest".to_string(),
            kind: "tool".to_string(),
            query: "create sphere".to_string(),
            dcc_type: Some("maya".to_string()),
            dcc_types: vec![],
            instance_id: None,
            limit: Some(5),
            total: 2,
            ranker_version: RANKER_VERSION.to_string(),
            index_generation: "idx1".to_string(),
            hits: vec![
                hit("maya.11111111.create_sphere", "maya-geometry", 1),
                hit("maya.11111111.create_cube", "maya-geometry", 2),
            ],
            trace_context: Some(trace("search-1")),
            session_id: Some("sess-1".to_string()),
            agent_context: None,
            tags_any: vec![],
        });
        for (kind, slug, skill, success) in [
            ("describe", Some("maya.11111111.create_sphere"), None, true),
            ("load_skill", None, Some("maya-geometry"), true),
            ("call", Some("maya.11111111.create_sphere"), None, false),
            ("call", Some("maya.11111111.create_sphere"), None, true),
            ("call", Some("maya.11111111.create_cube"), None, true),
        ] {
            store.record_followup(SearchFollowupInput {
                search_id: search_id.clone(),
                kind: kind.to_string(),
                tool_slug: slug.map(str::to_string),
                skill_name: skill.map(str::to_string),
                success,
                trace_context: Some(trace(kind)),
            });
        }

        let snapshot = store.snapshot(10);
        assert_eq!(snapshot.stats.total_searches, 1);
        assert_eq!(snapshot.stats.describe_after_search_rate, 1.0);
        assert_eq!(snapshot.stats.load_after_search_rate, 1.0);
        assert_eq!(snapshot.stats.call_after_search_rate, 1.0);
        assert_eq!(snapshot.stats.success_after_search_rate, 1.0);
        assert_eq!(snapshot.stats.top1_hit_rate, 1.0);
        assert_eq!(snapshot.recent[0].followups.len(), 5);
    }

    #[test]
    fn store_counts_zero_results_and_reformulations_without_full_prompt_leak() {
        let store = SearchTelemetryStore::with_capacity(10);
        let first = "search-a".to_string();
        store.record_search(SearchTelemetryInput {
            search_id: first,
            transport: "mcp".to_string(),
            kind: "tool".to_string(),
            query: "token=abc123 impossible query".to_string(),
            dcc_type: Some("maya".to_string()),
            dcc_types: vec![],
            instance_id: None,
            limit: Some(5),
            total: 0,
            ranker_version: RANKER_VERSION.to_string(),
            index_generation: "idx1".to_string(),
            hits: Vec::new(),
            trace_context: Some(trace("search-a")),
            session_id: None,
            agent_context: None,
            tags_any: vec![],
        });
        store.record_search(SearchTelemetryInput {
            search_id: "search-b".to_string(),
            transport: "mcp".to_string(),
            kind: "tool".to_string(),
            query: "sphere".to_string(),
            dcc_type: Some("maya".to_string()),
            dcc_types: vec![],
            instance_id: None,
            limit: Some(5),
            total: 1,
            ranker_version: RANKER_VERSION.to_string(),
            index_generation: "idx1".to_string(),
            hits: vec![hit("maya.11111111.create_sphere", "maya-geometry", 1)],
            trace_context: Some(trace("search-b")),
            session_id: None,
            agent_context: None,
            tags_any: vec![],
        });

        let snapshot = store.snapshot(10);
        assert_eq!(snapshot.stats.zero_result_count, 1);
        assert_eq!(snapshot.stats.query_reformulation_count, 1);
        assert_eq!(
            snapshot.recent[1].query_preview.as_deref(),
            Some("[redacted] impossible query")
        );
    }

    #[test]
    fn store_correlates_search_quality_with_agent_turn_context() {
        let store = SearchTelemetryStore::with_capacity(10);
        let turn_context = AgentContext {
            model_provider: Some("openai".to_string()),
            model_version: Some("gpt-5.1".to_string()),
            turn_id: Some("turn-search".to_string()),
            user_intent_summary: Some("Find a sphere creation tool.".to_string()),
            user_input_hash: Some("sha256:user".to_string()),
            user_input_chars: Some(48),
            ..AgentContext::default()
        };
        store.record_search(SearchTelemetryInput {
            search_id: "search-turn-a".to_string(),
            transport: "mcp".to_string(),
            kind: "tool".to_string(),
            query: "missing sphere creator".to_string(),
            dcc_type: Some("maya".to_string()),
            dcc_types: vec![],
            instance_id: None,
            limit: Some(5),
            total: 0,
            ranker_version: RANKER_VERSION.to_string(),
            index_generation: "idx1".to_string(),
            hits: Vec::new(),
            trace_context: None,
            session_id: None,
            agent_context: Some(turn_context.clone()),
            tags_any: vec![],
        });
        store.record_search(SearchTelemetryInput {
            search_id: "search-turn-b".to_string(),
            transport: "mcp".to_string(),
            kind: "tool".to_string(),
            query: "sphere".to_string(),
            dcc_type: Some("maya".to_string()),
            dcc_types: vec![],
            instance_id: None,
            limit: Some(5),
            total: 1,
            ranker_version: RANKER_VERSION.to_string(),
            index_generation: "idx1".to_string(),
            hits: vec![hit("maya.11111111.create_sphere", "maya-geometry", 1)],
            trace_context: None,
            session_id: None,
            agent_context: Some(turn_context),
            tags_any: vec![],
        });
        assert!(store.record_followup(SearchFollowupInput {
            search_id: "search-turn-b".to_string(),
            kind: "call".to_string(),
            tool_slug: Some("maya.11111111.create_sphere".to_string()),
            skill_name: None,
            success: true,
            trace_context: None,
        }));

        let snapshot = store.snapshot(10);
        let latest = &snapshot.recent[0];
        let agent = latest.agent_context.as_ref().expect("agent turn context");

        assert_eq!(snapshot.stats.query_reformulation_count, 1);
        assert_eq!(latest.search_id, "search-turn-b");
        assert_eq!(latest.reformulation_of.as_deref(), Some("search-turn-a"));
        assert_eq!(latest.first_success_ms, latest.followups[0].elapsed_ms);
        assert_eq!(latest.followups[0].selected_rank, Some(1));
        assert_eq!(agent.model_provider.as_deref(), Some("openai"));
        assert_eq!(agent.model_version.as_deref(), Some("gpt-5.1"));
        assert_eq!(agent.turn_id.as_deref(), Some("turn-search"));
        assert_eq!(agent.user_input_hash.as_deref(), Some("sha256:user"));
    }
}
