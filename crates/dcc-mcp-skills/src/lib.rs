//! dcc-mcp-skills: SKILL.md scanning and loading.

mod loader;
mod scanner;

pub use loader::parse_skill_md;
pub use scanner::SkillScanner;

#[cfg(feature = "python-bindings")]
pub use loader::py_parse_skill_md;
#[cfg(feature = "python-bindings")]
pub use scanner::py_scan_skill_paths;
