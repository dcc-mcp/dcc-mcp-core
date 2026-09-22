//! Report rendering (PIP-3408).
//!
//! Two renderings of the same measurement:
//!
//! * [`render_text`] — readable tables for an issue comment or a terminal.
//! * [`render_json`] — machine-readable, so the numbers can be diffed between
//!   runs instead of being re-read by eye.

use serde_json::{Value, json};

use crate::context::ContextPoint;
use crate::corpus::SCALE_300;
use crate::run::{Evaluation, Group};
use crate::synthetic::CORPUS_SCHEMA_VERSION;
use crate::thresholds;

/// Percent with one decimal, right-aligned in `width`.
fn pct(value: f64) -> String {
    format!("{:6.1}%", value * 100.0)
}

/// Microseconds with a thousands separator-free fixed format.
fn us(duration: std::time::Duration) -> String {
    format!("{:8.0}us", duration.as_secs_f64() * 1_000_000.0)
}

/// Render the whole report as text.
#[must_use]
pub fn render_text(evaluations: &[Evaluation], curve: &[ContextPoint]) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "skills benchmark — corpus schema {CORPUS_SCHEMA_VERSION}\n"
    ));

    for evaluation in evaluations {
        out.push_str(&render_scale(evaluation));
    }
    out.push_str(&render_curve(curve));
    out
}

fn render_scale(evaluation: &Evaluation) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "\n=== scale {} — {} skills ({} real seeds, {} hard-negative twins), {} queries\n",
        evaluation.scale,
        evaluation.corpus_size,
        evaluation.seeds,
        evaluation.hard_negatives,
        evaluation.queries
    ));
    out.push_str(&format!(
        "    targets {}: literal {}, paraphrase {}, intent {} (descriptive classes need distinctive vocabulary)\n",
        evaluation.coverage.targets,
        evaluation.coverage.literal,
        evaluation.coverage.paraphrase,
        evaluation.coverage.intent
    ));

    out.push_str("\n-- hit rate (dimension 1, gated)\n");
    out.push_str("group                          queries   top-1   top-5  top-10   MRR@10\n");
    for group in &evaluation.hit_rate {
        let m = group.metrics;
        out.push_str(&format!(
            "{:<30} {:>7} {} {} {} {}\n",
            group.name,
            m.queries,
            pct(m.top1),
            pct(m.top5),
            pct(m.top10),
            pct(m.mrr10)
        ));
    }

    out.push_str("\n-- query efficiency (dimension 2, trend only — not a gate)\n");
    out.push_str(
        "group                          samples       p50       p95       max   tokens/query\n",
    );
    for group in &evaluation.latency {
        let m = group.metrics;
        let tokens = evaluation
            .tokens
            .iter()
            .find(|t| t.name == group.name)
            .map_or(0.0, |t| t.metrics.mean_tokens_per_query);
        out.push_str(&format!(
            "{:<30} {:>7} {} {} {} {:>14.0}\n",
            group.name,
            m.samples,
            us(m.p50),
            us(m.p95),
            us(m.max),
            tokens
        ));
    }

    // Only the 300 scale is gated (see thresholds::MIN_TOP1_1000_INFORMATIVE).
    // Printing a verdict for 1000 would invent a gate that does not exist.
    if evaluation.scale == SCALE_300 {
        if let Some(gate) = evaluation.gate_group() {
            out.push_str(&format!(
                "\ngate ({}): top-1 >= {:.0}%, top-5 >= {:.0}%, MRR@10 >= {:.0}% -> {}\n",
                gate.name,
                thresholds::MIN_TOP1_300 * 100.0,
                thresholds::MIN_TOP5_300 * 100.0,
                thresholds::MIN_MRR10_300 * 100.0,
                if gate_passes(gate) { "PASS" } else { "FAIL" }
            ));
        }
    } else {
        out.push_str(&format!(
            "\nscale {} is a trend signal: top-1 {:.1}% is reported, not gated\n",
            evaluation.scale,
            evaluation
                .gate_group()
                .map_or(0.0, |gate| gate.metrics.top1)
                * 100.0
        ));
    }

    out
}

/// Whether the gated group clears every hit-rate threshold.
#[must_use]
pub fn gate_passes(group: &Group<crate::metrics::HitRates>) -> bool {
    group.metrics.top1 >= thresholds::MIN_TOP1_300
        && group.metrics.top5 >= thresholds::MIN_TOP5_300
        && group.metrics.mrr10 >= thresholds::MIN_MRR10_300
}

fn render_curve(curve: &[ContextPoint]) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "\n=== context growth (dimension 3, capped at {} tokens)\n",
        thresholds::MAX_CONTEXT_TOKENS
    ));
    out.push_str("hosts   skills  returned     bytes    tokens  marginal  truncated\n");
    for point in curve {
        out.push_str(&format!(
            "{:>5} {:>8} {:>9} {:>9} {:>9} {:>9} {:>10}\n",
            point.hosts,
            point.total,
            point.returned,
            point.bytes,
            point.tokens,
            point.marginal_tokens,
            point.truncated
        ));
    }
    out.push_str(&format!(
        "peak {} tokens (cap {}), peak marginal {} tokens/host (cap {})\n",
        crate::context::peak_tokens(curve),
        thresholds::MAX_CONTEXT_TOKENS,
        crate::context::peak_marginal_tokens(curve),
        thresholds::MAX_MARGINAL_CONTEXT_TOKENS
    ));
    out
}

/// Render the whole report as JSON.
#[must_use]
pub fn render_json(evaluations: &[Evaluation], curve: &[ContextPoint]) -> Value {
    json!({
        "corpus_schema": CORPUS_SCHEMA_VERSION,
        "token_estimator": "dcc-mcp-byte4-v1",
        "thresholds": {
            "min_top1_300": thresholds::MIN_TOP1_300,
            "min_top5_300": thresholds::MIN_TOP5_300,
            "min_mrr10_300": thresholds::MIN_MRR10_300,
            "max_context_tokens": thresholds::MAX_CONTEXT_TOKENS,
            "max_marginal_context_tokens": thresholds::MAX_MARGINAL_CONTEXT_TOKENS,
        },
        "scales": evaluations,
        "context_curve": curve,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Corpus;
    use crate::corpus::SCALE_300;

    #[test]
    fn text_report_covers_every_dimension() {
        let evaluation = crate::run::evaluate(&Corpus::build(SCALE_300));
        let text = render_text(std::slice::from_ref(&evaluation), &crate::context::scan());
        assert!(text.contains("hit rate"));
        assert!(text.contains("query efficiency"));
        assert!(text.contains("context growth"));
        assert!(text.contains("dcc_filtered/all"));
        assert!(text.contains("unfiltered/all"));
    }

    #[test]
    fn json_report_is_serialisable() {
        let evaluation = crate::run::evaluate(&Corpus::build(SCALE_300));
        let value = render_json(std::slice::from_ref(&evaluation), &crate::context::scan());
        assert!(value.get("scales").is_some());
        assert!(value.get("context_curve").is_some());
        let _ = serde_json::to_string(&value).expect("serialisable");
    }

    #[test]
    fn gate_passes_only_when_every_threshold_holds() {
        let group = Group {
            name: "test".to_string(),
            filter: crate::run::Filter::Dcc,
            metrics: crate::metrics::HitRates {
                queries: 1,
                top1: 1.0,
                top5: 1.0,
                top10: 1.0,
                mrr10: 1.0,
                misses: 0.0,
            },
        };
        assert!(gate_passes(&group));

        let mut weak = group;
        weak.metrics.top1 = 0.0;
        assert!(!gate_passes(&weak));
    }
}
