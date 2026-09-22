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
//! They are the measured baseline, minus roughly two and a half points of margin:
//!
//! | metric | measured at 300, `dcc`-filtered | gate |
//! |---|---|---|
//! | top-1  | 76.5% ([`BASELINE_TOP1_300`]) | 74% |
//! | top-5  | 82.8% ([`BASELINE_TOP5_300`]) | 80% |
//! | MRR@10 | 79.5% ([`BASELINE_MRR10_300`]) | 77% |
//!
//! The baseline already moved once. Reviewing the first submission caught that
//! the corpus was not reproducible (`StdRng` carries no cross-version
//! guarantee, now `ChaCha8Rng`) and that some literal queries deleted a
//! separator instead of a letter. Both changed the corpus, so both moved every
//! number.
//!
//! The margin is not slack for a sloppy ranker — it is room for the corpus to
//! move. The 26 real seeds are harvested from this repository, so every skill
//! someone ships changes the measurement slightly. Four points absorbs that
//! without absorbing a real regression.
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
//! and the first thing to fail on exactly the seed churn the four-point margin
//! exists to absorb. Two thresholds for one metric means the tighter one wins,
//! so there is now one.
//!
//! # Why not the 85 / 95 / 90 originally suggested
//!
//! Those targets were set before anything was measured, and the first
//! measurement does not reach them. The gap is a property of the corpus as
//! much as of the ranker: the synthetic filler draws its descriptions from a
//! 30-word pool, so most of its skills have no vocabulary that distinguishes
//! them from their neighbours, and a query built from their own words has no
//! single right answer. Descriptive query classes are only emitted for targets
//! that clear [`crate::queries::MAX_ANSWERABLE_DF`], which in practice means
//! the real seeds.
//!
//! Raising these numbers is therefore corpus work, not threshold work: give
//! the filler skills distinguishable content and hold the ranker to a higher
//! bar. Until then the gate still does its job — it fails on any change that
//! moves ranking quality down by more than the margin.

/// Minimum top-1 hit rate at [`crate::corpus::SCALE_300`], `dcc`-filtered.
pub const MIN_TOP1_300: f64 = 0.74;
/// Minimum top-5 hit rate at [`crate::corpus::SCALE_300`], `dcc`-filtered.
pub const MIN_TOP5_300: f64 = 0.80;
/// Minimum MRR@10 at [`crate::corpus::SCALE_300`], `dcc`-filtered.
pub const MIN_MRR10_300: f64 = 0.77;

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
pub const BASELINE_TOP1_300: f64 = 0.765;
/// See [`BASELINE_TOP1_300`].
pub const BASELINE_TOP5_300: f64 = 0.828;
/// See [`BASELINE_TOP1_300`].
pub const BASELINE_MRR10_300: f64 = 0.795;

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
