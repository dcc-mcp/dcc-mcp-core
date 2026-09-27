//! Regression thresholds (PIP-3408).
//!
//! Two of the three dimensions gate a merge; the third deliberately does not.
//!
//! * **Hit rate** gates. A ranking regression is a defect, and the measurement
//!   is deterministic — the corpus is seeded and the scorer is pure — so the
//!   number is reproducible on any machine.
//! * **Context growth** gates. It is deterministic too, and it is paid out of
//!   the user's context window on every discovery turn.
//! * **Latency does not gate.** It is the one dimension CI hardware can move
//!   by a factor of two on its own, so a cap here produces false failures
//!   instead of catching regressions. It is recorded as a trend signal.
//!
//! # Where these numbers come from
//!
//! They are the measured baseline, minus two and a half points of margin:
//!
//! | metric | measured at 300, `dcc`-filtered | gate |
//! |---|---|---|
//! | top-1  | 90.3% ([`BASELINE_TOP1_300`]) | 87.8% |
//! | top-5  | 96.8% ([`BASELINE_TOP5_300`]) | 94.3% |
//! | MRR@10 | 93.2% ([`BASELINE_MRR10_300`]) | 90.7% |
//!
//! # The baseline moved because the corpus moved (PIP-3701)
//!
//! The 76.5 / 82.8 / 79.5 baseline below this section's predecessor was a
//! property of a corpus that was 26 real skills and 274 rows of filler drawn
//! from a 30-word pool. Three things changed, all of them corpus work, none
//! of them a change to the scorer:
//!
//! 1. **The real seed pool grew from 26 to 197.** [`crate::adapters`] harvests
//!    the pinned skill catalogues of seven DCC adapter repositories. At
//!    `SCALE_300` the catalogue is now mostly real, so the number describes
//!    retrieval over a real catalogue rather than over filler.
//! 2. **Filler got a fingerprint.** Every synthetic skill owns three
//!    `{noun}{Form}` compounds derived from its index (see
//!    [`crate::synthetic::fingerprints`]). Descriptive query classes used to
//!    exist for the 26 real seeds only; at `SCALE_1000` they now cover 300 of
//!    300 targets instead of 167.
//! 3. **Three contradictory ground-truth cases were removed.** These are the
//!    ones worth reading twice, because they are the only changes that touch
//!    what is graded rather than what is in the catalogue:
//!
//!    * A two-segment name used to get a *longer* twin (`cancellable-loop` →
//!      `cancellable-loop-lite`), which outranked its own target on the plain
//!      name query. [`crate::corpus::near_name`] now always trims, so the
//!      target keeps every token of the query plus one more.
//!    * The truncated-tail literal variant is suppressed when the truncated
//!      form is *any* catalogue skill, not only this target's injected twin
//!      ([`crate::corpus::Corpus::truncated_tail_is_unanswerable`]). With a
//!      real catalogue there are dozens of two-segment names per host and one
//!      `blender` twin, so without this the corpus emitted the same
//!      unanswerable `"blender"` query for every Blender skill.
//!    * Intent queries open with a fixed `"i need to "`, whose `to` collided
//!      with the name token of any `*-import-to-scene` skill.
//!
//!    Each of those removed a query whose correct answer the corpus itself
//!    had made ambiguous. None of them removed a query class, and none of
//!    them changed how a query is scored.
//!
//! # Why the gate is not 85 / 90 / 87
//!
//! PIP-3701 asked for `top-1 >= 85%`, `top-5 >= 90%`, `MRR@10 >= 87%`. The
//! measured baseline clears all three, so those numbers are reachable — but
//! they are not what is committed, because a gate is defined by its margin
//! and this file's convention is two and a half points. Sitting the gate on
//! the requested round numbers would leave five to seven points of slack, and
//! `gates_leave_a_small_positive_margin` exists precisely to reject that:
//! slack that wide hides a real ranking regression. Baseline minus 2.5 lands
//! at 87.8 / 94.3 / 90.7, which is *stricter* than the requested gate and
//! still clears it.
//!
//! # The margin is not slack for a sloppy ranker
//!
//! It is room for the corpus to move. The real seeds are harvested from seven
//! pinned repositories plus this one, so every skill someone ships, and every
//! deliberate re-harvest, changes the measurement slightly. Two and a half
//! points absorbs that without absorbing a real regression.
//!
//! # There is exactly one gate
//!
//! [`MIN_TOP1_300`] / [`MIN_TOP5_300`] / [`MIN_MRR10_300`] are the merge gate,
//! and nothing else is. The [`BASELINE_TOP1_300`] trio below is a *record* of
//! what was measured, not a second threshold — no test asserts the measurement
//! against them.
//!
//! That distinction matters. An earlier revision added a canary asserting
//! `measured >= baseline - 0.01`, which silently became the real gate at
//! 77.1 / 81.5 / 79.3: an order of magnitude tighter than the declared gate,
//! and the first thing to fail on exactly the seed churn the margin exists to
//! absorb. Two thresholds for one metric means the tighter one wins, so there
//! is now one.
//!
//! # Known limitation: the hard/clean split is weak under the `dcc` filter
//!
//! `dcc_filtered/hard_negative` now measures *above* `dcc_filtered/clean`
//! (95.0% vs 86.8%). That is a consequence of point 3 above: a target with an
//! injected prefix twin loses its ambiguous truncated-tail query, while a
//! clean target keeps one whenever the real catalogue happens to contain a
//! natural near-neighbour (`blender-geometry` vs `blender-geometry-nodes`).
//! The `hard_negative` / `clean` comparison therefore no longer isolates the
//! injected twins under the `dcc` filter. It still holds unfiltered, where
//! cross-DCC twins compete (84.1% vs 84.3%), which is the split
//! `hard_negatives_are_harder_than_the_clean_control` asserts on. Reworking
//! the split so it means the same thing in both is follow-up work.

/// Minimum top-1 hit rate at [`crate::corpus::SCALE_300`], `dcc`-filtered.
pub const MIN_TOP1_300: f64 = 0.878;
/// Minimum top-5 hit rate at [`crate::corpus::SCALE_300`], `dcc`-filtered.
pub const MIN_TOP5_300: f64 = 0.943;
/// Minimum MRR@10 at [`crate::corpus::SCALE_300`], `dcc`-filtered.
pub const MIN_MRR10_300: f64 = 0.907;

/// Minimum top-1 hit rate at [`crate::corpus::SCALE_1000`], `dcc`-filtered.
///
/// Reported as a trend and asserted loosely. The 1000-skill corpus is not
/// gated: at that scale the number mostly tracks how much filler vocabulary
/// collides, so gating it would make CI sensitive to corpus size rather than
/// to ranker regressions.
pub const MIN_TOP1_1000_INFORMATIVE: f64 = 0.60;

/// Hard cap on a default discovery page, in estimated tokens, at any host count.
///
/// Aligned with `list_context::MAX_DEFAULT_PAGE_TOKENS`, which the `list_skills`
/// harness already asserts on. Reusing that constant rather than inventing a
/// second budget keeps one context number for the whole product.
pub const MAX_CONTEXT_TOKENS: usize =
    dcc_mcp_skills::catalog::list_context::MAX_DEFAULT_PAGE_TOKENS;

/// Largest acceptable increase in page tokens from adding one host.
///
/// Paging means the page cost is bounded by the page size, not by the
/// catalogue, so the marginal cost of a host is noise around zero. The bound
/// is set wide enough to absorb JSON framing differences as rows change, and
/// tight enough that a return to unbounded fan-out (measured at 21k tokens for
/// seven hosts) fails by two orders of magnitude.
pub const MAX_MARGINAL_CONTEXT_TOKENS: i64 = 200;

/// Measured baseline at 300 skills, `dcc`-filtered — a record, not a gate.
///
/// Nothing asserts the measurement against these. They exist so the gate has
/// a documented origin and so `gates_sit_below_the_baseline` can catch a gate
/// that has drifted above what the code actually achieves.
pub const BASELINE_TOP1_300: f64 = 0.903;
/// See [`BASELINE_TOP1_300`].
pub const BASELINE_TOP5_300: f64 = 0.968;
/// See [`BASELINE_TOP1_300`].
pub const BASELINE_MRR10_300: f64 = 0.932;

#[cfg(test)]
mod tests {
    use super::*;

    /// Read the gates through a runtime array so the lints see real values
    /// rather than folded constants.
    fn gates() -> [f64; 4] {
        [
            MIN_TOP1_300,
            MIN_TOP5_300,
            MIN_MRR10_300,
            MIN_TOP1_1000_INFORMATIVE,
        ]
    }

    #[test]
    fn gates_are_fractions_in_range() {
        for gate in gates() {
            assert!(
                gate > 0.0 && gate < 1.0,
                "gate {gate} is not a fraction in (0, 1)"
            );
        }
    }

    #[test]
    fn gates_are_ordered_the_way_the_metrics_are() {
        // Recall at 1 can never exceed recall at 5, so a gate claiming
        // otherwise is a typo, not a policy.
        //
        // Deliberately no `mrr10 <= top5` rule: MRR@10 is bounded by recall at
        // 10, not at 5. One query answered at rank 6 gives top-5 = 0 but
        // MRR@10 = 1/6, so that comparison would reject valid configurations.
        let ordered = gates();
        let (top1, top5) = (ordered[0], ordered[1]);
        assert!(
            top1 <= top5,
            "top-1 gate {top1} is above the top-5 gate {top5}"
        );
    }

    #[test]
    fn gates_sit_below_the_baseline() {
        // A gate above the baseline would fail on the commit that adds it.
        let baselines = [BASELINE_TOP1_300, BASELINE_TOP5_300, BASELINE_MRR10_300];
        for (gate, baseline) in gates()[..3].iter().zip(baselines) {
            assert!(
                *gate < baseline,
                "gate {gate} is at or above the measured baseline {baseline}"
            );
        }
    }

    #[test]
    fn gates_leave_a_small_positive_margin() {
        // Enough room for seed churn, not enough to hide a real drop.
        let baselines = [BASELINE_TOP1_300, BASELINE_TOP5_300, BASELINE_MRR10_300];
        for (gate, baseline) in gates()[..3].iter().zip(baselines) {
            let margin = baseline - *gate;
            assert!(
                margin > 0.0 && margin < 0.05,
                "gate {gate} sits {margin:.3} from baseline {baseline}; want a positive margin under five points"
            );
        }
    }

    #[test]
    fn context_cap_matches_the_list_skills_harness() {
        assert_eq!(
            MAX_CONTEXT_TOKENS,
            dcc_mcp_skills::catalog::list_context::MAX_DEFAULT_PAGE_TOKENS
        );
    }
}
