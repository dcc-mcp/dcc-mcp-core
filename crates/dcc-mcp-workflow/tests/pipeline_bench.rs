//! Full-pipeline regression harness (issue #2270).
//!
//! Runs the fixed six-stage scenario — `model → rig → animate → texture →
//! render → composite` — [`ROUNDS`] times against a seeded
//! [`FakeToolCaller`] and gates the per-stage median *overhead*
//! (`observed - intended`), not the latency itself.
//!
//! Every round builds a fresh caller from the same seed, so all rounds
//! replay an *identical* workload: the same six latencies in the same
//! order. What varies between rounds is therefore the workflow executor and
//! the machine it runs on, not the fake host — which is the regression
//! signal we want, and the reason this test can run in CI with no DCC
//! installed.
//!
//! This is a **stability gate, not a speed gate**. Absolute latency on a
//! shared runner means nothing; a stage's systematic overhead over the
//! latency it was asked for means something in the executor started
//! blocking, allocating, or spawning per step.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use dcc_mcp_workflow::WorkflowExecutor;
use dcc_mcp_workflow::bench::{FakeToolCaller, PIPELINE_STAGES, pipeline_spec};
use dcc_mcp_workflow::spec::{StepKind, WorkflowSpec, WorkflowStatus};
use serde_json::json;

/// Number of pipeline runs per invocation.
const ROUNDS: usize = 30;

/// Budget for a stage's **median** `observed - intended` overhead.
///
/// The derivation is written out on
/// [`pipeline_full_stage_jitter_stays_within_budget`]. In short: the
/// milliseconds a loaded runner adds to a wait are *additive* — the same on
/// a 40 ms stage as on a 116 ms one — so this budget is absolute rather than
/// a fraction of the stage, and it is sized from measured host noise rather
/// than from a round percentage.
const MAX_MEDIAN_OVERHEAD: Duration = Duration::from_millis(10);

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

/// One stage of one round: what the seeded model asked for, and what the
/// wall clock actually produced.
struct Sample {
    intended: Duration,
    observed: Duration,
}

/// One pipeline run: the [`Sample`] for each stage, in execution order.
struct Round {
    stages: Vec<(String, Sample)>,
}

impl Round {
    /// Wall clock this round spent beyond the latencies it was asked to
    /// wait for.
    ///
    /// Purely diagnostic — printed so a noisy run is visible in the log, but
    /// the gate does not read it. Budgeting the *sum* would let one stage
    /// over budget hide behind five stages under it, which is the blind spot
    /// the old round-total outlier rejection had: a round was only dropped
    /// once it was ~19 ms over, while a single stage could already be three
    /// times over its own budget.
    fn overhead(&self) -> Duration {
        self.stages
            .iter()
            .map(|(_, sample)| sample.observed.saturating_sub(sample.intended))
            .sum()
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
            stages.push((
                record.tool.clone(),
                Sample {
                    intended: record.intended,
                    observed: record.observed,
                },
            ));
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

/// Per-stage overhead report, plus the stages that blew the budget.
///
/// Overhead is `observed - intended`. `CallRecord::observed` is measured
/// *inside* the fake call and nowhere else: `sleep_precise` arms a timer,
/// reports the instant it armed, and the caller stores `started.elapsed()`
/// once the wait returns. The modelled latency is exact, so the difference
/// is the wait mechanism plus the machine — **not** the whole executor.
///
/// That boundary is narrower than the wording here used to claim, and it
/// was pinned down by mutation rather than by reading:
///
/// | injected +15 ms | where                | verdict           |
/// |-----------------|----------------------|-------------------|
/// | arm A           | inside the fake call | **FAIL** (rc=101) |
/// | arm B           | between two calls    | **PASS** (rc=0)   |
///
/// Arm B passing is the whole point. Executor work that happens *outside*
/// a call — template rendering, context bookkeeping, notifier fan-out — can
/// grow by more than this gate's entire budget and still not be seen. That
/// cost belongs to `stages/executor_overhead` in
/// `benches/pipeline_full.rs`, which reports as a trend and gates nothing.
///
/// p95 and max of that same overhead are printed but **not** gated: on a
/// shared runner the tail belongs to the scheduler, and on 30 samples "p95"
/// is the 2nd-largest sample. See the test's doc comment for where that
/// signal went instead.
fn overhead_report(rounds: &[Round]) -> (Vec<String>, Vec<String>) {
    let mut per_stage: BTreeMap<String, Vec<Duration>> = BTreeMap::new();
    for round in rounds {
        for (stage, sample) in &round.stages {
            let overhead = sample.observed.saturating_sub(sample.intended);
            per_stage.entry(stage.clone()).or_default().push(overhead);
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
        // Nearest-rank, as this gate always computed it. Worth spelling out:
        // at n = 30 this is `sorted[28]`, the 2nd-*largest* sample of the
        // run, which is why it is reported rather than budgeted.
        let p95 = sorted[((0.95 * n as f64).ceil() as usize).min(n) - 1];
        let max = sorted[n - 1];
        lines.push(format!(
            "{stage:<24} n={n:<3} median={median:>9.2?} p95={p95:>9.2?} max={max:>9.2?}"
        ));
        if median > MAX_MEDIAN_OVERHEAD {
            offenders.push(format!(
                "{stage}: median overhead {median:.2?} exceeds the {MAX_MEDIAN_OVERHEAD:.2?} \
                 budget (p95 {p95:.2?}, max {max:.2?})"
            ));
        }
    }
    (lines, offenders)
}

/// Every round completes, each stage is called exactly once per round in
/// order, and every stage's median overhead stays inside
/// [`MAX_MEDIAN_OVERHEAD`].
///
/// # Why the budgeted quantity changed
///
/// Until this was rewritten the gate allowed `(p95 - median) / median <= 5%`
/// per stage, and on the schedule-only `Rust Full Matrix` nightly it was red
/// 4/4 retries, on macOS *and* Windows, three weeks running. Retries never
/// once rescued it, so this was not runner flake. Three compounding faults
/// in the measurement itself:
///
/// * **The budget was proportional; the noise is additive.** Every stage on
///   the failing runs carried the same ~7.5-9.6 ms of scheduling delay.
///   Against the ~116 ms `render` stage that is 6%; against the ~46 ms
///   `composite` stage it is 21%. On one Windows run `render` (0.00%) and
///   `rig` (0.06%) passed while the four stages between them failed — an
///   ordering no code defect can produce, and exactly what one constant
///   additive penalty produces as stage length varies.
/// * **"p95" was a near-max statistic.** At n = 30, `ceil(0.95 * n) - 1`
///   selects `sorted[28]`, the second-*largest* sample of the run. The gate
///   tracked the worst scheduling hiccup of the night, not the executor.
/// * **The budget sat below its own noise floor.** Expressed absolutely, 5%
///   is 2.06 ms for `rig` and 5.81 ms for `render` of tail spread, against a
///   tail measured at 7.5-9.6 ms on those same runners.
///
/// So the budgeted quantity is now `observed - intended` in absolute
/// milliseconds, read with a median. `observed` spans only the wait inside
/// the fake call, so the difference is the wait mechanism plus the machine,
/// and the machine's share is additive and heavy-tailed — which is what a
/// median is for. Executor work performed outside that window is out of
/// scope for this gate; see [`overhead_report`] for where the boundary sits
/// and for the mutation experiment that established it.
///
/// # Where 10 ms comes from
///
/// Worst per-stage median overhead measured while triaging that failure, 30
/// rounds x 6 stages per run, on every host available:
///
/// | host                                              | worst stage |
/// |---------------------------------------------------|-------------|
/// | GitHub-hosted macOS runner, failing nightly run   | 0.167 ms    |
/// | GitHub-hosted Windows runner, failing nightly run | 0.531 ms    |
/// | idle workstation                                  | 0.639 ms    |
/// | workstation with other jobs sharing the CPU       | 5.16 ms     |
/// | workstation under 40- and 64-way CPU contention   | 5.26 ms     |
///
/// 10 ms is ~19x the worst figure from the runners this test actually gates
/// on, and ~1.9x the worst median measured anywhere, including a workstation
/// deliberately oversubscribed past its core count.
///
/// That trade is deliberate and should be stated plainly: this gate catches
/// a stage that starts *blocking* — on a lock, a spawn, a per-step
/// allocation. Those cost at least a scheduling quantum, so they land in
/// milliseconds. It does not catch a gradual slowdown or a widening tail;
/// neither is measurable on a shared runner, and `benches/pipeline_full.rs`
/// is the instrument for both. Rejecting a stage for exceeding a budget is
/// only defensible while that budget is above the noise you already know
/// about, and 5% of a stage was not.
///
/// # What the residual overhead actually is
///
/// On the two CI runners this gate cares about the medians are 0.167 ms and
/// 0.531 ms — the harness lands essentially on target. On a workstation
/// carrying sustained foreign load the same measurement sits at 3-5 ms per
/// stage. That difference is host latency, not executor cost, and three
/// measurements pin it down so it is not re-litigated as a code defect:
///
/// * **It is not a function of stage duration.** In one run `rig`
///   (52.185 ms intended) carried 5.22 ms of overhead while `texture`
///   (50.699 ms intended) carried 3.6 us — near-identical lengths, three
///   orders of magnitude apart in overhead.
/// * **It is not tick granularity.** Probing `tokio::time::sleep` overshoot
///   across 1/7/23/39/46/50/55/76/114 ms showed no duration dependence at
///   all, only 6-15 ms of spread that tracks machine load. A 1 ms sleep
///   overshooting by 14.5 ms is the thread not being *scheduled* once the
///   timer fired, not the timer being coarse.
/// * **It is the wake-up latency the reserve declines to absorb.** The
///   harness caps that reserve well below the shortest stage on purpose:
///   buying accuracy past the cap means burning a core, and letting the
///   reserve grow on observed overshoot instead — what this harness used to
///   do — is a positive-feedback loop that walks it to its ceiling within
///   one test, at which point the shortest stage stops sleeping entirely.
///
/// # What the tail is for now
///
/// p95 and max are still printed, so a change in dispersion remains visible
/// in the log when someone has to triage this again, but nothing is asserted
/// on them. A budget derived mostly from scheduler state is a budget that
/// fails on days when nothing changed.
#[tokio::test]
async fn pipeline_full_stage_jitter_stays_within_budget() {
    let spec = pipeline_spec();
    let rounds = measure_rounds(&spec).await;
    assert_eq!(rounds.len(), ROUNDS);

    let (lines, offenders) = overhead_report(&rounds);
    let mut per_round: Vec<Duration> = rounds.iter().map(Round::overhead).collect();
    per_round.sort_unstable();

    eprintln!(
        "per-stage overhead (observed - intended) over {} rounds; host added \
         {:.2?}-{:.2?} per round:\n  {}",
        rounds.len(),
        per_round[0],
        per_round[per_round.len() - 1],
        lines.join("\n  "),
    );

    assert!(
        offenders.is_empty(),
        "per-stage median overhead exceeded the {:.2?} budget — a stage is \
         blocking that did not used to:\n  {}",
        MAX_MEDIAN_OVERHEAD,
        offenders.join("\n  "),
    );
}
