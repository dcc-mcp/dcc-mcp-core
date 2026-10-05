//! Deterministic, host-free [`ToolCaller`] for full-pipeline benchmarks.
//!
//! The workflow executor only touches the outside world through
//! [`ToolCaller`] (`run_tool_step`) and [`RemoteCaller`]
//! (`run_remote_step`). Swapping in a fake implementation therefore
//! isolates a benchmark from every DCC host without changing a line of
//! production executor code.
//!
//! [`FakeToolCaller`] is deliberately reproducible:
//!
//! * Latencies come from a **seeded** PRNG ([`SplitMix64`]), so a given
//!   seed always produces the same sequence of per-call latencies and
//!   failures.
//! * Each call waits for its drawn latency with [`sleep_precise`], which
//!   measures the host's timer granularity once, sleeps only up to
//!   `deadline - reserve`, and then spins out the remainder on its own
//!   clock. A bare `tokio::time::sleep` overshoots by up to a
//!   full timer tick — over 15 ms on Windows — which is several times the
//!   jitter budget the regression harness checks.
//! * Every call is recorded in [`FakeToolCaller::records`], which
//!   [`summarise`] turns into per-stage median / p95 statistics.
//!
//! Because a fresh caller built from the same seed replays the same latency
//! sequence, running the same scenario N times yields N samples of an
//! *identical* workload. Cross-round jitter then measures the executor and
//! the machine, which is exactly the regression signal we want.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use crate::callers::{CallFuture, ToolCaller};

// ── Latency model ───────────────────────────────────────────────────────

/// Latency distribution for a single tool: a fixed `base` plus a uniform
/// `[0, jitter)` top-up drawn from the seeded PRNG.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LatencyModel {
    /// Latency paid on every call.
    pub base: Duration,
    /// Upper bound of the uniform extra latency added per call.
    pub jitter: Duration,
}

impl LatencyModel {
    /// Fixed latency with no per-call variation.
    #[must_use]
    pub fn fixed(ms: u64) -> Self {
        Self {
            base: Duration::from_millis(ms),
            jitter: Duration::ZERO,
        }
    }

    /// Latency of `base_ms` plus a uniform `[0, jitter_ms)` extra.
    #[must_use]
    pub fn jittered(base_ms: u64, jitter_ms: u64) -> Self {
        Self {
            base: Duration::from_millis(base_ms),
            jitter: Duration::from_millis(jitter_ms),
        }
    }
}

// ── Configuration ───────────────────────────────────────────────────────

/// Configuration for a [`FakeToolCaller`].
///
/// Tools without an explicit entry fall back to [`Self::default_latency`].
#[derive(Debug, Clone)]
pub struct FakeCallerConfig {
    /// PRNG seed. Identical seeds replay identical latency / failure
    /// sequences.
    pub seed: u64,
    /// Per-tool latency model.
    pub latencies: BTreeMap<String, LatencyModel>,
    /// Latency model for tools absent from [`Self::latencies`].
    pub default_latency: LatencyModel,
    /// Probability in `[0, 1]` that a call fails. `0.0` never fails.
    ///
    /// Clamped into range on use, so an out-of-range value degrades to
    /// "always" / "never" instead of producing surprising statistics.
    pub failure_rate: f64,
}

impl FakeCallerConfig {
    /// Config with `seed`, no per-tool latencies and a zero default
    /// latency.
    #[must_use]
    pub fn new(seed: u64) -> Self {
        Self {
            seed,
            latencies: BTreeMap::new(),
            default_latency: LatencyModel::fixed(0),
            failure_rate: 0.0,
        }
    }

    /// Set the latency model for `tool`.
    #[must_use]
    pub fn with_tool(mut self, tool: &str, model: LatencyModel) -> Self {
        self.latencies.insert(tool.to_string(), model);
        self
    }

    /// Set the failure rate and return the config.
    #[must_use]
    pub fn with_failure_rate(mut self, rate: f64) -> Self {
        self.failure_rate = rate;
        self
    }

    /// Latency model for `tool`, falling back to [`Self::default_latency`].
    #[must_use]
    pub fn latency_for(&self, tool: &str) -> LatencyModel {
        self.latencies
            .get(tool)
            .copied()
            .unwrap_or(self.default_latency)
    }

    /// Latency table matching [`super::PIPELINE_SCENARIO`].
    ///
    /// The bases are deliberately in the tens of milliseconds: the
    /// regression harness budgets p95 jitter at 5% of the stage median, and
    /// a stage shorter than ~30 ms would leave that budget under a
    /// millisecond, which is inside normal OS scheduling noise.
    #[must_use]
    pub fn pipeline_full(seed: u64) -> Self {
        Self::new(seed)
            .with_tool("bench_stage_model", LatencyModel::jittered(50, 10))
            .with_tool("bench_stage_rig", LatencyModel::jittered(40, 8))
            .with_tool("bench_stage_animate", LatencyModel::jittered(70, 15))
            .with_tool("bench_stage_texture", LatencyModel::jittered(45, 10))
            .with_tool("bench_stage_render", LatencyModel::jittered(100, 20))
            .with_tool("bench_stage_composite", LatencyModel::jittered(40, 8))
    }
}

// ── Records & statistics ────────────────────────────────────────────────

/// Whether a recorded call succeeded or failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CallOutcome {
    /// The call returned a value.
    Ok,
    /// The call failed because the seeded PRNG drew a failure.
    Failed,
    /// The call was aborted through its cancellation token.
    Cancelled,
}

/// One recorded tool call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallRecord {
    /// Tool name.
    pub tool: String,
    /// Zero-based call index across the whole caller lifetime.
    pub seq: usize,
    /// Latency the seeded model asked for.
    pub intended: Duration,
    /// Wall-clock time actually spent inside the call.
    pub observed: Duration,
    /// How the call ended.
    pub outcome: CallOutcome,
}

/// Per-stage latency statistics derived from a set of [`CallRecord`]s.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StageStats {
    /// Tool (stage) name.
    pub tool: String,
    /// Number of samples.
    pub samples: usize,
    /// Fastest sample.
    pub min: Duration,
    /// Median sample.
    pub median: Duration,
    /// 95th-percentile sample (nearest-rank).
    pub p95: Duration,
    /// Slowest sample.
    pub max: Duration,
    /// `(p95 - median) / median`.
    ///
    /// This is the number the regression harness gates on. It is
    /// [`f64::INFINITY`] when the median is zero, because no relative
    /// statement can be made about a zero-length measurement.
    pub jitter_ratio: f64,
}

/// Nearest-rank percentile of an already-sorted, non-empty slice.
///
/// `p` is clamped into `(0, 1]`. Index selection is `ceil(p * n) - 1`, so
/// `p = 0.95` over 30 samples returns the 29th-smallest value: exactly one
/// outlying sample is tolerated.
fn percentile(sorted: &[Duration], p: f64) -> Duration {
    let n = sorted.len();
    let rank = (p.clamp(0.0, 1.0) * n as f64).ceil() as usize;
    sorted[rank.saturating_sub(1).min(n - 1)]
}

/// Summarise `records` into per-tool statistics, keyed by tool name.
///
/// Cancelled calls are excluded: their duration measures the cancellation
/// path, not the stage.
#[must_use]
pub fn summarise(records: &[CallRecord]) -> BTreeMap<String, StageStats> {
    let mut by_tool: BTreeMap<String, Vec<Duration>> = BTreeMap::new();
    for r in records {
        if r.outcome == CallOutcome::Cancelled {
            continue;
        }
        by_tool.entry(r.tool.clone()).or_default().push(r.observed);
    }

    by_tool
        .into_iter()
        .map(|(tool, mut samples)| {
            samples.sort_unstable();
            let min = samples[0];
            let median = percentile(&samples, 0.5);
            let p95 = percentile(&samples, 0.95);
            let max = samples[samples.len() - 1];
            let jitter_ratio = if median.is_zero() {
                if p95.is_zero() { 0.0 } else { f64::INFINITY }
            } else {
                (p95.as_secs_f64() - median.as_secs_f64()) / median.as_secs_f64()
            };
            (
                tool.clone(),
                StageStats {
                    tool,
                    samples: samples.len(),
                    min,
                    median,
                    p95,
                    max,
                    jitter_ratio,
                },
            )
        })
        .collect()
}

// ── PRNG ────────────────────────────────────────────────────────────────

/// SplitMix64 — small, allocation-free, deterministic PRNG.
///
/// `rand` is not a dependency of this crate, and a benchmark harness must
/// not depend on ambient entropy anyway: the whole point is that a seed
/// replays the exact same run.
#[derive(Debug, Clone)]
struct SplitMix64(u64);

impl SplitMix64 {
    const GOLDEN: u64 = 0x9E37_79B9_7F4A_7C15;

    fn new(seed: u64) -> Self {
        Self(seed)
    }

    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(Self::GOLDEN);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform sample in `[0, 1)`.
    fn next_f64(&mut self) -> f64 {
        // 53 bits of mantissa — enough for every rate we compare against.
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Uniform sample in `[0, n)`. Returns `0` when `n == 0` so an empty
    /// jitter window cannot panic on a modulo by zero.
    fn next_below(&mut self, n: u64) -> u64 {
        if n == 0 { 0 } else { self.next_u64() % n }
    }
}

// ── Timing helper ───────────────────────────────────────────────────────

/// Spin reserve in nanoseconds, shared by every fake caller in the process.
///
/// `0` means "not calibrated yet". The value is a property of the *host*, not
/// of any one caller, so it is measured once and reused.
static SLEEP_RESERVE_NS: AtomicU64 = AtomicU64::new(0);

/// Floor for the reserve — every platform deserves some margin.
const MIN_RESERVE: Duration = Duration::from_millis(2);

/// How much of the distance to the newest observation [`adapt_reserve`] sheds
/// per wait, as a divisor. At 16 the reserve closes half the gap in ~11
/// waits, so it tracks a host that is heating up or cooling down without
/// reacting to any single late wake.
const RESERVE_DECAY_DIVISOR: u64 = 16;

/// Ceiling for the reserve, so pathologically coarse timers cannot turn the
/// harness into a CPU burner.
///
/// The reserve is time we spend **spinning**, so it also caps how long the
/// final wait can burn a core. 40 ms used to be the ceiling, which is longer
/// than the shortest pipeline stage (`rig` models a 40 ms call): at full
/// inflation `head` collapsed to zero and that stage never slept at all.
/// 8 ms absorbs the wake-up latency a host shows when it is merely busy, and
/// stays far below the shortest stage. Past that it stops paying for itself:
/// the dominant error is then the OS rescheduling us after the timer has
/// already fired, which under contention reaches double-digit milliseconds
/// and is unbounded, so a reserve wide enough to cover a badly contended
/// host would spend most of a short stage spinning. [`adapt_reserve`] climbs
/// to this ceiling as conditions worsen; whatever is still left over is host
/// noise and belongs in the test's budget.
const MAX_RESERVE: Duration = Duration::from_millis(8);

/// Measure how late this host's timers can wake us.
///
/// `tokio::time::sleep` resolves on an OS timer tick, and that tick is not
/// the same everywhere: Linux hrtimers resolve within a few hundred
/// microseconds, while Windows defaults to a ~15.6 ms granularity and so
/// can return *sixteen milliseconds* late. A bare sleep is therefore far
/// too coarse to sit inside a 5% jitter budget.
///
/// The probe deliberately uses several durations, because a coarse tick
/// shows up as an overshoot of up to one full tick regardless of the
/// requested delay.
async fn calibrate_reserve() -> Duration {
    let mut worst = Duration::ZERO;
    for millis in [1u64, 7, 23] {
        for _ in 0..3 {
            let target = Duration::from_millis(millis);
            let started = Instant::now();
            tokio::time::sleep(target).await;
            let overshoot = started.elapsed().saturating_sub(target);
            worst = worst.max(overshoot);
        }
    }
    // 25% headroom over the worst probe, plus a millisecond, so a slightly
    // worse-than-observed tick still lands inside the spin window.
    let reserve = (worst + Duration::from_millis(1)).mul_f64(1.25);
    reserve.clamp(MIN_RESERVE, MAX_RESERVE)
}

/// Current spin reserve, calibrating on first use.
async fn sleep_reserve() -> Duration {
    let ns = SLEEP_RESERVE_NS.load(Ordering::Acquire);
    if ns != 0 {
        return Duration::from_nanos(ns);
    }
    let reserve = calibrate_reserve().await;
    SLEEP_RESERVE_NS.store(reserve.as_nanos() as u64, Ordering::Release);
    reserve
}

/// Move the reserve toward the lateness the timer just demonstrated.
///
/// Grow immediately, decay gradually: a late timer means every later wait of
/// a similar length will be late too, but a single bad wake must not inflate
/// the reserve for the rest of the process. Gradual decay is also what keeps
/// this from becoming the ratchet it replaces.
///
/// The argument must be the timer's lateness **on its own** — how late
/// `tokio::time::sleep` returned relative to the span it was given, sampled
/// before [`spin_until`] runs. Feeding in the wait's total overshoot instead
/// is what made the reserve self-amplifying: that total also contains time
/// the scheduler stole during the spin, and reacting to it *widens* the spin
/// window, which is exactly where the stealing happens. On a contended CI
/// runner that feedback walked the reserve to its ceiling inside one test,
/// at which point the shortest pipeline stage slept for no time at all.
fn adapt_reserve(timer_lateness: Duration) {
    let wanted = (timer_lateness + Duration::from_millis(1))
        .mul_f64(1.25)
        .clamp(MIN_RESERVE, MAX_RESERVE)
        .as_nanos() as u64;
    let _ = SLEEP_RESERVE_NS.fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
        if current == 0 || wanted > current {
            Some(wanted)
        } else {
            let decayed = current - ((current - wanted) / RESERVE_DECAY_DIVISOR).max(1);
            Some(decayed.max(MIN_RESERVE.as_nanos() as u64))
        }
    });
}

/// Result of [`sleep_precise`] — when the wait actually began.
///
/// The start instant is returned rather than captured by the caller because
/// calibration (a handful of sleeps) happens *inside* the wait and must not
/// be billed to the delay the caller asked for.
enum WaitStart {
    /// The wait ran to completion; carries the instant it began at.
    Done(Instant),
    /// The wait was aborted before it began.
    Cancelled,
}

/// Sleep for `d`, spending the last [`sleep_reserve`] window waiting on our
/// own clock.
///
/// Sleeping to `deadline - reserve` and then waiting out the remainder keeps
/// the observed delay accurate to microseconds: the timer's (potentially
/// multi-millisecond) latency is absorbed by the reserve, and the final wait
/// cannot overshoot. The only remaining error is a preemption inside that
/// window, which is unlikely precisely because the window is short.
///
/// The reserve follows the timer's own lateness, not the overshoot of the
/// wait as a whole — see [`adapt_reserve`]. Those are different quantities:
/// the latter also carries the time the OS took to schedule us back in once
/// the timer had already fired, which on a contended runner reaches tens of
/// milliseconds and is not something a bigger reserve can buy back. Widening
/// the spin window in response to it makes the next preemption more likely
/// rather than less — a positive-feedback loop that walked the reserve to
/// its ceiling within one test, at which point the shortest pipeline stage
/// never slept at all.
///
/// Returns the instant the wait began, so callers can measure the delay
/// without paying for one-off calibration.
async fn sleep_precise(d: Duration) -> WaitStart {
    let reserve = sleep_reserve().await.min(d);
    let started = Instant::now();
    let deadline = started + d;
    let head = d.saturating_sub(reserve);
    if !head.is_zero() {
        let timer_started = Instant::now();
        tokio::time::sleep(head).await;
        adapt_reserve(timer_started.elapsed().saturating_sub(head));
    }
    spin_until(deadline);
    WaitStart::Done(started)
}

/// Wait until `deadline` on our own clock.
///
/// Deliberately does **not** yield. `yield_now` hands off the remainder of
/// the current quantum and only gets the core back once every other runnable
/// thread there has had a turn, so every call pays for a full scheduling
/// rotation — measured at 4-14 ms per call with 64 contending threads.
/// Spinning keeps the core and loses time only when a preemption actually
/// lands inside the window, which [`MAX_RESERVE`] bounds to a handful of
/// milliseconds. [`std::hint::spin_loop`] is also the right hint for the
/// hardware: `PAUSE` on x86 and `YIELD` on AArch64, which lets a
/// hyperthread sibling or an E-core partner run while we wait.
fn spin_until(deadline: Instant) {
    while Instant::now() < deadline {
        std::hint::spin_loop();
    }
}

// ── Caller ──────────────────────────────────────────────────────────────

/// Host-free [`ToolCaller`] with a seeded latency table and a configurable
/// failure rate.
///
/// # Examples
///
/// ```no_run
/// use dcc_mcp_workflow::bench::{FakeCallerConfig, FakeToolCaller};
///
/// let caller = FakeToolCaller::new(FakeCallerConfig::pipeline_full(0xC0FFEE));
/// ```
#[derive(Debug)]
pub struct FakeToolCaller {
    config: FakeCallerConfig,
    rng: Mutex<SplitMix64>,
    records: Mutex<Vec<CallRecord>>,
}

impl FakeToolCaller {
    /// Build a caller from `config`.
    #[must_use]
    pub fn new(config: FakeCallerConfig) -> Self {
        Self {
            rng: Mutex::new(SplitMix64::new(config.seed)),
            config,
            records: Mutex::new(Vec::new()),
        }
    }

    /// Caller whose latency table matches [`super::PIPELINE_SCENARIO`].
    #[must_use]
    pub fn pipeline_full(seed: u64) -> Self {
        Self::new(FakeCallerConfig::pipeline_full(seed))
    }

    /// Recorded calls, in call order.
    #[must_use]
    pub fn records(&self) -> Vec<CallRecord> {
        self.records.lock().clone()
    }

    /// Per-stage statistics over the recorded calls.
    #[must_use]
    pub fn stats(&self) -> BTreeMap<String, StageStats> {
        summarise(&self.records())
    }

    /// Drop recorded calls and rewind the PRNG back to the configured seed.
    ///
    /// Lets one caller serve several rounds that are all identical
    /// workloads.
    pub fn reset(&self) {
        self.records.lock().clear();
        *self.rng.lock() = SplitMix64::new(self.config.seed);
    }

    /// Warm up the host-timer calibration so the first measured call is as
    /// accurate as the rest.
    ///
    /// Calibration sleeps a handful of times to discover how late this
    /// host's timers can wake a task (see [`calibrate_reserve`]). Call it
    /// once before a measured batch — otherwise the first calls pay the
    /// calibration cost as extra latency and show up as outliers in the p95.
    ///
    /// The reserve is process-wide, so this only has to happen once per
    /// test binary; calling it again is cheap.
    pub async fn calibrate() {
        let _ = sleep_reserve().await;
    }

    /// Draw the next latency / failure pair for `tool`.
    ///
    /// The lock is taken and dropped synchronously — never held across an
    /// await.
    fn draw(&self, tool: &str) -> (Duration, bool) {
        let model = self.config.latency_for(tool);
        let mut rng = self.rng.lock();
        let extra_us = rng.next_below(model.jitter.as_micros() as u64);
        let intended = model.base + Duration::from_micros(extra_us);
        let rate = self.config.failure_rate.clamp(0.0, 1.0);
        let fails = rate > 0.0 && rng.next_f64() < rate;
        (intended, fails)
    }
}

impl ToolCaller for FakeToolCaller {
    fn call<'a>(
        &'a self,
        tool_name: &'a str,
        args: Value,
        cancel: CancellationToken,
    ) -> CallFuture<'a> {
        let (intended, fails) = self.draw(tool_name);
        Box::pin(async move {
            let seq = self.records.lock().len();
            // `sleep_precise` reports its own start instant so that one-off
            // host-timer calibration is not billed to the delay we asked
            // for.
            let (started, cancelled) = match tokio::select! {
                biased;
                _ = cancel.cancelled() => WaitStart::Cancelled,
                start = sleep_precise(intended) => start,
            } {
                WaitStart::Done(start) => (start, false),
                WaitStart::Cancelled => (Instant::now(), true),
            };
            let observed = started.elapsed();

            let outcome = if cancelled {
                CallOutcome::Cancelled
            } else if fails {
                CallOutcome::Failed
            } else {
                CallOutcome::Ok
            };
            self.records.lock().push(CallRecord {
                tool: tool_name.to_string(),
                seq,
                intended,
                observed,
                outcome,
            });

            if cancelled {
                return Err(format!("call to {tool_name:?} was cancelled"));
            }
            if fails {
                return Err(format!(
                    "simulated failure in {tool_name:?} (call #{seq}, rate {})",
                    self.config.failure_rate
                ));
            }
            Ok(json!({
                "stage": tool_name,
                "call": seq,
                "latency_ms": intended.as_secs_f64() * 1_000.0,
                "args": args,
            }))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn same_seed_replays_the_same_latency_sequence() {
        let a = FakeToolCaller::new(
            FakeCallerConfig::new(42)
                .with_tool("t", LatencyModel::jittered(5, 20))
                .with_failure_rate(0.5),
        );
        let b = FakeToolCaller::new(
            FakeCallerConfig::new(42)
                .with_tool("t", LatencyModel::jittered(5, 20))
                .with_failure_rate(0.5),
        );
        for _ in 0..8 {
            assert_eq!(a.draw("t"), b.draw("t"));
        }
    }

    #[test]
    fn different_seeds_diverge() {
        let a = FakeToolCaller::new(
            FakeCallerConfig::new(1).with_tool("t", LatencyModel::jittered(5, 50)),
        );
        let b = FakeToolCaller::new(
            FakeCallerConfig::new(2).with_tool("t", LatencyModel::jittered(5, 50)),
        );
        let same = (0..8).all(|_| a.draw("t") == b.draw("t"));
        assert!(!same, "two seeds produced an identical sequence");
    }

    #[test]
    fn zero_jitter_window_does_not_panic() {
        let caller =
            FakeToolCaller::new(FakeCallerConfig::new(0).with_tool("t", LatencyModel::fixed(3)));
        assert_eq!(caller.draw("t"), (Duration::from_millis(3), false));
    }

    #[tokio::test]
    async fn observed_latency_tracks_intended_latency() {
        // Warm the host-timer calibration up front; the first wait in a
        // process would otherwise fold it into its measured duration.
        FakeToolCaller::calibrate().await;
        let caller = Arc::new(FakeToolCaller::pipeline_full(7));
        let token = CancellationToken::new();
        for tool in [
            "bench_stage_model",
            "bench_stage_rig",
            "bench_stage_animate",
        ] {
            caller
                .call(tool, json!({}), token.clone())
                .await
                .expect("call succeeds with a zero failure rate");
        }
        for record in caller.records() {
            assert_eq!(record.outcome, CallOutcome::Ok);
            assert!(
                record.observed >= record.intended,
                "{} finished early: {:?} < {:?}",
                record.tool,
                record.observed,
                record.intended
            );
            // The lower bound above is the hard contract. The upper bound is
            // deliberately loose: how tightly wall-clock tracks the model is
            // a property of the host's timer and its load, and that is what
            // the 30-round harness in `tests/pipeline_bench.rs` measures.
            //
            // That harness gates the **median** per-stage overhead, not the
            // p95 it also prints. p95 and max are diagnostic output only —
            // at n = 30 the p95 is the 2nd-largest sample, so budgeting it
            // budgeted the worst scheduling hiccup of the run. Do not
            // reintroduce a p95 or max assertion here on the strength of
            // that printout.
            //
            // A unit test may not assume a quiet machine regardless — it
            // runs once, alongside every other test in the binary.
            assert!(
                record.observed < record.intended + Duration::from_millis(100),
                "{} overshot: {:?} vs {:?}",
                record.tool,
                record.observed,
                record.intended
            );
        }
    }

    #[tokio::test]
    async fn failure_rate_is_honoured() {
        // A rate of 1.0 must fail every call.
        let caller = FakeToolCaller::new(
            FakeCallerConfig::new(3)
                .with_tool("t", LatencyModel::fixed(0))
                .with_failure_rate(1.0),
        );
        let err = caller
            .call("t", json!({}), CancellationToken::new())
            .await
            .expect_err("rate 1.0 always fails");
        assert!(err.contains("simulated failure"));
        assert_eq!(caller.records()[0].outcome, CallOutcome::Failed);
    }

    #[tokio::test]
    async fn cancellation_short_circuits_the_delay() {
        FakeToolCaller::calibrate().await;
        let caller =
            FakeToolCaller::new(FakeCallerConfig::new(4).with_tool("t", LatencyModel::fixed(500)));
        let token = CancellationToken::new();
        token.cancel();
        let started = Instant::now();
        let err = caller
            .call("t", json!({}), token)
            .await
            .expect_err("cancel fails the call");
        assert!(err.contains("cancelled"));
        assert!(started.elapsed() < Duration::from_millis(100));
        assert_eq!(caller.records()[0].outcome, CallOutcome::Cancelled);
    }

    #[test]
    fn summarise_reports_jitter_ratio() {
        let records = vec![
            CallRecord {
                tool: "t".into(),
                seq: 0,
                intended: Duration::from_millis(10),
                observed: Duration::from_millis(10),
                outcome: CallOutcome::Ok,
            },
            CallRecord {
                tool: "t".into(),
                seq: 1,
                intended: Duration::from_millis(10),
                observed: Duration::from_millis(20),
                outcome: CallOutcome::Ok,
            },
        ];
        let stats = summarise(&records);
        let t = &stats["t"];
        assert_eq!(t.samples, 2);
        assert_eq!(t.median, Duration::from_millis(10));
        // 20 samples -> p95 rank is ceil(0.95*2)=2 -> the 2nd sample.
        assert_eq!(t.p95, Duration::from_millis(20));
        assert!((t.jitter_ratio - 1.0).abs() < 1e-9);
    }

    #[test]
    fn summarise_excludes_cancelled_calls() {
        let records = vec![CallRecord {
            tool: "t".into(),
            seq: 0,
            intended: Duration::ZERO,
            observed: Duration::from_millis(1),
            outcome: CallOutcome::Cancelled,
        }];
        assert!(summarise(&records).is_empty());
    }

    #[test]
    fn reset_rewinds_the_prng() {
        let caller = FakeToolCaller::new(
            FakeCallerConfig::new(9).with_tool("t", LatencyModel::jittered(1, 30)),
        );
        let first = (0..4).map(|_| caller.draw("t")).collect::<Vec<_>>();
        caller.reset();
        let second = (0..4).map(|_| caller.draw("t")).collect::<Vec<_>>();
        assert_eq!(first, second);
    }
}
