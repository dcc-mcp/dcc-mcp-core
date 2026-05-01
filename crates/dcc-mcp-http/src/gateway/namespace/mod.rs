//! Tool-name namespace helpers for the aggregating gateway.
//!
//! ## Per-DCC server: proactive `<skill>.<name>` namespacing (#238)
//!
//! Non-core tools registered from a skill use `<skill-name>.<tool-name>` format
//! (e.g. `maya-animation.set_keyframe`) so the AI agent immediately sees which
//! skill a tool belongs to.
//!
//! ## Per-DCC server: bare-name mode (#307)
//!
//! When enabled via [`crate::McpHttpConfig::bare_tool_names`] (default `true`),
//! the server publishes tools under their **bare action name** whenever no
//! other skill on the same instance registers the same bare name. Collisions
//! fall back to `<skill>.<action>` and log a one-shot warning. This cuts the
//! `tools/list` token footprint by ~40% on Maya-sized skill sets without
//! breaking routing — [`handle_tools_call`](crate::handler) accepts both forms
//! for one release cycle (same policy as SEP-986 #258/#261).
//!
//! ## Gateway: `i_<id8>__<escaped>` Cursor-safe encoding (#656)
//!
//! The aggregating gateway emits an 8-hex-char instance id so duplicate
//! tool names across multiple DCC backends remain addressable. Up to and
//! including #258/#261 the separator was `.` (SEP-986 legal), but clients
//! such as Cursor filter out any tool name containing characters outside
//! `[A-Za-z0-9_]`, which silently hides the entire backend surface. The
//! `i_<id8>__<escaped_tool>` form published since #656 stays inside that
//! stricter alphabet by escaping `.` / `-` / `_` with the reversible
//! `_D_` / `_H_` / `_U_` triples — see [`encode::escape_cursor_safe`].
//!
//! Decoder accepts four historical encodings during the one-version
//! compatibility window. The three legacy forms emit a
//! `tracing::warn!` so operators notice any client that has not rolled
//! over yet.
//!
//! | Form | Status |
//! |------|--------|
//! | `i_{id8}__{escaped_tool}` | **Preferred** (#656) — current emitter, Cursor-safe |
//! | `{id8}.{tool}` | Accepted during the #656 compatibility window — the pre-#656 SEP-986 form |
//! | `{id8}/{tool}` | Deprecated — previous unreleased build, decoded + warned |
//! | `{id8}__{tool}` | Legacy — pre-#258, decoded + warned |
//!
//! ## Maintainer layout
//!
//! `namespace.rs` is a thin facade; implementation lives in focused siblings:
//!
//! - [`namespace_constants`](self::constants) — name lists, separators, prefix predicates
//! - [`namespace_encode`](self::encode) — encoder / decoder helpers (skill + gateway forms)
//! - [`namespace_bare`](self::bare) — `resolve_bare_names` + one-shot warn helpers (#307)
//! - `namespace_tests.rs` — unit tests (compiled only under `#[cfg(test)]`)

mod bare;
mod constants;
mod encode;

pub use bare::{BareNameInput, resolve_bare_names, warn_legacy_prefixed_once};
pub use constants::{
    CORE_TOOL_NAMES, CURSOR_SAFE_PREFIX, CURSOR_SAFE_SEP, DEPRECATED_SLASH_SEP,
    GATEWAY_LOCAL_TOOLS, ID_PREFIX_LEN, INSTANCE_SEP, LEGACY_NAMESPACE_SEP, SKILL_TOOL_SEP,
    instance_short, is_core_tool, is_local_tool,
};
pub use encode::{
    assert_gateway_tool_name, decode_skill_tool_name, decode_tool_name, encode_tool_name,
    encode_tool_name_cursor_safe, escape_cursor_safe, extract_bare_tool_name,
    is_cursor_safe_alphabet, skill_tool_name, unescape_cursor_safe,
};

#[cfg(test)]
pub use bare::__reset_warn_state_for_tests;

#[cfg(test)]
mod tests;
