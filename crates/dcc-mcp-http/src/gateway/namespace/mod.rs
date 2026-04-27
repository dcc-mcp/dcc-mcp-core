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
//! ## Gateway: `<id8>.<tool>` instance prefix (#261)
//!
//! The aggregating gateway prepends an 8-hex-char instance id so duplicate
//! tool names across multiple DCC backends remain addressable. The chosen
//! separator is **`.` (dot)** because [SEP-986](
//! https://github.com/modelcontextprotocol/modelcontextprotocol/pull/1603)
//! restricts MCP tool names to `[A-Za-z0-9_.-]`, 1–128 chars — `/` is **not**
//! legal. Major LLM clients (Anthropic, OpenAI, Cursor) apply even stricter
//! regexes and will reject names containing `/` outright.
//!
//! Decoder accepts three historical encodings for one-version backward
//! compatibility (each with a `tracing::warn!` on the legacy forms):
//!
//! | Form | Status |
//! |------|--------|
//! | `{id8}.{tool}` | **Preferred** — current emitter |
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
    CORE_TOOL_NAMES, DEPRECATED_SLASH_SEP, GATEWAY_LOCAL_TOOLS, ID_PREFIX_LEN, INSTANCE_SEP,
    LEGACY_NAMESPACE_SEP, SKILL_TOOL_SEP, instance_short, is_core_tool, is_local_tool,
};
pub use encode::{
    assert_gateway_tool_name, decode_skill_tool_name, decode_tool_name, encode_tool_name,
    extract_bare_tool_name, skill_tool_name,
};

#[cfg(test)]
pub use bare::__reset_warn_state_for_tests;

#[cfg(test)]
mod tests;
