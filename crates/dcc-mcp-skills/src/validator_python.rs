#[cfg(feature = "python-bindings")]
use pyo3::prelude::*;

#[cfg(feature = "python-bindings")]
use super::{IssueSeverity, SkillValidationIssue, SkillValidationReport, validate_skill_dir};

#[cfg(feature = "python-bindings")]
use std::path::Path;

#[cfg(feature = "python-bindings")]
#[pyclass(name = "SkillValidationIssue", from_py_object)]
#[derive(Clone)]
pub struct PySkillValidationIssue {
    #[pyo3(get)]
    pub severity: String,
    #[pyo3(get)]
    pub category: String,
    #[pyo3(get)]
    pub message: String,
}

#[cfg(feature = "python-bindings")]
#[pyclass(name = "SkillValidationReport", from_py_object)]
#[derive(Clone)]
pub struct PySkillValidationReport {
    #[pyo3(get)]
    pub skill_dir: String,
    #[pyo3(get)]
    pub issues: Vec<PySkillValidationIssue>,
    #[pyo3(get)]
    pub has_errors: bool,
    #[pyo3(get)]
    pub is_clean: bool,
}

#[cfg(feature = "python-bindings")]
#[pyfunction(name = "validate_skill")]
pub fn py_validate_skill(skill_dir: &str) -> PyResult<PySkillValidationReport> {
    let path = Path::new(skill_dir);
    let report = validate_skill_dir(path);
    Ok(report.into())
}

#[cfg(feature = "python-bindings")]
impl From<SkillValidationReport> for PySkillValidationReport {
    fn from(report: SkillValidationReport) -> Self {
        Self {
            skill_dir: report.skill_dir.to_string_lossy().to_string(),
            has_errors: report.has_errors(),
            is_clean: report.is_clean(),
            issues: report.issues.into_iter().map(Into::into).collect(),
        }
    }
}

#[cfg(feature = "python-bindings")]
impl From<SkillValidationIssue> for PySkillValidationIssue {
    fn from(issue: SkillValidationIssue) -> Self {
        Self {
            severity: match issue.severity {
                IssueSeverity::Error => "error".to_string(),
                IssueSeverity::Warning => "warning".to_string(),
            },
            category: format!("{:?}", issue.category).to_lowercase(),
            message: issue.message,
        }
    }
}

#[cfg(feature = "python-bindings")]
/// Register Python bindings for the validator module.
pub fn register_classes(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PySkillValidationIssue>()?;
    m.add_class::<PySkillValidationReport>()?;
    m.add_function(wrap_pyfunction!(py_validate_skill, m)?)?;
    Ok(())
}
