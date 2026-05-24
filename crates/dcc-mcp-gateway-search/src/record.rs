//! Minimal record surface consumed by ranking — implement for your index row type.

use uuid::Uuid;

/// Fields required to score and filter a capability row.
///
/// Implement this on your gateway index record type (for example
/// `dcc_mcp_gateway_core::capability::CapabilityRecord`) in the crate that
/// owns that type.
pub trait SearchRecord {
    /// Client-visible slug (tie-breaker ordering).
    fn tool_slug(&self) -> &str;
    /// Backend `tools/call` name.
    fn backend_tool(&self) -> &str;
    /// One-line description.
    fn summary(&self) -> &str;
    /// Owning skill, if any.
    fn skill_name(&self) -> Option<&str>;
    /// Free-form tags.
    fn tags(&self) -> &[String];
    /// Bounded internal search-only tokens, such as aliases or schema terms.
    fn search_tokens(&self) -> &[String] {
        &[]
    }
    /// DCC bucket (`maya`, `blender`, …).
    fn dcc_type(&self) -> &str;
    /// Owning instance id.
    fn instance_id(&self) -> Uuid;
    /// Whether the skill is loaded on the backend.
    fn loaded(&self) -> bool;
}
