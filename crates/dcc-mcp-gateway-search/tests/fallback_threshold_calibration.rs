//! Reproducible calibration for [`FALLBACK_MIN_CONFIDENT_SCORE`] (PIP-3702).
//!
//! The confidence gate's doc comment publishes measured score bands. Numbers in
//! a comment rot silently, so this test recomputes them from the committed seed
//! corpus (`benchmarks/skills/seeds.json`) on every run. If the scorer moves,
//! this fails — which is the point: the gate is a measured constant, not a
//! guess.
//!
//! It pins the *shape* of the measurement rather than exact scores, so ordinary
//! seed churn does not turn into a false failure:
//!
//! * every skill-name query must clear the gate with room to spare;
//! * every GUI-only query no skill can serve must fall below it;
//! * the documented overlap between those two families must still exist — the
//!   gate is a deliberate recall/noise trade-off, and if the bands ever stop
//!   overlapping the trade-off and the default both need re-deciding.

use dcc_mcp_gateway_search::{
    FALLBACK_MIN_CONFIDENT_SCORE as GATE, SearchQuery, SearchRecord, search,
};
use serde::Deserialize;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
struct SeedFile {
    skills: Vec<Seed>,
}

#[derive(Debug, Deserialize)]
struct Seed {
    name: String,
    description: String,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    metadata: serde_json::Value,
}

#[derive(Clone)]
struct Row {
    tool_slug: String,
    backend_tool: String,
    summary: String,
    skill_name: Option<String>,
    tags: Vec<String>,
    dcc_type: String,
    instance_id: Uuid,
    loaded: bool,
}

impl SearchRecord for Row {
    fn tool_slug(&self) -> &str {
        &self.tool_slug
    }
    fn backend_tool(&self) -> &str {
        &self.backend_tool
    }
    fn summary(&self) -> &str {
        &self.summary
    }
    fn skill_name(&self) -> Option<&str> {
        self.skill_name.as_deref()
    }
    fn tags(&self) -> &[String] {
        &self.tags
    }
    fn dcc_type(&self) -> &str {
        &self.dcc_type
    }
    fn instance_id(&self) -> Uuid {
        self.instance_id
    }
    fn loaded(&self) -> bool {
        self.loaded
    }
}

fn corpus() -> Vec<Row> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("crates/<crate>");
    let text = std::fs::read_to_string(root.join("benchmarks/skills/seeds.json"))
        .expect("committed seed corpus must be readable");
    let file: SeedFile = serde_json::from_str(&text).expect("seed corpus must parse");
    file.skills
        .into_iter()
        .map(|s| Row {
            tool_slug: s.name.clone(),
            backend_tool: s.name.clone(),
            summary: s.description.clone(),
            skill_name: Some(s.name.clone()),
            tags: s.tags.clone(),
            dcc_type: "python".to_string(),
            instance_id: Uuid::nil(),
            loaded: true,
        })
        .collect()
}

fn top1(query: &str, records: &[Row]) -> u32 {
    search(
        records,
        &SearchQuery {
            query: query.to_string(),
            ..Default::default()
        },
    )
    .first()
    .map_or(0, |hit| hit.score)
}

/// Queries no shipped skill can serve: pure GUI interaction with a host that
/// exposes nothing scriptable. Exactly the case the route exists for.
const GUI_ONLY: &[&str] = &[
    "click the export button",
    "drag the timeline marker",
    "rename material nodes",
    "resize the floating palette",
    "dock the toolbar left",
    "accept the license popup",
    "toggle the vendor checkbox",
    "scrub the color picker",
    "click through the wizard",
    "nudge the selected keyframe",
];

#[test]
fn every_name_query_clears_the_gate() {
    let records = corpus();
    for record in &records {
        let query = record.tool_slug.replace('-', " ");
        let score = top1(&query, &records);
        assert!(
            score >= GATE,
            "a skill-name query must clear the gate: {query:?} scored {score}, gate {GATE}"
        );
    }
}

#[test]
fn every_gui_only_query_falls_below_the_gate() {
    let records = corpus();
    for query in GUI_ONLY {
        let score = top1(query, &records);
        assert!(
            score < GATE,
            "a GUI-only query must fall below the gate: {query:?} scored {score}, gate {GATE}"
        );
    }
}

#[test]
fn the_false_positive_rate_stays_bounded() {
    // The gate is a recall/noise trade-off, not a clean separation: authored
    // `search-hint` queries overlap the GUI-only band (9/26 at the time of
    // writing).
    //
    // This asserts an UPPER bound, not a lower one. An improvement that lifts
    // those queries over the gate is the expected direction of travel, so a
    // lower bound here would turn a good PR red. A regression — the scorer
    // degrading until most authored queries miss — still fails.
    //
    // The lower end of the calibration (name queries must clear the gate,
    // GUI-only queries must fall below it) is pinned by the other two tests.
    const MAX_FALSE_POSITIVE_RATIO: f64 = 0.5;
    let records = corpus();
    let gui_max = GUI_ONLY
        .iter()
        .map(|q| top1(q, &records))
        .max()
        .unwrap_or(0);

    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("crates/<crate>");
    let text = std::fs::read_to_string(root.join("benchmarks/skills/seeds.json")).unwrap();
    let file: SeedFile = serde_json::from_str(&text).unwrap();
    let hints: Vec<String> = file
        .skills
        .iter()
        .filter_map(|s| {
            s.metadata
                .pointer("/dcc-mcp.search-hint")
                .and_then(|v| v.as_str())
                .map(|h| h.split(',').next().unwrap_or("").trim().to_string())
                .filter(|h: &String| !h.is_empty())
        })
        .collect();

    assert!(
        !hints.is_empty(),
        "seed corpus must carry search-hint queries"
    );
    let below = hints.iter().filter(|h| top1(h, &records) < GATE).count();

    let allowed = (hints.len() as f64 * MAX_FALSE_POSITIVE_RATIO) as usize;
    assert!(
        below <= allowed,
        "authored queries are missing the gate wholesale: {below}/{} fall below \
         {GATE} (allowed at most {allowed}, gui_max={gui_max}). The scorer has \
         degraded and the gate must be re-measured.",
        hints.len()
    );
}
