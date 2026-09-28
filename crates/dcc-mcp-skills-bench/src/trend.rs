//! Rolling-baseline trend tracking and latency regression alerts (PIP-3703).
//!
//! # Why latency is still not gated, and what replaces the gate
//!
//! [`crate::thresholds`] refuses to gate latency, because CI hardware alone can
//! move the number by a factor of two. That decision stands. What it left
//! behind is silence: a regression was recorded, and nobody was told.
//!
//! This module closes the gap in the way the original argument demands — by
//! comparing against a *rolling* baseline instead of a fixed constant, so the
//! machine term cancels out:
//!
//! * the baseline is the **median p95 of the last [`BASELINE_WINDOW`] runs in
//!   the same corpus epoch**. Not one run — a single run is exactly the noisy
//!   measurement `thresholds.rs` declined to trust.
//! * an alert needs [`LATENCY_REGRESSION_RATIO`] of headroom, because the
//!   cost of a missed regression is a slower week and the cost of a false one
//!   is everyone learning to ignore the alert. That headroom is a *starting
//!   point*, not a calibrated threshold — see the constant for what it is
//!   measured against.
//! * an alert **never fails the build**. Gating is what `thresholds.rs`
//!   rejects; telling someone is not. The caller decides what an alert means.
//!
//! # Corpus epochs: why this is safe to land before S1
//!
//! A latency baseline is only meaningful across runs that measured the same
//! corpus. Every point therefore carries a [`CorpusFingerprint`], and points
//! whose fingerprint differs are excluded from the window.
//!
//! That is the mechanism that lets this land in parallel with S1 (PIP-3701).
//! S1 expands the real-seed set from 26 to 80+, which changes the fingerprint,
//! which opens a new epoch with an empty window: alerts stay disarmed until
//! [`BASELINE_WINDOW`] runs have accumulated under the new corpus. No number
//! measured against the old corpus can ever become the baseline for a run
//! measured against the new one, so the "first weeks are polluted" failure
//! mode is prevented by construction rather than by remembering to wait.

use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};
use std::io;
use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::context::ContextPoint;
use crate::corpus::{Corpus, SCALE_300};
use crate::run::Evaluation;
use crate::synthetic::CORPUS_SCHEMA_VERSION;
use crate::thresholds;

/// Runs that make up the latency baseline.
///
/// Four weekly runs is a month: long enough that a single slow CI machine is
/// one quarter of the window rather than all of it, short enough that a
/// regression which landed five weeks ago is not still the baseline.
pub const BASELINE_WINDOW: usize = 4;

/// How far p95 may sit above the rolling median before it is reported.
///
/// Set wide on purpose. The first instinct after `thresholds.rs` declined to
/// gate latency is to invent a tighter cap; that reproduces the flakiness the
/// original decision avoided. Starting at +50% means the alert fires on a
/// regression a human would notice by reading the numbers, and stays quiet
/// otherwise.
///
/// **It is a starting point, not a calibrated threshold.** The quantity being
/// compared is one wall-clock sample per query with no warm-up (`run::grade`),
/// and its p95 is a nearest-rank tail statistic — the noisiest summary of the
/// noisiest measurement available. On one machine, running the same code,
/// `dcc_filtered` p95 has been observed between 1417us and 4274us, a spread of
/// roughly 3x. The rolling median removes that spread from the *denominator*;
/// the current run's own draw is still sitting in the *numerator*, so until the
/// spread is measured, +50% cannot be claimed to sit outside it.
///
/// The trend report therefore prints the baseline min–max next to the median.
/// Read those two numbers off the first full window and re-mark this constant:
/// if the window's min–max spread is wider than +50%, alerts are noise.
pub const LATENCY_REGRESSION_RATIO: f64 = 0.50;

/// Points retained in the history file.
///
/// A year of weekly runs, so a regression that took two months to arrive can
/// still be read back against its own baseline instead of against a window
/// that has already forgotten it.
pub const MAX_POINTS: usize = 52;

/// Schema version of the persisted history file.
pub const HISTORY_VERSION: u32 = 1;

/// Latency series carried in the trend report.
///
/// Narrow on purpose. Recording every measured group would multiply the number
/// of series — and the number of chances to alert — by sixteen. These two are
/// the ones a reviewer reads: the gated `dcc`-filtered path, and the
/// full-catalogue scan it is always interpreted next to.
pub const TRACKED_LATENCY_GROUPS: [&str; 2] = ["dcc_filtered/all", "unfiltered/all"];

/// Corpus scale the latency baseline is tracked at.
///
/// The gated scale. The 1000-skill corpus is a trend signal about filler
/// vocabulary, not about the product, so a latency baseline over it would
/// mostly track corpus size.
pub const TRACKED_SCALE: usize = SCALE_300;

/// Duration in whole microseconds, saturating rather than wrapping.
fn micros(duration: Duration) -> u64 {
    u64::try_from(duration.as_micros()).unwrap_or(u64::MAX)
}

/// FNV-1a, 64 bit.
///
/// Chosen over [`std::collections::hash_map::DefaultHasher`] because
/// `DefaultHasher` makes no stability promise across Rust releases, and this
/// digest is persisted: the workflow builds with `dtolnay/rust-toolchain@stable`,
/// so a toolchain bump that changes the default algorithm would move every
/// fingerprint at once, drop the whole history into a brand-new epoch, and buy
/// another four weeks of warm-up — silently, because the failure looks exactly
/// like a corpus change. FNV-1a is specified here in full, so it cannot move.
struct Fnv(u64);

impl Fnv {
    /// FNV-1a offset basis.
    const BASIS: u64 = 0xcbf2_9ce4_8422_2325;
    /// FNV-1a prime.
    const PRIME: u64 = 0x0000_0100_0000_01b3;

    fn new() -> Self {
        Self(Self::BASIS)
    }

    fn write_byte(&mut self, byte: u8) {
        self.0 ^= u64::from(byte);
        self.0 = self.0.wrapping_mul(Self::PRIME);
    }
}

impl Hasher for Fnv {
    fn write(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.write_byte(*byte);
        }
    }

    fn finish(&self) -> u64 {
        self.0
    }
}

/// Order-independent digest of a set of skill names.
///
/// Sorting first, because the corpus order is an implementation detail of
/// [`Corpus::build`] and the digest has to describe *which* skills were
/// measured, not the order they happened to be built in.
///
/// The algorithm is [`Fnv`], spelled out in this module: the digest is written
/// to a file and read back months later, so it has to be defined here rather
/// than inherited from whatever the current toolchain defaults to.
#[must_use]
fn digest_names<'a>(names: impl Iterator<Item = &'a str>) -> u64 {
    let mut sorted: Vec<&str> = names.collect();
    sorted.sort_unstable();
    let mut hasher = Fnv::new();
    for name in sorted {
        name.hash(&mut hasher);
    }
    hasher.finish()
}

/// Identity of the corpus a trend point was measured against.
///
/// Two points are only comparable when their fingerprints match. S1 (PIP-3701)
/// grows the real-seed set, which moves both `seed_count` and `digest`, which
/// opens a new epoch and leaves the previous window behind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CorpusFingerprint {
    /// [`CORPUS_SCHEMA_VERSION`] at the time of the run.
    pub schema: String,
    /// Real seeds in the corpus.
    pub seed_count: usize,
    /// Digest of every skill name in the corpus.
    pub digest: u64,
}

impl CorpusFingerprint {
    /// Fingerprint the corpus a run was measured against.
    #[must_use]
    pub fn of(corpus: &Corpus) -> Self {
        Self {
            schema: CORPUS_SCHEMA_VERSION.to_string(),
            seed_count: corpus.seeds,
            digest: digest_names(corpus.skills.iter().map(|skill| skill.name.as_str())),
        }
    }

    /// Short form for reports, where a bare `u64` says nothing.
    #[must_use]
    pub fn short(&self) -> String {
        format!("{:016x}", self.digest)
    }
}

/// Median of a small sample, averaging the two middle values when even.
#[must_use]
pub fn median(values: &[u64]) -> Option<u64> {
    if values.is_empty() {
        return None;
    }
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    let middle = sorted.len() / 2;
    if sorted.len() % 2 == 1 {
        Some(sorted[middle])
    } else {
        let sum = u128::from(sorted[middle - 1]) + u128::from(sorted[middle]);
        Some(u64::try_from(sum / 2).unwrap_or(u64::MAX))
    }
}

/// One group's latency distribution, as recorded.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LatencySample {
    /// Median latency in microseconds.
    pub p50_us: u64,
    /// 95th-percentile latency in microseconds.
    pub p95_us: u64,
}

impl LatencySample {
    /// Record a measured distribution.
    #[must_use]
    pub fn new(p50: Duration, p95: Duration) -> Self {
        Self {
            p50_us: micros(p50),
            p95_us: micros(p95),
        }
    }
}

/// One group's hit rates, as recorded.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct HitRateSample {
    /// Fraction where the expected skill ranked first.
    pub top1: f64,
    /// Fraction where it ranked in the top 5.
    pub top5: f64,
    /// Mean reciprocal rank, truncated at 10.
    pub mrr10: f64,
}

impl HitRateSample {
    /// Record a measured group.
    #[must_use]
    pub fn new(top1: f64, top5: f64, mrr10: f64) -> Self {
        Self { top1, top5, mrr10 }
    }
}

/// The context dimension, as recorded.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextSample {
    /// Highest page cost anywhere on the growth curve.
    pub peak_tokens: usize,
    /// Largest page-cost increase from adding one host.
    pub peak_marginal_tokens: i64,
}

impl ContextSample {
    /// Record a measured growth curve.
    #[must_use]
    pub fn of(curve: &[ContextPoint]) -> Self {
        Self {
            peak_tokens: crate::context::peak_tokens(curve),
            peak_marginal_tokens: crate::context::peak_marginal_tokens(curve),
        }
    }
}

/// One run, across all three dimensions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrendPoint {
    /// UTC timestamp, RFC 3339.
    pub recorded_at: String,
    /// Commit the run measured, when the environment supplies one.
    pub git_sha: Option<String>,
    /// Corpus the run measured.
    pub corpus: CorpusFingerprint,
    /// Per-group latency.
    pub latency: BTreeMap<String, LatencySample>,
    /// Per-group hit rates.
    pub hit_rate: BTreeMap<String, HitRateSample>,
    /// Context growth.
    pub context: ContextSample,
}

impl TrendPoint {
    /// Record one run.
    ///
    /// `git_sha` is taken from `GITHUB_SHA` when it is set, and is absent
    /// otherwise — a local run has no commit to name.
    #[must_use]
    pub fn record(corpus: &Corpus, evaluations: &[Evaluation], curve: &[ContextPoint]) -> Self {
        let mut latency = BTreeMap::new();
        let mut hit_rate = BTreeMap::new();

        for evaluation in evaluations {
            if evaluation.scale != TRACKED_SCALE {
                continue;
            }
            for group in &evaluation.latency {
                if TRACKED_LATENCY_GROUPS.contains(&group.name.as_str()) {
                    latency.insert(
                        group.name.clone(),
                        LatencySample::new(group.metrics.p50, group.metrics.p95),
                    );
                }
            }
            for group in &evaluation.hit_rate {
                if TRACKED_LATENCY_GROUPS.contains(&group.name.as_str()) {
                    hit_rate.insert(
                        group.name.clone(),
                        HitRateSample::new(
                            group.metrics.top1,
                            group.metrics.top5,
                            group.metrics.mrr10,
                        ),
                    );
                }
            }
        }

        Self {
            recorded_at: utc_now(),
            git_sha: std::env::var("GITHUB_SHA")
                .ok()
                .filter(|sha| !sha.is_empty()),
            corpus: CorpusFingerprint::of(corpus),
            latency,
            hit_rate,
            context: ContextSample::of(curve),
        }
    }
}

/// Current UTC time as RFC 3339, falling back to the epoch.
///
/// The fallback is not defensive padding: a bench that cannot name the time of
/// its own measurement is worse than one that names it approximately.
fn utc_now() -> String {
    use time::format_description::well_known::Rfc3339;

    time::OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
}

/// The persisted trend series.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrendHistory {
    /// [`HISTORY_VERSION`].
    pub version: u32,
    /// Points, oldest first.
    pub points: Vec<TrendPoint>,
}

impl Default for TrendHistory {
    /// An empty history stamped with the current version.
    ///
    /// Not `#[derive(Default)]`: a derived default would write `version: 0`,
    /// and the next [`Self::load`] would reject its own output as a schema it
    /// does not understand — a history that forgets everything on every run and
    /// can never warm its baseline window up.
    fn default() -> Self {
        Self {
            version: HISTORY_VERSION,
            points: Vec::new(),
        }
    }
}

impl TrendHistory {
    /// Read a history file, treating an absent or unreadable one as empty.
    ///
    /// A trend history is derived data. Losing it costs a few weeks of
    /// warm-up, so this never fails the run: it says what happened on stderr
    /// and starts over.
    #[must_use]
    pub fn load(path: &Path) -> Self {
        let Ok(text) = std::fs::read_to_string(path) else {
            return Self::default();
        };
        match serde_json::from_str::<Self>(&text) {
            Ok(history) if history.version == HISTORY_VERSION => history,
            Ok(history) => {
                eprintln!(
                    "trend history at {} is version {}, expected {HISTORY_VERSION}; starting a new one",
                    path.display(),
                    history.version
                );
                Self::default()
            }
            Err(error) => {
                eprintln!(
                    "trend history at {} is unreadable ({error}); starting a new one",
                    path.display()
                );
                Self::default()
            }
        }
    }

    /// Write the history, creating parent directories.
    ///
    /// # Errors
    ///
    /// Propagates any I/O or serialisation failure.
    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)?;
        }
        let mut text = serde_json::to_string_pretty(self).map_err(io::Error::other)?;
        text.push('\n');
        std::fs::write(path, text)
    }

    /// Append `point`, dropping the oldest points past [`MAX_POINTS`].
    pub fn push(&mut self, point: TrendPoint) {
        self.points.push(point);
        let excess = self.points.len().saturating_sub(MAX_POINTS);
        if excess > 0 {
            self.points.drain(..excess);
        }
    }

    /// Points that may serve as a baseline for `current`.
    ///
    /// Same corpus epoch only, newest [`BASELINE_WINDOW`] of them. `current`
    /// is not in the history yet when this is called, so no exclusion is
    /// needed — see [`compare`].
    #[must_use]
    pub fn baseline_window(&self, current: &TrendPoint) -> Vec<&TrendPoint> {
        let matching = self
            .points
            .iter()
            .filter(|point| point.corpus == current.corpus);
        let skip = matching.clone().count().saturating_sub(BASELINE_WINDOW);
        matching.skip(skip).collect()
    }

    /// Runs recorded against `fingerprint`, across the whole retained history.
    #[must_use]
    pub fn epoch_len(&self, fingerprint: &CorpusFingerprint) -> usize {
        self.points
            .iter()
            .filter(|point| &point.corpus == fingerprint)
            .count()
    }

    /// Most recent point recorded against `fingerprint`, if any.
    #[must_use]
    pub fn latest_in_epoch(&self, fingerprint: &CorpusFingerprint) -> Option<&TrendPoint> {
        self.points
            .iter()
            .rfind(|point| &point.corpus == fingerprint)
    }
}

/// Whether a measured group is worth interrupting someone for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LatencyState {
    /// Not enough history in this corpus epoch to compare.
    WarmingUp,
    /// Compared and within [`LATENCY_REGRESSION_RATIO`].
    WithinBudget,
    /// Over the ratio. Reported, never gated.
    Regression,
}

impl LatencyState {
    /// Short label for tables.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::WarmingUp => "warming up",
            Self::WithinBudget => "ok",
            Self::Regression => "REGRESSION",
        }
    }
}

/// One group's latency compared against the rolling baseline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LatencyComparison {
    /// Group name, e.g. `dcc_filtered/all`.
    pub group: String,
    /// p95 measured by this run, in microseconds.
    pub current_p95_us: u64,
    /// Median p95 of the baseline window; `None` while the epoch is empty.
    pub baseline_p95_us: Option<u64>,
    /// Fastest p95 in the baseline window; `None` while it is empty.
    ///
    /// Carried so the report shows the noise band the median summarises. The
    /// alert ratio is only meaningful next to it — see
    /// [`LATENCY_REGRESSION_RATIO`].
    pub baseline_min_us: Option<u64>,
    /// Slowest p95 in the baseline window; `None` while it is empty.
    pub baseline_max_us: Option<u64>,
    /// `current / baseline - 1.0`; `None` without a baseline.
    pub ratio: Option<f64>,
}

impl LatencyComparison {
    /// What this comparison means.
    #[must_use]
    pub fn state(&self) -> LatencyState {
        match self.ratio {
            None => LatencyState::WarmingUp,
            Some(ratio) if ratio > LATENCY_REGRESSION_RATIO => LatencyState::Regression,
            Some(_) => LatencyState::WithinBudget,
        }
    }
}

/// One run compared against its corpus epoch.
#[derive(Debug, Clone)]
pub struct TrendReport {
    /// The point that was measured.
    pub point: TrendPoint,
    /// Runs that formed the baseline.
    pub baseline_points: usize,
    /// Per-group latency comparison.
    pub latency: Vec<LatencyComparison>,
    /// Actionable regressions.
    ///
    /// Empty until the epoch has [`BASELINE_WINDOW`] runs, however bad the
    /// number looks: comparing against a window of one is the single-run
    /// comparison `thresholds.rs` exists to prevent.
    pub alerts: Vec<LatencyComparison>,
}

impl TrendReport {
    /// Whether the baseline is long enough for alerts to mean anything.
    #[must_use]
    pub fn baseline_ready(&self) -> bool {
        self.baseline_points >= BASELINE_WINDOW
    }
}

/// Compare `current` against the matching runs in `history`.
#[must_use]
pub fn compare(history: &TrendHistory, current: &TrendPoint) -> TrendReport {
    let window = history.baseline_window(current);
    let baseline_points = window.len();

    let mut latency = Vec::new();
    for (group, sample) in &current.latency {
        let baseline: Vec<u64> = window
            .iter()
            .filter_map(|point| point.latency.get(group))
            .map(|sample| sample.p95_us)
            .collect();
        let median_p95 = median(&baseline);
        let ratio = median_p95
            .filter(|base| *base > 0)
            .map(|base| sample.p95_us as f64 / base as f64 - 1.0);
        latency.push(LatencyComparison {
            group: group.clone(),
            current_p95_us: sample.p95_us,
            baseline_p95_us: median_p95,
            baseline_min_us: baseline.iter().copied().min(),
            baseline_max_us: baseline.iter().copied().max(),
            ratio,
        });
    }

    // Alerts need a full window, not merely a non-empty one.
    let alerts = if baseline_points >= BASELINE_WINDOW {
        latency
            .iter()
            .filter(|comparison| comparison.state() == LatencyState::Regression)
            .cloned()
            .collect()
    } else {
        Vec::new()
    };

    TrendReport {
        point: current.clone(),
        baseline_points,
        latency,
        alerts,
    }
}

/// Microseconds with a thousands separator, for tables.
///
/// Hand-rolled rather than locale-aware: these numbers are read next to each
/// other in a table, and `1,417us` next to `4,274us` is scannable in a way that
/// `1417us` next to `4274us` is not.
fn us(value: u64) -> String {
    let digits = value.to_string();
    let mut reversed = String::with_capacity(digits.len() + digits.len() / 3 + 2);
    let mut since_separator = 0;
    for digit in digits.chars().rev() {
        if since_separator == 3 {
            reversed.push(',');
            since_separator = 0;
        }
        reversed.push(digit);
        since_separator += 1;
    }
    let mut out: String = reversed.chars().rev().collect();
    out.push_str("us");
    out
}

/// Short commit label for tables.
///
/// `chars()`, not bytes: `GITHUB_SHA` is hex so the distinction is invisible
/// in CI, but truncating a `String` at a byte index panics on any value whose
/// seventh byte lands inside a character.
fn short_sha(sha: &str) -> String {
    sha.chars().take(7).collect()
}

/// Percent with one decimal, right-aligned in `width`.
fn pct(value: f64) -> String {
    format!("{:6.1}%", value * 100.0)
}

/// The baseline window's min–max band, or an em dash for an empty window.
fn spread(comparison: &LatencyComparison) -> String {
    match (comparison.baseline_min_us, comparison.baseline_max_us) {
        (Some(min), Some(max)) => format!("{} – {}", us(min), us(max)),
        _ => "—".to_string(),
    }
}

/// Signed percent change, or an em dash when there is nothing to compare to.
fn delta(ratio: Option<f64>) -> String {
    match ratio {
        Some(ratio) => format!("{:+.1}%", ratio * 100.0),
        None => "—".to_string(),
    }
}

/// Render the three-dimension trend as one readable Markdown document.
///
/// This is the "one place to do a periodic review" the trend series was
/// missing: previously the three dimensions lived in three different workflow
/// artifacts and the only way to compare them was to open each one.
#[must_use]
pub fn render_trend_markdown(
    history: &TrendHistory,
    current: &TrendPoint,
    report: &TrendReport,
) -> String {
    let mut out = String::new();
    out.push_str("# Skills benchmark — three-dimension trend\n\n");

    out.push_str(&format!(
        "- corpus `{}` — {} real seeds, digest `{}`\n",
        current.corpus.schema,
        current.corpus.seed_count,
        current.corpus.short()
    ));
    out.push_str(&format!(
        "- recorded {} at {}\n",
        current.recorded_at,
        current.git_sha.as_deref().unwrap_or("unknown commit")
    ));
    out.push_str(&format!(
        "- latency baseline: {}/{} runs in this corpus epoch — alerts {}\n",
        report.baseline_points,
        BASELINE_WINDOW,
        if report.baseline_ready() {
            "armed"
        } else {
            "warming up"
        }
    ));

    out.push_str("\n## Latency (p95 against the rolling median)\n\n");
    out.push_str(&format!(
        "Alerts at {:+.0}%. **Not a merge gate** — see `thresholds.rs` for why.\n\n",
        LATENCY_REGRESSION_RATIO * 100.0
    ));
    out.push_str("| group | current p95 | baseline p95 | baseline min–max | delta | state |\n");
    out.push_str("|---|---|---|---|---|---|\n");
    for comparison in &report.latency {
        out.push_str(&format!(
            "| `{}` | {} | {} | {} | {} | {} |\n",
            comparison.group,
            us(comparison.current_p95_us),
            comparison
                .baseline_p95_us
                .map_or_else(|| "—".to_string(), us),
            spread(comparison),
            delta(comparison.ratio),
            comparison.state().label()
        ));
    }
    if report.latency.is_empty() {
        out.push_str("| _no tracked group was measured_ | | | | | |\n");
    }
    out.push_str("\nThe min–max band is the whole point of the column: an alert ratio is only\n");
    out.push_str("readable next to the spread it has to exceed.\n");

    out.push_str("\n## Hit rate (gated)\n\n");
    out.push_str(&format!(
        "Gate: top-1 >= {:.1}%, top-5 >= {:.1}%, MRR@10 >= {:.1}%.\n\n",
        thresholds::MIN_TOP1_300 * 100.0,
        thresholds::MIN_TOP5_300 * 100.0,
        thresholds::MIN_MRR10_300 * 100.0
    ));
    out.push_str("| group | top-1 | top-5 | MRR@10 | previous top-1 | verdict |\n");
    out.push_str("|---|---|---|---|---|---|\n");
    let previous = history.latest_in_epoch(&current.corpus);
    for (group, sample) in &current.hit_rate {
        let before = previous.and_then(|point| point.hit_rate.get(group));
        // Only the gated group gets a verdict. The unfiltered group is carried
        // because the gap between the two is itself a finding, but printing
        // PASS/FAIL for it would invent a gate that does not exist — the same
        // mistake `report::render_text` avoids for the 1000-skill scale.
        let verdict = if group == TRACKED_LATENCY_GROUPS[0] {
            if crate::report::gate_passes_at(sample.top1, sample.top5, sample.mrr10) {
                "PASS"
            } else {
                "FAIL"
            }
        } else {
            "not gated"
        };
        out.push_str(&format!(
            "| `{}` | {} | {} | {} | {} | {} |\n",
            group,
            pct(sample.top1),
            pct(sample.top5),
            pct(sample.mrr10),
            before.map_or_else(|| "—".to_string(), |before| pct(before.top1)),
            verdict
        ));
    }
    if current.hit_rate.is_empty() {
        out.push_str("| _no tracked group was measured_ | | | | | |\n");
    }

    out.push_str("\n## Context growth (capped, gated)\n\n");
    let context_ok = current.context.peak_tokens <= thresholds::MAX_CONTEXT_TOKENS
        && current.context.peak_marginal_tokens <= thresholds::MAX_MARGINAL_CONTEXT_TOKENS;
    out.push_str(&format!(
        "| peak page tokens | cap | peak marginal tokens/host | cap | verdict |\n|---|---|---|---|---|\n| {} | {} | {} | {} | {} |\n",
        current.context.peak_tokens,
        thresholds::MAX_CONTEXT_TOKENS,
        current.context.peak_marginal_tokens,
        thresholds::MAX_MARGINAL_CONTEXT_TOKENS,
        if context_ok { "PASS" } else { "FAIL" }
    ));

    out.push_str("\n## Alerts\n\n");
    if report.alerts.is_empty() {
        out.push_str(if report.baseline_ready() {
            "None — every tracked group is within budget.\n"
        } else {
            "None — the latency baseline is still warming up, so no comparison is \
             being acted on. This is expected for the first runs after the corpus \
             changes (S1, PIP-3701) and is not a defect.\n"
        });
    } else {
        out.push_str(
            "| group | current p95 | baseline p95 | baseline min–max | delta | action |\n",
        );
        out.push_str("|---|---|---|---|---|---|\n");
        for alert in &report.alerts {
            out.push_str(&format!(
                "| `{}` | {} | {} | {} | {} | open an issue for latency triage |\n",
                alert.group,
                us(alert.current_p95_us),
                alert.baseline_p95_us.map_or_else(|| "—".to_string(), us),
                spread(alert),
                delta(alert.ratio)
            ));
        }
    }

    out.push_str("\n## Recent runs\n\n");
    out.push_str("| recorded | commit | dcc_filtered p95 | top-1 | peak tokens |\n");
    out.push_str("|---|---|---|---|---|\n");
    let mut recent: Vec<&TrendPoint> = history.points.iter().rev().take(8).collect();
    recent.insert(0, current);
    for point in recent {
        let p95 = point
            .latency
            .get(TRACKED_LATENCY_GROUPS[0])
            .map_or_else(|| "—".to_string(), |sample| us(sample.p95_us));
        let top1 = point
            .hit_rate
            .get(TRACKED_LATENCY_GROUPS[0])
            .map_or_else(|| "—".to_string(), |sample| pct(sample.top1));
        out.push_str(&format!(
            "| {} | `{}` | {} | {} | {} |\n",
            point.recorded_at,
            point
                .git_sha
                .as_deref()
                .map_or_else(|| "unknown".to_string(), short_sha),
            p95,
            top1,
            point.context.peak_tokens
        ));
    }

    out
}

/// Render the actionable alerts as JSON, for CI to branch on.
#[must_use]
pub fn render_alerts_json(report: &TrendReport) -> serde_json::Value {
    serde_json::json!({
        "baseline_ready": report.baseline_ready(),
        "baseline_points": report.baseline_points,
        "baseline_window": BASELINE_WINDOW,
        "regression_ratio": LATENCY_REGRESSION_RATIO,
        "corpus": report.point.corpus,
        "alerts": report.alerts,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run::Filter;
    use std::time::Duration;

    fn fingerprint(seed_count: usize) -> CorpusFingerprint {
        CorpusFingerprint {
            schema: CORPUS_SCHEMA_VERSION.to_string(),
            seed_count,
            digest: seed_count as u64,
        }
    }

    fn point(at: &str, fingerprint: CorpusFingerprint, p95: u64) -> TrendPoint {
        let mut latency = BTreeMap::new();
        latency.insert(
            TRACKED_LATENCY_GROUPS[0].to_string(),
            LatencySample {
                p50_us: p95 / 2,
                p95_us: p95,
            },
        );
        TrendPoint {
            recorded_at: at.to_string(),
            git_sha: None,
            corpus: fingerprint,
            latency,
            hit_rate: BTreeMap::new(),
            context: ContextSample::default(),
        }
    }

    fn history(points: Vec<TrendPoint>) -> TrendHistory {
        TrendHistory {
            version: HISTORY_VERSION,
            points,
        }
    }

    #[test]
    fn median_handles_odd_even_and_empty() {
        assert_eq!(median(&[]), None);
        assert_eq!(median(&[5]), Some(5));
        assert_eq!(median(&[1, 2, 3]), Some(2));
        assert_eq!(median(&[1, 2, 3, 4]), Some(2));
        assert_eq!(median(&[4, 1, 3, 2]), Some(2));
    }

    #[test]
    fn digest_is_order_independent() {
        let forward = digest_names(["b", "a", "c"].into_iter());
        let backward = digest_names(["c", "b", "a"].into_iter());
        let different = digest_names(["b", "a", "d"].into_iter());
        assert_eq!(forward, backward);
        assert_ne!(forward, different);
    }

    #[test]
    fn digest_is_pinned_to_a_specified_algorithm() {
        // `DefaultHasher` would have made this digest a function of the
        // toolchain. The value below is FNV-1a over b"a" ++ 0xff (Rust's
        // `str` hashing terminator), computed independently of this crate, so
        // a change of algorithm fails here instead of silently opening a new
        // corpus epoch on the next stable release.
        assert_eq!(digest_names(["a"].into_iter()), 0x089b_c907_b544_c769);
        assert_eq!(
            digest_names(["maya-skill", "blender-skill"].into_iter()),
            0xf601_5103_e9dc_7cb9
        );
    }

    #[test]
    fn latency_is_formatted_with_a_thousands_separator() {
        assert_eq!(us(0), "0us");
        assert_eq!(us(999), "999us");
        assert_eq!(us(1_417), "1,417us");
        assert_eq!(us(4_274_000), "4,274,000us");
    }

    #[test]
    fn a_short_sha_survives_a_multibyte_value() {
        // Truncating at a byte index panics here; `GITHUB_SHA` is hex, but the
        // value comes from the environment and is not guaranteed to be.
        assert_eq!(short_sha("abc"), "abc");
        assert_eq!(short_sha("abcdef123456"), "abcdef1");
        assert_eq!(short_sha("中文字符abcdef"), "中文字符abc");
    }

    #[test]
    fn alerts_stay_disarmed_while_the_epoch_warms_up() {
        // The whole point of the epoch: a brand-new corpus has nothing to
        // compare against, and must not invent a comparison.
        let current = point("2026-02-02", fingerprint(26), 10_000);
        let report = compare(
            &history(vec![
                point("2026-01-26", fingerprint(26), 100),
                point("2026-01-19", fingerprint(26), 100),
                point("2026-01-12", fingerprint(26), 100),
            ]),
            &current,
        );
        assert!(!report.baseline_ready());
        assert!(report.alerts.is_empty());
    }

    #[test]
    fn a_full_window_alerts_on_a_real_regression() {
        let current = point("2026-02-02", fingerprint(26), 200);
        let report = compare(
            &history(vec![
                point("2026-01-26", fingerprint(26), 100),
                point("2026-01-19", fingerprint(26), 100),
                point("2026-01-12", fingerprint(26), 100),
                point("2026-01-05", fingerprint(26), 100),
            ]),
            &current,
        );
        assert!(report.baseline_ready());
        assert_eq!(report.alerts.len(), 1);
        assert_eq!(report.latency[0].state(), LatencyState::Regression);
    }

    #[test]
    fn a_full_window_stays_quiet_within_the_ratio() {
        // +40% is a real slowdown and still under the +50% bar. Missing it for
        // a week costs less than crying wolf.
        let current = point("2026-02-02", fingerprint(26), 140);
        let report = compare(
            &history(vec![
                point("2026-01-26", fingerprint(26), 100),
                point("2026-01-19", fingerprint(26), 100),
                point("2026-01-12", fingerprint(26), 100),
                point("2026-01-05", fingerprint(26), 100),
            ]),
            &current,
        );
        assert!(report.baseline_ready());
        assert!(report.alerts.is_empty());
        assert_eq!(report.latency[0].state(), LatencyState::WithinBudget);
    }

    #[test]
    fn the_report_shows_the_baseline_spread_the_ratio_is_read_against() {
        // The alert ratio is only interpretable next to the noise band: +50%
        // inside a 4x band is noise, +50% outside a 5% band is a regression.
        // This is the data requested to calibrate LATENCY_REGRESSION_RATIO.
        let current = point("2026-02-02", fingerprint(26), 110);
        let report = compare(
            &history(vec![
                point("2026-01-26", fingerprint(26), 400),
                point("2026-01-19", fingerprint(26), 100),
                point("2026-01-12", fingerprint(26), 100),
                point("2026-01-05", fingerprint(26), 100),
            ]),
            &current,
        );
        let comparison = &report.latency[0];
        assert_eq!(comparison.baseline_p95_us, Some(100));
        assert_eq!(comparison.baseline_min_us, Some(100));
        assert_eq!(comparison.baseline_max_us, Some(400));

        let text = render_trend_markdown(&TrendHistory::default(), &current, &report);
        assert!(text.contains("100us – 400us"), "{text}");
    }

    #[test]
    fn an_empty_window_reports_no_spread() {
        let current = point("2026-02-02", fingerprint(26), 110);
        let report = compare(&TrendHistory::default(), &current);
        assert_eq!(report.latency[0].baseline_min_us, None);
        assert_eq!(report.latency[0].baseline_max_us, None);
        assert_eq!(spread(&report.latency[0]), "—");
    }

    #[test]
    fn one_slow_machine_does_not_move_the_median_enough_to_alert() {
        // The failure mode thresholds.rs was written to avoid: a window with
        // one pathological run in it. The median ignores the outlier.
        let current = point("2026-02-02", fingerprint(26), 110);
        let report = compare(
            &history(vec![
                point("2026-01-26", fingerprint(26), 400),
                point("2026-01-19", fingerprint(26), 100),
                point("2026-01-12", fingerprint(26), 100),
                point("2026-01-05", fingerprint(26), 100),
            ]),
            &current,
        );
        assert!(report.alerts.is_empty());
        assert_eq!(report.latency[0].baseline_p95_us, Some(100));
    }

    #[test]
    fn a_corpus_change_starts_a_new_epoch() {
        // S1 (PIP-3701) grows the seed set. Pre-S1 runs must never become the
        // baseline for a post-S1 measurement.
        let current = point("2026-02-02", fingerprint(80), 300);
        let history = history(vec![
            point("2026-01-26", fingerprint(26), 100),
            point("2026-01-19", fingerprint(26), 100),
            point("2026-01-12", fingerprint(26), 100),
            point("2026-01-05", fingerprint(26), 100),
        ]);
        assert_eq!(history.epoch_len(&fingerprint(80)), 0);
        let report = compare(&history, &current);
        assert_eq!(report.baseline_points, 0);
        assert!(report.alerts.is_empty());
    }

    #[test]
    fn the_window_only_looks_at_the_last_base_line_window_runs() {
        let current = point("2026-03-02", fingerprint(26), 110);
        let report = compare(
            &history(vec![
                point("2026-01-05", fingerprint(26), 10),
                point("2026-01-12", fingerprint(26), 10),
                point("2026-02-23", fingerprint(26), 100),
                point("2026-02-16", fingerprint(26), 100),
                point("2026-02-09", fingerprint(26), 100),
                point("2026-02-02", fingerprint(26), 100),
            ]),
            &current,
        );
        assert_eq!(report.baseline_points, BASELINE_WINDOW);
        assert_eq!(report.latency[0].baseline_p95_us, Some(100));
    }

    #[test]
    fn history_trims_to_the_retention_cap() {
        let mut history = TrendHistory::default();
        for index in 0..(MAX_POINTS + 10) {
            history.push(point(
                &format!("run-{index}"),
                fingerprint(26),
                100 + index as u64,
            ));
        }
        assert_eq!(history.points.len(), MAX_POINTS);
        // The oldest survive the trim, the newest are kept.
        assert_eq!(history.points[0].recorded_at, "run-10");
        assert_eq!(
            history.points[MAX_POINTS - 1].recorded_at,
            format!("run-{}", MAX_POINTS + 9)
        );
    }

    #[test]
    fn history_round_trips_through_json() {
        let mut history = TrendHistory::default();
        history.push(point("2026-01-05", fingerprint(26), 100));
        let text = serde_json::to_string(&history).expect("serialisable");
        let back: TrendHistory = serde_json::from_str(&text).expect("deserialisable");
        assert_eq!(back.points.len(), 1);
        assert_eq!(back.points[0].corpus, history.points[0].corpus);
    }

    #[test]
    fn a_newly_created_history_survives_a_reload() {
        // Regression: a derived `Default` stamped `version: 0`, so the history
        // rejected its own output on the next run and silently restarted every
        // time — the baseline window could never fill and no alert would ever
        // have fired in CI.
        let dir = std::env::temp_dir().join("dcc-mcp-skills-bench-trend-roundtrip");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("history.json");
        let _ = std::fs::remove_file(&path);

        let mut first = TrendHistory::default();
        first.push(point("run-1", fingerprint(26), 100));
        first.save(&path).expect("writable");
        let reloaded = TrendHistory::load(&path);
        assert_eq!(
            reloaded.points.len(),
            1,
            "the history was discarded on reload"
        );

        let mut second = reloaded;
        second.push(point("run-2", fingerprint(26), 110));
        second.save(&path).expect("writable");
        assert_eq!(TrendHistory::load(&path).points.len(), 2);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn an_unreadable_history_is_treated_as_empty() {
        let path = std::env::temp_dir().join("dcc-mcp-skills-bench-trend-absent.json");
        let _ = std::fs::remove_file(&path);
        assert!(TrendHistory::load(&path).points.is_empty());
    }

    #[test]
    fn tracked_group_names_match_the_filter_labels() {
        // The constants are literals so they can be `const`; this is what
        // keeps them honest against the names `run.rs` actually emits.
        assert_eq!(
            TRACKED_LATENCY_GROUPS[0],
            format!("{}/all", Filter::Dcc.label())
        );
        assert_eq!(
            TRACKED_LATENCY_GROUPS[1],
            format!("{}/all", Filter::Unfiltered.label())
        );
        assert_eq!(TRACKED_SCALE, SCALE_300);
    }

    #[test]
    fn the_trend_markdown_names_all_three_dimensions() {
        let current = point("2026-02-02", fingerprint(26), 100);
        let mut with_hit_rate = current.clone();
        with_hit_rate
            .hit_rate
            .insert(TRACKED_LATENCY_GROUPS[0].to_string(), passing_hit_rate());
        let report = compare(
            &history(vec![point("2026-01-26", fingerprint(26), 100)]),
            &with_hit_rate,
        );
        let text = render_trend_markdown(
            &history(vec![point("2026-01-26", fingerprint(26), 100)]),
            &with_hit_rate,
            &report,
        );
        assert!(text.contains("## Latency"), "{text}");
        assert!(text.contains("## Hit rate"), "{text}");
        assert!(text.contains("## Context growth"), "{text}");
        assert!(text.contains("warming up"), "{text}");
    }

    #[test]
    fn an_alert_is_rendered_as_actionable() {
        let current = point("2026-02-02", fingerprint(26), 300);
        let history = history(vec![
            point("2026-01-26", fingerprint(26), 100),
            point("2026-01-19", fingerprint(26), 100),
            point("2026-01-12", fingerprint(26), 100),
            point("2026-01-05", fingerprint(26), 100),
        ]);
        let report = compare(&history, &current);
        let text = render_trend_markdown(&history, &current, &report);
        assert!(text.contains("REGRESSION"), "{text}");
        assert!(text.contains("open an issue for latency triage"), "{text}");
    }

    /// Hit rates that clear the gate, derived from the thresholds themselves.
    ///
    /// The gate moves when the corpus does — S1 (PIP-3701) raised top-1 from
    /// 74% to 87.8% — so a literal here would fail the next time it moves for
    /// a reason that has nothing to do with what this test is about.
    fn passing_hit_rate() -> HitRateSample {
        HitRateSample::new(
            (thresholds::MIN_TOP1_300 + 1.0) / 2.0,
            (thresholds::MIN_TOP5_300 + 1.0) / 2.0,
            (thresholds::MIN_MRR10_300 + 1.0) / 2.0,
        )
    }

    #[test]
    fn only_the_gated_group_gets_a_hit_rate_verdict() {
        let mut current = point("2026-02-02", fingerprint(26), 100);
        current
            .hit_rate
            .insert(TRACKED_LATENCY_GROUPS[0].to_string(), passing_hit_rate());
        current.hit_rate.insert(
            TRACKED_LATENCY_GROUPS[1].to_string(),
            HitRateSample::new(0.30, 0.40, 0.35),
        );
        let report = compare(&TrendHistory::default(), &current);
        let text = render_trend_markdown(&TrendHistory::default(), &current, &report);
        // A 30% top-1 on the unfiltered group is a finding, not a gate
        // failure: the gate is defined on the `dcc`-filtered group alone, and a
        // report that says FAIL here would invent a gate that does not exist.
        assert!(!text.contains("FAIL"), "{text}");
        assert!(text.contains("not gated"), "{text}");
        assert!(text.contains("PASS"), "{text}");
    }

    #[test]
    fn alerts_json_reports_the_baseline_state() {
        let current = point("2026-02-02", fingerprint(26), 100);
        let report = compare(&TrendHistory::default(), &current);
        let value = render_alerts_json(&report);
        assert_eq!(value["baseline_ready"], serde_json::Value::Bool(false));
        assert_eq!(value["alerts"], serde_json::json!([]));
    }

    #[test]
    fn duration_conversion_does_not_wrap() {
        assert_eq!(
            LatencySample::new(Duration::from_micros(7), Duration::from_micros(9)).p95_us,
            9
        );
    }
}
