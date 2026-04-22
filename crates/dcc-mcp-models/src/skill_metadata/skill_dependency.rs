use serde::{Deserialize, Serialize};

// ── SkillDependencies ─────────────────────────────────────────────────────

/// Category of an external dependency.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SkillDependencyType {
    /// Requires a running MCP server.
    #[default]
    Mcp,
    /// Requires an environment variable to be set.
    EnvVar,
    /// Requires a binary to be present on `$PATH`.
    Bin,
}

impl std::fmt::Display for SkillDependencyType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Mcp => write!(f, "mcp"),
            Self::EnvVar => write!(f, "env_var"),
            Self::Bin => write!(f, "bin"),
        }
    }
}

/// A single external dependency declared by a skill.
///
/// ```yaml
/// external_deps:
///   tools:
///     - type: mcp
///       value: "render-server"
///       description: "Needs the render MCP server"
///       transport: stdio
///       command: "python -m render_mcp"
///     - type: env_var
///       value: "MAYA_LICENSE_KEY"
///       description: "Maya license key must be set"
/// ```
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SkillDependency {
    /// Dependency category.
    #[serde(default, rename = "type")]
    pub dep_type: SkillDependencyType,

    /// Identifier: server name, env-var name, or binary name.
    #[serde(default)]
    pub value: String,

    /// Human-readable explanation shown when dependency is missing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// MCP transport (`"stdio"`, `"http"`, …) — only for `Mcp` deps.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transport: Option<String>,

    /// Command to launch the MCP server — only for `Mcp`/`stdio` deps.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,

    /// URL of the MCP server — only for `Mcp`/`http` deps.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

/// External dependency declarations for a skill.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SkillDependencies {
    /// List of external tool / server / environment dependencies.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<SkillDependency>,
}

impl SkillDependencies {
    /// `true` when no dependencies are declared.
    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }

    /// Iterator over `Mcp`-type dependencies.
    pub fn mcp_deps(&self) -> impl Iterator<Item = &SkillDependency> {
        self.tools
            .iter()
            .filter(|d| d.dep_type == SkillDependencyType::Mcp)
    }

    /// Iterator over `EnvVar`-type dependencies.
    pub fn env_var_deps(&self) -> impl Iterator<Item = &SkillDependency> {
        self.tools
            .iter()
            .filter(|d| d.dep_type == SkillDependencyType::EnvVar)
    }

    /// Iterator over `Bin`-type dependencies.
    pub fn bin_deps(&self) -> impl Iterator<Item = &SkillDependency> {
        self.tools
            .iter()
            .filter(|d| d.dep_type == SkillDependencyType::Bin)
    }
}
