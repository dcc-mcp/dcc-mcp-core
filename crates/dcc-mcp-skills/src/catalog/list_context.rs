//! Context-budget measurement harness for `list_skills` (issue PIP-3407).
//!
//! `list_skills` is the one discovery surface whose payload grows with the
//! number of live DCC hosts: the gateway fans the call out to every instance
//! and merges the per-host `skills` arrays into a single payload, so a studio
//! running Maya + Blender + Houdini + 3ds Max + ZBrush + Photoshop + Unreal
//! ships every skill of every host in one tool result.
//!
//! This module makes that cost measurable without a live DCC farm:
//!
//! * [`synthetic_summaries`] builds a deterministic, realistic catalogue for a
//!   DCC host (field widths calibrated against the shipped skills in this
//!   repository — see [`DESCRIPTION_CHARS`] and [`TOOLS_PER_SKILL`]).
//! * [`measure`] runs those summaries through the real projection
//!   ([`super::list_projection::build_list_skills_response`]) and reports the
//!   page's byte/token cost on the exact wire format the handlers emit
//!   (`serde_json::to_string_pretty`).
//! * [`paged_walk`] walks every page so "paging still reaches everything" is a
//!   measurable claim rather than an assertion.
//!
//! Run the human-readable table with:
//!
//! ```text
//! cargo run --example list_skills_context -p dcc-mcp-skills
//! ```

use serde_json::{Value, json};

use super::list_projection::build_list_skills_response;
use super::types::SkillSummary;

/// Identifier of the deterministic byte-based tokenizer estimate used here.
///
/// Deliberately the same estimator the gateway records on admin traces
/// (`dcc_mcp_gateway_admin::estimate_tokens`, `TOKEN_ESTIMATOR`): one
/// estimator for the whole product so context-budget numbers stay comparable
/// between the gateway trace view and this harness. It is a byte heuristic,
/// not a real BPE tokenizer — treat it as ±10%.
pub const TOKEN_ESTIMATOR_ID: &str = "dcc-mcp-byte4-v1";

/// Estimated tokens for `bytes` of wire payload.
///
/// Mirrors `dcc_mcp_gateway_admin::estimate_tokens` (`len.div_ceil(4)`) so the
/// harness and the product report the same number for the same payload.
#[must_use]
pub fn estimate_tokens(bytes: usize) -> usize {
    bytes.div_ceil(4)
}

/// Mean `description` length of the shipped skills in this repository
/// (12 skills measured: mean 359, median 350, min 168, max 524 chars).
pub const DESCRIPTION_CHARS: usize = 350;
/// Mean tool count of the shipped `tools.yaml` files in this repository
/// (10 files measured: mean 3.3, max 8).
pub const TOOLS_PER_SKILL: usize = 3;

/// Shipped-skill counts per DCC host, measured at upstream HEAD (2026-09-22,
/// `src/` catalogues only — mirrored copies under `.claude`/`.cursor`/`.cline`
/// agent directories excluded).
///
/// Ordered as the seven-host studio scenario used for the context-budget
/// regression test; total 167 skills.
pub const SEVEN_DCC_COUNTS: [(&str, usize); 7] = [
    ("maya", 29),
    ("blender", 36),
    ("houdini", 43),
    ("3dsmax", 20),
    ("zbrush", 7),
    ("photoshop", 10),
    ("unreal", 22),
];

const NAME_WORDS: &[&str] = &[
    "mesh", "curve", "rig", "shader", "light", "camera", "cloth", "fluid", "uv", "bake", "scene",
    "asset", "layer", "shelf", "cache", "proxy", "sculpt", "retopo", "fx", "render",
];

const DESCRIPTION_WORDS: &[&str] = &[
    "create",
    "edit",
    "manage",
    "process",
    "export",
    "import",
    "generate",
    "apply",
    "transform",
    "analyse",
    "compute",
    "render",
    "polygon",
    "mesh",
    "curve",
    "surface",
    "volume",
    "light",
    "camera",
    "material",
    "texture",
    "shader",
    "bone",
    "skin",
    "blend",
    "shape",
    "morph",
    "deform",
    "simulate",
    "bake",
];

const TAGS: &[&str] = &[
    "modeling",
    "rigging",
    "animation",
    "rendering",
    "texturing",
    "lighting",
    "simulation",
    "fx",
    "layout",
    "pipeline",
];

const STAGES: &[&str] = &[
    "model", "rig", "lookdev", "layout", "fx", "lighting", "render",
];

/// Deterministic xorshift64* generator — the fixture must be reproducible on
/// every machine and in CI, so no `rand` and no entropy source.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        // xorshift64* never escapes zero; guard the degenerate seed.
        Self(if seed == 0 {
            0x9E37_79B9_7F4A_7C15
        } else {
            seed
        })
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, bound: usize) -> usize {
        (self.next_u64() % bound as u64) as usize
    }

    fn pick(&mut self, pool: &[&str]) -> String {
        pool[self.below(pool.len())].to_string()
    }
}

/// Seed derived from a host name so each DCC host gets its own stable fixture.
fn seed_for(dcc: &str) -> u64 {
    let mut hash: u64 = 0xCBF2_9CE4_8422_2325;
    for byte in dcc.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x100_0000_01B3);
    }
    hash
}

/// Build `count` deterministic, realistic [`SkillSummary`] rows for one host.
///
/// Field widths follow the calibration constants above: descriptions are
/// [`DESCRIPTION_CHARS`] long (the projection truncates them to
/// `DEFAULT_SUMMARY_CHARS` in compact mode) and each skill declares
/// [`TOOLS_PER_SKILL`] tools.
#[must_use]
pub fn synthetic_summaries(dcc: &str, count: usize) -> Vec<SkillSummary> {
    let mut rng = Rng::new(seed_for(dcc));
    (0..count)
        .map(|i| {
            let name = format!(
                "{}-{}-{}",
                dcc,
                rng.pick(NAME_WORDS),
                rng.pick(&["ops", "toolkit", "utils", "bridge", "export", "sync"])
            );
            let mut description = String::with_capacity(DESCRIPTION_CHARS);
            while description.len() < DESCRIPTION_CHARS {
                if !description.is_empty() {
                    description.push(' ');
                }
                description.push_str(&rng.pick(DESCRIPTION_WORDS));
            }
            let search_hint = (0..4)
                .map(|_| rng.pick(DESCRIPTION_WORDS))
                .collect::<Vec<_>>()
                .join(" ");
            let tool_names: Vec<String> = (0..TOOLS_PER_SKILL)
                .map(|t| format!("{}_{}_{}", dcc, NAME_WORDS[rng.below(NAME_WORDS.len())], t))
                .collect();
            SkillSummary {
                name: format!("{name}-{i:03}"),
                description,
                search_hint,
                tags: vec![rng.pick(TAGS), rng.pick(TAGS)],
                dcc: dcc.to_string(),
                version: "1.4.2".to_string(),
                tool_count: tool_names.len(),
                tool_names,
                loaded: i % 8 == 0,
                status: if i % 8 == 0 {
                    "loaded".to_string()
                } else {
                    "discovered".to_string()
                },
                missing_dependencies: Vec::new(),
                scope: "repo".to_string(),
                path_source: "project".to_string(),
                implicit_invocation: true,
                layer: Some(if i % 5 == 0 {
                    "infrastructure".to_string()
                } else {
                    "domain".to_string()
                }),
                stage: Some(rng.pick(STAGES)),
                runtime: None,
            }
        })
        .collect()
}

/// Build the merged catalogue for a scenario, sorted the way the projection
/// sorts it (`build_list_skills_response` sorts by name).
fn scenario_summaries(hosts: &[(&str, usize)]) -> Vec<SkillSummary> {
    let mut all: Vec<SkillSummary> = hosts
        .iter()
        .flat_map(|(dcc, count)| synthetic_summaries(dcc, *count))
        .collect();
    all.sort_by(|a, b| a.name.cmp(&b.name));
    all
}

/// One `list_skills` page, measured on the real projection and wire format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ListPageMeasurement {
    /// Number of live DCC hosts contributing to the merged catalogue.
    pub hosts: usize,
    /// Skills in the catalogue across all hosts (the `total` field).
    pub total: usize,
    /// Rows actually returned on this page.
    pub returned: usize,
    /// Bytes of the pretty-printed JSON payload the handlers emit.
    pub bytes: usize,
    /// [`estimate_tokens`] of `bytes`.
    pub tokens: usize,
    /// `truncated` flag from the payload.
    pub truncated: bool,
    /// `next_offset` from the payload (`None` when the page is the last one).
    pub next_offset: Option<usize>,
}

/// Measure a single `list_skills` call for `hosts` with arguments `args`.
///
/// `args` is the raw MCP/REST argument object, so `limit`, `offset` and
/// `fields` behave exactly as they do on the wire.
#[must_use]
pub fn measure(hosts: &[(&str, usize)], args: &Value) -> ListPageMeasurement {
    let summaries = scenario_summaries(hosts);
    let payload = build_list_skills_response(summaries, args);
    let text = serde_json::to_string_pretty(&payload).unwrap_or_default();
    let returned = payload
        .get("skills")
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or(0);
    ListPageMeasurement {
        hosts: hosts.len(),
        total: payload.get("total").and_then(Value::as_u64).unwrap_or(0) as usize,
        returned,
        bytes: text.len(),
        tokens: estimate_tokens(text.len()),
        truncated: payload
            .get("truncated")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        next_offset: payload
            .get("next_offset")
            .and_then(Value::as_u64)
            .map(|n| n as usize),
    }
}

/// Walk every page with the given page size and measure each one.
///
/// Used to prove that paginating still reaches the whole catalogue: the union
/// of `returned` over the walk must equal `total`.
#[must_use]
pub fn paged_walk(hosts: &[(&str, usize)], limit: usize) -> Vec<ListPageMeasurement> {
    let mut pages = Vec::new();
    let mut offset = 0usize;
    loop {
        let page = measure(hosts, &json!({"limit": limit, "offset": offset}));
        let returned = page.returned;
        let next = page.next_offset;
        pages.push(page);
        if returned == 0 {
            break;
        }
        match next {
            Some(next) if next > offset => offset = next,
            _ => break,
        }
    }
    pages
}

/// Trim [`SEVEN_DCC_COUNTS`] to its first `hosts` entries.
#[must_use]
pub fn seven_dcc_scenario(hosts: usize) -> Vec<(&'static str, usize)> {
    SEVEN_DCC_COUNTS
        .iter()
        .take(hosts)
        .map(|(dcc, count)| (*dcc, *count))
        .collect()
}

/// Upper bound a single default (`limit`-less) `list_skills` page may cost,
/// in estimated tokens, regardless of how many hosts are live.
///
/// Derived from the measurements in `examples/list_skills_context.rs` and
/// asserted by `tests/list_skills_context.rs`:
///
/// | page                                          | tokens |
/// |---|---|
/// | unbounded default page, 7 hosts (pre-PIP-3407) | 21,204 |
/// | 25-row default page, 7 hosts (this fixture)    |  2,398 |
/// | 25-row page, legacy 13-field projection        |  3,234 |
/// | 50-row page (hard cap)                         |  4,745 |
///
/// The fixture is an upper bound: every synthetic skill carries a
/// full-length description, so every `summary` hits the 200-character cap.
/// `3,000` leaves ~25% headroom over the measured default page for future
/// field additions while staying an order of magnitude below the unbounded
/// baseline. Raising it means either adding fields or accepting more context
/// per discovery turn — the number is a budget, not a measurement.
pub const MAX_DEFAULT_PAGE_TOKENS: usize = 3_000;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_is_deterministic() {
        let a = synthetic_summaries("maya", 5);
        let b = synthetic_summaries("maya", 5);
        assert_eq!(a.len(), 5);
        assert_eq!(a[0].name, b[0].name);
        assert_eq!(a[0].description, b[0].description);
        assert_ne!(a[0].name, synthetic_summaries("blender", 5)[0].name);
    }

    #[test]
    fn estimator_matches_gateway_byte4_convention() {
        assert_eq!(estimate_tokens(0), 0);
        assert_eq!(estimate_tokens(4), 1);
        assert_eq!(estimate_tokens(5), 2);
        assert_eq!(TOKEN_ESTIMATOR_ID, "dcc-mcp-byte4-v1");
    }

    #[test]
    fn seven_dcc_scenario_totals_167() {
        let all = seven_dcc_scenario(7);
        assert_eq!(all.iter().map(|(_, n)| n).sum::<usize>(), 167);
        assert_eq!(seven_dcc_scenario(1).len(), 1);
        assert_eq!(seven_dcc_scenario(99).len(), 7);
    }
}
