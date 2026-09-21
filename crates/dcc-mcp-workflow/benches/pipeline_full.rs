//! Criterion benchmark for the full six-stage pipeline (issue #2270).
//!
//! Drives `scenarios/pipeline_full.yaml` (`model → rig → animate → texture
//! → render → composite`) through the real [`WorkflowExecutor`] with a
//! seeded [`FakeToolCaller`] standing in for a DCC host, so the numbers are
//! reproducible on any machine and in CI.
//!
//! Two cases:
//!
//! * `stages/simulated_latency` — the realistic latency table. End-to-end
//!   trend for a whole pipeline.
//! * `stages/executor_overhead` — the same pipeline with the latency table
//!   zeroed, so what is left is the executor's own cost: template
//!   rendering, context bookkeeping, policy evaluation, notifier fan-out.
//!
//! Results are a diagnostic trend, **not a PR merge gate**. Run with:
//!
//! ```bash
//! cargo bench -p dcc-mcp-workflow --bench pipeline_full
//! ```

use std::hint::black_box;
use std::sync::Arc;
use std::time::{Duration, Instant};

use criterion::{Criterion, criterion_group, criterion_main};
use dcc_mcp_workflow::bench::{
    FakeCallerConfig, FakeToolCaller, LatencyModel, PIPELINE_STAGES, pipeline_spec,
};
use dcc_mcp_workflow::{WorkflowExecutor, WorkflowStatus};
use serde_json::json;

/// Seed for the fake caller. Fixed so successive bench runs are comparable.
const SEED: u64 = 0xC0FF_EE01;

/// Keep the whole run inside the 3 min measurement budget (the 20 min Sentry
/// E2E job currently sets the pace for the workflow, so this is not the
/// binding constraint).
///
/// The two knobs are a **floor**, not a ceiling: criterion takes at least
/// `sample_size` samples *and* runs for at least `measurement_time`, so the
/// slower of the two wins per benchmark.
///
/// - `stages/simulated_latency` costs ~390 ms per iteration, and criterion
///   reports `Collecting 10 samples in estimated 5.7 s`, so the sample count
///   is what binds and `MEASUREMENT` never applies. This is why we keep the
///   sample count small, not the wall time.
/// - `stages/executor_overhead` costs ~0.1 ms per iteration, so it fills
///   `MEASUREMENT` (3 s) after ~19k iterations, long before it reaches 10
///   samples, and the time is what binds.
///
///   That ~0.1 ms figure is **host-dependent**: measured runs of this bench
///   have landed anywhere between ~80 us and ~175 us. Only the order of
///   magnitude matters here — it is ~3 orders below the simulated case, so
///   the time knob wins either way.
///
/// Measured total for this file: ~9 s (5.7 s + 3.0 s plus warm-up).
///
/// 10 is criterion's recommended floor for a stable estimate; anything
/// lower and it starts warning that the confidence interval is unreliable.
const SAMPLE_SIZE: usize = 10;
const WARM_UP: Duration = Duration::from_millis(500);
const MEASUREMENT: Duration = Duration::from_secs(3);

/// Drive one full pipeline run to completion and return its wall time.
async fn run_once(caller: Arc<FakeToolCaller>) -> Duration {
    let spec = pipeline_spec();
    let runner = WorkflowExecutor::builder()
        .tool_caller(caller.clone())
        .build();
    let started = Instant::now();
    let handle = runner
        .run(black_box(spec), json!({"asset": "hero_prop"}), None)
        .expect("the embedded scenario is validated at load time");
    let status = handle.wait().await;
    assert_eq!(status, WorkflowStatus::Completed);
    started.elapsed()
}

fn bench_pipeline_full(c: &mut Criterion) {
    // A current-thread runtime keeps the executor's own scheduling off the
    // measurement; the fake caller's delays are explicit, so nothing needs
    // a worker pool.
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .expect("bench runtime");

    let mut group = c.benchmark_group("pipeline_full");
    group.sample_size(SAMPLE_SIZE);
    group.warm_up_time(WARM_UP);
    group.measurement_time(MEASUREMENT);

    group.bench_function("stages/simulated_latency", |b| {
        b.iter_custom(|iters| {
            rt.block_on(async {
                let mut total = Duration::ZERO;
                for _ in 0..iters {
                    let caller = Arc::new(FakeToolCaller::pipeline_full(SEED));
                    total += run_once(caller).await;
                }
                total
            })
        });
    });

    group.bench_function("stages/executor_overhead", |b| {
        // Zero the latency table: whatever is left is the executor.
        let config = FakeCallerConfig::pipeline_full(SEED)
            .with_tool("bench_stage_model", LatencyModel::fixed(0));
        let config = PIPELINE_STAGES.iter().fold(config, |acc, stage| {
            acc.with_tool(stage, LatencyModel::fixed(0))
        });

        b.iter_custom(|iters| {
            rt.block_on(async {
                let mut total = Duration::ZERO;
                for _ in 0..iters {
                    let caller = Arc::new(FakeToolCaller::new(config.clone()));
                    total += run_once(caller).await;
                }
                total
            })
        });
    });

    group.finish();
}

criterion_group!(benches, bench_pipeline_full);
criterion_main!(benches);
