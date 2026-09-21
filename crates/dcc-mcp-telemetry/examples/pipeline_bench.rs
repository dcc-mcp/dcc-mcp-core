//! Pipeline stage benchmark harness.
//!
//! Runs a fixed set of MCP request-path stages under a [`ToolRecorder`] and
//! writes the aggregated per-stage metrics to a JSON report that serde can
//! consume (`pipeline-bench-raw.json` by default). The companion script
//! `scripts/bench_pipeline_report.py` turns that raw export into the
//! trend-friendly `pipeline-bench.json` and applies per-stage budgets.
//!
//! The stage list is a built-in default today. Once
//! `scenarios/pipeline_full.yaml` lands it will drive both the stage set and
//! the budgets; the harness is intentionally driven by name so that transition
//! only swaps the stage source, not the reporting contract.
//!
//! ```text
//! cargo run -p dcc-mcp-telemetry --example pipeline_bench -- \
//!     --iterations 500 --output target/pipeline-bench-raw.json
//! ```

use std::collections::HashMap;
use std::hint::black_box;

use serde_json::{Value, json};

use dcc_mcp_telemetry::provider;
use dcc_mcp_telemetry::recorder::ToolRecorder;
use dcc_mcp_telemetry::report::BenchReport;
use dcc_mcp_telemetry::types::TelemetryConfig;

/// Default number of measured iterations per stage (after warmup).
const DEFAULT_ITERATIONS: usize = 500;
/// Default number of discarded warmup iterations per stage.
const DEFAULT_WARMUP: usize = 50;
/// Default output path for the raw report.
const DEFAULT_OUTPUT: &str = "target/pipeline-bench-raw.json";

/// Every stage in the harness, in declaration order.
const STAGE_NAMES: [&str; 6] = [
    "serialize_request",
    "parse_request",
    "route_tool",
    "validate_params",
    "serialize_response",
    "record_telemetry",
];

/// Inputs shared by every stage.
struct Workload {
    /// A representative `tools/call` payload.
    request: Value,
    /// Pre-encoded form of [`Workload::request`].
    encoded: String,
    /// Route table: tool slug → handler id.
    routes: HashMap<String, u32>,
}

impl Workload {
    /// Build the fixed benchmark inputs.
    fn new() -> Self {
        let request = json!({
            "jsonrpc": "2.0",
            "id": 42,
            "method": "tools/call",
            "params": {
                "name": "maya_geometry__create_sphere",
                "arguments": {
                    "radius": 1.5,
                    "subdivisions": 32,
                    "name": "bench_sphere",
                    "transform": [0.0, 1.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0],
                    "tags": ["bench", "pipeline"]
                }
            }
        });
        let encoded = serde_json::to_string(&request).expect("request is serializable");
        let mut routes = HashMap::new();
        for (index, slug) in [
            "maya_geometry__create_sphere",
            "maya_geometry__create_cube",
            "maya_pipeline__export_usd",
            "maya_pipeline__setup_project",
            "blender_geometry__create_mesh",
        ]
        .iter()
        .enumerate()
        {
            routes.insert((*slug).to_string(), index as u32 + 1);
        }
        Workload {
            request,
            encoded,
            routes,
        }
    }
}

/// A single benchmarked stage.
struct Stage {
    /// Stage name as recorded by the [`ToolRecorder`].
    name: &'static str,
    /// The workload the stage measures. Returns `true` when the iteration succeeded.
    run: fn(&Workload) -> bool,
}

impl Stage {
    /// Every stage exposed by the harness.
    fn all() -> Vec<Stage> {
        vec![
            Stage {
                name: "serialize_request",
                run: serialize_request,
            },
            Stage {
                name: "parse_request",
                run: parse_request,
            },
            Stage {
                name: "route_tool",
                run: route_tool,
            },
            Stage {
                name: "validate_params",
                run: validate_params,
            },
            Stage {
                name: "serialize_response",
                run: serialize_response,
            },
            Stage {
                name: "record_telemetry",
                run: record_telemetry,
            },
        ]
    }
}

/// Encode the request payload to JSON.
fn serialize_request(workload: &Workload) -> bool {
    match serde_json::to_string(black_box(&workload.request)) {
        Ok(encoded) => !black_box(encoded).is_empty(),
        Err(_) => false,
    }
}

/// Decode the request payload from JSON.
fn parse_request(workload: &Workload) -> bool {
    match serde_json::from_str::<Value>(black_box(&workload.encoded)) {
        Ok(value) => black_box(value).is_object(),
        Err(_) => false,
    }
}

/// Resolve the tool slug to a handler id.
fn route_tool(workload: &Workload) -> bool {
    let slug = workload.request["params"]["name"]
        .as_str()
        .unwrap_or_default();
    black_box(workload.routes.get(black_box(slug)).copied()).is_some()
}

/// Validate the typed subset of the request params.
fn validate_params(workload: &Workload) -> bool {
    let params = &workload.request["params"];
    let name = params["name"].as_str().unwrap_or_default();
    let arguments = &params["arguments"];
    let subdivisions = arguments["subdivisions"].as_u64().unwrap_or_default();
    let transform_len = arguments["transform"].as_array().map_or(0, Vec::len);
    black_box(name.starts_with("maya_") && subdivisions > 0 && transform_len == 16)
}

/// Encode a representative `tools/call` response.
fn serialize_response(workload: &Workload) -> bool {
    let response = json!({
        "jsonrpc": "2.0",
        "id": workload.request["id"],
        "result": {
            "content": [{ "type": "text", "text": "created bench_sphere" }],
            "isError": false,
            "structuredContent": {
                "node": "|bench_sphere",
                "radius": workload.request["params"]["arguments"]["radius"],
            }
        }
    });
    match serde_json::to_string(black_box(&response)) {
        Ok(encoded) => !black_box(encoded).is_empty(),
        Err(_) => false,
    }
}

/// Cost of recording one instrumented action (nested recorder + span).
fn record_telemetry(workload: &Workload) -> bool {
    let recorder = ToolRecorder::new("pipeline-bench");
    let span = tracing::info_span!(
        "pipeline_stage",
        action = %STAGE_NAMES[0],
        dcc = %"bench"
    );
    let _enter = span.enter();
    let guard = recorder.start("record_telemetry_inner", "bench");
    let slug = workload.request["params"]["name"]
        .as_str()
        .unwrap_or_default();
    let ok = black_box(!slug.is_empty());
    guard.finish(ok);
    ok
}

/// Parsed command line options.
#[derive(Debug)]
struct Options {
    /// Measured iterations per stage.
    iterations: usize,
    /// Discarded warmup iterations per stage.
    warmup: usize,
    /// Output path for the raw report.
    output: String,
    /// Selected stage names; empty means "every stage".
    stages: Vec<String>,
}

impl Options {
    /// Parse `argv`, falling back to documented defaults.
    fn parse() -> Result<Options, String> {
        let mut iterations = DEFAULT_ITERATIONS;
        let mut warmup = DEFAULT_WARMUP;
        let mut output = DEFAULT_OUTPUT.to_string();
        let mut stages: Vec<String> = Vec::new();

        let mut args = std::env::args().skip(1);
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--help" | "-h" => return Err(String::new()),
                "--iterations" => {
                    iterations = Options::value(&mut args, "--iterations")?
                        .parse()
                        .map_err(|e: std::num::ParseIntError| format!("--iterations: {e}"))?;
                }
                "--warmup" => {
                    warmup = Options::value(&mut args, "--warmup")?
                        .parse()
                        .map_err(|e: std::num::ParseIntError| format!("--warmup: {e}"))?;
                }
                "--output" => output = Options::value(&mut args, "--output")?,
                "--stages" => {
                    stages = Options::value(&mut args, "--stages")?
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect();
                }
                other => return Err(format!("unknown argument: {other}")),
            }
        }

        Ok(Options {
            iterations,
            warmup,
            output,
            stages,
        })
    }

    /// Consume the value that belongs to `flag`.
    fn value(args: &mut impl Iterator<Item = String>, flag: &str) -> Result<String, String> {
        args.next()
            .ok_or_else(|| format!("{flag} requires a value"))
    }
}

/// Print usage information.
fn print_usage() {
    eprintln!(
        "usage: pipeline_bench [--iterations N] [--warmup N] [--output PATH] [--stages a,b,c]\n\
         \n\
         defaults: iterations={DEFAULT_ITERATIONS} warmup={DEFAULT_WARMUP} output={DEFAULT_OUTPUT}\n\
         stages:   {}",
        STAGE_NAMES.join(", ")
    );
}

fn main() {
    let options = match Options::parse() {
        Ok(options) => options,
        Err(message) => {
            if !message.is_empty() {
                eprintln!("pipeline_bench: {message}");
            }
            print_usage();
            std::process::exit(2);
        }
    };

    if !provider::is_initialized() {
        let cfg = TelemetryConfig::builder("pipeline-bench")
            .with_noop_exporter()
            .build();
        if let Err(e) = provider::init(&cfg) {
            eprintln!("pipeline_bench: telemetry init failed: {e}");
            std::process::exit(1);
        }
    }

    let workload = Workload::new();
    let recorder = ToolRecorder::new("pipeline-bench");

    let stages: Vec<Stage> = Stage::all()
        .into_iter()
        .filter(|stage| options.stages.is_empty() || options.stages.iter().any(|s| s == stage.name))
        .collect();
    if stages.is_empty() {
        eprintln!(
            "pipeline_bench: no stages selected; available stages: {}",
            STAGE_NAMES.join(", ")
        );
        std::process::exit(2);
    }

    for stage in &stages {
        for _ in 0..options.warmup {
            let _ = black_box((stage.run)(&workload));
        }
        for _ in 0..options.iterations {
            let guard = recorder.start(stage.name, "bench");
            let ok = (stage.run)(&workload);
            guard.finish(ok);
        }
    }

    let mut metrics = recorder.all_metrics();
    metrics.retain(|m| stages.iter().any(|s| s.name == m.action_name));
    metrics.sort_by(|a, b| a.action_name.cmp(&b.action_name));

    let report = BenchReport::from_metrics(metrics.iter());
    if let Err(e) = report.write_json(&options.output) {
        eprintln!("pipeline_bench: failed to write {}: {e}", options.output);
        std::process::exit(1);
    }

    println!(
        "pipeline_bench: {} stage(s) x {} iteration(s) -> {}",
        report.stages.len(),
        options.iterations,
        options.output
    );
    for stage in &report.stages {
        println!(
            "  {:<20} p95={:>8.3} ms  p99={:>8.3} ms  avg={:>8.3} ms  success_rate={:.3}",
            stage.stage,
            stage.p95_duration_ms,
            stage.p99_duration_ms,
            stage.avg_duration_ms,
            stage.success_rate
        );
    }
}
