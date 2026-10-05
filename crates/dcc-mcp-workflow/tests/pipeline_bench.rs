//! Full-pipeline regression harness (issue #2270).
//!
//! Runs the fixed six-stage scenario — `model → rig → animate → texture →
//! render → composite` — [`ROUNDS`] times against a seeded
//! [`FakeToolCaller`].
//!
//! Every round builds a fresh caller from the same seed, so all rounds
//! replay an *identical* workload: the same six latencies in the same
//! order. Whatever a round costs on top of that model is executor
//! overhead plus host scheduling noise, and telling those two apart is the
//! whole difficulty — see [the budget derivation][self#why-the-budget-is-what-it-is].
//!
//! This is a **stability gate, not a speed gate**, and it is split across
//! two channels because the two halves have very different reliability:
//!
//! * [`pipeline_full_replays_every_stage_in_order`] runs in the ordinary
//!   `cargo nextest run --workspace` gate. It asserts only invariants that
//!   are properties of *our* code and cannot be changed by a busy
//!   machine: every round completes, every stage is called exactly once in
//!   order, and no call returns before the latency it was seeded with.
//! * [`pipeline_full_round_overhead_stays_within_budget`] is compiled only
//!   under the `wallclock-bench` feature and runs in
//!   `.github/workflows/transport-bench.yml`. It is the wall-clock
//!   measurement, and it lives in the benchmark channel because a
//!   wall-clock budget is a drift envelope, not a pass/fail property.
//!
//! # Why the per-stage jitter gate left the unit suite
//!
//! Until PIP-4223 this file gated `(p95 - median) / median` of each stage's
//! observed duration at 5%, and `Rust Full Matrix` failed that gate 4/4
//! retries on macOS and Windows in three consecutive weekly runs (worst
//! observed: composite p95 20.67% above its median). Two things were wrong
//! with it, and neither was the host-timer compensation.
//!
//! **The compensation works.** On an idle host the same harness reports
//! 0.00–0.04% per-stage jitter, and `FakeToolCaller` tracks its seeded
//! latency to ~2 µs. The doc comment claimed that made the 5% gate safe;
//! what the compensation actually removes is *timer granularity* (a
//! ~15.6 ms Windows tick), not *scheduler preemption*, which no in-process
//! wait can absorb.
//!
//! **The statistic measured the host, not us.** Because the seeded workload
//! is byte-identical every round, the dispersion of a stage's observed
//! duration has no executor component at all — it is the distribution of
//! how badly this round's thread was preempted. Worse, it cannot see the
//! regression it claims to catch: a steady per-step slowdown moves the
//! median and the p95 by the same amount, so the ratio is unchanged. Only
//! preemption moves them apart.
//!
//! The round-total outlier filter could not rescue it. A 10 ms preemption
//! inside a 40 ms stage is 25% of that stage but only 2.5% of the ~390 ms
//! round, so the poisoned round passed the 5% round-total ceiling and then
//! failed the 5% stage budget.
//!
//! Reproduced locally by pinning the harness to 2 cores alongside 32
//! spinning processes (emulating a 3–4 core runner oversubscribed by
//! nextest): 3/3 runs failed, all six stages over budget, 0.97%–18.91% —
//! the same signature as CI.
//!
//! # Why the budget is what it is
//!
//! The replacement gates the **minimum** end-to-end round overhead over
//! [`ROUNDS`] runs, where overhead is `round wall clock − Σ intended stage
//! latencies`. Preemption is additive and one-sided, so the minimum of many
//! samples converges on the executor's true cost while the median and p95
//! drift with the host. Measured on a 32-core Windows host, cargo 1.95,
//! `test` profile, 5 runs per condition:
//!
//! | host state                              | overhead min | p10   | median | p95     |
//! |-----------------------------------------|--------------|-------|--------|---------|
//! | idle                                    | 0.67–0.79 ms | 0.70–0.81 ms | 0.86–0.98 ms | 1.35–1.63 ms |
//! | 32 spinners, all cores available        | 1.06–11.25 ms| 1.94–22.38 ms | 3.14–57.90 ms | 5.10–106.55 ms |
//! | 32 spinners, pinned to 2 cores          | 1.26–4.93 ms | 1.77–6.77 ms | 5.37–15.92 ms | 9.98–182.69 ms |
//!
//! Under contention the median moves ~60× and the p95 ~100×; the minimum
//! moves ~7×, and the p10 ~28×. So the gate uses the minimum, and
//! [`MAX_ROUND_OVERHEAD`] is set at **30 ms**: ~2.7× the worst minimum
//! observed under any condition above (11.25 ms), and ~40× the idle value.
//!
//! That is deliberately a *sanity* bound, not a precision measurement: it
//! catches "someone put a blocking operation in the executor's per-step hot
//! path" and nothing finer. The ~0.7 ms idle floor above is not all
//! executor — it also carries the fake caller's reserve-and-spin wait
//! machinery. The executor alone is cheaper still:
//! `benches/pipeline_full.rs` zeroes the latency table and measures
//! `stages/executor_overhead` at ~0.13 ms per round, and that criterion case
//! is where fine-grained drift belongs, because criterion compares each run
//! against the previous one instead of against a fixed number.

use std::sync::Arc;
use std::time::{Duration, Instant};

use dcc_mcp_workflow::WorkflowExecutor;
use dcc_mcp_workflow::bench::{FakeToolCaller, PIPELINE_STAGES, pipeline_spec};
use dcc_mcp_workflow::spec::{StepKind, WorkflowSpec, WorkflowStatus};
use serde_json::json;

/// Number of pipeline runs per invocation.
const ROUNDS: usize = 30;

/// Seed for the fake caller. Fixed so a regression is attributable to the
/// executor rather than to a new latency draw.
const SEED: u64 = 0xC0FF_EE01;

/// Ceiling on the cheapest round's end-to-end overhead.
///
/// Derived from measurement — see [the module docs][self#why-the-budget-is-what-it-is].
#[cfg(feature = "wallclock-bench")]
const MAX_ROUND_OVERHEAD: Duration = Duration::from_millis(30);

#[test]
fn scenario_has_the_six_documented_stages() {
    let spec = pipeline_spec();
    let tools: Vec<&str> = spec
        .steps
        .iter()
        .map(|step| match &step.kind {
            StepKind::Tool { tool, .. } => tool.as_str(),
            other => panic!("expected a tool step, got {}", other.kind_str()),
        })
        .collect();
    assert_eq!(
        tools, PIPELINE_STAGES,
        "scenarios/pipeline_full.yaml drifted from PIPELINE_STAGES"
    );
    assert_eq!(spec.steps.len(), 6);
}

/// One pipeline run: the wall-clock cost of the whole round plus, per stage,
/// what the seeded model asked for and what the host delivered.
struct Round {
    /// End-to-end wall time of `WorkflowExecutor::run(..).wait()`.
    wall: Duration,
    /// Sum of the latencies the seeded model asked for.
    intended: Duration,
    /// Per stage: `(tool, observed, intended)`, in execution order.
    ///
    /// Only the wall-clock report reads these; the unit gate works from
    /// [`Round::overhead`] alone.
    #[cfg(feature = "wallclock-bench")]
    stages: Vec<(String, Duration, Duration)>,
}

impl Round {
    /// Everything the round cost that the latency model does not account
    /// for: executor overhead plus host scheduling noise.
    ///
    /// Saturating because the two clocks are independent: a round can
    /// theoretically be timed as finishing inside the model's own budget.
    fn overhead(&self) -> Duration {
        self.wall.saturating_sub(self.intended)
    }
}

/// Run [`ROUNDS`] pipelines, asserting every round completes, calls each
/// stage exactly once in order, and never returns early.
///
/// These are the invariants a loaded machine cannot break. No stage ever
/// finishing before its seeded latency is the one wall-clock property that
/// noise cannot fake: preemption only ever makes an observed duration
/// *longer*, so a shortfall means the wait itself regressed — which is the
/// failure `FakeToolCaller`'s reserve-and-spin logic exists to prevent.
async fn measure_rounds(spec: &WorkflowSpec) -> Vec<Round> {
    // Warm the host-timer calibration up front; the first wait in a process
    // would otherwise fold it into its measured duration.
    FakeToolCaller::calibrate().await;

    let mut rounds = Vec::with_capacity(ROUNDS);
    let mut incomplete = Vec::new();

    for round in 0..ROUNDS {
        let caller = Arc::new(FakeToolCaller::pipeline_full(SEED));
        let runner = WorkflowExecutor::builder()
            .tool_caller(caller.clone())
            .build();
        let inputs = json!({
            "asset": "hero_prop",
            "frame_start": 1001,
            "frame_end": 1120,
        });
        let started = Instant::now();
        let status = runner
            .run(spec.clone(), inputs, None)
            .expect("run accepts a validated spec")
            .wait()
            .await;
        let wall = started.elapsed();
        if status != WorkflowStatus::Completed {
            incomplete.push(format!("round {round} ended in {status:?}"));
        }

        let records = caller.records();
        assert_eq!(
            records.len(),
            PIPELINE_STAGES.len(),
            "round {round}: expected one call per stage"
        );
        #[cfg(feature = "wallclock-bench")]
        let mut stages = Vec::with_capacity(records.len());
        let mut intended = Duration::ZERO;
        for (record, stage) in records.iter().zip(PIPELINE_STAGES.iter()) {
            assert_eq!(
                &record.tool, stage,
                "round {round}: stages executed out of order"
            );
            assert!(
                record.observed >= record.intended,
                "round {round}: {} returned before its seeded latency: \
                 observed {:.2?} < intended {:.2?} — the fake caller's \
                 reserve-and-spin wait under-waited",
                record.tool,
                record.observed,
                record.intended,
            );
            intended += record.intended;
            #[cfg(feature = "wallclock-bench")]
            stages.push((record.tool.clone(), record.observed, record.intended));
        }
        rounds.push(Round {
            wall,
            intended,
            #[cfg(feature = "wallclock-bench")]
            stages,
        });
    }

    assert!(
        incomplete.is_empty(),
        "{}/{} rounds did not complete:\n  {}",
        incomplete.len(),
        ROUNDS,
        incomplete.join("\n  "),
    );
    rounds
}

/// Nearest-rank percentile of an already-sorted, non-empty slice, in ms.
fn percentile_ms(sorted_ms: &[f64], p: f64) -> f64 {
    let n = sorted_ms.len();
    let rank = (p.clamp(0.0, 1.0) * n as f64).ceil() as usize;
    sorted_ms[rank.saturating_sub(1).min(n - 1)]
}

/// Every round completes, each stage is called exactly once per round in
/// order, and no call returns early.
///
/// This is the unit gate: it runs on every PR and on all three OSes in
/// `Rust Full Matrix`, so it asserts only what a busy runner cannot
/// influence. The wall-clock numbers it prints are diagnostics — the gate
/// that uses them is [`pipeline_full_round_overhead_stays_within_budget`],
/// behind the `wallclock-bench` feature.
#[tokio::test]
async fn pipeline_full_replays_every_stage_in_order() {
    let spec = pipeline_spec();
    let rounds = measure_rounds(&spec).await;
    assert_eq!(rounds.len(), ROUNDS);

    let mut overheads_ms: Vec<f64> = rounds
        .iter()
        .map(|r| r.overhead().as_secs_f64() * 1e3)
        .collect();
    overheads_ms.sort_by(|a, b| a.partial_cmp(b).unwrap());

    eprintln!(
        "round overhead over {ROUNDS} rounds (ms): min={:.2} p50={:.2} p95={:.2} max={:.2}\n\
         (diagnostic only — the wall-clock gate runs under --features wallclock-bench)",
        overheads_ms[0],
        percentile_ms(&overheads_ms, 0.5),
        percentile_ms(&overheads_ms, 0.95),
        overheads_ms[overheads_ms.len() - 1],
    );
}

/// Per-stage dispersion report, kept as a diagnostic.
///
/// Not gated: on an oversubscribed runner these numbers are a measurement of
/// the host, not of the executor — see [the module docs][self#why-the-per-stage-jitter-gate-left-the-unit-suite].
#[cfg(feature = "wallclock-bench")]
fn stage_report(rounds: &[Round]) -> Vec<String> {
    use std::collections::BTreeMap;

    let mut per_stage: BTreeMap<&str, Vec<Duration>> = BTreeMap::new();
    for round in rounds {
        for (stage, observed, _intended) in &round.stages {
            per_stage.entry(stage.as_str()).or_default().push(*observed);
        }
    }

    let mut lines = Vec::new();
    for stage in PIPELINE_STAGES {
        let samples = per_stage
            .get(stage)
            .unwrap_or_else(|| panic!("no samples for {stage}"));
        let mut sorted = samples.clone();
        sorted.sort_unstable();
        let n = sorted.len();
        let median = sorted[n / 2];
        // Nearest-rank p95: tolerates a single outlying sample in 30.
        let p95 = sorted[((0.95 * n as f64).ceil() as usize).min(n) - 1];
        let jitter = if median.is_zero() {
            f64::INFINITY
        } else {
            (p95.as_secs_f64() - median.as_secs_f64()) / median.as_secs_f64()
        };
        lines.push(format!(
            "{stage:<24} n={n:<3} median={median:>9.2?} p95={p95:>9.2?} jitter={:>6.2}%",
            jitter * 100.0
        ));
    }
    lines
}

/// The cheapest round's end-to-end overhead stays inside
/// [`MAX_ROUND_OVERHEAD`].
///
/// Runs only under `--features wallclock-bench`, from
/// `.github/workflows/transport-bench.yml`. It is kept out of
/// `cargo nextest run --workspace` because a wall-clock budget is a
/// statement about the host as much as about the executor: the previous
/// in-gate version of this measurement failed 4/4 retries on two OSes in
/// three consecutive weekly runs while reporting nothing about our code.
///
/// The estimator is the **minimum** round overhead, not the median or the
/// p95, because preemption is additive and one-sided — see [the module
/// docs][self#why-the-budget-is-what-it-is] for the measurements behind both
/// the estimator and [`MAX_ROUND_OVERHEAD`].
#[cfg(feature = "wallclock-bench")]
#[tokio::test]
async fn pipeline_full_round_overhead_stays_within_budget() {
    let spec = pipeline_spec();
    let rounds = measure_rounds(&spec).await;
    assert_eq!(rounds.len(), ROUNDS);

    let mut overheads: Vec<Duration> = rounds.iter().map(Round::overhead).collect();
    overheads.sort_unstable();
    let overheads_ms: Vec<f64> = overheads.iter().map(|d| d.as_secs_f64() * 1e3).collect();

    let cheapest = overheads[0];
    eprintln!(
        "per-stage latency over {ROUNDS} rounds (diagnostic, not gated):\n  {}\n\
         round overhead (ms): min={:.2} p10={:.2} p50={:.2} p95={:.2} max={:.2}",
        stage_report(&rounds).join("\n  "),
        overheads_ms[0],
        percentile_ms(&overheads_ms, 0.10),
        percentile_ms(&overheads_ms, 0.50),
        percentile_ms(&overheads_ms, 0.95),
        overheads_ms[overheads_ms.len() - 1],
    );

    assert!(
        cheapest <= MAX_ROUND_OVERHEAD,
        "the cheapest of {ROUNDS} rounds still cost {cheapest:.2?} more than the \
         seeded latency model — the executor's per-step hot path regressed, or \
         this host is too loaded to measure (budget {MAX_ROUND_OVERHEAD:.2?}):\n  {}",
        stage_report(&rounds).join("\n  "),
    );
}
