use serde::{Deserialize, Serialize};

// ── SkillPolicy ───────────────────────────────────────────────────────────

/// Invocation policy declared in the SKILL.md frontmatter.
///
/// Controls how AI agents may invoke this skill.
///
/// ```yaml
/// policy:
///   allow_implicit_invocation: false   # default: true
///   products: ["maya", "houdini"]      # empty = all products
/// ```
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SkillPolicy {
    /// When `false`, the skill must be explicitly loaded via `load_skill`
    /// before any of its tools can be called.  Defaults to `true`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allow_implicit_invocation: Option<bool>,

    /// Restricts this skill to specific DCC products (case-insensitive).
    /// An empty list means the skill is available for all products.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub products: Vec<String>,
}

impl SkillPolicy {
    /// Returns `true` if implicit invocation is allowed (default when absent).
    pub fn is_implicit_invocation_allowed(&self) -> bool {
        self.allow_implicit_invocation.unwrap_or(true)
    }

    /// Returns `true` if this skill is available for the given DCC product.
    /// Empty `products` list means available for all.
    pub fn matches_product(&self, product: &str) -> bool {
        self.products.is_empty()
            || self
                .products
                .iter()
                .any(|p| p.eq_ignore_ascii_case(product))
    }
}
