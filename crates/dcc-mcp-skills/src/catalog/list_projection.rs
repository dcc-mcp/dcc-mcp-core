//! Progressive `list_skills` projection (issue #995 / #582).

use serde_json::{Map, Value, json};

use super::SkillSummary;

/// Default page size when the caller passes no `limit`.
///
/// Aligned with the search path's `dcc_mcp_gateway_search::query::DEFAULT_LIMIT`
/// so browsing and searching hand back comparable page sizes (PIP-3407).
pub const DEFAULT_LIST_SKILLS_LIMIT: usize = 25;
/// Hard cap for `limit`.
///
/// Kept below the search path's `MAX_LIMIT` (100) on purpose: a `list_skills`
/// row is far heavier than a search hit (it carries `summary`, `stage` and
/// `tool_count`), so the same cap would cost several times more context.
pub const MAX_LIST_SKILLS_LIMIT: usize = 50;
/// Default truncation length for `summary` / compact `description`.
///
/// Unchanged by PIP-3407: trimming it to 120 chars measured a further ~21%
/// saving per page, but a clipped one-liner pushes the agent into an extra
/// `get_skill_info` round trip to choose a skill, and that round trip costs
/// far more than the ~500 tokens saved. Bounding the page is the fix; the
/// per-row width stays readable.
pub const DEFAULT_SUMMARY_CHARS: usize = 200;

/// Fields included when `fields` is omitted (compact mode).
///
/// Slimmed in PIP-3407: this payload is fetched on nearly every discovery
/// turn and its cost multiplies with the number of live DCC hosts, so the
/// default carries only what an agent needs to *pick* a skill — identity
/// (`name`, `dcc`), a one-line `summary`, and the cheap routing signals
/// (`tool_count`, `status`, `stage`). Everything else stays reachable through
/// the `fields` allow-list or `get_skill_info`.
///
/// `loaded` is part of the routing signals, not the heavy tail: it is the
/// one field clients branch on without a second call (`list_skills` is the
/// progressive-loading contract, see
/// `tests/test_mcp_mcpcall_e2e.py::TestMcpcallProgressiveLoading`). It is
/// redundant with `status == "loaded"` but was already public before
/// PIP-3407, so dropping it silently here broke that contract. It costs one
/// boolean per row (~4 tokens); the 13-field legacy set it replaced is still
/// ~20% heavier.
const COMPACT_FIELDS: &[&str] = &[
    "name",
    "dcc",
    "summary",
    "tool_count",
    "status",
    "loaded",
    "stage",
    "missing_dependencies",
];

/// Every field the projection can emit.
///
/// Used by the gateway's fan-out so each backend returns every column of its
/// catalogue: paging has to happen once, on the merged result, otherwise
/// every host applies `offset` to its own list and pages overlap (PIP-3407).
/// The gateway walks each host with bounded pages of
/// [`MAX_LIST_SKILLS_LIMIT`] rows and follows `next_offset`.
pub const ALL_LIST_SKILLS_FIELDS: &[&str] = &[
    "name",
    "description",
    "summary",
    "search_hint",
    "tags",
    "dcc",
    "version",
    "tool_count",
    "tool_names",
    "loaded",
    "status",
    "missing_dependencies",
    "scope",
    "implicit_invocation",
    "layer",
    "stage",
    "runtime",
    "runtime_state",
];

fn truncate_chars(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    s.chars().take(max_chars).collect()
}

fn parse_fields(args: &Value) -> Vec<String> {
    args.get("fields")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| COMPACT_FIELDS.iter().map(|s| (*s).to_string()).collect())
}

fn parse_offset(args: &Value) -> usize {
    args.get("offset").and_then(Value::as_u64).unwrap_or(0) as usize
}

/// Resolve the effective page size.
///
/// A missing (or zero) `limit` falls back to [`DEFAULT_LIST_SKILLS_LIMIT`]
/// instead of returning the whole catalogue: `list_skills` fans out across
/// every live DCC host, so an unbounded default grows the tool result with
/// the size of the whole studio (PIP-3407). `0` is treated as "unset" to
/// match the search path (`SearchQuery::limit`).
///
/// There is deliberately no opt-out. The gateway fan-out used to pass an
/// `unbounded` argument so each host would hand over its whole catalogue in
/// one hop, but that argument reached `tools/call` from any MCP client too,
/// letting an ordinary caller defeat the very bound this module exists to
/// enforce. The gateway now walks each host with bounded pages instead.
fn parse_limit(args: &Value) -> usize {
    match args.get("limit").and_then(Value::as_u64) {
        None | Some(0) => DEFAULT_LIST_SKILLS_LIMIT,
        Some(n) => (n as usize).min(MAX_LIST_SKILLS_LIMIT),
    }
}

fn project_summary(summary: &SkillSummary, fields: &[String]) -> Value {
    let mut obj = Map::new();
    for field in fields {
        match field.as_str() {
            "name" => {
                obj.insert("name".into(), json!(summary.name));
            }
            "description" => {
                obj.insert("description".into(), json!(summary.description));
            }
            "summary" => {
                obj.insert(
                    "summary".into(),
                    json!(truncate_chars(&summary.description, DEFAULT_SUMMARY_CHARS)),
                );
            }
            "search_hint" => {
                obj.insert("search_hint".into(), json!(summary.search_hint));
            }
            "tags" => {
                obj.insert("tags".into(), json!(summary.tags));
            }
            "dcc" => {
                obj.insert("dcc".into(), json!(summary.dcc));
            }
            "version" => {
                obj.insert("version".into(), json!(summary.version));
            }
            "tool_count" => {
                obj.insert("tool_count".into(), json!(summary.tool_count));
            }
            "tool_names" => {
                obj.insert("tool_names".into(), json!(summary.tool_names.join(",")));
            }
            "loaded" => {
                obj.insert("loaded".into(), json!(summary.loaded));
            }
            "status" => {
                obj.insert("status".into(), json!(summary.status));
            }
            "missing_dependencies" if !summary.missing_dependencies.is_empty() => {
                obj.insert(
                    "missing_dependencies".into(),
                    json!(summary.missing_dependencies),
                );
            }
            "scope" => {
                obj.insert("scope".into(), json!(summary.scope));
            }
            "implicit_invocation" => {
                obj.insert(
                    "implicit_invocation".into(),
                    json!(summary.implicit_invocation),
                );
            }
            "layer" => {
                if let Some(layer) = &summary.layer {
                    obj.insert("layer".into(), json!(layer));
                }
            }
            "stage" => {
                if let Some(stage) = &summary.stage {
                    obj.insert("stage".into(), json!(stage));
                }
            }
            "runtime" => {
                if let Some(runtime) = &summary.runtime {
                    obj.insert("runtime".into(), json!(runtime));
                }
            }
            "runtime_state" => {
                if let Some(runtime) = &summary.runtime {
                    obj.insert("runtime_state".into(), json!(runtime.state));
                }
            }
            _ => {}
        }
    }
    Value::Object(obj)
}

/// Build the wire payload for `list_skills` / `POST /v1/list_skills`.
pub fn build_list_skills_response(mut summaries: Vec<SkillSummary>, args: &Value) -> Value {
    summaries.sort_by(|a, b| a.name.cmp(&b.name));
    let total = summaries.len();
    let offset = parse_offset(args).min(total);
    let limit = parse_limit(args);
    let fields = parse_fields(args);

    let end = offset.saturating_add(limit).min(total);
    let page: Vec<SkillSummary> = summaries[offset..end].to_vec();
    let next_offset = (end < total).then_some(end);

    let skills: Vec<Value> = page.iter().map(|s| project_summary(s, &fields)).collect();

    let mut payload = json!({
        "skills": skills,
        "total": total,
        "limit": limit,
        "offset": offset,
        "truncated": next_offset.is_some(),
    });
    if let (Some(next_offset), Some(obj)) = (next_offset, payload.as_object_mut()) {
        obj.insert("next_offset".into(), json!(next_offset));
        obj.insert(
            "next_step".into(),
            json!(format!(
                "More skills available: call list_skills with offset={next_offset} limit={limit}. Prefer search_skills with a query to jump straight to a skill by intent."
            )),
        );
    }
    payload
}

/// Re-project an aggregated gateway payload (`skills` array of objects).
pub fn project_list_skills_payload(mut payload: Value, args: &Value) -> Value {
    let Some(skills) = payload.get_mut("skills").and_then(Value::as_array_mut) else {
        return payload;
    };
    let summaries: Vec<SkillSummary> = skills.iter().filter_map(skill_summary_from_value).collect();
    let mut projected = build_list_skills_response(summaries, args);
    if let Some(instances) = payload.get("instances") {
        projected
            .as_object_mut()
            .map(|obj| obj.insert("instances".into(), instances.clone()));
    }
    if let Some(skipped_count) = payload.get("skipped_count") {
        projected
            .as_object_mut()
            .map(|obj| obj.insert("skipped_count".into(), skipped_count.clone()));
    }
    if let Some(skipped) = payload.get("skipped") {
        projected
            .as_object_mut()
            .map(|obj| obj.insert("skipped".into(), skipped.clone()));
    }
    projected
}

fn skill_summary_from_value(v: &Value) -> Option<SkillSummary> {
    let name = v.get("name")?.as_str()?.to_string();
    Some(SkillSummary {
        name,
        description: v
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        search_hint: v
            .get("search_hint")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        tags: v
            .get("tags")
            .and_then(|t| t.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default(),
        dcc: v
            .get("dcc")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        version: v
            .get("version")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        tool_count: v.get("tool_count").and_then(Value::as_u64).unwrap_or(0) as usize,
        tool_names: v
            .get("tool_names")
            .and_then(Value::as_str)
            .map(|s| s.split(',').map(str::to_string).collect())
            .unwrap_or_default(),
        loaded: v.get("loaded").and_then(Value::as_bool).unwrap_or(false),
        status: v
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or_else(|| {
                if v.get("loaded").and_then(Value::as_bool).unwrap_or(false) {
                    "loaded"
                } else {
                    "discovered"
                }
            })
            .to_string(),
        missing_dependencies: v
            .get("missing_dependencies")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default(),
        scope: v
            .get("scope")
            .and_then(Value::as_str)
            .unwrap_or("repo")
            .to_string(),
        path_source: v
            .get("path_source")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string(),
        implicit_invocation: v
            .get("implicit_invocation")
            .and_then(Value::as_bool)
            .unwrap_or(true),
        layer: v.get("layer").and_then(Value::as_str).map(str::to_string),
        stage: v.get("stage").and_then(Value::as_str).map(str::to_string),
        runtime: v
            .get("runtime")
            .cloned()
            .and_then(|value| serde_json::from_value(value).ok()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_summary(name: &str) -> SkillSummary {
        SkillSummary {
            name: name.to_string(),
            description: "x".repeat(500),
            search_hint: "hint".to_string(),
            tags: vec!["t".to_string()],
            dcc: "maya".to_string(),
            version: "0.0.0".to_string(),
            tool_count: 3,
            tool_names: vec!["a".to_string(), "b".to_string()],
            loaded: false,
            status: "discovered".to_string(),
            missing_dependencies: Vec::new(),
            scope: "repo".to_string(),
            path_source: "unknown".to_string(),
            implicit_invocation: true,
            layer: None,
            stage: Some("scene".to_string()),
            runtime: None,
        }
    }

    #[test]
    fn compact_default_omits_heavy_fields() {
        let summaries = vec![sample_summary("alpha")];
        let payload = build_list_skills_response(summaries, &json!({}));
        let skill = &payload["skills"][0];
        assert!(skill.get("description").is_none());
        assert!(skill.get("search_hint").is_none());
        assert!(skill.get("tool_names").is_none());
        assert!(skill.get("tags").is_none());
        assert_eq!(payload["truncated"], false);
        // Low-signal fields stay out of the default page (PIP-3407).
        assert!(skill.get("version").is_none());
        assert!(skill.get("scope").is_none());
        assert!(skill.get("layer").is_none());
        assert!(skill.get("runtime_state").is_none());
        assert!(skill.get("tool_names").is_none());
    }

    #[test]
    fn compact_default_keeps_the_loaded_flag() {
        // `loaded` is the progressive-loading contract: clients branch on it
        // from `list_skills` without a follow-up call, so slimming the
        // projection may not drop it. It must agree with `status`.
        let mut loaded = sample_summary("loaded-skill");
        loaded.loaded = true;
        loaded.status = "loaded".to_string();
        let mut idle = sample_summary("idle-skill");
        idle.loaded = false;
        idle.status = "discovered".to_string();

        let payload = build_list_skills_response(vec![loaded, idle], &json!({}));
        let skills = payload["skills"].as_array().unwrap();

        for skill in skills {
            let name = skill["name"].as_str().unwrap();
            let status = skill["status"].as_str().unwrap();
            let loaded = skill
                .get("loaded")
                .and_then(Value::as_bool)
                .unwrap_or_else(|| panic!("compact row {name} is missing 'loaded': {skill}"));
            assert_eq!(
                loaded,
                status == "loaded",
                "{name}: loaded={loaded} disagrees with status={status}"
            );
        }
        // Rows come back sorted by name, so look each one up rather than
        // relying on the order they were passed in.
        let flag = |name: &str| {
            skills
                .iter()
                .find(|s| s["name"] == name)
                .unwrap_or_else(|| panic!("{name} missing from the page"))["loaded"]
                .as_bool()
                .unwrap()
        };
        assert_eq!(flag("loaded-skill"), true);
        assert_eq!(flag("idle-skill"), false);
    }

    #[test]
    fn default_page_is_bounded_without_limit() {
        let summaries: Vec<SkillSummary> = (0..100)
            .map(|i| sample_summary(&format!("skill-{i:03}")))
            .collect();
        let payload = build_list_skills_response(summaries, &json!({}));
        assert_eq!(
            payload["skills"].as_array().unwrap().len(),
            DEFAULT_LIST_SKILLS_LIMIT
        );
        assert_eq!(payload["limit"], DEFAULT_LIST_SKILLS_LIMIT);
        assert_eq!(payload["total"], 100);
        assert_eq!(payload["truncated"], true);
        assert_eq!(payload["next_offset"], DEFAULT_LIST_SKILLS_LIMIT);
        assert!(payload["next_step"].as_str().unwrap().contains("offset=25"));
    }

    #[test]
    fn limit_zero_falls_back_to_default_page() {
        let summaries: Vec<SkillSummary> = (0..3)
            .map(|i| sample_summary(&format!("skill-{i}")))
            .collect();
        let payload = build_list_skills_response(summaries, &json!({"limit": 0}));
        assert_eq!(payload["skills"].as_array().unwrap().len(), 3);
        assert_eq!(payload["truncated"], false);
        assert!(payload.get("next_offset").is_none());
    }

    #[test]
    fn no_argument_can_defeat_the_page_bound() {
        // The bound is the whole point of this module, so it must hold for
        // every argument combination — including undocumented ones. An
        // `unbounded` argument used to be honoured here and let any MCP
        // client pull the entire catalogue in one call.
        let summaries: Vec<SkillSummary> = (0..70)
            .map(|i| sample_summary(&format!("skill-{i:03}")))
            .collect();
        // (arguments, expected page size) — an explicit `limit` is clamped to
        // the hard cap, anything else falls back to the default page.
        for (bypass, expected) in [
            (json!({"unbounded": true}), DEFAULT_LIST_SKILLS_LIMIT),
            (
                json!({"unbounded": true, "limit": 10_000}),
                MAX_LIST_SKILLS_LIMIT,
            ),
            (json!({"limit": 10_000}), MAX_LIST_SKILLS_LIMIT),
            (
                json!({"unbounded": true, "fields": ALL_LIST_SKILLS_FIELDS}),
                DEFAULT_LIST_SKILLS_LIMIT,
            ),
        ] {
            let payload = build_list_skills_response(summaries.clone(), &bypass);
            let rows = payload["skills"].as_array().unwrap().len();
            assert_eq!(
                rows, expected,
                "{bypass} produced {rows} rows, escaping the page bound"
            );
            assert!(rows < 70, "{bypass} returned the whole catalogue");
            assert_eq!(payload["truncated"], true, "{bypass} hid the truncation");
            assert_eq!(payload["next_offset"], expected);
        }
    }

    #[test]
    fn pages_walk_the_whole_catalogue() {
        // The gateway walks each host page by page, so a full traversal must
        // yield every skill exactly once and carry every projected column.
        let summaries: Vec<SkillSummary> = (0..70)
            .map(|i| sample_summary(&format!("skill-{i:03}")))
            .collect();
        let mut offset = 0usize;
        let mut seen: Vec<String> = Vec::new();
        loop {
            let payload = build_list_skills_response(
                summaries.clone(),
                &json!({
                    "offset": offset,
                    "limit": MAX_LIST_SKILLS_LIMIT,
                    "fields": ALL_LIST_SKILLS_FIELDS,
                }),
            );
            for row in payload["skills"].as_array().unwrap() {
                for field in ["name", "dcc", "summary", "description", "tool_names"] {
                    assert!(row.get(field).is_some(), "missing {field} in {row}");
                }
                seen.push(row["name"].as_str().unwrap().to_string());
            }
            match payload.get("next_offset").and_then(Value::as_u64) {
                Some(next) => {
                    let next = next as usize;
                    assert!(next > offset, "next_offset did not advance");
                    offset = next;
                }
                None => break,
            }
        }
        assert_eq!(seen.len(), 70);
        let unique: std::collections::BTreeSet<_> = seen.iter().collect();
        assert_eq!(unique.len(), 70, "traversal repeated or dropped skills");
    }

    #[test]
    fn limit_is_clamped_to_the_hard_max() {
        let summaries: Vec<SkillSummary> = (0..200)
            .map(|i| sample_summary(&format!("skill-{i:03}")))
            .collect();
        let payload = build_list_skills_response(summaries, &json!({"limit": 10_000}));
        assert_eq!(payload["limit"], MAX_LIST_SKILLS_LIMIT);
        assert_eq!(
            payload["skills"].as_array().unwrap().len(),
            MAX_LIST_SKILLS_LIMIT
        );
    }

    #[test]
    fn pages_are_disjoint_and_cover_the_catalogue() {
        let summaries: Vec<SkillSummary> = (0..47)
            .map(|i| sample_summary(&format!("skill-{i:03}")))
            .collect();
        let mut seen: Vec<String> = Vec::new();
        let mut offset = 0usize;
        loop {
            let payload = build_list_skills_response(summaries.clone(), &json!({"offset": offset}));
            let names: Vec<String> = payload["skills"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|v| v.get("name").and_then(Value::as_str))
                .map(str::to_string)
                .collect();
            assert!(!names.is_empty());
            seen.extend(names);
            match payload.get("next_offset").and_then(Value::as_u64) {
                Some(next) => offset = next as usize,
                None => break,
            }
        }
        assert_eq!(seen.len(), 47, "paging lost or duplicated rows");
        let mut unique = seen.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), 47, "paging duplicated rows");
    }

    #[test]
    fn limit_and_offset_paginate() {
        let summaries: Vec<SkillSummary> = (0..5)
            .map(|i| sample_summary(&format!("skill-{i}")))
            .collect();
        let page_a =
            build_list_skills_response(summaries.clone(), &json!({"limit": 2, "offset": 0}));
        let page_b = build_list_skills_response(summaries, &json!({"limit": 2, "offset": 2}));
        assert_eq!(page_a["skills"].as_array().unwrap().len(), 2);
        assert_eq!(page_b["skills"].as_array().unwrap().len(), 2);
        assert_eq!(page_a["limit"], 2);
        assert_eq!(page_a["truncated"], true);
        assert_eq!(page_a["next_offset"], 2);
        assert_eq!(page_b["truncated"], true);
        assert_eq!(page_b["next_offset"], 4);
        let names_a: Vec<_> = page_a["skills"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|v| v.get("name").and_then(Value::as_str))
            .collect();
        let names_b: Vec<_> = page_b["skills"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|v| v.get("name").and_then(Value::as_str))
            .collect();
        assert!(names_a.iter().all(|n| !names_b.contains(n)));
    }

    #[test]
    fn fields_selector_is_strict_allow_list() {
        let summaries = vec![sample_summary("only-name")];
        let payload = build_list_skills_response(summaries, &json!({"fields": ["name"]}));
        let skill = &payload["skills"][0];
        assert_eq!(skill.get("name").and_then(Value::as_str), Some("only-name"));
        assert!(skill.get("description").is_none());
        assert!(skill.get("summary").is_none());
    }
}
