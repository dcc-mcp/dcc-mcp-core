//! PyO3 bindings for the skill-feedback log.

use std::path::Path;

use pyo3::prelude::*;

use crate::feedback::{SkillFeedback, get_skill_feedback, record_skill_feedback};

#[pyfunction]
#[pyo3(name = "record_skill_feedback")]
#[pyo3(signature = (skill_dir, success, correction=None, notes=None, caller=None))]
pub fn py_record_skill_feedback(
    skill_dir: &str,
    success: bool,
    correction: Option<&str>,
    notes: Option<&str>,
    caller: Option<&str>,
) -> PyResult<()> {
    record_skill_feedback(Path::new(skill_dir), success, correction, notes, caller)?;
    Ok(())
}

#[pyfunction]
#[pyo3(name = "get_skill_feedback")]
#[pyo3(signature = (skill_dir, limit=None))]
pub fn py_get_skill_feedback(
    skill_dir: &str,
    limit: Option<usize>,
) -> PyResult<Vec<SkillFeedback>> {
    Ok(get_skill_feedback(Path::new(skill_dir), limit)?)
}
