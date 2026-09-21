//! Full-pipeline regression harness (issue #2270).
//!
//! Runs the fixed six-stage scenario — `model → rig → animate → texture →
//! render → composite` — [`ROUNDS`] times against a seeded
//! [`FakeToolCaller`] and gates the per-stage p95 jitter.
//!
//! Every round builds a fresh caller from the same seed, so all rounds
//! replay an *identical* workload: the same six latencies in the same
//! order. Cross-round jitter therefore measures the workflow executor and
//! the machine it runs on, not the fake host — which is the regression
//! signal we want, and the reason this test can run in CI with no DCC
//! installed.
//!
//! This is a **stability gate, not a speed gate**. Absolute latency on a
//! shared runner means nothing; a stage whose p95 diverges from its median
//! means something in the executor started blocking, allocating, or
//! spawning per step.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use dcc_mcp_workflow::WorkflowExecutor;
use dcc_mcp_workflow::bench::{FakeToolCaller, PIPELINE_STAGES, pipeline_spec};
use dcc_mcp_workflow::spec::{StepKind, WorkflowSpec, WorkflowStatus};
use serde_json::json;

/// Number of pipeline runs per invocation.
const ROUNDS: usize = 30;

/// Maximum tolerated `(p95 - median) / median` per stage.
const MAX_JITTER_RATIO: f64 = 0.05;

/// How far a round's *total* may sit above the median round before it is
/// discarded as a machine hiccup — see [`Round::total`].
const ROUND_OUTLIER_RATIO: f64 = 0.05;

/// Minimum number of rounds that must survive outlier rejection.
const MIN_USABLE_ROUNDS: usize = 27;

/// Seed for the fake caller. Fixed so a regression is attributable to the
/// executor rather than to a new latency draw.
const SEED: u64 = 0xC0FF_EE01;

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

/// One pipeline run: the observed duration of each stage, in execution
/// order.
struct Round {
    stages: Vec<(String, Duration)>,
}

impl Round {
    /// Sum of the stage durations.
    ///
    /// Used to spot machine hiccups. A scheduling hiccup, a noisy neighbour,
    /// or CPU steal on a shared runner inflates *every* stage in a round at
    /// once, because all six are waiting on the same overtaxed core. An
    /// executor regression does not look like that: it lands on one stage,
    /// or shifts all of them by a level, rather than making single rounds
    /// uniformly slow. So a round whose total sits well above the median
    /// round is a measurement artifact and is dropped.
    fn total(&self) -> Duration {
        self.stages.iter().map(|(_, d)| *d).sum()
    }
}

/// Run [`ROUNDS`] pipelines, asserting every round completes and calls each
/// stage exactly once, in order.
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
        let status = runner
            .run(spec.clone(), inputs, None)
            .expect("run accepts a validated spec")
            .wait()
            .await;
        if status != WorkflowStatus::Completed {
            incomplete.push(format!("round {round} ended in {status:?}"));
        }

        let records = caller.records();
        assert_eq!(
            records.len(),
            PIPELINE_STAGES.len(),
            "round {round}: expected one call per stage"
        );
        let mut stages = Vec::with_capacity(records.len());
        for (record, stage) in records.iter().zip(PIPELINE_STAGES.iter()) {
            assert_eq!(
                &record.tool, stage,
                "round {round}: stages executed out of order"
            );
            stages.push((record.tool.clone(), record.observed));
        }
        rounds.push(Round { stages });
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

/// Drop rounds whose total duration is a machine artifact, keeping the rest.
///
/// Rejection is one-sided: only rounds *slower* than the median are dropped.
/// An unusually fast round cannot be produced by load, so keeping it can
/// only make the jitter estimate more conservative.
fn discard_noisy_rounds(rounds: Vec<Round>) -> Vec<Round> {
    let mut totals: Vec<Duration> = rounds.iter().map(Round::total).collect();
    totals.sort_unstable();
    let median_total = totals[totals.len() / 2];
    let ceiling = median_total.mul_f64(1.0 + ROUND_OUTLIER_RATIO);

    let kept: Vec<Round> = rounds
        .into_iter()
        .filter(|r| r.total() <= ceiling)
        .collect();
    eprintln!(
        "round totals: median {median_total:.2?}, ceiling {ceiling:.2?} — kept {}/{} rounds",
        kept.len(),
        totals.len(),
    );
    kept
}

/// Per-stage dispersion report plus the stages that blew the budget.
fn jitter_report(rounds: &[Round]) -> (Vec<String>, Vec<String>) {
    let mut per_stage: BTreeMap<String, Vec<Duration>> = BTreeMap::new();
    for round in rounds {
        for (stage, observed) in &round.stages {
            per_stage.entry(stage.clone()).or_default().push(*observed);
        }
    }

    let mut lines = Vec::new();
    let mut offenders = Vec::new();
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
        if jitter > MAX_JITTER_RATIO {
            offenders.push(format!(
                "{stage}: p95 {p95:?} is {:.2}% above median {median:?} (budget {:.0}%)",
                jitter * 100.0,
                MAX_JITTER_RATIO * 100.0,
            ));
        }
    }
    (lines, offenders)
}

/// Every round completes, each stage is called exactly once per round in
/// order, and the per-stage p95 jitter stays inside [`MAX_JITTER_RATIO`].
///
/// # Why a wall-clock gate is safe here
///
/// A 5% budget on a 40-100 ms stage is a couple of milliseconds, which a
/// bare `tokio::time::sleep` cannot deliver: on Windows the default timer
/// granularity is ~15.6 ms, so a sleep can return sixteen milliseconds late
/// — several times the whole budget.
///
/// [`FakeToolCaller`] absorbs that. It measures the host's timer granularity
/// once, sleeps only up to `deadline - reserve`, and waits out the remainder
/// itself (yielding while the deadline is far away, spinning for the last
/// few microseconds). The delay then tracks the seeded model to well under a
/// millisecond, which is what turns a 5% wall-clock gate from a flake into a
/// measurement. Rounds perturbed by system-wide load are dropped separately
/// — see [`discard_noisy_rounds`].
#[tokio::test]
async fn pipeline_full_stage_jitter_stays_within_budget() {
    let spec = pipeline_spec();
    let rounds = measure_rounds(&spec).await;
    assert_eq!(rounds.len(), ROUNDS);

    let usable = discard_noisy_rounds(rounds);
    let (lines, offenders) = jitter_report(&usable);

    eprintln!(
        "per-stage latency over {} usable rounds:\n  {}",
        usable.len(),
        lines.join("\n  ")
    );

    assert!(
        usable.len() >= MIN_USABLE_ROUNDS,
        "only {}/{} rounds were within {:.0}% of the median round total — \
         the host was too noisy to measure (stages reported below):\n  {}",
        usable.len(),
        ROUNDS,
        ROUND_OUTLIER_RATIO * 100.0,
        lines.join("\n  "),
    );

    assert!(
        offenders.is_empty(),
        "per-stage p95 jitter exceeded the {:.0}% budget:\n  {}",
        MAX_JITTER_RATIO * 100.0,
        offenders.join("\n  "),
    );
}
