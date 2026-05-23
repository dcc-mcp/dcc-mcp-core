//! Regression tests for issue #994 — search ranking must not let meta-tools
//! out-rank semantically relevant domain tools.
//!
//! The contract under test (paraphrased from #994):
//!
//!   For domain queries (e.g. "three point rig light", "playblast viewport
//!   capture"), the top-1 ranked hit MUST be a domain tool whose backend
//!   name reflects the query intent. Generic meta-tools (`project_*`,
//!   `recipes__*`, `dcc_capability_manifest`, `diagnostics__*`) MUST NOT
//!   appear in the top-3 simply because their long descriptions happen to
//!   contain query tokens.
//!
//! These tests build small in-memory snapshots and rank with the default
//! `FuzzyScorer` (`SearchMode::Fuzzy`). They will fail RED today and turn
//! GREEN when:
//!
//!   1. Backend / skill name token matches are weighted above
//!      summary/description token matches, OR
//!   2. Meta tools opt into a `tags: ["meta"]` (or equivalent) marker
//!      that the search engine excludes by default.
//!
//! Either implementation choice satisfies the user-visible contract.

use dcc_mcp_gateway_search::{SearchMode, SearchQuery, SearchRecord, search};
use uuid::Uuid;

/// Minimal in-test record so we don't depend on `dcc-mcp-gateway-core`.
#[derive(Clone)]
struct R {
    tool_slug: String,
    backend_tool: String,
    summary: String,
    skill_name: Option<String>,
    tags: Vec<String>,
    dcc_type: String,
    instance_id: Uuid,
    loaded: bool,
}

impl SearchRecord for R {
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

fn iid() -> Uuid {
    Uuid::from_u128(0xfeed_beef_0000_0000_0000_0000_0000_0001)
}

fn record(backend_tool: &str, skill_name: Option<&str>, summary: &str, tags: &[&str]) -> R {
    let iid = iid();
    R {
        tool_slug: format!("maya.feedbeef.{}", backend_tool),
        backend_tool: backend_tool.to_owned(),
        summary: summary.to_owned(),
        skill_name: skill_name.map(str::to_owned),
        tags: tags.iter().map(|t| (*t).to_owned()).collect(),
        dcc_type: "maya".to_owned(),
        instance_id: iid,
        loaded: true,
    }
}

/// The summaries here are paraphrased from real bundled skills so the
/// fuzzy scorer sees realistic token distributions, not synthetic strings.
fn build_realistic_snapshot() -> Vec<R> {
    vec![
        // --- Domain tools that should win --------------------------------
        record(
            "maya_light_rig__create_three_point_rig",
            Some("maya-light-rig"),
            "Create a three-point light rig in the current Maya scene.",
            &["maya", "lighting", "light-rig", "three-point"],
        ),
        record(
            "maya_render__playblast",
            Some("maya-render"),
            "Capture a viewport screenshot as a base64-encoded PNG via playblast.",
            &["maya", "render", "playblast", "viewport"],
        ),
        record(
            "maya_render__capture_viewport",
            Some("maya-render"),
            "Capture the active Maya viewport as a base64 PNG via playblast.",
            &["maya", "render", "viewport"],
        ),
        // --- Meta tools that must NOT dominate ---------------------------
        record(
            "project_resume",
            Some("project"),
            "Return the full resume payload for a scene: scene_path, loaded_assets, \
             active_skills, active_tool_groups, checkpoint_ids, and metadata.",
            &["project", "state"],
        ),
        record(
            "dcc_capability_manifest",
            None,
            "Return a compact Maya capability manifest listing every discoverable action.",
            &["capability", "manifest"],
        ),
        record(
            "recipes__list",
            Some("recipes"),
            "List available recipe anchors or structured domain recipes for a skill.",
            &["recipe"],
        ),
        record(
            "recipes__validate",
            Some("recipes"),
            "Validate candidate inputs against a structured recipe pack input schema.",
            &["recipe", "validation"],
        ),
        record(
            "diagnostics__screenshot",
            Some("diagnostics"),
            "Capture the DCC window (or full screen).",
            &["diagnostics", "screenshot"],
        ),
        record(
            "maya_scene__list_objects",
            Some("maya-scene"),
            "List objects in the current Maya scene.",
            &["maya", "scene"],
        ),
    ]
}

fn fuzzy(query: &str) -> SearchQuery {
    SearchQuery {
        query: query.to_owned(),
        mode: SearchMode::Fuzzy,
        limit: Some(5),
        ..Default::default()
    }
}

const META_TOOLS: &[&str] = &[
    "project_resume",
    "dcc_capability_manifest",
    "recipes__list",
    "recipes__validate",
    "diagnostics__screenshot",
];

fn assert_top1_is(snap: &[R], query: &str, expected_backend_tool: &str) {
    let hits = search(snap, &fuzzy(query));
    assert!(!hits.is_empty(), "query {:?} returned zero hits", query);
    let top = &hits[0].record;
    assert_eq!(
        top.backend_tool(),
        expected_backend_tool,
        "regression #994: query {:?} expected top-1 = {:?}, got = {:?} (score {})",
        query,
        expected_backend_tool,
        top.backend_tool(),
        hits[0].score
    );
}

fn assert_no_meta_tool_in_top_n(snap: &[R], query: &str, n: usize) {
    let hits = search(snap, &fuzzy(query));
    let top: Vec<&str> = hits
        .iter()
        .take(n)
        .map(|h| h.record.backend_tool())
        .collect();
    let leaks: Vec<&&str> = top
        .iter()
        .filter(|name| META_TOOLS.contains(name))
        .collect();
    assert!(
        leaks.is_empty(),
        "regression #994: meta-tools should not appear in top-{}: {:?} (full top: {:?})",
        n,
        leaks,
        top,
    );
}

// ── Tests ───────────────────────────────────────────────────────────────

#[test]
fn light_rig_query_picks_light_rig_tool_not_project_resume() {
    let snap = build_realistic_snapshot();
    assert_top1_is(
        &snap,
        "three point rig light",
        "maya_light_rig__create_three_point_rig",
    );
}

#[test]
fn light_rig_query_keeps_meta_tools_out_of_top3() {
    let snap = build_realistic_snapshot();
    assert_no_meta_tool_in_top_n(&snap, "three point rig light", 3);
}

#[test]
fn playblast_query_picks_a_render_tool() {
    let snap = build_realistic_snapshot();
    let hits = search(&snap, &fuzzy("playblast viewport capture"));
    assert!(!hits.is_empty(), "query returned zero hits");
    let top = hits[0].record.backend_tool();
    assert!(
        matches!(
            top,
            "maya_render__playblast" | "maya_render__capture_viewport"
        ),
        "regression #994: query 'playblast viewport capture' top-1 = {:?}; expected a maya_render__* tool",
        top
    );
}

#[test]
fn playblast_query_keeps_meta_tools_out_of_top3() {
    let snap = build_realistic_snapshot();
    assert_no_meta_tool_in_top_n(&snap, "playblast viewport capture", 3);
}

#[test]
fn create_query_keeps_meta_tools_out_of_top3() {
    let snap = build_realistic_snapshot();
    // Generic "create" with no other anchor — meta tools that mention
    // "create" / "Create" in their summary should still NOT win.
    assert_no_meta_tool_in_top_n(&snap, "create three point", 3);
}
