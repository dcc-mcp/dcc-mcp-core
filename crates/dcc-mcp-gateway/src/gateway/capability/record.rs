//! Capability record — the unit of the gateway index.
//!
//! One `CapabilityRecord` describes one addressable backend action.
//! Records are intentionally compact (~200 B serialised) so the index
//! can hold thousands of entries without blowing up the gateway
//! process's working set.
//!
//! The record does **not** carry the full JSON Schema of the action;
//! schemas are pulled on demand by the `describe_tool` wrapper and
//! cached by the backend's tool cache.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::gateway::namespace::{instance_short, is_cursor_safe_alphabet};

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
/// The DCC type and id8 prefix are always cursor-safe (a-z0-9) by
/// construction; `backend_tool` is copied verbatim (it was already
/// validated through [`dcc_mcp_naming::validate_tool_name`] on the
/// backend side, so it is SEP-986-legal).
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
/// recognisable 8-hex middle — that is the discipline callers rely on
/// to decide whether to route via the capability index at all.
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
    // id8 must look like an 8-char hex prefix; this is the guard that
    // prevents `foo.bar.baz` from being mistaken for a capability slug.
    if id8.len() != 8 || !id8.chars().all(|c| c.is_ascii_hexdigit()) {
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
pub fn is_valid_dcc_bucket(dcc_type: &str) -> bool {
    !dcc_type.is_empty() && is_cursor_safe_alphabet(dcc_type) && !dcc_type.contains('.')
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
    /// DCC type bucket (e.g. `"maya"`).
    pub dcc_type: String,
    /// UUID of the owning instance, serialised as a canonical string.
    pub instance_id: Uuid,
    /// True when the backend advertises a non-empty `inputSchema`.
    /// Agents should check this before calling `describe_tool` —
    /// actions with no schema can be invoked with an empty object.
    pub has_schema: bool,
}

impl CapabilityRecord {
    /// Maximum summary length kept per record. Longer descriptions
    /// are truncated with an ellipsis before the record is inserted;
    /// anything beyond this is token cost without search value.
    pub const MAX_SUMMARY_LEN: usize = 160;

    /// Build a record by normalising `summary` to fit the size cap.
    ///
    /// The eight-argument signature is deliberate: every field of a
    /// `CapabilityRecord` is routing-relevant, and grouping them into
    /// a builder would trade one cursor of boilerplate for another.
    #[allow(clippy::too_many_arguments)]
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
    ) -> Self {
        Self {
            tool_slug,
            backend_tool,
            callable_id,
            skill_name,
            summary: normalise_summary(summary),
            tags,
            dcc_type,
            instance_id,
            has_schema,
        }
    }
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
    fn slug_preserves_backend_tool_with_dots_and_hyphens() {
        // Backend tool names may carry SEP-986-legal `.` and `-`
        // (skill-prefixed actions, e.g. `maya-animation.set_keyframe`).
        // The slug keeps them verbatim so reverse parsing stays
        // unambiguous — the first two dots are the fixed delimiters,
        // everything after that is the backend tool.
        let id = Uuid::parse_str("abcdef0123456789abcdef0123456789").unwrap();
        let slug = tool_slug("maya", &id, "maya-animation.set_keyframe");
        assert_eq!(slug, "maya.abcdef01.maya-animation.set_keyframe");
        let (dcc, id8, tool) = parse_slug(&slug).unwrap();
        assert_eq!(dcc, "maya");
        assert_eq!(id8, "abcdef01");
        assert_eq!(tool, "maya-animation.set_keyframe");
    }

    #[test]
    fn parse_slug_rejects_non_hex_instance_prefix() {
        // Guard: `foo.bar.baz` must not silently decode as a
        // capability slug — otherwise plain-identifier backend tool
        // names registered without a gateway wrapper could be routed
        // into the dynamic-capability path.
        assert!(parse_slug("foo.bar.baz").is_none());
        // 7 hex chars is still wrong — the id8 prefix is always 8.
        assert!(parse_slug("maya.abcdef0.create_sphere").is_none());
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
}
