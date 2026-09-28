//! Skills benchmark CLI (PIP-3408, PIP-3703).

use std::path::PathBuf;

use dcc_mcp_skills_bench::corpus::{SCALE_300, SCALE_1000};
use dcc_mcp_skills_bench::{Corpus, adapters, context, recall, run, seeds, trend};

fn main() {
    let Some(command) = std::env::args().nth(1) else {
        eprintln!(
            "usage: skills-bench <report [--json] [--output <path>] \
             [--trend-history <path>] [--trend-report <path>] [--trend-alerts <path>]|\
             regenerate-seeds|harvest-adapters>"
        );
        std::process::exit(2);
    };

    match command.as_str() {
        "regenerate-seeds" => regenerate_seeds(),
        "harvest-adapters" => harvest_adapters(),
        "report" => report(),
        other => {
            eprintln!(
                "unknown command {other}; expected `report`, `regenerate-seeds` or `harvest-adapters`"
            );
            std::process::exit(2);
        }
    }
}

/// Re-harvest the pinned adapter repositories into the committed snapshot.
///
/// Needs network and `git`; nothing in the benchmark itself does.
fn harvest_adapters() {
    let root = seeds::workspace_root()
        .join("target")
        .join("adapter-harvest");
    if let Err(error) = std::fs::create_dir_all(&root) {
        eprintln!("failed to create {}: {error}", root.display());
        std::process::exit(1);
    }
    let sources = adapters::adapter_sources();
    for source in &sources {
        println!("fetching {} at {}", source.repo, source.commit);
        if let Err(error) = adapters::checkout(source, &root) {
            eprintln!("failed to check out {}: {error}", source.repo);
            std::process::exit(1);
        }
    }
    let set = adapters::build_adapter_set(&root, &sources);
    match adapters::write_committed(&set) {
        Ok(()) => println!(
            "wrote {} adapter seeds from {} repositories to {}",
            set.skills.len(),
            set.sources.len(),
            adapters::adapters_path().display()
        ),
        Err(error) => {
            eprintln!("failed to write adapter seeds: {error}");
            std::process::exit(1);
        }
    }
    let coverage = recall::RecallCoverage::measure(&set.skills);
    println!(
        "recall-context coverage: {:.1}% of field slots, {:.1}% of skills carry a block",
        coverage.field_coverage() * 100.0,
        coverage.context_coverage() * 100.0
    );
}

fn regenerate_seeds() {
    let set = seeds::build_seed_set(&seeds::workspace_root());
    match seeds::write_committed(&set) {
        Ok(()) => println!(
            "wrote {} seeds to {}",
            set.skills.len(),
            seeds::seeds_path().display()
        ),
        Err(error) => {
            eprintln!("failed to write seeds: {error}");
            std::process::exit(1);
        }
    }
}

/// Look up a `--flag <value>` pair, so flag order does not matter.
fn flag_value(args: &[String], name: &str) -> Option<PathBuf> {
    args.windows(2)
        .find(|pair| pair[0] == name)
        .map(|pair| PathBuf::from(&pair[1]))
}

/// Write `text` to `path`, reporting a failure without aborting the run.
///
/// Every file this CLI emits is derived data; none of them are worth losing
/// the measurement over.
fn write_text(path: &PathBuf, text: &str) {
    if let Err(error) = std::fs::write(path, text) {
        eprintln!("failed to write {}: {error}", path.display());
    }
}

fn report() {
    let args: Vec<String> = std::env::args().collect();
    let json = args.iter().any(|arg| arg == "--json");
    // `--output <path>` always writes JSON, whatever the console format is.
    let output = flag_value(&args, "--output");
    let history_path = flag_value(&args, "--trend-history");
    let trend_report_path = flag_value(&args, "--trend-report");
    let alerts_path = flag_value(&args, "--trend-alerts");

    let seeds = seeds::all_seeds();
    let coverage = recall::RecallCoverage::measure(&seeds);

    let corpus_300 = Corpus::build(SCALE_300);
    let corpus_1000 = Corpus::build(SCALE_1000);
    let evaluations = vec![run::evaluate(&corpus_300), run::evaluate(&corpus_1000)];
    let curve = context::scan();

    // The trend point is recorded against the gated 300-skill corpus: that is
    // the only scale the latency baseline is tracked at, because a baseline
    // over the 1000-skill corpus would measure filler vocabulary.
    let mut history = history_path
        .as_ref()
        .map(|path| (path, trend::TrendHistory::load(path)));

    let trend_report = match &mut history {
        Some((path, history)) => {
            let point = trend::TrendPoint::record(&corpus_300, &evaluations, &curve);
            let report = trend::compare(history, &point);

            // Rendered before the point is pushed, so "recent runs" reads as a
            // list of predecessors with the current run on top, rather than
            // showing the current run twice.
            if let Some(report_path) = &trend_report_path {
                let text = trend::render_trend_markdown(history, &point, &report);
                write_text(report_path, &text);
                if !json {
                    println!("{text}");
                }
            }

            history.push(point);
            if let Some(error_path) = history.save(path).err() {
                eprintln!(
                    "failed to write the trend history to {}: {error_path}",
                    path.display()
                );
            }
            Some(report)
        }
        None => None,
    };

    if let Some(path) = &output {
        let text =
            serde_json::to_string_pretty(&dcc_mcp_skills_bench::report::render_json_with_trend(
                &evaluations,
                &curve,
                &coverage,
                trend_report.as_ref(),
            ))
            .expect("report is serialisable");
        write_text(path, &format!("{text}\n"));
    }

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&dcc_mcp_skills_bench::report::render_json_with_trend(
                &evaluations,
                &curve,
                &coverage,
                trend_report.as_ref(),
            ))
            .expect("report is serialisable")
        );
    } else {
        print!(
            "{}",
            dcc_mcp_skills_bench::report::render_text(&evaluations, &curve, &coverage)
        );
    }

    // Alerts are surfaced, never enforced. `::warning` puts them in the GitHub
    // job UI where a human will see them; the exit code below stays reserved
    // for the hit-rate gate, which is the only gate this crate has.
    if let Some(report) = &trend_report {
        for alert in &report.alerts {
            let ratio = alert.ratio.unwrap_or(0.0) * 100.0;
            let baseline = alert
                .baseline_p95_us
                .map_or_else(|| "none".to_string(), |us| format!("{us}us"));
            eprintln!(
                "::warning::skills benchmark latency regression in {}: p95 {}us vs \
                 rolling median {} ({:+.1}%, alert at {:+.0}%)",
                alert.group,
                alert.current_p95_us,
                baseline,
                ratio,
                trend::LATENCY_REGRESSION_RATIO * 100.0
            );
        }
        if let Some(path) = &alerts_path {
            let text = serde_json::to_string_pretty(&trend::render_alerts_json(report))
                .expect("alerts are serialisable");
            write_text(path, &format!("{text}\n"));
        }
    }

    if !dcc_mcp_skills_bench::report::gate_passes(
        evaluations[0]
            .gate_group()
            .expect("the gated group is always measured"),
    ) {
        eprintln!("hit-rate gate FAILED");
        std::process::exit(1);
    }
}
