//! Skill directory validator — structured linting for SKILL.md and adjacent files.
//!
//! Inspired by Anthropic's skill-creator `quick_validate.py`, this module provides
//! a programmatic way to check that a skill directory follows the dcc-mcp-core
//! specification before it is loaded at runtime.

#[path = "validator_python.rs"]
mod python;
#[path = "validator_rules.rs"]
mod rules;
#[cfg(test)]
#[path = "validator_tests.rs"]
mod tests;
#[path = "validator_types.rs"]
mod types;

pub use rules::validate_skill_dir;
pub use types::{IssueCategory, IssueSeverity, SkillValidationIssue, SkillValidationReport};

#[cfg(feature = "python-bindings")]
pub use python::*;
