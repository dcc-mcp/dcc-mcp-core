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

/// Keep the whole run inside the measurement budget. The simulated-latency
/// case costs ~0.4 s per iteration, so the sample count — not the wall time
/// — is what has to be bounded; `measurement_time` only matters for the
/// zero-latency case, which is cheap enough to fill it many times over.
///
/// Budget: 10 samples of a ~0.4 s pipeline plus warm-up lands the whole
/// `cargo bench` invocation around 15 s of measuring, well inside the 3 min
/// ceiling (and far below the 20 min Sentry E2E job that currently sets the
/// pace for the workflow).
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
