//! Skill health and adoption projections for the Admin UI.
//!
//! The gateway already records search-quality telemetry and call traces. This
//! module joins those streams with the capability index so the admin surface can
//! answer skill-level questions without exposing raw local skill paths.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use serde_json::{Value, json};

use crate::gateway::admin::activity::{collect_audits, collect_traces};
use crate::gateway::admin::state::{AdminAuditRecord, AdminState};
use crate::gateway::admin::trace::DispatchTrace;
use crate::gateway::capability::CapabilityRecord;
use crate::gateway::search_telemetry::{SearchFollowupTelemetry, SearchTelemetrySnapshot};

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

pub(super) async fn build_skill_inventory_payload(
    state: &AdminState,
    records: Arc<[CapabilityRecord]>,
) -> Value {
    let search_snapshot = state.gateway.search_telemetry.snapshot(1_000);
    let audits = collect_audits(state, 1_000).await;
    let traces = collect_traces(state, 1_000).await;
    let adoption = build_adoption_metrics(&records, &search_snapshot, &audits, &traces);

    let mut grouped: BTreeMap<(String, String, bool), Vec<CapabilityRecord>> = BTreeMap::new();
    for record in records.iter().cloned() {
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
            for r in &records {
                let id = r.instance_id.to_string();
                instance_details.entry(id.clone()).or_insert_with(|| {
                    json!({
                        "id": id,
                        "instance_id": r.instance_id.to_string(),
                        "prefix": instance_short(&r.instance_id),
                        "instance_short": instance_short(&r.instance_id),
                        "dcc_type": r.dcc_type,
                    })
                });
            }
            let instances: BTreeSet<String> = instance_details
                .values()
                .filter_map(|v| v.get("instance_short").and_then(Value::as_str))
                .map(str::to_owned)
                .collect();
            let instance_ids: Vec<String> = instance_details.keys().cloned().collect();
            let instance_details: Vec<Value> = instance_details.into_values().collect();
            let tools: Vec<String> = records.iter().map(|r| r.backend_tool.clone()).collect();
            let summary = records
                .iter()
                .find_map(|r| (!r.summary.is_empty()).then(|| r.summary.clone()))
                .unwrap_or_default();
            let key = SkillKey::new(&dcc_type, &name);
            let metrics = adoption.metrics_for(&key, is_loaded);
            if metrics.searched {
                searched_skills += 1;
            }
            if metrics.used {
                used_skills += 1;
            }
            if metrics.low_adoption {
                low_adoption_skills += 1;
            }
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

    let missing_path_count = build_skill_path_rows(state)
        .iter()
        .filter(|row| row.get("status").and_then(Value::as_str) == Some("missing"))
        .count();
    let health = SkillHealthSummary {
        discovered_skill_roots: skill_path_count(state),
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
        "health": health,
        "skills": skills,
    })
}

pub(super) fn build_skill_paths_payload(state: &AdminState) -> Value {
    let paths = build_skill_path_rows(state);
    let missing = paths
        .iter()
        .filter(|row| row.get("status").and_then(Value::as_str) == Some("missing"))
        .count();
    json!({
        "paths": paths,
        "summary": {
            "total": paths.len(),
            "missing": missing,
            "present": paths.len().saturating_sub(missing),
            "path_redaction": "alias",
        }
    })
}

fn build_adoption_metrics(
    records: &[CapabilityRecord],
    search_snapshot: &SearchTelemetrySnapshot,
    audits: &[AdminAuditRecord],
    traces: &[DispatchTrace],
) -> AdoptionIndex {
    let mut index = AdoptionIndex::from_records(records);
    index.ingest_searches(search_snapshot);
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
    fn from_records(records: &[CapabilityRecord]) -> Self {
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
        let search_hits = builder.map_or(0, |b| b.search_hits);
        let selected_count = builder.map_or(0, |b| b.selected_count);
        let call_count = builder.map_or(0, |b| b.call_count);
        let failure_count = builder.map_or(0, |b| b.failure_count);
        let load_error_count = builder.map_or(0, |b| b.load_error_count);
        let fallback_displaced_by_scripting =
            builder.map_or(0, |b| b.fallback_displaced_by_scripting);
        let average_rank = builder
            .and_then(|b| (b.search_hits > 0).then(|| b.rank_sum as f64 / b.search_hits as f64));
        let low_adoption = loaded
            && search_hits > 0
            && selected_count == 0
            && call_count == 0
            && load_error_count == 0;
        SkillAdoptionMetrics {
            search_hits,
            best_rank: builder.and_then(|b| b.best_rank),
            average_rank,
            selected_count,
            call_count,
            failure_count,
            load_error_count,
            last_searched: builder.and_then(|b| b.last_searched_ms).map(ms_to_rfc3339),
            last_used: builder.and_then(|b| b.last_used_ms).map(ms_to_rfc3339),
            fallback_displaced_by_scripting,
            searched: search_hits > 0,
            used: call_count > 0,
            low_adoption,
        }
    }

    fn ingest_searches(&mut self, snapshot: &SearchTelemetrySnapshot) {
        for record in &snapshot.recent {
            let mut hit_keys = BTreeSet::new();
            for hit in &record.hits {
                if let Some(key) = self.key_for_hit(
                    hit.dcc_type.as_str(),
                    hit.skill_name.as_deref(),
                    &hit.tool_slug,
                ) {
                    let builder = self.builders.entry(key.clone()).or_default();
                    builder.search_hits += 1;
                    builder.rank_sum += u64::from(hit.rank);
                    builder.best_rank = Some(
                        builder
                            .best_rank
                            .map_or(hit.rank, |best| best.min(hit.rank)),
                    );
                    builder.last_searched_ms =
                        max_ms(builder.last_searched_ms, Some(record.timestamp_ms));
                    hit_keys.insert(key);
                }
            }
            for followup in &record.followups {
                if let Some(key) = self.key_for_followup(followup) {
                    self.ingest_followup_for_key(key, followup);
                    continue;
                }
                if followup.kind == "call"
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

    fn ingest_followup_for_key(&mut self, key: SkillKey, followup: &SearchFollowupTelemetry) {
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
            let ms = timestamp_ms(audit.timestamp);
            self.ingest_call_for_key(key, &audit.request_id, audit.success, Some(ms));
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
            let ms = timestamp_ms(trace.started_at);
            self.ingest_call_for_key(key, &trace.request_id, trace.ok, Some(ms));
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

    fn key_for_followup(&self, followup: &SearchFollowupTelemetry) -> Option<SkillKey> {
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
        let backend = action
            .rsplit_once("__")
            .map(|(_, suffix)| suffix)
            .unwrap_or(action)
            .to_ascii_lowercase();
        let dcc = dcc_type?.to_ascii_lowercase();
        self.backend_to_skill.get(&(dcc, backend)).cloned()
    }
}

fn build_skill_path_rows(state: &AdminState) -> Vec<Value> {
    let mut rows: Vec<Value> = state
        .skill_paths_snapshot
        .iter()
        .enumerate()
        .map(|(idx, e)| safe_skill_path_row(&e.path, &e.source, None, idx + 1))
        .collect();
    if let Some(ref lane) = state.admin_sqlite_lane {
        let r = lane.reader();
        for (id, path) in r.list_custom_skill_paths() {
            if !rows.iter().any(|v| {
                v.get("path_hash").and_then(Value::as_str) == Some(path_hash(&path).as_str())
            }) {
                rows.push(safe_skill_path_row(
                    &path,
                    "admin_custom",
                    Some(id),
                    rows.len() + 1,
                ));
            }
        }
    }
    rows
}

fn safe_skill_path_row(path: &str, source: &str, id: Option<i64>, ordinal: usize) -> Value {
    let hash = path_hash(path);
    let status = skill_path_status(path);
    let source_label = friendly_source_label(source);
    let tail = safe_path_tail(path);
    // Prefer a non-sensitive folder tail (e.g. "studio/skills") so operators
    // can tell same-source rows apart; fall back to the ordinal id only when no
    // meaningful tail is available. The full local path is never exposed.
    let display_path = if tail.is_empty() {
        format!("{source_label} #{}", id.unwrap_or(ordinal as i64))
    } else {
        format!("{source_label} · {tail}")
    };
    let mut row = json!({
        "path": display_path,
        "display_path": display_path,
        "source_label": source_label,
        "path_tail": tail,
        "path_alias": format!("skill-path:{hash}"),
        "path_hash": hash,
        "path_redacted": true,
        "source": source,
        "status": status,
        "exists": status == "present",
        "package": Value::Null,
        "version": Value::Null,
    });
    if let Some(id) = id
        && let Some(obj) = row.as_object_mut()
    {
        obj.insert("id".to_string(), json!(id));
    }
    row
}

/// Map a raw path-source token (e.g. `bundled`, `admin_custom`,
/// `env:DCC_MCP_SKILL_PATHS`) to a friendly, human-readable label for the
/// admin UI. Unknown sources are title-cased from their safe form.
fn friendly_source_label(source: &str) -> String {
    if source.trim().is_empty() {
        return "Skill path".to_string();
    }
    let normalized: String = source
        .trim()
        .to_ascii_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { ' ' })
        .collect();
    let key = normalized.split_whitespace().next().unwrap_or("");
    match key {
        "bundled" => "Bundled".to_string(),
        "admin" | "admincustom" => "Admin custom".to_string(),
        "env" | "envvar" => "Env var".to_string(),
        "explicit" | "explicitarg" => "Explicit arg".to_string(),
        "local" | "localdev" => "Local dev".to_string(),
        "platform" => "Platform".to_string(),
        "user" => "User".to_string(),
        "team" => "Team".to_string(),
        "repo" => "Repo".to_string(),
        "system" => "System".to_string(),
        _ => {
            let safe = safe_source_label(source);
            let mut chars = safe.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => "Skill path".to_string(),
            }
        }
    }
}

/// Return the final one or two path components, which are safe to show because
/// they describe the skill *folder* rather than the user's filesystem layout.
/// Any component matching the current OS username is replaced with `~` so a
/// `C:\Users\<name>\...` style root cannot leak the operator's identity.
fn safe_path_tail(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    let username = std::env::var("USERNAME")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_default()
        .to_ascii_lowercase();
    let components: Vec<&str> = normalized
        .split('/')
        .filter(|c| !c.is_empty() && *c != "." && *c != "..")
        // Drop drive letters like "C:" so the tail stays folder-focused.
        .filter(|c| !(c.len() == 2 && c.ends_with(':')))
        .collect();
    let take = components.len().min(2);
    components[components.len() - take..]
        .iter()
        .map(|c| {
            if !username.is_empty() && c.to_ascii_lowercase() == username {
                "~".to_string()
            } else {
                (*c).to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn skill_path_count(state: &AdminState) -> usize {
    build_skill_path_rows(state).len()
}

fn safe_source_label(source: &str) -> String {
    let trimmed = source.trim();
    if trimmed.is_empty() {
        return "skill_path".to_string();
    }
    trimmed
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | ':' | '.') {
                ch
            } else {
                '_'
            }
        })
        .take(64)
        .collect()
}

fn skill_path_status(path: &str) -> &'static str {
    if path.trim().is_empty() {
        return "missing";
    }
    if Path::new(path).exists() {
        "present"
    } else {
        "missing"
    }
}

fn path_hash(path: &str) -> String {
    let mut hasher = StableHasher::new();
    normalise_path_for_hash(path).hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn normalise_path_for_hash(path: &str) -> String {
    path.replace('\\', "/").to_ascii_lowercase()
}

struct StableHasher(u64);

impl StableHasher {
    fn new() -> Self {
        Self(0xcbf29ce484222325)
    }
}

impl Hasher for StableHasher {
    fn write(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 ^= u64::from(*byte);
            self.0 = self.0.wrapping_mul(0x100000001b3);
        }
    }

    fn finish(&self) -> u64 {
        self.0
    }
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

fn instance_short(id: &uuid::Uuid) -> String {
    id.to_string().chars().take(8).collect()
}

#[cfg(test)]
mod skill_path_display_tests {
    use super::*;

    #[test]
    fn friendly_labels_map_known_sources() {
        assert_eq!(friendly_source_label("bundled"), "Bundled");
        assert_eq!(friendly_source_label("admin_custom"), "Admin custom");
        assert_eq!(friendly_source_label("env:DCC_MCP_SKILL_PATHS"), "Env var");
        assert_eq!(friendly_source_label("LocalDev"), "Local dev");
        assert_eq!(friendly_source_label("ExplicitArg"), "Explicit arg");
        assert_eq!(friendly_source_label("platform"), "Platform");
    }

    #[test]
    fn friendly_labels_title_case_unknown_sources() {
        assert_eq!(friendly_source_label("studio-shared"), "Studio-shared");
        assert_eq!(friendly_source_label(""), "Skill path");
    }

    #[test]
    fn path_tail_keeps_last_two_components_and_drops_drive() {
        assert_eq!(
            safe_path_tail("G:/studio/pipeline/skills"),
            "pipeline/skills"
        );
        assert_eq!(safe_path_tail("C:\\repo\\skills"), "repo/skills");
        assert_eq!(safe_path_tail("skills"), "skills");
        assert_eq!(safe_path_tail(""), "");
    }

    #[test]
    fn path_tail_redacts_os_username_component() {
        let _g = dcc_mcp_test_utils::EnvVarGuard::set("USERNAME", Some("alice"));
        // A home-rooted path must not leak the operator's username.
        assert_eq!(safe_path_tail("C:/Users/alice/skills"), "~/skills");
    }

    #[test]
    fn skill_path_row_is_redacted_and_labelled() {
        let row = safe_skill_path_row("G:/studio/pipeline/skills", "bundled", None, 1);
        assert_eq!(row["path_redacted"], serde_json::json!(true));
        assert_eq!(row["source_label"], serde_json::json!("Bundled"));
        assert_eq!(row["path_tail"], serde_json::json!("pipeline/skills"));
        // The display string must never contain the original absolute path.
        let display = row["display_path"].as_str().unwrap();
        assert!(!display.contains("G:/studio"));
        assert!(display.starts_with("Bundled"));
    }
}
