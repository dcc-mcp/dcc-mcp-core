//! Hit-rate regression gate (PIP-3408).
//!
//! This is the gate the issue asks CI to run: the 300-skill corpus,
//! `dcc`-filtered, against [`thresholds`]. It is deterministic — the corpus is
//! seeded and the scorer is pure — so it reports the same number on a laptop
//! and in CI.
//!
//! The 1000-skill scale is measured and asserted loosely as a trend signal. It
//! is deliberately not a hard gate: at that size the number tracks filler
//! vocabulary collisions more than ranking quality.

use dcc_mcp_skills_bench::Corpus;
use dcc_mcp_skills_bench::corpus::{SCALE_300, SCALE_1000};
use dcc_mcp_skills_bench::report::gate_passes;
use dcc_mcp_skills_bench::run::Filter;
use dcc_mcp_skills_bench::thresholds;

/// The gated measurement: 300 skills, `dcc`-filtered, all query classes.
#[test]
fn hit_rate_gate_at_300() {
    let evaluation = dcc_mcp_skills_bench::run::evaluate(&Corpus::build(SCALE_300));
    let group = evaluation
        .gate_group()
        .expect("the gated group is always measured");

    assert!(
        group.metrics.top1 >= thresholds::MIN_TOP1_300,
        "top-1 regressed: {:.3} < {:.3}\n{gate_note}",
        group.metrics.top1,
        thresholds::MIN_TOP1_300,
        gate_note = note(&evaluation)
    );
    assert!(
        group.metrics.top5 >= thresholds::MIN_TOP5_300,
        "top-5 regressed: {:.3} < {:.3}\n{gate_note}",
        group.metrics.top5,
        thresholds::MIN_TOP5_300,
        gate_note = note(&evaluation)
    );
    assert!(
        group.metrics.mrr10 >= thresholds::MIN_MRR10_300,
        "MRR@10 regressed: {:.3} < {:.3}\n{gate_note}",
        group.metrics.mrr10,
        thresholds::MIN_MRR10_300,
        gate_note = note(&evaluation)
    );
    assert!(gate_passes(group));
}

/// The `dcc`-filtered path must never be worse than the unfiltered path.
///
/// Filtering narrows the candidate set to one shard before scoring, so it
/// removes competitors rather than adding them. If the filtered group ever
/// falls behind, the shard path is dropping the right answer.
#[test]
fn dcc_filter_is_never_worse_than_a_full_scan() {
    let evaluation = dcc_mcp_skills_bench::run::evaluate(&Corpus::build(SCALE_300));
    let filtered = evaluation
        .hit_rate_group(&format!("{}/all", Filter::Dcc.label()))
        .expect("filtered group");
    let unfiltered = evaluation
        .hit_rate_group(&format!("{}/all", Filter::Unfiltered.label()))
        .expect("unfiltered group");

    assert!(
        filtered.metrics.top1 >= unfiltered.metrics.top1,
        "filtered top-1 {:.3} below unfiltered {:.3}: the shard is losing the answer",
        filtered.metrics.top1,
        unfiltered.metrics.top1
    );
}

/// Hard negatives must actually cost something.
///
/// If this stops holding, the injected twins are no longer competing with their
/// targets and the `hard_negative` split is measuring nothing.
#[test]
fn hard_negatives_are_harder_than_the_clean_control() {
    let evaluation = dcc_mcp_skills_bench::run::evaluate(&Corpus::build(SCALE_300));
    let hard = evaluation
        .hit_rate_group(&format!("{}/hard_negative", Filter::Unfiltered.label()))
        .expect("hard group");
    let clean = evaluation
        .hit_rate_group(&format!("{}/clean", Filter::Unfiltered.label()))
        .expect("clean group");

    assert!(hard.metrics.queries > 0, "no hard-negative queries");
    assert!(clean.metrics.queries > 0, "no clean control queries");
    assert!(
        hard.metrics.top1 < clean.metrics.top1,
        "hard negatives no longer cost anything: {:.3} vs clean {:.3}",
        hard.metrics.top1,
        clean.metrics.top1
    );
}

/// The 1000-skill scale is a trend signal, asserted loosely.
///
/// `#[ignore]`d because it costs ~78s against ~9s for the 300-skill gate, and
/// `cargo nextest run --workspace` runs in debug for every PR. `skills-bench.yml`
/// runs it explicitly with `--release --include-ignored`, which is both faster
/// and the only place the 1000-skill figure is actually consumed.
#[test]
#[ignore = "slow trend signal; run from skills-bench.yml with --release"]
fn trend_signal_at_1000_stays_in_a_sane_range() {
    let evaluation = dcc_mcp_skills_bench::run::evaluate(&Corpus::build(SCALE_1000));
    let group = evaluation.gate_group().expect("gated group");
    assert!(
        group.metrics.top1 >= thresholds::MIN_TOP1_1000_INFORMATIVE,
        "1000-skill top-1 collapsed: {:.3} < {:.3}\n{gate_note}",
        group.metrics.top1,
        thresholds::MIN_TOP1_1000_INFORMATIVE,
        gate_note = note(&evaluation)
    );
}

/// Short context printed with every failure, so a red CI job says what moved.
fn note(evaluation: &dcc_mcp_skills_bench::run::Evaluation) -> String {
    let mut lines = vec![format!(
        "corpus: {} skills, {} real seeds, {} twins; queries {}",
        evaluation.corpus_size, evaluation.seeds, evaluation.hard_negatives, evaluation.queries
    )];
    for group in &evaluation.hit_rate {
        if group.filter == Filter::Dcc {
            lines.push(format!(
                "  {:<26} top1 {:.3}  top5 {:.3}  mrr10 {:.3}",
                group.name, group.metrics.top1, group.metrics.top5, group.metrics.mrr10
            ));
        }
    }
    lines.join("\n")
}
