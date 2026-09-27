//! Skills benchmark CLI (PIP-3408).

use dcc_mcp_skills_bench::corpus::{SCALE_300, SCALE_1000};
use dcc_mcp_skills_bench::{adapters, context, recall, run, seeds};

fn main() {
    let Some(command) = std::env::args().nth(1) else {
        eprintln!(
            "usage: skills-bench <report [--json] [--output <path>]|regenerate-seeds|harvest-adapters>"
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

fn report() {
    let args: Vec<String> = std::env::args().collect();
    let json = args.iter().any(|arg| arg == "--json");
    // `--output <path>` always writes JSON, whatever the console format is.
    let output = args
        .windows(2)
        .find(|pair| pair[0] == "--output")
        .map(|pair| pair[1].clone());

    let seeds = seeds::all_seeds();
    let coverage = recall::RecallCoverage::measure(&seeds);

    let evaluations: Vec<_> = [SCALE_300, SCALE_1000]
        .into_iter()
        .map(|scale| {
            let corpus = dcc_mcp_skills_bench::Corpus::build(scale);
            run::evaluate(&corpus)
        })
        .collect();
    let curve = context::scan();

    if let Some(path) = &output {
        let text = serde_json::to_string_pretty(&dcc_mcp_skills_bench::report::render_json(
            &evaluations,
            &curve,
            &coverage,
        ))
        .expect("report is serialisable");
        if let Err(error) = std::fs::write(path, format!("{text}\n")) {
            eprintln!("failed to write {path}: {error}");
            std::process::exit(1);
        }
    }

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&dcc_mcp_skills_bench::report::render_json(
                &evaluations,
                &curve,
                &coverage
            ))
            .expect("report is serialisable")
        );
    } else {
        print!(
            "{}",
            dcc_mcp_skills_bench::report::render_text(&evaluations, &curve, &coverage)
        );
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
