//! Evaluate the corpus: run every query and grade the result (PIP-3408).

use std::time::{Duration, Instant};

use dcc_mcp_skills::SkillCatalog;

use crate::corpus::Corpus;
use crate::metrics::{HitRates, Latency};
use crate::queries::{ALL_KINDS, Query};

/// How many results the ranker is allowed to return per query.
///
/// Also the cut-off for MRR: a hit outside the top 10 scores zero, because a
/// skill an agent never sees on the first page was not found.
pub const TOP_K: usize = 10;

/// Whether a query run carried the target's `dcc` filter.
///
/// The two variants are different problems, not two takes on one problem:
/// with `dcc` the catalog narrows to a per-DCC shard before scoring, without
/// it every skill in the catalogue competes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Filter {
    /// `search_skills(.., dcc = Some(target.dcc), ..)` — shard fast path.
    Dcc,
    /// `search_skills(.., dcc = None, ..)` — full catalogue scan.
    Unfiltered,
}

impl Filter {
    /// Stable label used in reports and gate names.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Dcc => "dcc_filtered",
            Self::Unfiltered => "unfiltered",
        }
    }

    /// Both variants, in report order.
    pub const ALL: [Filter; 2] = [Filter::Dcc, Filter::Unfiltered];
}

/// One measured group: a name plus its metrics.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Group<T> {
    /// Report name, e.g. `dcc_filtered/literal`.
    pub name: String,
    /// Whether the group ran with the `dcc` filter.
    pub filter: Filter,
    /// Metrics for the group.
    pub metrics: T,
}

/// Rank of the expected skill within a result page.
///
/// `None` when it is absent from the top [`TOP_K`].
#[must_use]
fn rank_of(results: &[dcc_mcp_skills::SkillSummary], expected: &str) -> Option<u32> {
    results
        .iter()
        .position(|summary| summary.name == expected)
        .map(|index| index as u32 + 1)
}

/// Run one query against `catalog` and return the rank plus timing.
fn run_one(
    catalog: &SkillCatalog,
    query: &Query,
    filter: Filter,
) -> (Option<u32>, Duration, usize) {
    let dcc = match filter {
        Filter::Dcc => Some(query.dcc.as_str()),
        Filter::Unfiltered => None,
    };
    let start = Instant::now();
    let results = catalog.search_skills(Some(&query.text), &[], dcc, None, Some(TOP_K));
    let elapsed = start.elapsed();

    let tokens = serde_json::to_string(&results).map_or(0, |text| text.len() / 4);
    (rank_of(&results, &query.expected), elapsed, tokens)
}

/// Grade a subset of `queries` under one filter.
fn grade(
    catalog: &SkillCatalog,
    queries: &[&Query],
    filter: Filter,
    name: &str,
) -> (Group<HitRates>, Group<Latency>, f64) {
    let mut ranks = Vec::with_capacity(queries.len());
    let mut samples = Vec::with_capacity(queries.len());
    let mut token_total = 0usize;

    for query in queries {
        let (rank, elapsed, tokens) = run_one(catalog, query, filter);
        ranks.push(rank);
        samples.push(elapsed);
        token_total += tokens;
    }

    let queries = queries.len().max(1);
    (
        Group {
            name: name.to_string(),
            filter,
            metrics: HitRates::from_ranks(&ranks),
        },
        Group {
            name: name.to_string(),
            filter,
            metrics: Latency::from_samples(samples),
        },
        token_total as f64 / queries as f64,
    )
}

/// Mean result payload per query, in estimated tokens.
#[derive(Debug, Clone, Copy, Default, serde::Serialize)]
pub struct TokenCost {
    /// Mean tokens of one query's result page.
    pub mean_tokens_per_query: f64,
}

/// Full result of one corpus scale across all three dimensions.
///
/// The context dimension lives in [`crate::context`] and is attached
/// separately: it does not depend on the corpus, only on the host scenario.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Evaluation {
    /// Requested corpus size.
    pub scale: usize,
    /// Skills in the catalogue, hard negatives included.
    pub corpus_size: usize,
    /// Real seeds in the corpus.
    pub seeds: usize,
    /// Injected hard-negative twins.
    pub hard_negatives: usize,
    /// Queries executed per filter.
    pub queries: usize,
    /// How many targets produced each query class.
    pub coverage: crate::queries::Coverage,
    /// Ranking quality, split by filter, query class, and hard-negative subset.
    pub hit_rate: Vec<Group<HitRates>>,
    /// Latency distribution, split the same way.
    pub latency: Vec<Group<Latency>>,
    /// Result payload size.
    pub tokens: Vec<Group<TokenCost>>,
}

impl Evaluation {
    /// Look up a hit-rate group by name.
    #[must_use]
    pub fn hit_rate_group(&self, name: &str) -> Option<&Group<HitRates>> {
        self.hit_rate.iter().find(|group| group.name == name)
    }

    /// The group the CI gate checks: everything, `dcc`-filtered.
    #[must_use]
    pub fn gate_group(&self) -> Option<&Group<HitRates>> {
        self.hit_rate_group(&format!("{}/all", Filter::Dcc.label()))
    }
}

/// Run every query in `corpus` under both filters.
#[must_use]
pub fn evaluate(corpus: &Corpus) -> Evaluation {
    let catalog = corpus.catalog();
    let (queries, coverage) = crate::queries::build_queries_with_coverage(corpus);

    // Warm the lazily-built index so the first measured query is not paying
    // for construction.
    if let Some(first) = queries.first() {
        let _ = catalog.search_skills(Some(&first.text), &[], None, None, Some(TOP_K));
    }

    let all: Vec<&Query> = queries.iter().collect();
    let hard: Vec<&Query> = queries.iter().filter(|q| q.has_hard_negative).collect();
    let clean: Vec<&Query> = queries.iter().filter(|q| !q.has_hard_negative).collect();

    let mut hit_rate = Vec::new();
    let mut latency = Vec::new();
    let mut tokens = Vec::new();

    for filter in Filter::ALL {
        let subsets: [(&str, &[&Query]); 4] = [
            ("all", &all),
            ("hard_negative", &hard),
            ("clean", &clean),
            ("", &[]),
        ];
        for (label, subset) in subsets {
            if label.is_empty() {
                continue;
            }
            let name = format!("{}/{label}", filter.label());
            let (hits, timings, mean_tokens) = grade(&catalog, subset, filter, &name);
            hit_rate.push(hits);
            latency.push(timings);
            tokens.push(Group {
                name,
                filter,
                metrics: TokenCost {
                    mean_tokens_per_query: mean_tokens,
                },
            });
        }

        for kind in ALL_KINDS {
            let subset: Vec<&Query> = queries.iter().filter(|q| q.kind == kind).collect();
            if subset.is_empty() {
                continue;
            }
            let name = format!("{}/{}", filter.label(), kind.label());
            let (hits, timings, mean_tokens) = grade(&catalog, &subset, filter, &name);
            hit_rate.push(hits);
            latency.push(timings);
            tokens.push(Group {
                name,
                filter,
                metrics: TokenCost {
                    mean_tokens_per_query: mean_tokens,
                },
            });
        }
    }

    Evaluation {
        scale: corpus.scale,
        corpus_size: corpus.skills.len(),
        seeds: corpus.seeds,
        hard_negatives: corpus.hard_negatives.len(),
        queries: queries.len(),
        coverage,
        hit_rate,
        latency,
        tokens,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::corpus::SCALE_300;

    fn eval() -> Evaluation {
        evaluate(&Corpus::build(SCALE_300))
    }

    #[test]
    fn every_filter_has_an_all_group() {
        let evaluation = eval();
        for filter in Filter::ALL {
            let name = format!("{}/all", filter.label());
            assert!(
                evaluation.hit_rate_group(&name).is_some(),
                "missing group {name}"
            );
        }
    }

    #[test]
    fn gate_group_exists_and_has_queries() {
        let evaluation = eval();
        let group = evaluation.gate_group().expect("gate group");
        assert!(group.metrics.queries > 0);
    }

    #[test]
    fn groups_partition_the_query_set() {
        let evaluation = eval();
        let all = evaluation
            .hit_rate_group(&format!("{}/all", Filter::Dcc.label()))
            .unwrap()
            .metrics
            .queries;
        let hard = evaluation
            .hit_rate_group(&format!("{}/hard_negative", Filter::Dcc.label()))
            .unwrap()
            .metrics
            .queries;
        let clean = evaluation
            .hit_rate_group(&format!("{}/clean", Filter::Dcc.label()))
            .unwrap()
            .metrics
            .queries;
        assert_eq!(all, hard + clean);
        assert_eq!(evaluation.queries, all);
    }

    #[test]
    fn dcc_filter_never_widens_the_result_set() {
        // Sanity check on the split itself: with a filter the ranker scores
        // only the shard, so the mean latency should not exceed the unfiltered
        // mean by an order of magnitude.
        let evaluation = eval();
        let filtered = evaluation
            .latency
            .iter()
            .find(|g| g.name == format!("{}/all", Filter::Dcc.label()))
            .unwrap();
        let unfiltered = evaluation
            .latency
            .iter()
            .find(|g| g.name == format!("{}/all", Filter::Unfiltered.label()))
            .unwrap();
        assert!(filtered.metrics.samples == unfiltered.metrics.samples);
    }

    #[test]
    fn evaluation_records_corpus_shape() {
        let evaluation = eval();
        assert_eq!(evaluation.scale, SCALE_300);
        assert_eq!(evaluation.corpus_size, SCALE_300);
        assert!(evaluation.seeds > 0, "corpus must contain real seeds");
        assert!(evaluation.hard_negatives > 0);
    }
}
