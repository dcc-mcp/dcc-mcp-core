//! Skill execution feedback — append-only `<skill>/.feedback.jsonl` records.
//!
//! Moved from `dcc-mcp-utils::filesystem` (issue #498).

use dcc_mcp_paths::FilesystemError;
use std::path::Path;

/// Feedback entry for an evolved skill execution.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[cfg_attr(
    feature = "python-bindings",
    pyo3::pyclass(name = "SkillFeedback", get_all, set_all, skip_from_py_object)
)]
pub struct SkillFeedback {
    /// ISO-8601 timestamp.
    pub timestamp: String,
    /// Whether the skill execution succeeded.
    pub success: bool,
    /// Optional user/agent correction or improved prompt.
    pub correction: Option<String>,
    /// Optional free-form notes.
    pub notes: Option<String>,
    /// Who provided the feedback (e.g. agent id, user name).
    pub caller: Option<String>,
}

/// Record feedback for a skill.
///
/// Appends a JSON line to `<skill_dir>/.feedback.jsonl`.
///
/// # Errors
/// Returns [`FilesystemError`] if the skill directory is invalid or writing fails.
pub fn record_skill_feedback(
    skill_dir: &Path,
    success: bool,
    correction: Option<&str>,
    notes: Option<&str>,
    caller: Option<&str>,
) -> Result<(), FilesystemError> {
    if !skill_dir.is_dir() {
        return Err(FilesystemError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("skill directory not found: {}", skill_dir.display()),
        )));
    }

    let feedback = SkillFeedback {
        timestamp: crate::versioning::format_system_time_now(),
        success,
        correction: correction.map(|s| s.to_string()),
        notes: notes.map(|s| s.to_string()),
        caller: caller.map(|s| s.to_string()),
    };

    let feedback_path = skill_dir.join(".feedback.jsonl");
    let line = serde_json::to_string(&feedback)?;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&feedback_path)?;
    use std::io::Write;
    writeln!(file, "{line}")?;

    Ok(())
}

/// Read feedback entries for a skill.
///
/// Reads `<skill_dir>/.feedback.jsonl` and returns the most recent `limit` entries.
///
/// # Errors
/// Returns [`FilesystemError`] if reading fails.
pub fn get_skill_feedback(
    skill_dir: &Path,
    limit: Option<usize>,
) -> Result<Vec<SkillFeedback>, FilesystemError> {
    let feedback_path = skill_dir.join(".feedback.jsonl");
    if !feedback_path.is_file() {
        return Ok(Vec::new());
    }

    let content = std::fs::read_to_string(&feedback_path)?;
    let mut entries: Vec<SkillFeedback> = content
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect();

    entries.reverse();
    if let Some(n) = limit {
        entries.truncate(n);
    }
    Ok(entries)
}

// PyO3 bindings live in `crate::python::feedback`.
#[cfg(feature = "python-bindings")]
pub use crate::python::feedback::{py_get_skill_feedback, py_record_skill_feedback};
