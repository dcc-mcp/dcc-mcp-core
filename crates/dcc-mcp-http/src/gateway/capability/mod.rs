//! Gateway capability index — the dynamic-tools substrate for
//! tracking issue [#657] (phases [#653]/[#654]/[#655]).
//!
//! ## Why
//!
//! Publishing every backend DCC action as its own MCP tool does not
//! scale:
//!
//! * `tools/list` size grows as
//!   `|gateway tools| + sum(|backend tools|)` and blows up the agent's
//!   token budget in multi-instance setups.
//! * Client-side name filters (Cursor, Anthropic) hide any tool that
//!   does not match `[A-Za-z0-9_]+`; #656 routed around that but
//!   doing so multiplies the fan-out further when skills with dotted
//!   or hyphenated names must be published twice (encoded + bare).
//! * Backends that advertise hundreds of capabilities pay the full
//!   `tools/list` cost on every MCP client connection even when the
//!   agent only needs one of them.
//!
//! ## What this module does
//!
//! Keep a compact, queryable index of every live DCC instance's
//! capabilities and feed three bounded entry points:
//!
//! * **MCP wrappers** (#655) — `search_tools`, `describe_tool`,
//!   `call_tool` expose the index through a fixed MCP tool surface.
//! * **REST APIs** (#654) — `POST /v1/search`, `POST /v1/describe`,
//!   `POST /v1/call`, `GET /v1/instances` mirror the wrappers for
//!   non-MCP clients.
//! * **Slim/Rest tools/list** (#652) — in bounded modes the index is
//!   the only capability surface; Tier 3 fan-out is skipped entirely.
//!
//! The index carries **just enough** routing metadata per capability
//! (~200 B wire size) to resolve a selected backend action without
//! shipping the full JSON Schema — schemas are pulled on demand by
//! `describe_tool` and cached by the backend's own tool cache.
//!
//! ## Maintainer layout
//!
//! This facade re-exports the public surface; implementation lives in
//! focused siblings so each file has one reason to change
//! ([SOLID SRP](https://en.wikipedia.org/wiki/Single-responsibility_principle)):
//!
//! * [`record`] — the `CapabilityRecord` struct + `tool_slug` format.
//! * [`index`] — thread-safe `CapabilityIndex` store + swap.
//! * [`builder`] — build a per-instance record set from backend
//!   `tools/list` + `list_skills` responses.
//! * [`search`] — `SearchQuery`, keyword scoring, ranking.
//! * [`refresh`] — lifecycle: rebuild on instance join/leave, skill
//!   load/unload, `tools/list_changed`.
//!
//! [#657]: https://github.com/loonghao/dcc-mcp-core/issues/657
//! [#653]: https://github.com/loonghao/dcc-mcp-core/issues/653
//! [#654]: https://github.com/loonghao/dcc-mcp-core/issues/654
//! [#655]: https://github.com/loonghao/dcc-mcp-core/issues/655

mod builder;
mod index;
mod record;
mod refresh;
mod search;

#[cfg(test)]
mod tests;

pub use builder::{BuildInput, BuildOutcome, build_records_from_backend};
pub use index::{CapabilityIndex, IndexSnapshot, InstanceFingerprint};
pub use record::{CapabilityRecord, SCHEMA_AVAILABLE, parse_slug, tool_slug};
pub use refresh::{RefreshReason, refresh_instance, remove_instance};
pub use search::{DEFAULT_LIMIT, MAX_LIMIT, SearchHit, SearchQuery, search};
