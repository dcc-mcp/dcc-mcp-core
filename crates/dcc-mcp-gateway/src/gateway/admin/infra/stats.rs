//! Gateway data-source adapter for admin statistics.
//!
//! The admin crate owns the pure statistics contract and calculations. This
//! module only merges the gateway's durable SQLite rows with its in-memory
//! trace log before delegating the projection.

use std::collections::HashMap;
use std::sync::Arc;

use dcc_mcp_gateway_admin::{DispatchTrace, TraceLog, compute_stats_filtered};

use super::super::sqlite_lane::AdminSqliteReader;

pub use dcc_mcp_gateway_admin::{
    AttributionFacet, GatewayStats, LatencyStats, PayloadTokenUsageStats, StatsFilter, StatsRange,
    StatsStatus, TokenBreakdownEntry, TokenUsageStats, TopEntry,
};

/// Combines gateway trace stores before computing an admin statistics snapshot.
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

    pub fn compute(&self, range: StatsRange) -> GatewayStats {
        self.compute_filtered(range, &StatsFilter::default())
    }

    pub fn compute_filtered(&self, range: StatsRange, filter: &StatsFilter) -> GatewayStats {
        let cutoff = range.cutoff();
        let mut by_id: HashMap<String, DispatchTrace> = HashMap::new();
        if let Some(db) = &self.sqlite_reader {
            for trace in db.list_traces_since(cutoff, 500_000) {
                by_id.insert(trace.request_id.clone(), trace);
            }
        }
        for trace in self.trace_log.recent(usize::MAX) {
            by_id.insert(trace.request_id.clone(), trace);
        }
        compute_stats_filtered(by_id.into_values().collect(), range, filter)
    }
}
