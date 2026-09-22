//! Ranking and latency metrics (PIP-3408).

use std::time::Duration;

/// Ranking quality over a set of queries.
///
/// Every field is a mean over the queries in the group, in `0.0..=1.0`.
#[derive(Debug, Clone, Copy, Default, PartialEq, serde::Serialize)]
pub struct HitRates {
    /// Queries in the group.
    pub queries: usize,
    /// Fraction where the expected skill ranked first.
    pub top1: f64,
    /// Fraction where it ranked in the top 5.
    pub top5: f64,
    /// Fraction where it ranked in the top 10.
    pub top10: f64,
    /// Mean reciprocal rank, truncated at 10 (0 when absent from the top 10).
    pub mrr10: f64,
    /// Fraction where the expected skill was not in the top 10 at all.
    pub misses: f64,
}

impl HitRates {
    /// Aggregate the per-query ranks of one group.
    ///
    /// `ranks` holds the 1-based rank of the expected skill, or `None` when it
    /// did not appear within the measured cut-off.
    #[must_use]
    pub fn from_ranks(ranks: &[Option<u32>]) -> Self {
        let total = ranks.len();
        if total == 0 {
            return Self::default();
        }
        let reciprocal_cutoff = 10u32;
        let mut top1 = 0usize;
        let mut top5 = 0usize;
        let mut top10 = 0usize;
        let mut misses = 0usize;
        let mut reciprocal_sum = 0.0f64;

        for rank in ranks {
            match rank {
                Some(rank) => {
                    if *rank == 1 {
                        top1 += 1;
                    }
                    if *rank <= 5 {
                        top5 += 1;
                    }
                    if *rank <= reciprocal_cutoff {
                        top10 += 1;
                        reciprocal_sum += 1.0 / f64::from(*rank);
                    } else {
                        misses += 1;
                    }
                }
                None => misses += 1,
            }
        }

        let total_f = total as f64;
        Self {
            queries: total,
            top1: top1 as f64 / total_f,
            top5: top5 as f64 / total_f,
            top10: top10 as f64 / total_f,
            mrr10: reciprocal_sum / total_f,
            misses: misses as f64 / total_f,
        }
    }
}

/// Latency distribution over a set of timed queries.
#[derive(Debug, Clone, Copy, Default, PartialEq, serde::Serialize)]
pub struct Latency {
    /// Timed queries.
    pub samples: usize,
    /// Median latency.
    pub p50: Duration,
    /// 95th percentile latency.
    pub p95: Duration,
    /// Slowest query.
    pub max: Duration,
}

impl Latency {
    /// Percentiles over `samples`, which are sorted internally.
    ///
    /// Uses the nearest-rank method: `p95` is the sample at
    /// `ceil(0.95 * n) - 1`, so a single-sample group reports that sample at
    /// every percentile instead of interpolating towards zero.
    #[must_use]
    pub fn from_samples(mut samples: Vec<Duration>) -> Self {
        if samples.is_empty() {
            return Self::default();
        }
        samples.sort_unstable();
        let count = samples.len();
        Self {
            samples: count,
            p50: percentile(&samples, 50),
            p95: percentile(&samples, 95),
            max: samples[count - 1],
        }
    }
}

/// Nearest-rank percentile of an already-sorted slice.
fn percentile(sorted: &[Duration], pct: usize) -> Duration {
    assert!(!sorted.is_empty());
    // `ceil(pct/100 * n)`, at least 1, then 1-based → 0-based.
    let rank = ((pct * sorted.len()).div_ceil(100)).max(1);
    sorted[rank - 1]
}

/// Estimated tokens of a payload, using the product's byte4 estimator.
///
/// Mirrors `dcc_mcp_skills::catalog::list_context::estimate_tokens` so context
/// numbers reported by this crate are directly comparable with the gateway
/// trace view and the `list_skills` context harness.
#[must_use]
pub fn estimate_tokens(bytes: usize) -> usize {
    bytes.div_ceil(4)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hit_rates_count_ranks() {
        let rates = HitRates::from_ranks(&[Some(1), Some(3), Some(10), Some(11), None]);
        assert_eq!(rates.queries, 5);
        assert!((rates.top1 - 0.2).abs() < 1e-9);
        assert!((rates.top5 - 0.4).abs() < 1e-9);
        assert!((rates.top10 - 0.6).abs() < 1e-9);
        assert!((rates.misses - 0.4).abs() < 1e-9);
        // 1 + 1/3 + 1/10 over 5 queries.
        let expected = (1.0 + 1.0 / 3.0 + 0.1) / 5.0;
        assert!((rates.mrr10 - expected).abs() < 1e-9);
    }

    #[test]
    fn empty_group_is_all_zero() {
        let rates = HitRates::from_ranks(&[]);
        assert_eq!(rates.queries, 0);
        assert_eq!(rates.top1, 0.0);
    }

    #[test]
    fn percentiles_never_fall_below_the_single_sample() {
        let one = Latency::from_samples(vec![Duration::from_micros(7)]);
        assert_eq!(one.p50, Duration::from_micros(7));
        assert_eq!(one.p95, Duration::from_micros(7));
        assert_eq!(one.max, Duration::from_micros(7));
    }

    #[test]
    fn percentiles_pick_nearest_rank() {
        let samples: Vec<Duration> = (1..=100).map(Duration::from_millis).collect();
        let latency = Latency::from_samples(samples);
        assert_eq!(latency.p50, Duration::from_millis(50));
        assert_eq!(latency.p95, Duration::from_millis(95));
        assert_eq!(latency.max, Duration::from_millis(100));
    }

    #[test]
    fn token_estimator_matches_the_product_convention() {
        assert_eq!(estimate_tokens(0), 0);
        assert_eq!(estimate_tokens(4), 1);
        assert_eq!(estimate_tokens(5), 2);
    }
}
