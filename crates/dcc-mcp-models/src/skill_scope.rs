//! [`SkillScope`] — trust level / origin of a skill.

#[cfg(feature = "python-bindings")]
use pyo3::prelude::*;

use serde::{Deserialize, Serialize};

/// Trust level / origin scope of a skill.
///
/// Determines default policy and precedence when multiple skills with
/// the same name exist at different scope levels.
///
/// Precedence (highest → lowest): `Admin > System > User > Repo`
///
/// # Example SKILL.md usage
/// Scope is **not** declared in the SKILL.md file itself — it is inferred
/// at discovery time from the directory the skill was found in:
///
/// | Path pattern                            | Scope  |
/// |----------------------------------------|--------|
/// | `<project>/.dcc_skills/`               | Repo   |
/// | `~/.dcc_mcp/skills/`                   | User   |
/// | `<install>/share/dcc_mcp/skills/`      | System |
/// | Managed enterprise distribution        | Admin  |
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default,
)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(
    feature = "python-bindings",
    pyclass(name = "SkillScope", eq, skip_from_py_object)
)]
pub enum SkillScope {
    /// Project-local skill (e.g. `./<project>/.dcc_skills/`).
    ///
    /// Lowest trust — cannot silently override higher-scope skills.
    #[default]
    Repo,

    /// User-level skill (e.g. `~/.dcc_mcp/skills/`).
    User,

    /// System-level skill bundled with the package (read-only).
    System,

    /// Enterprise/admin-managed skill.  Highest trust.
    Admin,
}

impl SkillScope {
    /// Short string label used in JSON, logs, and Python `__str__`.
    pub fn label(self) -> &'static str {
        match self {
            Self::Repo => "repo",
            Self::User => "user",
            Self::System => "system",
            Self::Admin => "admin",
        }
    }

    /// Returns `true` for scopes that cannot be overridden by user/repo skills.
    pub fn is_elevated(self) -> bool {
        matches!(self, Self::System | Self::Admin)
    }
}

impl std::fmt::Display for SkillScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.label())
    }
}

#[cfg(feature = "python-bindings")]
#[pymethods]
impl SkillScope {
    fn __repr__(&self) -> String {
        format!("SkillScope.{}", self.label())
    }

    fn __str__(&self) -> String {
        self.label().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scope_ordering() {
        assert!(SkillScope::Repo < SkillScope::User);
        assert!(SkillScope::User < SkillScope::System);
        assert!(SkillScope::System < SkillScope::Admin);
    }

    #[test]
    fn test_scope_default() {
        assert_eq!(SkillScope::default(), SkillScope::Repo);
    }

    #[test]
    fn test_scope_serde_roundtrip() {
        let scope = SkillScope::User;
        let json = serde_json::to_string(&scope).unwrap();
        assert_eq!(json, "\"user\"");
        let back: SkillScope = serde_json::from_str(&json).unwrap();
        assert_eq!(back, SkillScope::User);
    }

    #[test]
    fn test_scope_is_elevated() {
        assert!(!SkillScope::Repo.is_elevated());
        assert!(!SkillScope::User.is_elevated());
        assert!(SkillScope::System.is_elevated());
        assert!(SkillScope::Admin.is_elevated());
    }

    #[test]
    fn test_scope_display() {
        assert_eq!(SkillScope::Admin.to_string(), "admin");
    }
}
