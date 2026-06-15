//! Progressive `list_skills` projection (issue #995 / #582).

use serde_json::{Map, Value, json};

use super::SkillSummary;

/// Default page size when callers pass an explicit `limit`.
pub const DEFAULT_LIST_SKILLS_LIMIT: usize = 10;
/// Hard cap for `limit`.
pub const MAX_LIST_SKILLS_LIMIT: usize = 50;
/// Default truncation length for `summary` / compact `description`.
pub const DEFAULT_SUMMARY_CHARS: usize = 200;

/// Fields included when `fields` is omitted (compact mode).
const COMPACT_FIELDS: &[&str] = &[
    "name",
    "stage",
    "tool_count",
    "loaded",
    "status",
    "missing_dependencies",
    "scope",
    "summary",
    "dcc",
    "version",
    "layer",
    "runtime_state",
    "implicit_invocation",
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

fn parse_limit(args: &Value) -> Option<usize> {
    args.get("limit").and_then(Value::as_u64).map(|n| {
        let n = n as usize;
        if n == 0 {
            0
        } else {
            n.min(MAX_LIST_SKILLS_LIMIT)
        }
    })
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

    let end = match limit {
        Some(lim) => (offset + lim).min(total),
        None => total,
    };
    let page: Vec<SkillSummary> = summaries[offset..end].to_vec();
    let truncated = limit.is_some() && end < total;
    let response_limit = limit.unwrap_or(page.len());

    let skills: Vec<Value> = page.iter().map(|s| project_summary(s, &fields)).collect();

    json!({
        "skills": skills,
        "total": total,
        "limit": response_limit,
        "offset": offset,
        "truncated": truncated,
    })
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
