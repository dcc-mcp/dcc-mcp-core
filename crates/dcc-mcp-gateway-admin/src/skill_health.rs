//! Backend-neutral skill inventory and adoption projections.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use serde_json::{Value, json};

use crate::{AdminAuditRecord, DispatchTrace};

/// One gateway capability reduced to the fields needed by the admin projection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillCapabilityInput {
    pub tool_slug: String,
    pub backend_tool: String,
    pub skill_name: Option<String>,
    pub summary: String,
    pub dcc_type: String,
    pub instance_id: String,
    pub loaded: bool,
}

/// One search hit reduced to the fields needed by the admin projection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillSearchHitInput {
    pub tool_slug: String,
    pub skill_name: Option<String>,
    pub dcc_type: String,
    pub rank: u32,
}

/// One follow-up operation correlated with an admin-visible search.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillSearchFollowupInput {
    pub kind: String,
    pub timestamp_ms: u64,
    pub request_id: Option<String>,
    pub tool_slug: Option<String>,
    pub skill_name: Option<String>,
    pub selected_rank: Option<u32>,
    pub success: bool,
}

/// One search event reduced to the fields needed by the admin projection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillSearchInput {
    pub timestamp_ms: u64,
    pub hits: Vec<SkillSearchHitInput>,
    pub followups: Vec<SkillSearchFollowupInput>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct SkillKey {
    dcc_type: String,
    name: String,
}

impl SkillKey {
    fn new(dcc_type: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            dcc_type: dcc_type.into(),
            name: name.into(),
        }
    }
}

#[derive(Debug, Default)]
struct SkillAdoptionBuilder {
    search_hits: usize,
    rank_sum: u64,
    best_rank: Option<u32>,
    selected_count: usize,
    call_count: usize,
    failure_count: usize,
    load_error_count: usize,
    fallback_displaced_by_scripting: usize,
    last_searched_ms: Option<u64>,
    last_used_ms: Option<u64>,
    call_request_ids: HashSet<String>,
}

#[derive(Debug, Clone, Serialize)]
struct SkillAdoptionMetrics {
    search_hits: usize,
    best_rank: Option<u32>,
    average_rank: Option<f64>,
    selected_count: usize,
    call_count: usize,
    failure_count: usize,
    load_error_count: usize,
    last_searched: Option<String>,
    last_used: Option<String>,
    fallback_displaced_by_scripting: usize,
    searched: bool,
    used: bool,
    low_adoption: bool,
}

#[derive(Debug, Clone, Serialize)]
struct SkillHealthSummary {
    discovered_skill_roots: usize,
    loaded_skills: usize,
    unloaded_skills: usize,
    action_count: usize,
    searched_skills: usize,
    used_skills: usize,
    low_adoption_skills: usize,
    load_error_count: usize,
    missing_path_count: usize,
    path_redaction: &'static str,
}

/// Build the public admin skill inventory from backend-neutral projection inputs.
#[must_use]
pub fn skill_inventory_payload(
    records: Vec<SkillCapabilityInput>,
    searches: &[SkillSearchInput],
    audits: &[AdminAuditRecord],
    traces: &[DispatchTrace],
    discovered_skill_roots: usize,
    missing_path_count: usize,
) -> Value {
    let adoption = build_adoption_metrics(&records, searches, audits, traces);
    let mut grouped: BTreeMap<(String, String, bool), Vec<SkillCapabilityInput>> = BTreeMap::new();
    for record in records {
        let skill_name = record
            .skill_name
            .clone()
            .unwrap_or_else(|| record.backend_tool.clone());
        grouped
            .entry((record.dcc_type.clone(), skill_name, record.loaded))
            .or_default()
            .push(record);
    }

    let mut loaded = 0usize;
    let mut action_count = 0usize;
    let mut searched_skills = 0usize;
    let mut used_skills = 0usize;
    let mut low_adoption_skills = 0usize;
    let mut load_error_count = 0usize;

    let skills: Vec<Value> = grouped
        .into_iter()
        .map(|((dcc_type, name, is_loaded), records)| {
            if is_loaded {
                loaded += 1;
            }
            action_count += records.len();
            let mut instance_details = BTreeMap::new();
            for record in &records {
                let id = record.instance_id.clone();
                instance_details.entry(id.clone()).or_insert_with(|| {
                    json!({
                        "id": id,
                        "instance_id": record.instance_id,
                        "prefix": instance_short(&record.instance_id),
                        "instance_short": instance_short(&record.instance_id),
                        "dcc_type": record.dcc_type,
                    })
                });
            }
            let instances: BTreeSet<String> = instance_details
                .values()
                .filter_map(|value| value.get("instance_short").and_then(Value::as_str))
                .map(str::to_owned)
                .collect();
            let instance_ids: Vec<String> = instance_details.keys().cloned().collect();
            let instance_details: Vec<Value> = instance_details.into_values().collect();
            let tools: Vec<String> = records
                .iter()
                .map(|record| record.backend_tool.clone())
                .collect();
            let summary = records
                .iter()
                .find_map(|record| (!record.summary.is_empty()).then(|| record.summary.clone()))
                .unwrap_or_default();
            let metrics = adoption.metrics_for(&SkillKey::new(&dcc_type, &name), is_loaded);
            searched_skills += usize::from(metrics.searched);
            used_skills += usize::from(metrics.used);
            low_adoption_skills += usize::from(metrics.low_adoption);
            load_error_count += metrics.load_error_count;

            json!({
                "name": name,
                "dcc_type": dcc_type,
                "loaded": is_loaded,
                "action_count": records.len(),
                "instance_count": instances.len(),
                "instances": instances.into_iter().collect::<Vec<_>>(),
                "instance_ids": instance_ids,
                "instance_details": instance_details,
                "tools": tools,
                "summary": summary,
                "adoption": metrics,
                "package": Value::Null,
                "version": Value::Null,
            })
        })
        .collect();

    let health = SkillHealthSummary {
        discovered_skill_roots,
        loaded_skills: loaded,
        unloaded_skills: skills.len().saturating_sub(loaded),
        action_count,
        searched_skills,
        used_skills,
        low_adoption_skills,
        load_error_count,
        missing_path_count,
        path_redaction: "alias",
    };
    json!({
        "total": skills.len(),
        "loaded": loaded,
        "unloaded": skills.len().saturating_sub(loaded),
        "action_count": action_count,
        "adoption_scope": "gateway_routed",
        "health": health,
        "skills": skills,
    })
}

fn build_adoption_metrics(
    records: &[SkillCapabilityInput],
    searches: &[SkillSearchInput],
    audits: &[AdminAuditRecord],
    traces: &[DispatchTrace],
) -> AdoptionIndex {
    let mut index = AdoptionIndex::from_records(records);
    index.ingest_searches(searches);
    index.ingest_audits(audits);
    index.ingest_traces(traces);
    index
}

struct AdoptionIndex {
    builders: HashMap<SkillKey, SkillAdoptionBuilder>,
    tool_to_skill: HashMap<String, SkillKey>,
    backend_to_skill: HashMap<(String, String), SkillKey>,
}

impl AdoptionIndex {
    fn from_records(records: &[SkillCapabilityInput]) -> Self {
        let mut builders = HashMap::new();
        let mut tool_to_skill = HashMap::new();
        let mut backend_to_skill = HashMap::new();
        for record in records {
            let skill_name = record
                .skill_name
                .clone()
                .unwrap_or_else(|| record.backend_tool.clone());
            let key = SkillKey::new(record.dcc_type.clone(), skill_name);
            builders.entry(key.clone()).or_default();
            tool_to_skill.insert(record.tool_slug.clone(), key.clone());
            backend_to_skill.insert(
                (
                    record.dcc_type.to_ascii_lowercase(),
                    record.backend_tool.to_ascii_lowercase(),
                ),
                key,
            );
        }
        Self {
            builders,
            tool_to_skill,
            backend_to_skill,
        }
    }

    fn metrics_for(&self, key: &SkillKey, loaded: bool) -> SkillAdoptionMetrics {
        let builder = self.builders.get(key);
        let search_hits = builder.map_or(0, |item| item.search_hits);
        let selected_count = builder.map_or(0, |item| item.selected_count);
        let call_count = builder.map_or(0, |item| item.call_count);
        let failure_count = builder.map_or(0, |item| item.failure_count);
        let load_error_count = builder.map_or(0, |item| item.load_error_count);
        let fallback_displaced_by_scripting =
            builder.map_or(0, |item| item.fallback_displaced_by_scripting);
        let average_rank = builder.and_then(|item| {
            (item.search_hits > 0).then(|| item.rank_sum as f64 / item.search_hits as f64)
        });
        let low_adoption = loaded
            && search_hits > 0
            && selected_count == 0
            && call_count == 0
            && load_error_count == 0;
        SkillAdoptionMetrics {
            search_hits,
            best_rank: builder.and_then(|item| item.best_rank),
            average_rank,
            selected_count,
            call_count,
            failure_count,
            load_error_count,
            last_searched: builder
                .and_then(|item| item.last_searched_ms)
                .map(ms_to_rfc3339),
            last_used: builder
                .and_then(|item| item.last_used_ms)
                .map(ms_to_rfc3339),
            fallback_displaced_by_scripting,
            searched: search_hits > 0,
            used: call_count > 0,
            low_adoption,
        }
    }

    fn ingest_searches(&mut self, searches: &[SkillSearchInput]) {
        for search in searches {
            let mut hit_keys = BTreeSet::new();
            for hit in &search.hits {
                if let Some(key) =
                    self.key_for_hit(&hit.dcc_type, hit.skill_name.as_deref(), &hit.tool_slug)
                {
                    let builder = self.builders.entry(key.clone()).or_default();
                    builder.search_hits += 1;
                    builder.rank_sum += u64::from(hit.rank);
                    builder.best_rank = Some(
                        builder
                            .best_rank
                            .map_or(hit.rank, |best| best.min(hit.rank)),
                    );
                    builder.last_searched_ms =
                        max_ms(builder.last_searched_ms, Some(search.timestamp_ms));
                    hit_keys.insert(key);
                }
            }
            for followup in &search.followups {
                if let Some(key) = self.key_for_followup(followup) {
                    self.ingest_followup_for_key(key, followup);
                } else if followup.kind == "call"
                    && followup
                        .tool_slug
                        .as_deref()
                        .is_some_and(is_scripting_fallback)
                {
                    for key in &hit_keys {
                        self.builders
                            .entry(key.clone())
                            .or_default()
                            .fallback_displaced_by_scripting += 1;
                    }
                }
            }
        }
    }

    fn ingest_followup_for_key(&mut self, key: SkillKey, followup: &SkillSearchFollowupInput) {
        let builder = self.builders.entry(key).or_default();
        if followup.selected_rank.is_some()
            || matches!(followup.kind.as_str(), "describe" | "load_skill" | "call")
        {
            builder.selected_count += 1;
        }
        if followup.kind == "load_skill" && !followup.success {
            builder.load_error_count += 1;
        }
        if followup.kind == "call" {
            let request_id = followup.request_id.clone().unwrap_or_else(|| {
                format!(
                    "search-followup:{}:{}",
                    followup.timestamp_ms,
                    followup.tool_slug.as_deref().unwrap_or_default()
                )
            });
            if builder.call_request_ids.insert(request_id) {
                builder.call_count += 1;
                if !followup.success {
                    builder.failure_count += 1;
                }
                builder.last_used_ms = max_ms(builder.last_used_ms, Some(followup.timestamp_ms));
            }
        }
    }

    fn ingest_audits(&mut self, audits: &[AdminAuditRecord]) {
        for audit in audits {
            let Some(key) = self.key_for_action(&audit.action, audit.dcc_type.as_deref()) else {
                continue;
            };
            self.ingest_call_for_key(
                key,
                &audit.request_id,
                audit.success,
                Some(timestamp_ms(audit.timestamp)),
            );
        }
    }

    fn ingest_traces(&mut self, traces: &[DispatchTrace]) {
        for trace in traces {
            let Some(tool_slug) = trace.tool_slug.as_deref() else {
                continue;
            };
            let Some(key) = self.key_for_action(tool_slug, trace.dcc_type.as_deref()) else {
                continue;
            };
            self.ingest_call_for_key(
                key,
                &trace.request_id,
                trace.ok,
                Some(timestamp_ms(trace.started_at)),
            );
        }
    }

    fn ingest_call_for_key(
        &mut self,
        key: SkillKey,
        request_id: &str,
        success: bool,
        timestamp_ms: Option<u64>,
    ) {
        let builder = self.builders.entry(key).or_default();
        if !builder.call_request_ids.insert(request_id.to_string()) {
            return;
        }
        builder.call_count += 1;
        if !success {
            builder.failure_count += 1;
        }
        builder.last_used_ms = max_ms(builder.last_used_ms, timestamp_ms);
    }

    fn key_for_hit(
        &self,
        dcc_type: &str,
        skill_name: Option<&str>,
        tool_slug: &str,
    ) -> Option<SkillKey> {
        skill_name
            .map(|skill| SkillKey::new(dcc_type, skill))
            .or_else(|| self.tool_to_skill.get(tool_slug).cloned())
    }

    fn key_for_followup(&self, followup: &SkillSearchFollowupInput) -> Option<SkillKey> {
        followup
            .tool_slug
            .as_deref()
            .and_then(|slug| self.tool_to_skill.get(slug).cloned())
            .or_else(|| {
                let skill = followup.skill_name.as_deref()?;
                self.builders.keys().find(|key| key.name == skill).cloned()
            })
    }

    fn key_for_action(&self, action: &str, dcc_type: Option<&str>) -> Option<SkillKey> {
        if let Some(key) = self.tool_to_skill.get(action) {
            return Some(key.clone());
        }
        self.backend_to_skill
            .get(&(dcc_type?.to_ascii_lowercase(), backend_action_name(action)))
            .cloned()
    }
}

fn backend_action_name(action: &str) -> String {
    action
        .splitn(3, '.')
        .nth(2)
        .unwrap_or(action)
        .to_ascii_lowercase()
}

fn max_ms(current: Option<u64>, candidate: Option<u64>) -> Option<u64> {
    match (current, candidate) {
        (Some(a), Some(b)) => Some(a.max(b)),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    }
}

fn timestamp_ms(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_millis() as u64
}

fn ms_to_rfc3339(ms: u64) -> String {
    chrono::DateTime::<chrono::Utc>::from(UNIX_EPOCH + Duration::from_millis(ms)).to_rfc3339()
}

fn is_scripting_fallback(tool_slug: &str) -> bool {
    let lower = tool_slug.to_ascii_lowercase();
    [
        "execute_python",
        "run_python",
        "python_exec",
        "execute_code",
        "eval",
        "script",
        "mel",
        "cmds",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

fn instance_short(id: &str) -> String {
    id.chars().take(8).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn capability(name: &str, action: &str) -> SkillCapabilityInput {
        SkillCapabilityInput {
            tool_slug: format!("maya.deadbeef.{action}"),
            backend_tool: action.to_string(),
            skill_name: Some(name.to_string()),
            summary: format!("{name} summary"),
            dcc_type: "maya".to_string(),
            instance_id: "deadbeef-0000-0000-0000-000000000000".to_string(),
            loaded: true,
        }
    }

    #[test]
    fn projects_search_adoption_without_gateway_state() {
        let modeling = capability("maya-modeling", "create_sphere");
        let render = capability("maya-render", "render_preview");
        let searches = [SkillSearchInput {
            timestamp_ms: 1_000,
            hits: vec![
                SkillSearchHitInput {
                    tool_slug: render.tool_slug.clone(),
                    skill_name: render.skill_name.clone(),
                    dcc_type: render.dcc_type.clone(),
                    rank: 1,
                },
                SkillSearchHitInput {
                    tool_slug: modeling.tool_slug.clone(),
                    skill_name: modeling.skill_name.clone(),
                    dcc_type: modeling.dcc_type.clone(),
                    rank: 2,
                },
            ],
            followups: vec![SkillSearchFollowupInput {
                kind: "call".to_string(),
                timestamp_ms: 1_100,
                request_id: Some("req-1".to_string()),
                tool_slug: Some(modeling.tool_slug.clone()),
                skill_name: None,
                selected_rank: Some(2),
                success: false,
            }],
        }];

        let payload = skill_inventory_payload(vec![modeling, render], &searches, &[], &[], 3, 1);

        assert_eq!(payload["health"]["searched_skills"], 2);
        assert_eq!(payload["health"]["used_skills"], 1);
        assert_eq!(payload["health"]["low_adoption_skills"], 1);
        assert_eq!(payload["health"]["discovered_skill_roots"], 3);
        assert_eq!(payload["health"]["missing_path_count"], 1);
        let skills = payload["skills"].as_array().unwrap();
        let modeling = skills
            .iter()
            .find(|skill| skill["name"] == "maya-modeling")
            .unwrap();
        assert_eq!(modeling["adoption"]["best_rank"], 2);
        assert_eq!(modeling["adoption"]["call_count"], 1);
        assert_eq!(modeling["adoption"]["failure_count"], 1);
        let render = skills
            .iter()
            .find(|skill| skill["name"] == "maya-render")
            .unwrap();
        assert_eq!(render["adoption"]["low_adoption"], true);
    }

    #[test]
    fn historical_instance_prefix_preserves_full_backend_tool() {
        assert_eq!(
            backend_action_name("unreal.deadbeef.unreal_actors__spawn_actor"),
            "unreal_actors__spawn_actor"
        );
        assert_eq!(
            backend_action_name("maya_render__capture_viewport"),
            "maya_render__capture_viewport"
        );
    }
}
