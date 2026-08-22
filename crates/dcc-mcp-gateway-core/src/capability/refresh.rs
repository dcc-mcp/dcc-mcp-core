//! Pure capability refresh assembly and telemetry types.
//!
//! The mutable refresh loop and its `reqwest` I/O stay in
//! `dcc-mcp-gateway`. This module owns the deterministic part: combining
//! loaded builder output with unloaded skill hints, assigning
//! instance-scoped slugs, sorting, fingerprinting, and classifying no-op
//! refreshes.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::builder::BuildOutcome;
use super::index::{InstanceFingerprint, compute_fingerprint};
use super::record::{CapabilityGroupInfo, CapabilityRecord, tool_slug};

/// Search metadata for one tool belonging to an unloaded skill.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnloadedCapabilityHint {
    /// Owning skill package name.
    pub skill_name: String,
    /// Backend callable name that becomes routable after loading.
    pub tool_name: String,
    /// Search summary shown before the skill is loaded.
    pub summary: String,
    /// Extra non-wire search tokens.
    pub search_tokens: Vec<String>,
    /// Skill-layer rank hint supplied by the per-DCC REST catalog.
    pub rank_layer: Option<String>,
    /// Discovery-path rank hint supplied by the per-DCC REST catalog.
    pub rank_path_source: Option<String>,
    /// Progressive groups advertised by the skill.
    pub available_groups: Vec<CapabilityGroupInfo>,
    /// Progressive group containing this tool, when known.
    pub tool_group: Option<String>,
}

/// Fully assembled per-instance slice ready for an index upsert.
#[derive(Debug, Clone, Default)]
pub struct RefreshBuildOutcome {
    /// Loaded and unloaded records, sorted by stable tool slug.
    pub records: Vec<CapabilityRecord>,
    /// Fingerprint covering the complete sorted slice.
    pub fingerprint: InstanceFingerprint,
    /// Number of records built from the live backend tool list.
    pub loaded: usize,
    /// Number of valid unloaded hints converted to records.
    pub unloaded: usize,
    /// Number of live backend tools rejected by the builder.
    pub skipped: usize,
}

impl RefreshBuildOutcome {
    /// Return whether a non-empty slice matches the currently indexed fingerprint.
    ///
    /// Empty slices deliberately return `false`: the runtime must upsert them so a
    /// backend that removed every capability also removes its stale index slice.
    #[must_use]
    pub fn is_unchanged(&self, previous: Option<InstanceFingerprint>) -> bool {
        !self.records.is_empty() && previous == Some(self.fingerprint)
    }
}

/// Combine live builder output with unloaded skill hints for one DCC instance.
#[must_use]
pub fn build_refresh_records(
    loaded: BuildOutcome,
    unloaded_hints: Vec<UnloadedCapabilityHint>,
    instance_id: Uuid,
    dcc_type: &str,
) -> RefreshBuildOutcome {
    let BuildOutcome {
        mut records,
        skipped,
        ..
    } = loaded;
    let loaded = records.len();
    records.extend(build_unloaded_records(
        unloaded_hints,
        instance_id,
        dcc_type,
    ));
    records.sort_by(|a, b| a.tool_slug.cmp(&b.tool_slug));
    let unloaded = records.len().saturating_sub(loaded);
    let fingerprint = compute_fingerprint(&records);
    RefreshBuildOutcome {
        records,
        fingerprint,
        loaded,
        unloaded,
        skipped,
    }
}

fn build_unloaded_records(
    unloaded_hints: Vec<UnloadedCapabilityHint>,
    instance_id: Uuid,
    dcc_type: &str,
) -> Vec<CapabilityRecord> {
    unloaded_hints
        .into_iter()
        .filter_map(|hint| {
            if hint.tool_name.is_empty() {
                return None;
            }
            let mut record = CapabilityRecord::from_skill_tool(
                &hint.skill_name,
                &hint.tool_name,
                &hint.summary,
                dcc_type,
                hint.tool_group,
            )
            .with_available_groups(hint.available_groups)
            .with_search_tokens(hint.search_tokens)
            .with_rank_policy(hint.rank_layer, hint.rank_path_source);
            record.instance_id = instance_id;
            record.tool_slug = tool_slug(dcc_type, &instance_id, &hint.tool_name);
            Some(record)
        })
        .collect()
}

/// Why a refresh cycle is running.
///
/// Surfaced through `tracing::info!` so operators can correlate an
/// index update with the event that triggered it. Also serialised
/// onto the admin UI's `/admin/api/calls` event log when the call
/// happens to be a refresh-driven dispatch (issue #772).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RefreshReason {
    /// Instance joined the registry for the first time.
    InstanceJoined,
    /// Instance is still live but sent a `tools/list_changed`
    /// notification (typically because a skill loaded/unloaded).
    ToolsListChanged,
    /// Background periodic refresh — catches any change that did
    /// not emit a push notification.
    Periodic,
}

impl RefreshReason {
    /// String label suitable for span tags. Returns the same
    /// snake_case form that the JSON wire emits, so log lines and
    /// JSON dumps are visually consistent.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::InstanceJoined => "instance_joined",
            Self::ToolsListChanged => "tools_list_changed",
            Self::Periodic => "periodic",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability::record::CapabilityRecord;

    #[test]
    fn refresh_reason_as_str_is_stable() {
        // The strings end up in tracing spans + the admin UI's event
        // log. Pin them so a future variant rename cannot silently
        // break log scrapers / Grafana dashboards.
        assert_eq!(RefreshReason::InstanceJoined.as_str(), "instance_joined");
        assert_eq!(
            RefreshReason::ToolsListChanged.as_str(),
            "tools_list_changed"
        );
        assert_eq!(RefreshReason::Periodic.as_str(), "periodic");
    }

    #[test]
    fn refresh_reason_wire_matches_as_str() {
        // The JSON wire form must match `as_str()` so a single string
        // serves both log lines and JSON dumps.
        assert_eq!(
            serde_json::to_string(&RefreshReason::InstanceJoined).unwrap(),
            "\"instance_joined\""
        );
        assert_eq!(
            serde_json::to_string(&RefreshReason::ToolsListChanged).unwrap(),
            "\"tools_list_changed\""
        );
        assert_eq!(
            serde_json::to_string(&RefreshReason::Periodic).unwrap(),
            "\"periodic\""
        );

        let back: RefreshReason = serde_json::from_str("\"periodic\"").unwrap();
        assert_eq!(back, RefreshReason::Periodic);
    }

    fn hint(skill: &str, tool: &str) -> UnloadedCapabilityHint {
        UnloadedCapabilityHint {
            skill_name: skill.to_string(),
            tool_name: tool.to_string(),
            summary: format!("Discover {tool}"),
            search_tokens: vec!["discover".to_string()],
            rank_layer: None,
            rank_path_source: None,
            available_groups: Vec::new(),
            tool_group: None,
        }
    }

    #[test]
    fn refresh_records_merge_loaded_and_unloaded_photoshop_tools() {
        let instance_id = Uuid::from_u128(0xfeed_0000_0000_0000_0000_0000_0000_0001);
        let loaded_record = CapabilityRecord::new(
            tool_slug("photoshop", &instance_id, "read_document"),
            "read_document".to_string(),
            "read_document".to_string(),
            Some("photoshop-document".to_string()),
            "Read the active document",
            vec!["read".to_string()],
            "photoshop".to_string(),
            instance_id,
            false,
            true,
            None,
        );
        let outcome = build_refresh_records(
            BuildOutcome {
                records: vec![loaded_record],
                fingerprint: InstanceFingerprint(1),
                skipped: 2,
            },
            vec![
                hint("photoshop-export", "export_document"),
                hint("broken", ""),
            ],
            instance_id,
            "photoshop",
        );

        assert_eq!(outcome.loaded, 1);
        assert_eq!(outcome.unloaded, 1);
        assert_eq!(outcome.skipped, 2);
        assert_eq!(outcome.records.len(), 2);
        assert_eq!(outcome.records[0].backend_tool, "export_document");
        assert!(!outcome.records[0].loaded);
        assert!(outcome.records[0].tool_slug.contains(".feed0000."));
        assert_eq!(outcome.records[1].backend_tool, "read_document");
        assert_ne!(outcome.fingerprint, InstanceFingerprint::default());
    }

    #[test]
    fn unchanged_policy_preserves_empty_slice_removal() {
        let empty = build_refresh_records(
            BuildOutcome::default(),
            Vec::new(),
            Uuid::from_u128(7),
            "zbrush",
        );
        assert!(!empty.is_unchanged(Some(empty.fingerprint)));

        let non_empty = build_refresh_records(
            BuildOutcome::default(),
            vec![hint("custom-inspection", "inspect_scene")],
            Uuid::from_u128(8),
            "custom",
        );
        assert!(non_empty.is_unchanged(Some(non_empty.fingerprint)));
        assert!(!non_empty.is_unchanged(None));
    }
}
