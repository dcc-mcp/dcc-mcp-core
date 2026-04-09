//! `ActionRecorder` — records per-Action execution timing and success/failure
//! counters using OpenTelemetry metrics.
//!
//! This module is deliberately self-contained: it uses the *global* meter
//! (set up by [`crate::provider::init`]) and keeps an in-memory histogram of
//! durations so callers can query aggregated statistics without going through
//! an OTLP backend.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;

use opentelemetry::metrics::{Counter, Histogram};
use opentelemetry::{KeyValue, global};
use tracing::instrument;

use crate::types::{ActionMetrics, span_keys};

// ── Duration store (in-memory) ────────────────────────────────────────────────

/// A lightweight ring buffer that stores the last N duration samples for an
/// action so we can compute approximate P95/P99 without a full histogram.
#[derive(Debug, Default)]
struct DurationStore {
    samples: Vec<f64>, // milliseconds
}

impl DurationStore {
    const MAX_SAMPLES: usize = 1024;

    fn push(&mut self, ms: f64) {
        if self.samples.len() >= Self::MAX_SAMPLES {
            self.samples.remove(0);
        }
        self.samples.push(ms);
    }

    fn avg(&self) -> f64 {
        if self.samples.is_empty() {
            return 0.0;
        }
        self.samples.iter().sum::<f64>() / self.samples.len() as f64
    }

    fn percentile(&self, pct: f64) -> f64 {
        if self.samples.is_empty() {
            return 0.0;
        }
        let mut sorted = self.samples.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let idx = ((pct / 100.0) * sorted.len() as f64).ceil() as usize;
        sorted[(idx.min(sorted.len()) - 1).max(0)]
    }
}

// ── Per-Action state ──────────────────────────────────────────────────────────

#[derive(Debug, Default)]
struct ActionState {
    invocation_count: u64,
    success_count: u64,
    failure_count: u64,
    durations: DurationStore,
}

// ── ActionRecorder ────────────────────────────────────────────────────────────

/// Records Action execution metrics.
///
/// # Usage
///
/// ```text
/// let recorder = ActionRecorder::new("dcc-mcp-core");
///
/// let guard = recorder.start("create_sphere", "maya");
/// // ... execute action ...
/// guard.finish(true, "maya");
/// ```
#[derive(Clone)]
pub struct ActionRecorder {
    /// OpenTelemetry counter for action invocations.
    invocation_counter: Counter<u64>,
    /// OpenTelemetry counter for action successes.
    success_counter: Counter<u64>,
    /// OpenTelemetry counter for action failures.
    failure_counter: Counter<u64>,
    /// OpenTelemetry histogram for action execution duration.
    duration_histogram: Histogram<f64>,
    /// In-memory per-action state for summary queries.
    state: Arc<Mutex<HashMap<String, ActionState>>>,
}

impl ActionRecorder {
    /// Create a new `ActionRecorder` using the global meter.
    pub fn new(scope: &'static str) -> Self {
        let meter = global::meter(scope);
        ActionRecorder {
            invocation_counter: meter
                .u64_counter("dcc_mcp.action.invocations")
                .with_description("Total number of action invocations")
                .build(),
            success_counter: meter
                .u64_counter("dcc_mcp.action.successes")
                .with_description("Number of successful action invocations")
                .build(),
            failure_counter: meter
                .u64_counter("dcc_mcp.action.failures")
                .with_description("Number of failed action invocations")
                .build(),
            duration_histogram: meter
                .f64_histogram("dcc_mcp.action.duration_ms")
                .with_description("Action execution duration in milliseconds")
                .with_unit("ms")
                .build(),
            state: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Begin timing an action execution.
    ///
    /// Returns a [`RecordingGuard`] that must be finished with
    /// [`RecordingGuard::finish`] to record the duration.
    #[instrument(skip(self), fields(action = action_name, dcc = dcc_name))]
    pub fn start(&self, action_name: &str, dcc_name: &str) -> RecordingGuard {
        let attrs = vec![
            KeyValue::new(span_keys::ACTION_NAME, action_name.to_string()),
            KeyValue::new(span_keys::DCC_NAME, dcc_name.to_string()),
        ];
        self.invocation_counter.add(1, &attrs);

        {
            let mut state = self.state.lock();
            let entry = state.entry(action_name.to_string()).or_default();
            entry.invocation_count += 1;
        }

        RecordingGuard {
            action_name: action_name.to_string(),
            dcc_name: dcc_name.to_string(),
            started_at: Instant::now(),
            recorder: self.clone(),
        }
    }

    /// Record a completed action (called by [`RecordingGuard::finish`]).
    pub(crate) fn record_completion(
        &self,
        action_name: &str,
        dcc_name: &str,
        duration: Duration,
        success: bool,
    ) {
        let ms = duration.as_secs_f64() * 1_000.0;
        let attrs = vec![
            KeyValue::new(span_keys::ACTION_NAME, action_name.to_string()),
            KeyValue::new(span_keys::DCC_NAME, dcc_name.to_string()),
            KeyValue::new(
                span_keys::OPERATION_SUCCESS,
                if success { "true" } else { "false" },
            ),
        ];

        self.duration_histogram.record(ms, &attrs);

        if success {
            self.success_counter.add(1, &attrs);
        } else {
            self.failure_counter.add(1, &attrs);
        }

        let mut state = self.state.lock();
        let entry = state.entry(action_name.to_string()).or_default();
        if success {
            entry.success_count += 1;
        } else {
            entry.failure_count += 1;
        }
        entry.durations.push(ms);
    }

    /// Get aggregated metrics for a specific action.
    ///
    /// Returns `None` if no invocations have been recorded for that action.
    pub fn metrics(&self, action_name: &str) -> Option<ActionMetrics> {
        let state = self.state.lock();
        state.get(action_name).map(|s| ActionMetrics {
            action_name: action_name.to_string(),
            invocation_count: s.invocation_count,
            success_count: s.success_count,
            failure_count: s.failure_count,
            avg_duration_ms: s.durations.avg(),
            p95_duration_ms: s.durations.percentile(95.0),
            p99_duration_ms: s.durations.percentile(99.0),
        })
    }

    /// Get aggregated metrics for all recorded actions.
    pub fn all_metrics(&self) -> Vec<ActionMetrics> {
        let state = self.state.lock();
        state
            .iter()
            .map(|(name, s)| ActionMetrics {
                action_name: name.clone(),
                invocation_count: s.invocation_count,
                success_count: s.success_count,
                failure_count: s.failure_count,
                avg_duration_ms: s.durations.avg(),
                p95_duration_ms: s.durations.percentile(95.0),
                p99_duration_ms: s.durations.percentile(99.0),
            })
            .collect()
    }

    /// Reset all in-memory statistics (does not affect OpenTelemetry counters).
    pub fn reset(&self) {
        let mut state = self.state.lock();
        state.clear();
    }
}

// ── RecordingGuard ────────────────────────────────────────────────────────────

/// RAII guard returned by [`ActionRecorder::start`].
///
/// Call [`RecordingGuard::finish`] to record the result.
/// If dropped without calling `finish`, the duration is recorded as a failure.
pub struct RecordingGuard {
    action_name: String,
    dcc_name: String,
    started_at: Instant,
    recorder: ActionRecorder,
}

impl RecordingGuard {
    /// Finish recording and store the result.
    pub fn finish(self, success: bool) {
        let elapsed = self.started_at.elapsed();
        self.recorder
            .record_completion(&self.action_name, &self.dcc_name, elapsed, success);
        // Consume self without running Drop logic.
        std::mem::forget(self);
    }
}

impl Drop for RecordingGuard {
    fn drop(&mut self) {
        // Guard dropped without `finish` — record as failure.
        let elapsed = self.started_at.elapsed();
        self.recorder
            .record_completion(&self.action_name, &self.dcc_name, elapsed, false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod test_duration_store {
        use super::*;

        #[test]
        fn avg_empty_is_zero() {
            let store = DurationStore::default();
            assert_eq!(store.avg(), 0.0);
        }

        #[test]
        fn percentile_empty_is_zero() {
            let store = DurationStore::default();
            assert_eq!(store.percentile(95.0), 0.0);
        }

        #[test]
        fn avg_single_value() {
            let mut store = DurationStore::default();
            store.push(42.0);
            assert!((store.avg() - 42.0).abs() < f64::EPSILON);
        }

        #[test]
        fn avg_multiple_values() {
            let mut store = DurationStore::default();
            store.push(10.0);
            store.push(20.0);
            store.push(30.0);
            assert!((store.avg() - 20.0).abs() < f64::EPSILON);
        }

        #[test]
        fn percentile_p100() {
            let mut store = DurationStore::default();
            for i in 1..=100 {
                store.push(i as f64);
            }
            let p100 = store.percentile(100.0);
            assert!((p100 - 100.0).abs() < f64::EPSILON);
        }

        #[test]
        fn max_samples_evicts_oldest() {
            let mut store = DurationStore::default();
            for i in 0..=DurationStore::MAX_SAMPLES {
                store.push(i as f64);
            }
            assert_eq!(store.samples.len(), DurationStore::MAX_SAMPLES);
            // The 0.0 entry should have been evicted.
            assert!(store.samples[0] > 0.0);
        }
    }

    mod test_recorder {
        use super::*;

        fn make_recorder() -> ActionRecorder {
            ActionRecorder::new("test-scope")
        }

        #[test]
        fn initial_metrics_is_none() {
            let recorder = make_recorder();
            assert!(recorder.metrics("nonexistent_action").is_none());
        }

        #[test]
        fn finish_success_increments_counts() {
            let recorder = make_recorder();
            let guard = recorder.start("create_sphere", "maya");
            guard.finish(true);

            let m = recorder.metrics("create_sphere").unwrap();
            assert_eq!(m.invocation_count, 1);
            assert_eq!(m.success_count, 1);
            assert_eq!(m.failure_count, 0);
        }

        #[test]
        fn finish_failure_increments_failure_count() {
            let recorder = make_recorder();
            let guard = recorder.start("delete_object", "blender");
            guard.finish(false);

            let m = recorder.metrics("delete_object").unwrap();
            assert_eq!(m.invocation_count, 1);
            assert_eq!(m.success_count, 0);
            assert_eq!(m.failure_count, 1);
        }

        #[test]
        fn drop_without_finish_counts_as_failure() {
            let recorder = make_recorder();
            {
                let _guard = recorder.start("dropped_action", "maya");
                // _guard dropped here without calling finish
            }
            let m = recorder.metrics("dropped_action").unwrap();
            assert_eq!(m.failure_count, 1);
        }

        #[test]
        fn multiple_invocations_accumulate() {
            let recorder = make_recorder();
            for _ in 0..5 {
                let g = recorder.start("render", "houdini");
                g.finish(true);
            }
            let g = recorder.start("render", "houdini");
            g.finish(false);

            let m = recorder.metrics("render").unwrap();
            assert_eq!(m.invocation_count, 6);
            assert_eq!(m.success_count, 5);
            assert_eq!(m.failure_count, 1);
        }

        #[test]
        fn success_rate_is_correct() {
            let recorder = make_recorder();
            for _ in 0..3 {
                recorder.start("act", "maya").finish(true);
            }
            recorder.start("act", "maya").finish(false);

            let m = recorder.metrics("act").unwrap();
            assert!((m.success_rate() - 0.75).abs() < 1e-9);
        }

        #[test]
        fn all_metrics_returns_all_actions() {
            let recorder = make_recorder();
            recorder.start("a1", "maya").finish(true);
            recorder.start("a2", "blender").finish(false);

            let all = recorder.all_metrics();
            assert_eq!(all.len(), 2);
        }

        #[test]
        fn reset_clears_all_stats() {
            let recorder = make_recorder();
            recorder.start("act", "maya").finish(true);
            recorder.reset();
            assert!(recorder.metrics("act").is_none());
        }
    }
}
