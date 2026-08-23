//! Unified admin activity projection.
//!
//! The dashboard has several raw observability lanes: audit rows, dispatch
//! traces, gateway events, and eventually workflow/job updates.  This module
//! gives both humans and agents a single timeline-shaped interface over those
//! lanes without changing the hot-path writers.

use std::cmp::Reverse;
use std::collections::HashMap;

#[cfg(feature = "admin")]
use dcc_mcp_gateway_admin::task_payload;
use dcc_mcp_gateway_admin::{GatewayActivityInput, activity_payload, debug_bundle_payload};
use serde_json::Value;

pub use dcc_mcp_gateway_admin::{
    ActivityCorrelation, ActivityEvent, TaskArtifact, TaskRelated, TaskSnapshot, TaskValidation,
};

use crate::gateway::admin::state::{AdminAuditRecord, AdminState};
use crate::gateway::admin::trace::DispatchTrace;
use crate::gateway::event_log::ContendEvent;
#[cfg(feature = "admin")]
use dcc_mcp_gateway_admin::AdminLinkBuilder;

pub async fn build_activity_payload(state: &AdminState, limit: usize) -> Value {
    let fetch_limit = limit.saturating_mul(2).max(200);
    let audits = collect_audits(state, fetch_limit).await;
    let traces = collect_traces(state, fetch_limit).await;
    let gateway_events = state
        .gateway
        .event_log
        .recent_events(limit.min(500))
        .iter()
        .map(gateway_activity_input)
        .collect::<Vec<_>>();
    activity_payload(&audits, &traces, &gateway_events, limit)
}

#[cfg(feature = "admin")]
pub(crate) async fn build_tasks_payload(
    state: &AdminState,
    limit: usize,
    links: AdminLinkBuilder,
) -> Value {
    let fetch_limit = limit.saturating_mul(4).max(500);
    let traces = collect_traces(state, fetch_limit).await;
    let audits = collect_audits(state, fetch_limit).await;
    task_payload(traces, audits, limit, &links)
}

pub async fn build_debug_bundle(state: &AdminState, lookup_id: &str) -> Option<Value> {
    let audits = collect_audits(state, 1_000).await;
    let mut traces = collect_traces(state, 1_000).await;
    if !traces
        .iter()
        .any(|trace| trace.request_id == lookup_id || trace.trace_id == lookup_id)
        && let Some(trace) = find_trace(state, lookup_id).await
    {
        traces.push(trace);
    }
    let gateway_events = state
        .gateway
        .event_log
        .recent_events(500)
        .iter()
        .map(gateway_activity_input)
        .collect();
    debug_bundle_payload(lookup_id, audits, traces, gateway_events)
}

pub async fn collect_audits(state: &AdminState, limit: usize) -> Vec<AdminAuditRecord> {
    let mut by_id: HashMap<String, AdminAuditRecord> = HashMap::new();
    if let Some(lane) = &state.admin_sqlite_lane {
        for rec in lane
            .reader()
            .list_audits_recent(limit.saturating_mul(4).max(500))
        {
            by_id.insert(rec.request_id.clone(), rec);
        }
    }
    if let Some(log) = &state.audit_log {
        for rec in log.lock().iter().rev().take(limit) {
            by_id.insert(rec.request_id.clone(), rec.clone());
        }
    }
    let mut rows: Vec<_> = by_id.into_values().collect();
    rows.sort_by_key(|row| Reverse(row.timestamp));
    rows.truncate(limit);
    rows
}

pub async fn collect_traces(state: &AdminState, limit: usize) -> Vec<DispatchTrace> {
    let mut by_id: HashMap<String, DispatchTrace> = HashMap::new();
    if let Some(lane) = &state.admin_sqlite_lane {
        for trace in lane
            .reader()
            .list_traces_since(None, limit.saturating_mul(4).max(500))
        {
            by_id.insert(trace.request_id.clone(), trace);
        }
    }
    if let Some(log) = &state.trace_log {
        for trace in log.recent(limit) {
            by_id.insert(trace.request_id.clone(), trace);
        }
    }
    let mut rows: Vec<_> = by_id.into_values().collect();
    rows.sort_by_key(|row| Reverse(row.started_at));
    rows.truncate(limit);
    rows
}

async fn find_trace(state: &AdminState, request_id: &str) -> Option<DispatchTrace> {
    if let Some(trace) = state.trace_log.as_ref().and_then(|log| log.get(request_id)) {
        return Some(trace);
    }
    state
        .admin_sqlite_lane
        .as_ref()
        .and_then(|lane| lane.reader().get_trace(request_id))
}

fn gateway_activity_input(event: &ContendEvent) -> GatewayActivityInput {
    GatewayActivityInput {
        timestamp: event.timestamp.clone(),
        status: event.event.as_label().to_string(),
        reason: event.reason.clone(),
        dcc_type: event.dcc_type.clone(),
        instance_id: event.instance_id.clone(),
    }
}
