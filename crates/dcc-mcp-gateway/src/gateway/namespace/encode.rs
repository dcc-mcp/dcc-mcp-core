//! Tool-name encoder / decoder helpers.
//!
//! Covers the three naming surfaces of the gateway namespace:
//!
//! * **Skill → tool** (`<skill>.<tool>`): [`extract_bare_tool_name`],
//!   [`skill_tool_name`], [`decode_skill_tool_name`].
//! * **Gateway instance prefix**: [`encode_tool_name`] (SEP-986 form
//!   `{id8}.{tool}`), [`encode_tool_name_cursor_safe`] (client-safe form
//!   `i_{id8}__{escaped_tool}`, issue #656), [`decode_tool_name`] (accepts
//!   every form below), [`assert_gateway_tool_name`].
//!
//! Backward compatibility: [`decode_tool_name`] still accepts three
//! deprecated separator forms (`.`, `/`, and `__`) — `.` is the SEP-986
//! form that predates the Cursor-safe rollout but stays legal during the
//! compatibility window; the other two emit a `tracing::warn!` every time
//! they are decoded.

use uuid::Uuid;

use super::constants::{
    CURSOR_SAFE_PREFIX, CURSOR_SAFE_SEP, DEPRECATED_SLASH_SEP, INSTANCE_SEP, LEGACY_NAMESPACE_SEP,
    SKILL_TOOL_SEP, instance_short, is_core_tool, is_instance_prefix, is_local_tool,
};

/// Extract the bare tool name from an internal action name.
///
/// # Examples
/// ```
/// # use dcc_mcp_gateway::gateway::namespace::extract_bare_tool_name;
/// assert_eq!(extract_bare_tool_name("maya-animation", "maya_animation__set_keyframe"),
///            "set_keyframe");
/// assert_eq!(extract_bare_tool_name("", "get_scene_info"), "get_scene_info");
/// ```
pub fn extract_bare_tool_name<'a>(skill_name: &str, action_name: &'a str) -> &'a str {
    if skill_name.is_empty() {
        return action_name;
    }
    let prefix = format!("{}__", skill_name.replace('-', "_"));
    action_name
        .strip_prefix(prefix.as_str())
        .unwrap_or(action_name)
}

/// Build the proactive `<skill-name>.<tool-name>` MCP name.
///
/// # Examples
/// ```
/// # use dcc_mcp_gateway::gateway::namespace::skill_tool_name;
/// assert_eq!(skill_tool_name("maya-animation", "maya_animation__set_keyframe"),
///            Some("maya-animation.set_keyframe".to_string()));
/// assert_eq!(skill_tool_name("", "set_keyframe"), None);
/// ```
pub fn skill_tool_name(skill_name: &str, action_name: &str) -> Option<String> {
    if skill_name.is_empty() {
        return None;
    }
    let bare = extract_bare_tool_name(skill_name, action_name);
    if is_core_tool(bare) || bare.contains(SKILL_TOOL_SEP) {
        return None;
    }
    Some(format!("{skill_name}{SKILL_TOOL_SEP}{bare}"))
}

/// Decode a `<skill>.<tool>` pair from a per-DCC tool name.
///
/// Rejects gateway-encoded names (`{id8}.<rest>` with an 8-hex prefix) and
/// skill stubs (`__skill__...`).
pub fn decode_skill_tool_name(namespaced: &str) -> Option<(&str, &str)> {
    if namespaced.starts_with("__") || namespaced.contains('/') {
        return None;
    }
    // Reject gateway-encoded form — the gateway prefix owns the first dot.
    if let Some((head, _)) = namespaced.split_once(SKILL_TOOL_SEP) {
        if is_instance_prefix(head) {
            return None;
        }
    }
    namespaced.split_once(SKILL_TOOL_SEP)
}

/// Encode a tool name for gateway aggregation: `{id8}.{original}`.
///
/// # Panics (debug builds only)
///
/// In debug builds the result is checked against
/// [`dcc_mcp_naming::validate_tool_name`]; the gateway never emits a name that
/// fails SEP-986. Release builds skip the check for zero overhead — invalid
/// names would have been caught at registration time (see
/// [`assert_gateway_tool_name`]).
pub fn encode_tool_name(id: &Uuid, original: &str) -> String {
    let encoded = format!("{}{INSTANCE_SEP}{original}", instance_short(id));
    debug_assert!(
        dcc_mcp_naming::validate_tool_name(&encoded).is_ok(),
        "gateway emitted tool name {encoded:?} that violates SEP-986"
    );
    encoded
}

/// Validate a tool name the gateway is about to publish.
///
/// Used by the registration path as a hard gate: if the composed name would
/// be rejected by a compliant MCP client, we refuse to register it rather
/// than ship it and watch the LLM client 400 at runtime.
///
/// # Errors
///
/// Propagates [`dcc_mcp_naming::NamingError`] unchanged.
pub fn assert_gateway_tool_name(name: &str) -> Result<(), dcc_mcp_naming::NamingError> {
    dcc_mcp_naming::validate_tool_name(name)
}

/// Decode a gateway-encoded tool name into `(id8, original)`.
///
/// Accepts the Cursor-safe form `i_<id8>__<escaped>` introduced in #656,
/// the SEP-986 `.` separator that predates it, plus two deprecated
/// encodings for backward compat (`/` and `__`); the three non-preferred
/// forms emit a `tracing::warn!` so operators notice leftover clients.
///
/// The returned `original` is always the **un-escaped** backend tool
/// name, regardless of which wire form carried it. That way every
/// caller (`route_tools_call`, diagnostics) can route the decoded name
/// through the same lookup path without worrying about which client
/// emitted it.
pub fn decode_tool_name(prefixed: &str) -> Option<(String, String)> {
    if is_local_tool(prefixed) {
        return None;
    }

    // 1. Preferred (#656): `i_{id8}__{escaped}`. When the shape matches
    //    the cursor-safe wire form we commit to this arm: either the
    //    payload unescapes cleanly and we return it, or the input is a
    //    malformed cursor-safe name and we refuse to decode rather
    //    than fall through to a legacy arm (which would happily
    //    rewrite `i_abcdef01__bad_` into `(i, abcdef01__bad_)` and
    //    silently route to the wrong tool).
    if let Some(rest) = prefixed.strip_prefix(CURSOR_SAFE_PREFIX) {
        if let Some((p, escaped)) = rest.split_once(CURSOR_SAFE_SEP) {
            if is_instance_prefix(p) {
                return unescape_cursor_safe(escaped).map(|u| (p.to_string(), u));
            }
        }
    }

    // 2. SEP-986 dot form: `{id8}.{tool}`. Still legal during the
    //    compatibility window so existing clients keep working while
    //    rollout is in progress.
    if let Some((p, r)) = prefixed.split_once(INSTANCE_SEP) {
        if is_instance_prefix(p) {
            return Some((p.to_string(), r.to_string()));
        }
    }

    // 3. Deprecated: `{id8}/{tool}` — the unreleased format fixed in #261.
    if let Some((p, r)) = prefixed.split_once(DEPRECATED_SLASH_SEP) {
        if is_instance_prefix(p) {
            tracing::warn!(
                tool = prefixed,
                "Deprecated `/` gateway separator (pre-#261). Use `i_{{id8}}__{{tool}}`."
            );
            return Some((p.to_string(), r.to_string()));
        }
    }

    // 4. Legacy: `{id8}__{tool}` — pre-#258. Deliberately checked last:
    //    the cursor-safe form also uses `__`, but its `i_` prefix
    //    disambiguates it above; without that prefix the `__` arm is
    //    only the pre-#258 shape.
    if let Some((p, r)) = prefixed.split_once(LEGACY_NAMESPACE_SEP) {
        if is_instance_prefix(p) {
            tracing::warn!(
                tool = prefixed,
                "Deprecated `__` gateway separator (pre-#258). Use `i_{{id8}}__{{tool}}`."
            );
            return Some((p.to_string(), r.to_string()));
        }
    }
    None
}

// ── Cursor-safe encoding (#656) ───────────────────────────────────────────

/// Encode a tool name for gateway aggregation in the Cursor-safe form
/// `i_<id8>__<escaped_tool>`.
///
/// Unlike [`encode_tool_name`], the output of this function contains
/// only characters from the stricter `^[A-Za-z0-9_]+$` alphabet that
/// clients such as Cursor enforce — so the emitted name survives both
/// the MCP SEP-986 validator and the more restrictive client-side
/// regex.
///
/// The escape vocabulary is `_U_` → `_`, `_D_` → `.`, `_H_` → `-`; all
/// other ASCII-alphanumeric bytes pass through unchanged. The encoding
/// is total over every backend tool name that validates against
/// [`dcc_mcp_naming::validate_tool_name`], and is reversed byte-for-byte
/// by [`decode_tool_name`] / [`unescape_cursor_safe`].
///
/// # Panics (debug builds only)
///
/// The produced name is checked against [`dcc_mcp_naming::validate_tool_name`]
/// and against the stricter cursor-safe regex; the gateway must never
/// emit something a compliant client would reject. Release builds skip
/// the assertion — the registration path should have caught the input
/// via [`assert_gateway_tool_name`] already.
pub fn encode_tool_name_cursor_safe(id: &Uuid, original: &str) -> String {
    let escaped = escape_cursor_safe(original);
    let encoded = format!(
        "{CURSOR_SAFE_PREFIX}{}{CURSOR_SAFE_SEP}{escaped}",
        instance_short(id)
    );
    debug_assert!(
        dcc_mcp_naming::validate_tool_name(&encoded).is_ok(),
        "gateway emitted cursor-safe tool name {encoded:?} that violates SEP-986"
    );
    debug_assert!(
        is_cursor_safe_alphabet(&encoded),
        "gateway emitted cursor-safe tool name {encoded:?} with characters outside [A-Za-z0-9_]"
    );
    encoded
}

/// Return `true` iff every byte of `s` is in the Cursor-safe alphabet
/// `[A-Za-z0-9_]`. Used as a cheap guard in debug assertions and in
/// tests — the stricter regex some MCP clients enforce.
pub fn is_cursor_safe_alphabet(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

/// Escape a backend tool name so it contains only `[A-Za-z0-9_]`.
///
/// Mapping (reversed by [`unescape_cursor_safe`]):
///
/// | Input byte | Escape |
/// |------------|--------|
/// | `_`        | `_U_`  |
/// | `.`        | `_D_`  |
/// | `-`        | `_H_`  |
///
/// The escape is defined only on the alphabet permitted by
/// [`dcc_mcp_naming::validate_tool_name`] (`[A-Za-z0-9_.\-]`); any other
/// byte is a bug in the caller's registration path and will be
/// asserted out in debug builds.
pub fn escape_cursor_safe(s: &str) -> String {
    // Heuristic: most names pass through; pre-size conservatively to
    // avoid reallocation on the common short-name case.
    let mut out = String::with_capacity(s.len() + 4);
    for b in s.bytes() {
        match b {
            b'_' => out.push_str("_U_"),
            b'.' => out.push_str("_D_"),
            b'-' => out.push_str("_H_"),
            b if b.is_ascii_alphanumeric() => out.push(b as char),
            _ => {
                // Any other byte is outside the SEP-986 alphabet and
                // should have been rejected at tool registration time.
                // Escape it to a deterministic `_X<hex>_` envelope so
                // the encoder never panics in release builds, but
                // trip the debug assertion loud and early.
                debug_assert!(
                    false,
                    "escape_cursor_safe called with byte {b:#04x} outside SEP-986 alphabet",
                );
                out.push_str(&format!("_X{b:02X}_"));
            }
        }
    }
    out
}

/// Reverse of [`escape_cursor_safe`]. Returns `None` on malformed input
/// (lone `_` not part of a recognised escape triple, unknown escape
/// letter) so a bad cursor-safe name never silently decodes to a
/// different backend tool.
pub fn unescape_cursor_safe(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'_' {
            // Every `_` must start a 3-byte escape `_?_`. Anything
            // else is invalid because the encoder never emits a bare
            // `_`.
            if i + 2 >= bytes.len() || bytes[i + 2] != b'_' {
                return None;
            }
            match bytes[i + 1] {
                b'U' => out.push('_'),
                b'D' => out.push('.'),
                b'H' => out.push('-'),
                b'X' => {
                    // `_X<hex>_` recovery escape for bytes that were
                    // never supposed to be registered. Two hex digits
                    // follow the `X`, so the escape is 5 bytes total.
                    if i + 4 >= bytes.len() || bytes[i + 4] != b'_' {
                        return None;
                    }
                    let hi = hex_nibble(bytes[i + 2])?;
                    let lo = hex_nibble(bytes[i + 3])?;
                    out.push((hi * 16 + lo) as char);
                    i += 5;
                    continue;
                }
                _ => return None,
            }
            i += 3;
        } else if b.is_ascii_alphanumeric() {
            out.push(b as char);
            i += 1;
        } else {
            // The cursor-safe wire alphabet is `[A-Za-z0-9_]`; any
            // other byte here means the caller handed us a name that
            // was never produced by `escape_cursor_safe`.
            return None;
        }
    }
    Some(out)
}

fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'A'..=b'F' => Some(b - b'A' + 10),
        b'a'..=b'f' => Some(b - b'a' + 10),
        _ => None,
    }
}
