//! Capability records — the unit of the gateway index.
//!
//! One [`CapabilityRecord`] describes one addressable backend action.
//! Records are intentionally compact (~200 B serialised) so the gateway
//! index can hold thousands of entries without blowing up the gateway
//! process's working set.
//!
//! The record does **not** carry the full JSON Schema of the action;
//! schemas are pulled on demand by the `describe_tool` wrapper and
//! cached by the backend's tool cache.
//!
//! # Why this lives in `dcc-mcp-gateway-core`
//!
//! [`CapabilityRecord`] is the wire-level shape of the REST contract
//! `POST /v1/search` — every infrastructure-layer crate that talks to the
//! gateway over REST needs to deserialise it. Moving it into the domain
//! crate (issue #845) lets external Rust tooling pull just
//! `dcc-mcp-gateway-core` instead of the full gateway crate.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::naming::{instance_short, is_cursor_safe_alphabet};

/// Placeholder annotation key signalling that an action has an input
/// schema on its owning backend but the index does not carry it yet.
/// `describe_tool` honours this hint by fetching the schema the first
/// time it is asked for.
pub const SCHEMA_AVAILABLE: &str = "schema:available";

/// Build the canonical tool slug for an action.
///
/// The slug is the **client-visible** id — agents use it as the
/// `tool_slug` argument to `describe_tool` / `call_tool`. The format
/// is deliberately mechanical so the gateway can reverse the slug in
/// O(1) without a table lookup when high-frequency calls arrive:
///
/// ```text
/// <dcc>.<id8>.<backend_tool>
/// ```
///
/// **REST mental model:** the three segments are the same routing tuple
/// you would place in a hypothetical path-style URL
/// `/<dcc_type>/<instance>/<backend_tool>`; the gateway and MCP clients
/// still use this dot-separated token everywhere today so instance ids and
/// backend names that contain punctuation stay unambiguous without extra
/// escaping. A future revision *could* accept slash-separated aliases in
/// addition to this canonical form if we add a normalisation layer.
///
/// The DCC type and id8 prefix are always cursor-safe (a-z0-9) by
/// construction; `backend_tool` is copied verbatim (it was already
/// validated through `dcc_mcp_naming::validate_tool_name` on the
/// backend side, so it is client-safe).
///
/// # Arguments
///
/// * `dcc_type` — backend DCC bucket (`"maya"`, `"blender"`, …). Must
///   itself be within `[a-z0-9_]` because it appears as a slug prefix.
/// * `instance_id` — the backend's UUID; we collapse to the 8-char
///   short form used everywhere else in the gateway namespace for
///   visual parity.
/// * `backend_tool` — the action name the backend exposes through
///   `tools/list`.
#[must_use]
pub fn tool_slug(dcc_type: &str, instance_id: &Uuid, backend_tool: &str) -> String {
    format!(
        "{dcc}.{id8}.{tool}",
        dcc = dcc_type,
        id8 = instance_short(instance_id),
        tool = backend_tool,
    )
}

/// Split a slug produced by [`tool_slug`] back into its components.
///
/// Returns `None` if the input is not a three-segment slug with a
/// recognisable UUID/prefix middle — that is the discipline callers rely on
/// to decide whether to route via the capability index at all.
#[must_use]
pub fn parse_slug(slug: &str) -> Option<(&str, &str, &str)> {
    let first = slug.find('.')?;
    let rest = &slug[first + 1..];
    let second = rest.find('.')?;
    let dcc = &slug[..first];
    let id8 = &rest[..second];
    let tool = &rest[second + 1..];
    if dcc.is_empty() || tool.is_empty() {
        return None;
    }
    // The instance segment accepts the canonical id8, any unique prefix
    // (validated by the caller), or a full hyphenated UUID. Requiring at
    // least 4 chars prevents `foo.bar.baz` from being mistaken for a
    // capability slug while still allowing the shared resolver's prefix rule.
    if id8.len() < 4 || id8.len() > 36 {
        return None;
    }
    if id8.contains('-') {
        Uuid::parse_str(id8).ok()?;
    } else if !id8.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    Some((dcc, id8, tool))
}

/// Return `true` when the DCC type string is safe to use as the leading
/// segment of a [`tool_slug`].
///
/// The gateway never invents DCC-type strings — they come from live
/// `ServiceEntry` rows — but exotic values (`""`, `"unknown"`,
/// hyphenated) would produce an ambiguous slug and should be rejected
/// by the builder before they reach the index.
#[must_use]
pub fn is_valid_dcc_bucket(dcc_type: &str) -> bool {
    !dcc_type.is_empty() && is_cursor_safe_alphabet(dcc_type) && !dcc_type.contains('.')
}

/// Compact MCP ToolAnnotations-style hints carried in gateway search records.
///
/// These fields mirror the MCP `ToolAnnotations` names so non-MCP clients can
/// apply the same safety policy before calling a dynamic gateway capability.
///
/// Field ordering and field names are part of the REST wire contract
/// (`POST /v1/search` returns an array of `CapabilityRecord`) — adjust
/// with care and bump the REST contract docs if you change the shape.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CapabilityAnnotations {
    /// Optional human-readable title from the backend's tool annotations.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Whether the backend declares the tool as read-only.
    #[serde(rename = "readOnlyHint", skip_serializing_if = "Option::is_none")]
    pub read_only_hint: Option<bool>,
    /// Whether the backend declares the tool may perform destructive changes.
    #[serde(rename = "destructiveHint", skip_serializing_if = "Option::is_none")]
    pub destructive_hint: Option<bool>,
    /// Whether repeated calls with the same arguments should be safe.
    #[serde(rename = "idempotentHint", skip_serializing_if = "Option::is_none")]
    pub idempotent_hint: Option<bool>,
    /// Whether the tool may interact with external systems or open-world state.
    #[serde(rename = "openWorldHint", skip_serializing_if = "Option::is_none")]
    pub open_world_hint: Option<bool>,
}

impl CapabilityAnnotations {
    /// Return `true` when no annotation hint is populated.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.title.is_none()
            && self.read_only_hint.is_none()
            && self.destructive_hint.is_none()
            && self.idempotent_hint.is_none()
            && self.open_world_hint.is_none()
    }
}

/// Compact execution metadata carried in gateway search records.
///
/// Full tool schemas remain behind `describe_tool`; this metadata is small
/// enough to include in discovery responses and lets agents decide whether a
/// call needs main-thread dispatch, a timeout budget, or extra approval.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CapabilityMetadata {
    /// Host thread affinity hint, such as `main` or `any`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub affinity: Option<String>,
    /// Execution mode hint, such as `in-process` or `subprocess`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub execution: Option<String>,
    /// Suggested timeout budget in seconds.
    #[serde(rename = "timeoutHintSecs", skip_serializing_if = "Option::is_none")]
    pub timeout_hint_secs: Option<u32>,
    /// Whether the gateway/dispatcher should reject mismatched thread context.
    #[serde(
        rename = "enforceThreadAffinity",
        skip_serializing_if = "Option::is_none"
    )]
    pub enforce_thread_affinity: Option<bool>,
    /// Coarse risk label derived from safety annotations.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub risk: Option<String>,
    /// Tool semantic role (`read_only`, `action`, `destructive`,
    /// `escape_hatch`, `debug_only`) propagated from
    /// `ToolDeclaration::tool_role` (issues #1335, #1325).
    ///
    /// When equal to `"escape_hatch"`, the gateway search ranker applies
    /// a fixed demotion so generic-scripting tools rank below typed
    /// alternatives unless the agent explicitly invoked scripting.
    #[serde(rename = "toolRole", skip_serializing_if = "Option::is_none")]
    pub tool_role: Option<String>,
}

impl CapabilityMetadata {
    /// Return `true` when no execution metadata is populated.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.affinity.is_none()
            && self.execution.is_none()
            && self.timeout_hint_secs.is_none()
            && self.enforce_thread_affinity.is_none()
            && self.risk.is_none()
            && self.tool_role.is_none()
    }
}

/// Lightweight progressive tool-group metadata surfaced in search hits.
///
/// The gateway keeps this bounded: it is enough for an agent to decide whether
/// a group must be explicitly activated, while full skill prose remains behind
/// skill detail surfaces.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CapabilityGroupInfo {
    /// Group identifier unique within the owning skill.
    pub name: String,
    /// Short group summary when the skill declared one.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// Tool names declared in this group.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<String>,
    /// Whether this group becomes active on a default lazy load.
    #[serde(default)]
    pub default_active: bool,
    /// Runtime activation state when the backend knows it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active: Option<bool>,
}

/// One compact capability record.
///
/// Field ordering and field names are part of the REST wire contract
/// (`POST /v1/search` returns an array of `CapabilityRecord`) — adjust
/// with care and bump the REST contract docs if you change the shape.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityRecord {
    /// Client-visible slug used to discover / describe / call the
    /// action. Built via [`tool_slug`].
    pub tool_slug: String,
    /// Exact backend action name (no gateway encoding). This is what
    /// the backend's `tools/call` expects.
    pub backend_tool: String,
    /// Alias of [`Self::backend_tool`] with an explicit routing name for
    /// clients that distinguish display slugs from backend callables.
    pub callable_id: String,
    /// Owning skill, if the backend advertised one; `None` for
    /// actions registered without a skill (unusual but possible).
    pub skill_name: Option<String>,
    /// One-line summary drawn from the backend tool description;
    /// truncated to keep the record size bounded.
    pub summary: String,
    /// Free-form tags to help `search_tools` match on domain
    /// keywords the description itself might lack.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    /// Bounded search-only tokens derived from aliases and schemas.
    ///
    /// These improve recall without expanding the public search response.
    #[serde(default, skip_serializing, skip_deserializing)]
    pub search_tokens: Vec<String>,
    /// DCC type bucket (e.g. `"maya"`).
    pub dcc_type: String,
    /// UUID of the owning instance, serialised as a canonical string.
    pub instance_id: Uuid,
    /// True when the backend advertises a non-empty `inputSchema`.
    /// Agents should check this before calling `describe_tool` —
    /// actions with no schema can be invoked with an empty object.
    pub has_schema: bool,
    /// True when the owning backend instance is currently connected
    /// (loaded). False for records built from unloaded skill metadata.
    ///
    /// `search_tools` uses this field to allow discovering unloaded
    /// skills; the agent can then call `load_skill` before invoking
    /// the tool.
    pub loaded: bool,
    /// MCP ToolAnnotations-style safety hints propagated from the backend.
    ///
    /// Search responses keep this compact and omit it when the backend did not
    /// declare any hint. Full schemas still live behind `describe_tool`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub annotations: Option<CapabilityAnnotations>,
    /// Execution metadata that agents need before dispatching a dynamic
    /// capability: affinity, execution mode, timeout, and risk.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<CapabilityMetadata>,
    /// Progressive tool groups declared by the owning skill, when known.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub available_groups: Vec<CapabilityGroupInfo>,
    /// The progressive tool group this tool belongs to, if any.
    /// `None` means the tool is not part of a progressive group and
    /// is always callable when `loaded=true`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_group: Option<String>,
}

impl CapabilityRecord {
    /// Maximum summary length kept per record. Longer descriptions
    /// are truncated with an ellipsis before the record is inserted;
    /// anything beyond this is token cost without search value.
    pub const MAX_SUMMARY_LEN: usize = 160;

    /// Build a record by normalising `summary` to fit the size cap.
    ///
    /// The nine-argument signature is deliberate: every field of a
    /// `CapabilityRecord` is routing-relevant, and grouping them into
    /// a builder would trade one cursor of boilerplate for another.
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub fn new(
        tool_slug: String,
        backend_tool: String,
        callable_id: String,
        skill_name: Option<String>,
        summary: &str,
        tags: Vec<String>,
        dcc_type: String,
        instance_id: Uuid,
        has_schema: bool,
        loaded: bool,
        tool_group: Option<String>,
    ) -> Self {
        Self {
            tool_slug,
            backend_tool,
            callable_id,
            skill_name,
            summary: normalise_summary(summary),
            tags,
            search_tokens: Vec::new(),
            dcc_type,
            instance_id,
            has_schema,
            loaded,
            annotations: None,
            metadata: None,
            available_groups: Vec::new(),
            tool_group,
        }
    }

    /// Attach compact safety and execution metadata to this record.
    ///
    /// Empty metadata objects are filtered out so REST search responses stay
    /// compact while preserving backend-declared policy hints when present.
    #[must_use]
    pub fn with_surface_metadata(
        mut self,
        annotations: Option<CapabilityAnnotations>,
        metadata: Option<CapabilityMetadata>,
    ) -> Self {
        self.annotations = annotations.filter(|ann| !ann.is_empty());
        self.metadata = metadata.filter(|meta| !meta.is_empty());
        self
    }

    /// Attach progressive group metadata discovered from the owning backend.
    #[must_use]
    pub fn with_available_groups(mut self, groups: Vec<CapabilityGroupInfo>) -> Self {
        self.available_groups = groups
            .into_iter()
            .filter(|group| !group.name.is_empty())
            .collect();
        self
    }

    /// Attach bounded internal search tokens derived from aliases/schemas.
    #[must_use]
    pub fn with_search_tokens(mut self, tokens: Vec<String>) -> Self {
        self.search_tokens = normalise_search_tokens(tokens);
        self
    }

    /// Build a record for a tool from an **unloaded** skill's metadata.
    ///
    /// Used by the gateway's `update_unloaded_skills` refresh path to
    /// index skills that exist in the catalog but are not yet loaded.
    ///
    /// The `instance_id` is set to `Uuid::nil()` (sentinel value)
    /// because there is no backend instance yet. The `tool_slug` is
    /// still computable (it will be `dcc.00000000.tool_name`), which
    /// is fine for **search discovery** — the agent calls `load_skill`
    /// before invoking the tool, at which point the real instance's
    /// records replace these sentinel ones.
    #[must_use]
    pub fn from_skill_tool(
        skill_name: &str,
        tool_name: &str,
        tool_description: &str,
        dcc_type: &str,
        tool_group: Option<String>,
    ) -> Self {
        let nil = Uuid::nil();
        let slug = tool_slug(dcc_type, &nil, tool_name);
        Self {
            tool_slug: slug,
            backend_tool: tool_name.to_string(),
            callable_id: tool_name.to_string(),
            skill_name: Some(skill_name.to_string()),
            summary: normalise_summary(tool_description),
            tags: Vec::new(),
            search_tokens: Vec::new(),
            dcc_type: dcc_type.to_string(),
            instance_id: nil,
            has_schema: false, // unknown until loaded
            loaded: false,
            annotations: None,
            metadata: None,
            available_groups: Vec::new(),
            tool_group,
        }
    }

    /// Return `true` when the tool is currently callable.
    ///
    /// A tool is callable when:
    /// - `loaded` is `true` (the owning skill is connected), AND
    /// - the tool either has no group, or its group is active.
    #[must_use]
    pub fn is_callable(&self) -> bool {
        if !self.loaded {
            return false;
        }
        let Some(ref group_name) = self.tool_group else {
            // No group → always callable when loaded.
            return true;
        };
        // Check whether this tool's group is active.
        self.available_groups
            .iter()
            .any(|g| g.name == *group_name && g.active == Some(true))
    }

    /// Return the group name that is blocking callability, if any.
    #[must_use]
    pub fn disabled_by_group(&self) -> Option<&str> {
        if !self.loaded {
            return None; // unloaded has its own next_step
        }
        self.tool_group.as_deref().filter(|group_name| {
            !self
                .available_groups
                .iter()
                .any(|g| g.name == *group_name && g.active == Some(true))
        })
    }
}

impl dcc_mcp_gateway_search::SearchRecord for CapabilityRecord {
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

    fn search_tokens(&self) -> &[String] {
        &self.search_tokens
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

    fn tool_role(&self) -> Option<&str> {
        self.metadata.as_ref().and_then(|m| m.tool_role.as_deref())
    }

    fn risk(&self) -> Option<&str> {
        self.metadata.as_ref().and_then(|m| m.risk.as_deref())
    }
}

fn normalise_search_tokens(tokens: Vec<String>) -> Vec<String> {
    let mut out = Vec::with_capacity(tokens.len().min(64));
    let mut seen = std::collections::HashSet::new();
    for raw in tokens {
        let token = raw.split_whitespace().collect::<Vec<_>>().join(" ");
        let token = token.trim();
        if token.len() < 2 || token.len() > 64 {
            continue;
        }
        let key = token.to_ascii_lowercase();
        if seen.insert(key) {
            out.push(token.to_string());
        }
        if out.len() >= 64 {
            break;
        }
    }
    out
}

/// Truncate a description to [`CapabilityRecord::MAX_SUMMARY_LEN`] and
/// collapse internal whitespace so search scoring sees a single blob
/// per record rather than multi-line markdown.
fn normalise_summary(raw: &str) -> String {
    // Collapse any whitespace run to a single space — this makes
    // keyword search scoring deterministic regardless of how the
    // backend formatted its description.
    let mut out = String::with_capacity(raw.len().min(CapabilityRecord::MAX_SUMMARY_LEN + 3));
    let mut prev_space = false;
    for ch in raw.chars() {
        if ch.is_whitespace() {
            if !prev_space && !out.is_empty() {
                out.push(' ');
                prev_space = true;
            }
            continue;
        }
        prev_space = false;
        out.push(ch);
        if out.len() >= CapabilityRecord::MAX_SUMMARY_LEN {
            break;
        }
    }
    // Trim trailing space introduced by the collapser above.
    while out.ends_with(' ') {
        out.pop();
    }
    if raw.chars().count() > CapabilityRecord::MAX_SUMMARY_LEN {
        out.push_str("...");
    }
    out
}

#[cfg(test)]
mod unit_tests {
    use super::*;

    #[test]
    fn slug_has_three_segments() {
        let id = Uuid::parse_str("abcdef0123456789abcdef0123456789").unwrap();
        assert_eq!(
            tool_slug("maya", &id, "create_sphere"),
            "maya.abcdef01.create_sphere",
        );
    }

    #[test]
    fn slug_preserves_backend_tool_with_skill_separator_and_hyphens() {
        // Backend tool names may carry client-safe `__` and `-`
        // (skill-prefixed actions, e.g. `maya-animation__set_keyframe`).
        // The slug keeps them verbatim so reverse parsing stays
        // unambiguous — the first two dots are the fixed delimiters,
        // everything after that is the backend tool.
        let id = Uuid::parse_str("abcdef0123456789abcdef0123456789").unwrap();
        let slug = tool_slug("maya", &id, "maya-animation__set_keyframe");
        assert_eq!(slug, "maya.abcdef01.maya-animation__set_keyframe");
        let (dcc, id8, tool) = parse_slug(&slug).unwrap();
        assert_eq!(dcc, "maya");
        assert_eq!(id8, "abcdef01");
        assert_eq!(tool, "maya-animation__set_keyframe");
    }

    #[test]
    fn parse_slug_rejects_non_hex_instance_prefix() {
        // Guard: `foo.bar.baz` must not silently decode as a
        // capability slug — otherwise plain-identifier backend tool
        // names registered without a gateway wrapper could be routed
        // into the dynamic-capability path.
        assert!(parse_slug("foo.bar.baz").is_none());
        // Prefixes shorter than the shared resolver minimum are rejected.
        assert!(parse_slug("maya.abc.create_sphere").is_none());
    }

    #[test]
    fn parse_slug_accepts_uuid_prefixes_and_full_uuid() {
        assert_eq!(
            parse_slug("maya.abcd.create_sphere").unwrap(),
            ("maya", "abcd", "create_sphere")
        );
        assert_eq!(
            parse_slug("maya.abcdef0123456789abcdef0123456789.create_sphere").unwrap(),
            ("maya", "abcdef0123456789abcdef0123456789", "create_sphere")
        );
        assert_eq!(
            parse_slug("maya.abcdef01-2345-6789-abcd-ef0123456789.create_sphere").unwrap(),
            (
                "maya",
                "abcdef01-2345-6789-abcd-ef0123456789",
                "create_sphere"
            )
        );
    }

    #[test]
    fn parse_slug_rejects_missing_segments() {
        assert!(parse_slug("onlyname").is_none());
        assert!(parse_slug("one.two").is_none());
        assert!(parse_slug(".").is_none());
    }

    #[test]
    fn summary_is_truncated_and_whitespace_collapsed() {
        let long = "  Lorem  ipsum\n\tdolor sit amet ".repeat(20);
        let rec = CapabilityRecord::new(
            "x.abcdef01.a".into(),
            "a".into(),
            "a".into(),
            None,
            &long,
            Vec::new(),
            "x".into(),
            Uuid::nil(),
            false,
            false,
            None,
        );
        assert!(rec.summary.len() <= CapabilityRecord::MAX_SUMMARY_LEN + 3);
        assert!(rec.summary.ends_with("..."));
        assert!(!rec.summary.contains('\n'));
        assert!(!rec.summary.contains('\t'));
        assert!(!rec.summary.contains("  "));
    }

    #[test]
    fn summary_under_cap_is_not_suffixed() {
        let rec = CapabilityRecord::new(
            "x.abcdef01.a".into(),
            "a".into(),
            "a".into(),
            None,
            "short and sweet",
            Vec::new(),
            "x".into(),
            Uuid::nil(),
            false,
            false,
            None,
        );
        assert_eq!(rec.summary, "short and sweet");
    }

    #[test]
    fn dcc_bucket_validation_is_strict() {
        assert!(is_valid_dcc_bucket("maya"));
        assert!(is_valid_dcc_bucket("blender"));
        assert!(is_valid_dcc_bucket("python"));
        // Empty and dotted DCC buckets would corrupt the slug shape.
        assert!(!is_valid_dcc_bucket(""));
        assert!(!is_valid_dcc_bucket("my.dcc"));
        // Hyphens fail the cursor-safe alphabet — agent clients would
        // filter `maya-adapter.<id8>.<tool>` on sight.
        assert!(!is_valid_dcc_bucket("maya-adapter"));
    }

    #[test]
    fn from_skill_tool_uses_nil_uuid() {
        let rec = CapabilityRecord::from_skill_tool(
            "animation",
            "set_keyframe",
            "Set a keyframe at the current time.",
            "maya",
            None,
        );
        assert_eq!(rec.instance_id, Uuid::nil());
        assert_eq!(rec.tool_slug, "maya.00000000.set_keyframe");
        assert_eq!(rec.skill_name.as_deref(), Some("animation"));
        assert!(!rec.loaded);
        assert!(!rec.has_schema);
    }

    #[test]
    fn record_roundtrip_json_preserves_wire_shape() {
        let rec = CapabilityRecord::new(
            "maya.abcdef01.create_sphere".into(),
            "create_sphere".into(),
            "create_sphere".into(),
            Some("modeling".into()),
            "Create a polygonal sphere.",
            vec!["3d".into(), "primitive".into()],
            "maya".into(),
            Uuid::parse_str("abcdef0123456789abcdef0123456789").unwrap(),
            true,
            true,
            None,
        );
        let s = serde_json::to_string(&rec).unwrap();
        let back: CapabilityRecord = serde_json::from_str(&s).unwrap();
        assert_eq!(rec, back);

        // Empty `tags` must skip serialisation per `skip_serializing_if`
        // so the wire stays compact.
        let bare = CapabilityRecord::new(
            "x.abcdef01.a".into(),
            "a".into(),
            "a".into(),
            None,
            "",
            Vec::new(),
            "x".into(),
            Uuid::nil(),
            false,
            false,
            None,
        );
        let s = serde_json::to_string(&bare).unwrap();
        assert!(!s.contains("\"tags\""), "bare record JSON: {s}");
    }
}
