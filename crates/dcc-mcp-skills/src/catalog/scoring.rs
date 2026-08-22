//! Cached search fields and compatibility rank-policy exports.
//!
//! The scorer itself is owned by dcc-mcp-gateway-search so skill catalog,
//! per-DCC REST, and gateway searches share one ranking contract (#2184).
//! This module retains the historical path for cached token fields and the
//! persisted discovery-source enum.

pub use dcc_mcp_gateway_search::{
    LAYER_DOMAIN, LAYER_EXAMPLE, LAYER_INFRASTRUCTURE, LAYER_THIN_HARNESS, layer_multiplier,
};
use dcc_mcp_models::SkillMetadata;

/// Where a skill was discovered from.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum SkillPathSource {
    /// No source information.
    #[default]
    Unknown,
    /// Shipped with the dcc-mcp package or an adapter.
    Bundled,
    /// Platform-wide install directory.
    Platform,
    /// Local developer skill directory.
    LocalDev,
    /// Configured through a DCC-MCP skill-path environment variable.
    EnvVar,
    /// Passed explicitly by the caller.
    ExplicitArg,
    /// Added through the gateway admin UI.
    AdminCustom,
}

impl SkillPathSource {
    /// Stable label shared with dcc-mcp-gateway-search.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Unknown => dcc_mcp_gateway_search::PATH_SOURCE_UNKNOWN,
            Self::Bundled => dcc_mcp_gateway_search::PATH_SOURCE_BUNDLED,
            Self::Platform => dcc_mcp_gateway_search::PATH_SOURCE_PLATFORM,
            Self::LocalDev => dcc_mcp_gateway_search::PATH_SOURCE_LOCAL_DEV,
            Self::EnvVar => dcc_mcp_gateway_search::PATH_SOURCE_ENV_VAR,
            Self::ExplicitArg => dcc_mcp_gateway_search::PATH_SOURCE_EXPLICIT_ARG,
            Self::AdminCustom => dcc_mcp_gateway_search::PATH_SOURCE_ADMIN_CUSTOM,
        }
    }
}

/// Compatibility wrapper for callers that hold SkillPathSource.
#[must_use]
pub fn path_source_multiplier(source: SkillPathSource) -> f64 {
    dcc_mcp_gateway_search::path_source_multiplier(Some(source.label()))
}

const STOPWORDS: &[&str] = &[
    "a", "an", "the", "of", "and", "or", "to", "for", "with", "from",
];

/// Tokenise a cached search field for record projection or the optional exact-token index.
#[must_use]
pub fn tokenize(value: &str) -> Vec<String> {
    value
        .to_lowercase()
        .split(|ch: char| {
            ch.is_whitespace() || matches!(ch, '_' | '-' | '.' | ',' | ';' | ':' | '/')
        })
        .filter(|token| !token.is_empty())
        .filter(|token| !STOPWORDS.contains(token))
        .map(str::to_string)
        .collect()
}

/// Pre-tokenised skill fields used by record projection and the optional exact-token index.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FieldTokens {
    pub name: Vec<String>,
    pub tags: Vec<String>,
    pub hint: Vec<String>,
    pub aliases: Vec<String>,
    pub description: Vec<String>,
    pub tool_names: Vec<String>,
    pub tool_aliases: Vec<String>,
    pub tool_descriptions: Vec<String>,
    pub dcc: Vec<String>,
}

impl FieldTokens {
    /// Total cached token count retained for persisted-state compatibility.
    #[must_use]
    pub fn doc_len(&self) -> usize {
        self.name.len()
            + self.tags.len()
            + self.hint.len()
            + self.aliases.len()
            + self.description.len()
            + self.tool_names.len()
            + self.tool_aliases.len()
            + self.tool_descriptions.len()
            + self.dcc.len()
    }

    /// Build cached fields from skill metadata.
    #[must_use]
    pub fn from_metadata(meta: &SkillMetadata) -> Self {
        let hint_source = if meta.search_hint.is_empty() {
            meta.description.as_str()
        } else {
            meta.search_hint.as_str()
        };

        let mut tool_names = Vec::new();
        let mut tool_aliases = Vec::new();
        let mut tool_descriptions = Vec::new();
        for tool in &meta.tools {
            tool_names.extend(tokenize(&tool.name));
            for alias in &tool.search_aliases {
                tool_aliases.extend(tokenize(alias));
            }
            tool_descriptions.extend(tokenize(&tool.description));
        }

        let tags = meta.tags.iter().flat_map(|tag| tokenize(tag)).collect();
        let aliases = meta
            .search_aliases
            .iter()
            .flat_map(|alias| tokenize(alias))
            .collect();

        Self {
            name: tokenize(&meta.name),
            tags,
            hint: tokenize(hint_source),
            aliases,
            description: tokenize(&meta.description),
            tool_names,
            tool_aliases,
            tool_descriptions,
            dcc: tokenize(&meta.dcc),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenization_and_source_policy_remain_compatible() {
        assert_eq!(tokenize("Polygon Bevel"), ["polygon", "bevel"]);
        assert_eq!(path_source_multiplier(SkillPathSource::Bundled), 0.70);
        assert_eq!(path_source_multiplier(SkillPathSource::Platform), 0.85);
        assert_eq!(path_source_multiplier(SkillPathSource::LocalDev), 1.0);
    }
}
