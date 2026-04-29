//! PyO3 bindings for the SKILL.md loader.

use std::path::Path;

use pyo3::prelude::*;
#[cfg(feature = "stub-gen")]
use pyo3_stub_gen_derive::gen_stub_pyfunction;

use dcc_mcp_models::SkillMetadata;

use crate::loader::{
    parse_skill_md, scan_and_load, scan_and_load_lenient, scan_and_load_strict, scan_and_load_team,
    scan_and_load_team_lenient, scan_and_load_user, scan_and_load_user_lenient,
};

/// Python wrapper for [`parse_skill_md`].
///
/// Accepts either a skill directory path or a direct path to a `SKILL.md` file.
/// If a file path is given, the parent directory is used automatically.
///
/// Returns `None` if the directory contains no valid `SKILL.md`.
/// Raises `FileNotFoundError` if the path does not exist at all.
#[cfg_attr(feature = "stub-gen", gen_stub_pyfunction)]
#[pyfunction]
#[pyo3(name = "parse_skill_md")]
pub fn py_parse_skill_md(skill_dir: &str) -> PyResult<Option<SkillMetadata>> {
    let raw = Path::new(skill_dir);

    let dir = if raw.is_file() {
        raw.parent()
            .ok_or_else(|| {
                pyo3::exceptions::PyValueError::new_err(format!(
                    "parse_skill_md: cannot determine parent directory of file: {skill_dir}"
                ))
            })?
            .to_owned()
    } else if raw.is_dir() {
        raw.to_owned()
    } else {
        return Err(pyo3::exceptions::PyFileNotFoundError::new_err(format!(
            "parse_skill_md: path does not exist: {skill_dir}"
        )));
    };

    Ok(parse_skill_md(&dir))
}

#[cfg_attr(feature = "stub-gen", gen_stub_pyfunction)]
#[pyfunction]
#[pyo3(name = "scan_and_load")]
#[pyo3(signature = (extra_paths=None, dcc_name=None))]
pub fn py_scan_and_load(
    extra_paths: Option<Vec<String>>,
    dcc_name: Option<&str>,
) -> PyResult<(Vec<SkillMetadata>, Vec<String>)> {
    let result = scan_and_load(extra_paths.as_deref(), dcc_name)
        .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))?;
    Ok((result.skills, result.skipped))
}

#[cfg_attr(feature = "stub-gen", gen_stub_pyfunction)]
#[pyfunction]
#[pyo3(name = "scan_and_load_lenient")]
#[pyo3(signature = (extra_paths=None, dcc_name=None))]
pub fn py_scan_and_load_lenient(
    extra_paths: Option<Vec<String>>,
    dcc_name: Option<&str>,
) -> PyResult<(Vec<SkillMetadata>, Vec<String>)> {
    let result = scan_and_load_lenient(extra_paths.as_deref(), dcc_name)
        .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))?;
    Ok((result.skills, result.skipped))
}

/// Strict pipeline (issue maya#138) — same as [`py_scan_and_load`] but
/// raises `ValueError` when any directory was silently skipped, so
/// embedders can fail start-up loudly instead of discovering missing
/// tools at run-time.
#[cfg_attr(feature = "stub-gen", gen_stub_pyfunction)]
#[pyfunction]
#[pyo3(name = "scan_and_load_strict")]
#[pyo3(signature = (extra_paths=None, dcc_name=None))]
pub fn py_scan_and_load_strict(
    extra_paths: Option<Vec<String>>,
    dcc_name: Option<&str>,
) -> PyResult<(Vec<SkillMetadata>, Vec<String>)> {
    let result = scan_and_load_strict(extra_paths.as_deref(), dcc_name)
        .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))?;
    Ok((result.skills, result.skipped))
}

#[cfg_attr(feature = "stub-gen", gen_stub_pyfunction)]
#[pyfunction]
#[pyo3(name = "scan_and_load_user")]
#[pyo3(signature = (extra_paths=None, dcc_name=None))]
pub fn py_scan_and_load_user(
    extra_paths: Option<Vec<String>>,
    dcc_name: Option<&str>,
) -> PyResult<(Vec<SkillMetadata>, Vec<String>)> {
    let result = scan_and_load_user(extra_paths.as_deref(), dcc_name)
        .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))?;
    Ok((result.skills, result.skipped))
}

#[cfg_attr(feature = "stub-gen", gen_stub_pyfunction)]
#[pyfunction]
#[pyo3(name = "scan_and_load_team")]
#[pyo3(signature = (extra_paths=None, dcc_name=None))]
pub fn py_scan_and_load_team(
    extra_paths: Option<Vec<String>>,
    dcc_name: Option<&str>,
) -> PyResult<(Vec<SkillMetadata>, Vec<String>)> {
    let result = scan_and_load_team(extra_paths.as_deref(), dcc_name)
        .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))?;
    Ok((result.skills, result.skipped))
}

#[cfg_attr(feature = "stub-gen", gen_stub_pyfunction)]
#[pyfunction]
#[pyo3(name = "scan_and_load_user_lenient")]
#[pyo3(signature = (extra_paths=None, dcc_name=None))]
pub fn py_scan_and_load_user_lenient(
    extra_paths: Option<Vec<String>>,
    dcc_name: Option<&str>,
) -> PyResult<(Vec<SkillMetadata>, Vec<String>)> {
    let result = scan_and_load_user_lenient(extra_paths.as_deref(), dcc_name)
        .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))?;
    Ok((result.skills, result.skipped))
}

#[cfg_attr(feature = "stub-gen", gen_stub_pyfunction)]
#[pyfunction]
#[pyo3(name = "scan_and_load_team_lenient")]
#[pyo3(signature = (extra_paths=None, dcc_name=None))]
pub fn py_scan_and_load_team_lenient(
    extra_paths: Option<Vec<String>>,
    dcc_name: Option<&str>,
) -> PyResult<(Vec<SkillMetadata>, Vec<String>)> {
    let result = scan_and_load_team_lenient(extra_paths.as_deref(), dcc_name)
        .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))?;
    Ok((result.skills, result.skipped))
}
