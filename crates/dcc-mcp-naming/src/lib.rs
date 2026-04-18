//! # dcc-mcp-naming
//!
//! Single source of truth for the two naming rules used across the whole
//! DCC-MCP ecosystem:
//!
//! * **Tool name** — the wire-visible string published in MCP `tools/list`.
//!   Must match [`TOOL_NAME_RE`].
//! * **Action id** — the internal, stable identifier used by Rust hosts and
//!   Python bridges to route `tools/call`. Must match [`ACTION_ID_RE`].
//!
//! ## Specs
//!
//! * [MCP `draft/server/tools#tool-names`](https://modelcontextprotocol.io/specification/draft/server/tools#tool-names)
//! * [SEP-986](https://github.com/modelcontextprotocol/modelcontextprotocol/issues/986)
//!   merged in [modelcontextprotocol#1603](https://github.com/modelcontextprotocol/modelcontextprotocol/pull/1603)
//!
//! The MCP spec allows up to 128 characters for a tool name; this crate caps
//! at **48** to leave room for namespace prefixes added by gateways (e.g.
//! `{id8}/` or `{skill}.`).
//!
//! Every other crate in this repository — `dcc-mcp-actions`,
//! `dcc-mcp-skills`, `dcc-mcp-http`, the Python wheel, macros, docs — **must**
//! go through these validators. Re-inventing the regex in another place is a
//! bug.

#![deny(missing_docs)]

use thiserror::Error;

#[cfg(feature = "python-bindings")]
pub mod python;

/// MCP wire-visible tool-name regex.
///
/// Syntax (see module docs): starts with ASCII alphanumeric, then up to 47
/// more ASCII alphanumeric / `_` / `.` / `-` characters, total length ≤ 48.
///
/// The trailing character class intentionally excludes `/`, `:`, space, `,`,
/// `@`, `+` and every other punctuation mark — these are reserved for
/// transport-layer composition (gateway prefixes, MCP URIs, etc.).
pub const TOOL_NAME_RE: &str = r"^[A-Za-z0-9](?:[A-Za-z0-9_.\-]{0,47})$";

/// Internal action-id regex.
///
/// A `.`-separated chain of lowercase-identifier segments (`[a-z][a-z0-9_]*`).
/// This is the canonical host-side identifier passed to tool dispatchers.
///
/// Examples: `scene.get_info`, `geometry.create_sphere`, `maya.render`.
pub const ACTION_ID_RE: &str = r"^[a-z][a-z0-9_]*(?:\.[a-z][a-z0-9_]*)*$";

/// Maximum length enforced on tool names (MCP spec allows 128, we cap lower
/// to leave room for gateway prefixes).
pub const MAX_TOOL_NAME_LEN: usize = 48;

/// Reasons a name can fail validation.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum NamingError {
    /// Name is the empty string.
    #[error("name must not be empty")]
    Empty,

    /// Name exceeds the per-rule length cap.
    #[error("name is {actual} chars, max is {max}")]
    TooLong {
        /// Length observed.
        actual: usize,
        /// Cap allowed.
        max: usize,
    },

    /// Name contains a non-ASCII character.
    #[error("name contains non-ASCII character {ch:?} at byte offset {offset}")]
    NonAscii {
        /// First offending character.
        ch: char,
        /// Byte offset into the input where it was seen.
        offset: usize,
    },

    /// Name's first character is not in the allowed leading class.
    #[error("name must start with an ASCII alphanumeric, got {ch:?}")]
    BadLeadingChar {
        /// First character observed.
        ch: char,
    },

    /// Name contains a character that is not in the allowed set.
    #[error("name contains disallowed character {ch:?} at byte offset {offset}")]
    BadChar {
        /// Offending character.
        ch: char,
        /// Byte offset into the input where it was seen.
        offset: usize,
    },

    /// Action-id contains an empty segment (e.g. `foo..bar` or `.foo`).
    #[error("action id has an empty `.`-separated segment")]
    EmptySegment,
}

// ── tool-name ───────────────────────────────────────────────────────────────

/// Validate an MCP tool name against [`TOOL_NAME_RE`] + the 48-char cap.
///
/// Runs in `O(n)` and does not allocate. Prefer this over hand-rolled checks.
///
/// # Errors
///
/// Returns a [`NamingError`] describing the *first* violation found.
///
/// # Examples
///
/// ```
/// use dcc_mcp_naming::validate_tool_name;
/// assert!(validate_tool_name("geometry.create_sphere").is_ok());
/// assert!(validate_tool_name("hello-world.greet").is_ok());
/// assert!(validate_tool_name("").is_err());
/// assert!(validate_tool_name("bad/name").is_err());
/// assert!(validate_tool_name("_leading").is_err());
/// ```
pub fn validate_tool_name(s: &str) -> Result<(), NamingError> {
    if s.is_empty() {
        return Err(NamingError::Empty);
    }
    if s.len() > MAX_TOOL_NAME_LEN {
        return Err(NamingError::TooLong {
            actual: s.len(),
            max: MAX_TOOL_NAME_LEN,
        });
    }
    let mut chars = s.char_indices();
    // Leading char: must be ASCII alphanumeric.
    let (_, first) = chars.next().expect("length checked above");
    if !first.is_ascii() {
        return Err(NamingError::NonAscii {
            ch: first,
            offset: 0,
        });
    }
    if !first.is_ascii_alphanumeric() {
        return Err(NamingError::BadLeadingChar { ch: first });
    }
    // Remainder: [A-Za-z0-9_.-]
    for (offset, ch) in chars {
        if !ch.is_ascii() {
            return Err(NamingError::NonAscii { ch, offset });
        }
        let ok = ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.' | '-');
        if !ok {
            return Err(NamingError::BadChar { ch, offset });
        }
    }
    Ok(())
}

// ── action-id ───────────────────────────────────────────────────────────────

/// Validate an internal action id against [`ACTION_ID_RE`].
///
/// Runs in `O(n)` and does not allocate.
///
/// # Errors
///
/// Returns a [`NamingError`] on the first offending character / segment.
///
/// # Examples
///
/// ```
/// use dcc_mcp_naming::validate_action_id;
/// assert!(validate_action_id("scene.get_info").is_ok());
/// assert!(validate_action_id("maya.render").is_ok());
/// assert!(validate_action_id("Scene.Get_Info").is_err()); // uppercase
/// assert!(validate_action_id("scene..get").is_err()); // empty segment
/// assert!(validate_action_id("1scene.get").is_err()); // leading digit
/// ```
pub fn validate_action_id(s: &str) -> Result<(), NamingError> {
    if s.is_empty() {
        return Err(NamingError::Empty);
    }
    // Walk segments without allocating.
    let mut segment_start = 0usize;
    let mut in_segment = false;
    for (offset, ch) in s.char_indices() {
        if !ch.is_ascii() {
            return Err(NamingError::NonAscii { ch, offset });
        }
        if ch == '.' {
            if !in_segment {
                return Err(NamingError::EmptySegment);
            }
            in_segment = false;
            segment_start = offset + 1;
            continue;
        }
        if !in_segment {
            // First char of a segment must be `[a-z]`.
            if !ch.is_ascii_lowercase() {
                return Err(NamingError::BadLeadingChar { ch });
            }
            in_segment = true;
            let _ = segment_start; // silence unused warning on non-debug builds
        } else {
            // Trailing chars: `[a-z0-9_]`.
            let ok = ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_';
            if !ok {
                return Err(NamingError::BadChar { ch, offset });
            }
        }
    }
    if !in_segment {
        // Trailing `.` with no segment after it.
        return Err(NamingError::EmptySegment);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── tool-name: positive cases ───────────────────────────────────────────

    #[test]
    fn tool_name_accepts_simple_identifier() {
        assert!(validate_tool_name("create_sphere").is_ok());
    }

    #[test]
    fn tool_name_accepts_dotted() {
        assert!(validate_tool_name("geometry.create_sphere").is_ok());
        assert!(validate_tool_name("scene.object.transform").is_ok());
    }

    #[test]
    fn tool_name_accepts_hyphens() {
        assert!(validate_tool_name("hello-world.greet").is_ok());
    }

    #[test]
    fn tool_name_accepts_mixed_case() {
        assert!(validate_tool_name("CamelCaseTool").is_ok());
    }

    #[test]
    fn tool_name_accepts_single_char() {
        assert!(validate_tool_name("a").is_ok());
        assert!(validate_tool_name("Z").is_ok());
        assert!(validate_tool_name("0").is_ok());
    }

    #[test]
    fn tool_name_accepts_exactly_max_len() {
        let s: String = std::iter::repeat_n('a', MAX_TOOL_NAME_LEN).collect();
        assert_eq!(s.len(), MAX_TOOL_NAME_LEN);
        assert!(validate_tool_name(&s).is_ok());
    }

    // ── tool-name: negative cases ───────────────────────────────────────────

    #[test]
    fn tool_name_rejects_empty() {
        assert_eq!(validate_tool_name(""), Err(NamingError::Empty));
    }

    #[test]
    fn tool_name_rejects_over_max_len() {
        let s: String = std::iter::repeat_n('a', MAX_TOOL_NAME_LEN + 1).collect();
        assert!(matches!(
            validate_tool_name(&s),
            Err(NamingError::TooLong { .. })
        ));
    }

    #[test]
    fn tool_name_rejects_leading_hyphen_dot_underscore() {
        for bad in ["-tool", ".tool", "_tool"] {
            assert!(matches!(
                validate_tool_name(bad),
                Err(NamingError::BadLeadingChar { .. })
            ));
        }
    }

    #[test]
    fn tool_name_rejects_forbidden_chars() {
        for bad in [
            "tool/call",
            "ns:tool",
            "tool name",
            "tool,other",
            "tool@host",
            "tool+v2",
            "tool?",
            "tool!",
            "tool#1",
        ] {
            assert!(
                matches!(validate_tool_name(bad), Err(NamingError::BadChar { .. })),
                "should reject {bad:?}"
            );
        }
    }

    #[test]
    fn tool_name_rejects_non_ascii() {
        assert!(matches!(
            validate_tool_name("tôol"),
            Err(NamingError::NonAscii { .. })
        ));
        assert!(matches!(
            validate_tool_name("工具"),
            Err(NamingError::NonAscii { .. })
        ));
    }

    // ── action-id: positive cases ───────────────────────────────────────────

    #[test]
    fn action_id_accepts_single_segment() {
        assert!(validate_action_id("scene").is_ok());
        assert!(validate_action_id("create_sphere").is_ok());
    }

    #[test]
    fn action_id_accepts_dotted_segments() {
        assert!(validate_action_id("scene.get_info").is_ok());
        assert!(validate_action_id("maya.geometry.create_sphere").is_ok());
    }

    #[test]
    fn action_id_accepts_digits_after_leader() {
        assert!(validate_action_id("v2.create").is_ok());
        assert!(validate_action_id("scene.frame_3d").is_ok());
    }

    // ── action-id: negative cases ───────────────────────────────────────────

    #[test]
    fn action_id_rejects_empty() {
        assert_eq!(validate_action_id(""), Err(NamingError::Empty));
    }

    #[test]
    fn action_id_rejects_uppercase() {
        assert!(matches!(
            validate_action_id("Scene.get"),
            Err(NamingError::BadLeadingChar { .. })
        ));
        assert!(matches!(
            validate_action_id("scene.Get"),
            Err(NamingError::BadLeadingChar { .. })
        ));
        assert!(matches!(
            validate_action_id("scene.getInfo"),
            Err(NamingError::BadChar { .. })
        ));
    }

    #[test]
    fn action_id_rejects_leading_digit() {
        assert!(matches!(
            validate_action_id("1scene.get"),
            Err(NamingError::BadLeadingChar { .. })
        ));
        assert!(matches!(
            validate_action_id("scene.1get"),
            Err(NamingError::BadLeadingChar { .. })
        ));
    }

    #[test]
    fn action_id_rejects_empty_segments() {
        for bad in [".scene", "scene.", "scene..get"] {
            assert!(
                matches!(validate_action_id(bad), Err(NamingError::EmptySegment)),
                "should reject {bad:?}"
            );
        }
    }

    #[test]
    fn action_id_rejects_hyphen_and_other_punct() {
        for bad in ["scene-get", "scene/get", "scene get", "scene@host"] {
            assert!(
                matches!(validate_action_id(bad), Err(NamingError::BadChar { .. })),
                "should reject {bad:?}"
            );
        }
    }

    #[test]
    fn action_id_rejects_non_ascii() {
        assert!(matches!(
            validate_action_id("scene.fü"),
            Err(NamingError::NonAscii { .. })
        ));
    }

    // ── regex constants stay aligned with validators ────────────────────────

    #[test]
    fn tool_name_regex_constant_is_anchored() {
        // Smoke-check: the constant is anchored so downstream tooling can
        // drop it straight into a regex engine without wrapping. The
        // handwritten validator above IS the authoritative check; this
        // regex is only surfaced for docs/schema generators.
        assert!(TOOL_NAME_RE.starts_with('^'));
        assert!(TOOL_NAME_RE.ends_with('$'));
    }

    #[test]
    fn action_id_regex_constant_is_anchored() {
        assert!(ACTION_ID_RE.starts_with('^'));
        assert!(ACTION_ID_RE.ends_with('$'));
    }
}
