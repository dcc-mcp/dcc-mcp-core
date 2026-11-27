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
    partial_skills: usize,
    unloaded_skills: usize,
    catalog_only_skills: usize,
    action_count: usize,
    searched_skills: usize,
    used_skills: usize,
    low_adoption_skills: usize,
    load_error_count: usize,
    missing_path_count: usize,
    path_redaction: &'static str,
}

const CATALOG_INSTANCE_ID: &str = "00000000-0000-0000-0000-000000000000";

#[derive(Debug, Default)]
struct DccSkillState {
    loaded_instances: usize,
    unloaded_instances: usize,
    catalog_available: bool,
}

fn skill_load_state(loaded: usize, unloaded: usize) -> &'static str {
    match (loaded, unloaded) {
        (0, 0) => "catalog_only",
        (0, _) => "unloaded",
        (_, 0) => "loaded",
        _ => "partial",
    }
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
    // A skill is a logical inventory item. DCC types and instances are
    // capabilities of that item, so they must not create duplicate rows.
    let mut grouped: BTreeMap<String, Vec<SkillCapabilityInput>> = BTreeMap::new();
    for record in records {
        let skill_name = record
            .skill_name
            .clone()
            .unwrap_or_else(|| record.backend_tool.clone());
        grouped.entry(skill_name).or_default().push(record);
    }

    let mut loaded = 0usize;
    let mut partial = 0usize;
    let mut unloaded = 0usize;
    let mut action_count = 0usize;
    let mut searched_skills = 0usize;
    let mut used_skills = 0usize;
    let mut low_adoption_skills = 0usize;
    let mut load_error_count = 0usize;

    let skills: Vec<Value> = grouped
        .into_iter()
        .map(|(name, records)| {
            let mut instance_states: BTreeMap<String, (bool, String)> = BTreeMap::new();
            let mut dcc_states: BTreeMap<String, DccSkillState> = BTreeMap::new();
            for record in &records {
                let dcc = dcc_states.entry(record.dcc_type.clone()).or_default();
                if is_catalog_record(&record.instance_id) {
                    dcc.catalog_available = true;
                } else {
                    instance_states
                        .entry(record.instance_id.clone())
                        .and_modify(|state| state.0 |= record.loaded)
                        .or_insert((record.loaded, record.dcc_type.clone()));
                }
            }
            let loaded_instance_count = instance_states
                .values()
                .filter(|(is_loaded, _)| *is_loaded)
                .count();
            let unloaded_instance_count =
                instance_states.len().saturating_sub(loaded_instance_count);
            for (is_loaded, dcc_type) in instance_states.values() {
                let dcc = dcc_states.entry(dcc_type.clone()).or_default();
                if *is_loaded {
                    dcc.loaded_instances += 1;
                } else {
                    dcc.unloaded_instances += 1;
                }
            }
            let load_state = skill_load_state(loaded_instance_count, unloaded_instance_count);
            match load_state {
                "loaded" => loaded += 1,
                "partial" => partial += 1,
                "unloaded" => unloaded += 1,
                _ => {}
            }

            let tools: BTreeSet<String> = records
                .iter()
                .map(|record| record.backend_tool.clone())
                .collect();
            action_count += tools.len();
            let instance_details: Vec<Value> = instance_states
                .iter()
                .map(|(id, (is_loaded, dcc_type))| {
                    json!({
                        "id": id,
                        "instance_id": id,
                        "prefix": instance_short(id),
                        "instance_short": instance_short(id),
                        "dcc_type": dcc_type,
                        "loaded": is_loaded,
                    })
                })
                .collect();
            let instances: Vec<String> = instance_states
                .keys()
                .map(|id| instance_short(id))
                .collect();
            let instance_ids: Vec<String> = instance_states.keys().cloned().collect();
            let dcc_types: Vec<String> = dcc_states.keys().cloned().collect();
            let catalog_dcc_types: Vec<String> = dcc_states
                .iter()
                .filter_map(|(dcc_type, state)| state.catalog_available.then_some(dcc_type.clone()))
                .collect();
            let dcc_states: Vec<Value> = dcc_states
                .into_iter()
                .map(|(dcc_type, state)| {
                    json!({
                        "dcc_type": dcc_type,
                        "load_state": skill_load_state(state.loaded_instances, state.unloaded_instances),
                        "loaded_instance_count": state.loaded_instances,
                        "unloaded_instance_count": state.unloaded_instances,
                        "catalog_available": state.catalog_available,
                    })
                })
                .collect();
            let dcc_type = if dcc_types.len() == 1 {
                dcc_types[0].clone()
            } else {
                String::new()
            };
            let summary = records
                .iter()
                .find_map(|record| (!record.summary.is_empty()).then(|| record.summary.clone()))
                .unwrap_or_default();
            let metrics = adoption.metrics_for_group(&name, &dcc_types, loaded_instance_count > 0);
            searched_skills += usize::from(metrics.searched);
            used_skills += usize::from(metrics.used);
            low_adoption_skills += usize::from(metrics.low_adoption);
            load_error_count += metrics.load_error_count;

            json!({
                "name": name,
                "dcc_type": dcc_type,
                "dcc_types": dcc_types,
                "load_state": load_state,
                "loaded": load_state == "loaded",
                "loaded_instance_count": loaded_instance_count,
                "unloaded_instance_count": unloaded_instance_count,
                "catalog_only": instance_states.is_empty() && !catalog_dcc_types.is_empty(),
                "catalog_dcc_types": catalog_dcc_types,
                "dcc_states": dcc_states,
                "action_count": tools.len(),
                "instance_count": instances.len(),
                "instances": instances,
                "instance_ids": instance_ids,
                "instance_details": instance_details,
                "tools": tools.into_iter().collect::<Vec<_>>(),
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
        partial_skills: partial,
        unloaded_skills: unloaded,
        catalog_only_skills: skills.len() - (loaded + partial + unloaded),
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
        "partial": partial,
        "unloaded": unloaded,
        "catalog_only": health.catalog_only_skills,
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

    fn metrics_for_group(
        &self,
        name: &str,
        dcc_types: &[String],
        loaded: bool,
    ) -> SkillAdoptionMetrics {
        let mut aggregate = SkillAdoptionBuilder::default();
        for dcc_type in dcc_types {
            let Some(builder) = self.builders.get(&SkillKey::new(dcc_type, name)) else {
                continue;
            };
            aggregate.search_hits += builder.search_hits;
            aggregate.rank_sum += builder.rank_sum;
            aggregate.best_rank = match (aggregate.best_rank, builder.best_rank) {
                (Some(current), Some(candidate)) => Some(current.min(candidate)),
                (None, candidate) => candidate,
                (current, None) => current,
            };
            aggregate.selected_count += builder.selected_count;
            aggregate.call_count += builder.call_count;
            aggregate.failure_count += builder.failure_count;
            aggregate.load_error_count += builder.load_error_count;
            aggregate.fallback_displaced_by_scripting += builder.fallback_displaced_by_scripting;
            aggregate.last_searched_ms =
                max_ms(aggregate.last_searched_ms, builder.last_searched_ms);
            aggregate.last_used_ms = max_ms(aggregate.last_used_ms, builder.last_used_ms);
        }
        self.metrics_from_builder(Some(&aggregate), loaded)
    }

    fn metrics_from_builder(
        &self,
        builder: Option<&SkillAdoptionBuilder>,
        loaded: bool,
    ) -> SkillAdoptionMetrics {
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

fn is_catalog_record(instance_id: &str) -> bool {
    instance_id.eq_ignore_ascii_case(CATALOG_INSTANCE_ID)
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

    #[test]
    fn projects_shared_skill_once_with_cross_dcc_partial_state() {
        let mut maya = capability("dcc-mcp", "discover_tools");
        maya.loaded = true;
        let mut blender = capability("dcc-mcp", "discover_tools");
        blender.dcc_type = "blender".to_string();
        blender.instance_id = "blender-0000-0000-0000-000000000000".to_string();
        blender.loaded = false;

        let payload = skill_inventory_payload(vec![maya, blender], &[], &[], &[], 2, 0);
        assert_eq!(payload["total"], 1);
        assert_eq!(payload["loaded"], 0);
        assert_eq!(payload["partial"], 1);
        assert_eq!(payload["unloaded"], 0);
        let skill = &payload["skills"][0];
        assert_eq!(skill["load_state"], "partial");
        assert_eq!(skill["loaded_instance_count"], 1);
        assert_eq!(skill["unloaded_instance_count"], 1);
        assert_eq!(skill["dcc_types"], json!(["blender", "maya"]));
        assert_eq!(skill["instance_details"][0]["loaded"], false);
        assert_eq!(skill["instance_details"][1]["loaded"], true);
    }

    #[test]
    fn catalog_placeholder_does_not_turn_a_loaded_skill_into_partial() {
        let mut catalog = capability("dcc-mcp", "discover_tools");
        catalog.instance_id = CATALOG_INSTANCE_ID.to_string();
        catalog.loaded = false;
        let mut loaded = capability("dcc-mcp", "discover_tools");
        loaded.instance_id = "maya-0000-0000-0000-000000000000".to_string();

        let payload = skill_inventory_payload(vec![catalog, loaded], &[], &[], &[], 1, 0);
        let skill = &payload["skills"][0];
        assert_eq!(skill["load_state"], "loaded");
        assert_eq!(skill["loaded_instance_count"], 1);
        assert_eq!(skill["unloaded_instance_count"], 0);
        assert_eq!(skill["instance_count"], 1);
        assert_eq!(skill["catalog_only"], false);
        assert_eq!(skill["catalog_dcc_types"], json!(["maya"]));
        assert_eq!(skill["dcc_states"][0]["catalog_available"], true);
    }

    #[test]
    fn counts_live_instances_by_full_id_across_tool_records() {
        let loaded = capability("shared", "inspect");
        let mut hidden_tool = capability("shared", "configure");
        hidden_tool.loaded = false;
        let mut unloaded = hidden_tool.clone();
        unloaded.instance_id = "deadbeef-1111-0000-0000-000000000000".into();
        let payload =
            skill_inventory_payload(vec![hidden_tool, loaded, unloaded], &[], &[], &[], 0, 0);
        let row = &payload["skills"][0];
        assert_eq!(row["instance_count"], 2);
        assert_eq!(row["instances"].as_array().unwrap().len(), 2);
        assert_eq!(row["dcc_states"][0]["loaded_instance_count"], 1);
        assert_eq!(row["dcc_states"][0]["unloaded_instance_count"], 1);
        assert_eq!(row["dcc_states"][0]["load_state"], "partial");
    }

    #[test]
    fn catalog_only_skill_has_no_fake_instance() {
        let mut maya = capability("dcc-mcp", "discover_tools");
        maya.instance_id = CATALOG_INSTANCE_ID.to_string();
        maya.loaded = false;
        let mut blender = maya.clone();
        blender.dcc_type = "blender".to_string();

        let payload = skill_inventory_payload(vec![maya, blender], &[], &[], &[], 2, 0);
        let skill = &payload["skills"][0];
        assert_eq!(skill["load_state"], "catalog_only");
        assert_eq!(skill["dcc_states"][0]["load_state"], "catalog_only");
        assert_eq!(payload["unloaded"], 0);
        assert_eq!(payload["catalog_only"], 1);
        assert_eq!(skill["instance_count"], 0);
        assert_eq!(skill["instances"], json!([]));
        assert_eq!(skill["catalog_only"], true);
        assert_eq!(skill["catalog_dcc_types"], json!(["blender", "maya"]));
    }
}
