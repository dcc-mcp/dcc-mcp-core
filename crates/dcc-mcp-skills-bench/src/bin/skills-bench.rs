//! Skills benchmark CLI (PIP-3408).

use dcc_mcp_skills_bench::corpus::{SCALE_300, SCALE_1000};
use dcc_mcp_skills_bench::{context, run, seeds};

fn main() {
    let Some(command) = std::env::args().nth(1) else {
        eprintln!("usage: skills-bench <report [--json] [--output <path>]|regenerate-seeds>");
        std::process::exit(2);
    };

    match command.as_str() {
        "regenerate-seeds" => regenerate_seeds(),
        "report" => report(),
        other => {
            eprintln!("unknown command {other}; expected `report` or `regenerate-seeds`");
            std::process::exit(2);
        }
    }
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
                &curve
            ))
            .expect("report is serialisable")
        );
    } else {
        print!(
            "{}",
            dcc_mcp_skills_bench::report::render_text(&evaluations, &curve)
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
