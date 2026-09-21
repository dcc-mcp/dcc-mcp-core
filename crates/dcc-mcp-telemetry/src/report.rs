//! Serialisable benchmark reports built from [`ToolMetrics`].
//!
//! [`ToolMetrics`] exposes [`ToolMetrics::success_rate`] as a *method*, so serde
//! never emits it. Trend tooling (and `pipeline-bench.json` consumers) needs the
//! success rate as a plain field, so this module projects every snapshot into a
//! [`StageReport`] where the value is materialised.
//!
//! ```text
//! let recorder = ToolRecorder::new("bench");
//! let guard = recorder.start("route_tool", "bench");
//! guard.finish(true);
//!
//! let report = BenchReport::from_metrics(recorder.all_metrics().iter());
//! report.write_json("target/pipeline-bench-raw.json")?;
//! ```

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::TelemetryError;
use crate::types::ToolMetrics;

/// Schema version of the exported report.
///
/// Bump this on any breaking field change so consumers can detect drift.
pub const REPORT_SCHEMA_VERSION: u32 = 1;

/// Per-stage projection of [`ToolMetrics`] with `success_rate` as a field.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StageReport {
    /// Stage / action name.
    pub stage: String,
    /// Total number of invocations.
    pub invocation_count: u64,
    /// Number of successful invocations.
    pub success_count: u64,
    /// Number of failed invocations.
    pub failure_count: u64,
    /// Average execution duration in milliseconds.
    pub avg_duration_ms: f64,
    /// P95 execution duration in milliseconds.
    pub p95_duration_ms: f64,
    /// P99 execution duration in milliseconds.
    pub p99_duration_ms: f64,
    /// Success rate as a fraction in `[0.0, 1.0]` (see [`ToolMetrics::success_rate`]).
    pub success_rate: f64,
}

impl StageReport {
    /// Project a metrics snapshot into its serialisable form.
    #[must_use]
    pub fn from_metrics(metrics: &ToolMetrics) -> Self {
        StageReport {
            stage: metrics.action_name.clone(),
            invocation_count: metrics.invocation_count,
            success_count: metrics.success_count,
            failure_count: metrics.failure_count,
            avg_duration_ms: metrics.avg_duration_ms,
            p95_duration_ms: metrics.p95_duration_ms,
            p99_duration_ms: metrics.p99_duration_ms,
            success_rate: metrics.success_rate(),
        }
    }
}

impl From<&ToolMetrics> for StageReport {
    fn from(metrics: &ToolMetrics) -> Self {
        StageReport::from_metrics(metrics)
    }
}

/// A collection of per-stage reports, ready to be written to disk.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BenchReport {
    /// Schema version of this payload ([`REPORT_SCHEMA_VERSION`]).
    pub schema_version: u32,
    /// Per-stage measurements, in the order they were supplied.
    pub stages: Vec<StageReport>,
}

impl Default for BenchReport {
    /// Delegates to [`BenchReport::new`] so the schema version is always stamped.
    fn default() -> Self {
        BenchReport::new()
    }
}

impl BenchReport {
    /// Build an empty report stamped with the current schema version.
    #[must_use]
    pub fn new() -> Self {
        BenchReport {
            schema_version: REPORT_SCHEMA_VERSION,
            stages: Vec::new(),
        }
    }

    /// Project metrics snapshots into a report, preserving iteration order.
    ///
    /// Callers that want stable output should sort the metrics first —
    /// [`ToolRecorder::all_metrics`] iterates a hash map and is unordered.
    pub fn from_metrics<'a, I>(metrics: I) -> Self
    where
        I: IntoIterator<Item = &'a ToolMetrics>,
    {
        let mut report = BenchReport::new();
        for metric in metrics {
            report.stages.push(StageReport::from_metrics(metric));
        }
        report
    }

    /// Look up a stage by name.
    #[must_use]
    pub fn stage(&self, name: &str) -> Option<&StageReport> {
        self.stages.iter().find(|s| s.stage == name)
    }

    /// Serialize the report to compact JSON.
    pub fn to_json(&self) -> Result<String, TelemetryError> {
        serde_json::to_string(self).map_err(|e| TelemetryError::ReportSerialization(e.to_string()))
    }

    /// Serialize the report to pretty-printed JSON.
    pub fn to_json_pretty(&self) -> Result<String, TelemetryError> {
        serde_json::to_string_pretty(self)
            .map_err(|e| TelemetryError::ReportSerialization(e.to_string()))
    }

    /// Write the report to `path`, creating parent directories as needed.
    pub fn write_json(&self, path: impl AsRef<Path>) -> Result<(), TelemetryError> {
        let path = path.as_ref();
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent).map_err(|source| TelemetryError::ReportIo {
                path: parent.display().to_string(),
                source,
            })?;
        }
        let json = self.to_json_pretty()?;
        std::fs::write(path, json).map_err(|source| TelemetryError::ReportIo {
            path: path.display().to_string(),
            source,
        })
    }

    /// Read a report previously written by [`BenchReport::write_json`].
    pub fn read_json(path: impl AsRef<Path>) -> Result<Self, TelemetryError> {
        let path = path.as_ref();
        let raw = std::fs::read_to_string(path).map_err(|source| TelemetryError::ReportIo {
            path: path.display().to_string(),
            source,
        })?;
        serde_json::from_str(&raw).map_err(|e| TelemetryError::ReportSerialization(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(action: &str, invocations: u64, successes: u64) -> ToolMetrics {
        ToolMetrics {
            action_name: action.to_string(),
            invocation_count: invocations,
            success_count: successes,
            failure_count: invocations - successes,
            avg_duration_ms: 2.5,
            p95_duration_ms: 4.5,
            p99_duration_ms: 6.5,
        }
    }

    mod test_stage_report {
        use super::*;

        #[test]
        fn success_rate_is_materialised_as_a_field() {
            let report = StageReport::from_metrics(&sample("route_tool", 10, 8));
            assert_eq!(report.stage, "route_tool");
            assert!((report.success_rate - 0.8).abs() < f64::EPSILON);
        }

        #[test]
        fn zero_invocations_yield_zero_success_rate() {
            let report = StageReport::from_metrics(&sample("cold_stage", 0, 0));
            assert_eq!(report.success_rate, 0.0);
        }

        #[test]
        fn durations_are_carried_over() {
            let report = StageReport::from_metrics(&sample("serialize", 4, 4));
            assert!((report.avg_duration_ms - 2.5).abs() < f64::EPSILON);
            assert!((report.p95_duration_ms - 4.5).abs() < f64::EPSILON);
            assert!((report.p99_duration_ms - 6.5).abs() < f64::EPSILON);
        }

        #[test]
        fn from_trait_matches_constructor() {
            let metrics = sample("parse_request", 5, 5);
            assert_eq!(
                StageReport::from(&metrics),
                StageReport::from_metrics(&metrics)
            );
        }
    }

    mod test_bench_report {
        use super::*;

        #[test]
        fn new_stamps_the_schema_version() {
            let report = BenchReport::new();
            assert_eq!(report.schema_version, REPORT_SCHEMA_VERSION);
            assert!(report.stages.is_empty());
        }

        #[test]
        fn from_metrics_preserves_order() {
            let metrics = [sample("b", 1, 1), sample("a", 2, 1)];
            let report = BenchReport::from_metrics(metrics.iter());
            let names: Vec<&str> = report.stages.iter().map(|s| s.stage.as_str()).collect();
            assert_eq!(names, vec!["b", "a"]);
        }

        #[test]
        fn json_round_trip_keeps_success_rate() {
            let metrics = [sample("dispatch", 4, 3)];
            let report = BenchReport::from_metrics(metrics.iter());
            let json = report.to_json().expect("serialize");
            assert!(json.contains("\"success_rate\""));
            let back: BenchReport = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(back, report);
        }

        #[test]
        fn stage_lookup_works() {
            let metrics = [sample("validate", 2, 2)];
            let report = BenchReport::from_metrics(metrics.iter());
            assert!(report.stage("validate").is_some());
            assert!(report.stage("missing").is_none());
        }

        #[test]
        fn write_and_read_json_round_trip() {
            let dir = std::env::temp_dir().join(format!(
                "dcc-mcp-telemetry-report-{:?}",
                std::thread::current().id()
            ));
            let path = dir.join("nested").join("pipeline-bench-raw.json");
            let metrics = [sample("respond", 3, 3)];
            let report = BenchReport::from_metrics(metrics.iter());
            report.write_json(&path).expect("write");

            let back = BenchReport::read_json(&path).expect("read");
            assert_eq!(back, report);

            let _ = std::fs::remove_file(&path);
            let _ = std::fs::remove_dir_all(&dir);
        }

        #[test]
        fn read_json_reports_a_missing_file() {
            let err = BenchReport::read_json("does/not/exist.json").expect_err("must fail");
            assert!(matches!(err, TelemetryError::ReportIo { .. }));
        }
    }
}
