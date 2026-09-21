//! Benchmark scaffolding for the workflow executor.
//!
//! Everything in this module exists to make pipeline benchmarks
//! reproducible **without a DCC host**: [`FakeToolCaller`] stands in for
//! the real `ToolDispatcher`, so a run measures the executor instead of a
//! Maya / Blender / Houdini workstation and produces the same numbers in
//! CI as on a laptop.
//!
//! Two entry points consume it:
//!
//! * `tests/pipeline_bench.rs` — the regression harness that gates per-stage
//!   p95 jitter.
//! * `benches/pipeline_full.rs` — a criterion benchmark producing the
//!   throughput trend.
//!
//! Neither is a PR merge gate on its own; see `.github/workflows`.
//!
//! # Why this is a public module and not feature-gated
//!
//! Both consumers are separate compilation units (an integration test and a
//! criterion bench), so they can only reach this code through the crate's
//! public API. Hiding it behind a cargo feature does not work: enabling a
//! feature of the crate *under test* from its own dev-dependencies requires
//! a self-referential dev-dependency, which is fragile and upsets feature
//! unification.
//!
//! The cost is API surface, and it is small: nothing in the production
//! executor references this module, so a release build that never calls it
//! gets it eliminated as dead code. The upside is that the scenario and the
//! latency model are shared verbatim between the regression harness and the
//! benchmark, so the two cannot drift apart.

pub mod fake_caller;

pub use fake_caller::{
    CallOutcome, CallRecord, FakeCallerConfig, FakeToolCaller, LatencyModel, StageStats, summarise,
};

/// The fixed six-stage scenario (`model → rig → animate → texture → render
/// → composite`), embedded so the test and the benchmark cannot drift apart
/// by reading different files.
///
/// The authoritative copy lives at `crates/dcc-mcp-workflow/scenarios/
/// pipeline_full.yaml`; `include_str!` is what keeps the two in sync.
pub const PIPELINE_SCENARIO: &str = include_str!("../../scenarios/pipeline_full.yaml");

/// Stage tool names of [`PIPELINE_SCENARIO`], in execution order.
///
/// Used by the harness to assert the scenario still contains exactly the
/// stages the latency table is keyed on.
pub const PIPELINE_STAGES: [&str; 6] = [
    "bench_stage_model",
    "bench_stage_rig",
    "bench_stage_animate",
    "bench_stage_texture",
    "bench_stage_render",
    "bench_stage_composite",
];

/// Parse [`PIPELINE_SCENARIO`] into a validated [`WorkflowSpec`].
///
/// # Panics
///
/// Panics if the embedded scenario stops parsing or validating — that is a
/// build-time regression in a checked-in file, not a runtime condition.
///
/// [`WorkflowSpec`]: crate::spec::WorkflowSpec
#[must_use]
pub fn pipeline_spec() -> crate::spec::WorkflowSpec {
    let spec = crate::spec::WorkflowSpec::from_yaml(PIPELINE_SCENARIO)
        .expect("scenarios/pipeline_full.yaml must parse");
    spec.validate()
        .expect("scenarios/pipeline_full.yaml must validate");
    spec
}
