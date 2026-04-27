//! PyO3 bindings for the skill-path helpers.

use std::path::Path;

use pyo3::prelude::*;

use dcc_mcp_paths::path_to_string;

use crate::paths::{
    copy_skill_to_team_dir, copy_skill_to_user_dir, get_app_skill_paths_from_env,
    get_app_team_skill_paths_from_env, get_app_user_skill_paths_from_env, get_skill_paths_from_env,
    get_skills_dir, get_team_skill_paths_from_env, get_team_skills_dir,
    get_user_skill_paths_from_env, get_user_skills_dir,
};

#[pyfunction]
#[pyo3(name = "get_skills_dir")]
#[pyo3(signature = (dcc_name=None))]
pub fn py_get_skills_dir(dcc_name: Option<&str>) -> PyResult<String> {
    Ok(get_skills_dir(dcc_name)?)
}

#[pyfunction]
#[pyo3(name = "get_skill_paths_from_env")]
pub fn py_get_skill_paths_from_env() -> Vec<String> {
    get_skill_paths_from_env()
}

#[pyfunction]
#[pyo3(name = "get_app_skill_paths_from_env")]
pub fn py_get_app_skill_paths_from_env(app_name: &str) -> Vec<String> {
    get_app_skill_paths_from_env(app_name)
}

#[pyfunction]
#[pyo3(name = "get_user_skill_paths_from_env")]
pub fn py_get_user_skill_paths_from_env() -> Vec<String> {
    get_user_skill_paths_from_env()
}

#[pyfunction]
#[pyo3(name = "get_team_skill_paths_from_env")]
pub fn py_get_team_skill_paths_from_env() -> Vec<String> {
    get_team_skill_paths_from_env()
}

#[pyfunction]
#[pyo3(name = "get_app_user_skill_paths_from_env")]
pub fn py_get_app_user_skill_paths_from_env(app_name: &str) -> Vec<String> {
    get_app_user_skill_paths_from_env(app_name)
}

#[pyfunction]
#[pyo3(name = "get_app_team_skill_paths_from_env")]
pub fn py_get_app_team_skill_paths_from_env(app_name: &str) -> Vec<String> {
    get_app_team_skill_paths_from_env(app_name)
}

#[pyfunction]
#[pyo3(name = "get_user_skills_dir")]
#[pyo3(signature = (dcc_name=None))]
pub fn py_get_user_skills_dir(dcc_name: Option<&str>) -> PyResult<String> {
    Ok(get_user_skills_dir(dcc_name)?)
}

#[pyfunction]
#[pyo3(name = "get_team_skills_dir")]
#[pyo3(signature = (dcc_name=None))]
pub fn py_get_team_skills_dir(dcc_name: Option<&str>) -> PyResult<String> {
    Ok(get_team_skills_dir(dcc_name)?)
}

#[pyfunction]
#[pyo3(name = "copy_skill_to_user_dir")]
#[pyo3(signature = (src, dcc_name=None))]
pub fn py_copy_skill_to_user_dir(src: &str, dcc_name: Option<&str>) -> PyResult<String> {
    let dest = copy_skill_to_user_dir(Path::new(src), dcc_name)?;
    Ok(path_to_string(&dest))
}

#[pyfunction]
#[pyo3(name = "copy_skill_to_team_dir")]
#[pyo3(signature = (src, dcc_name=None))]
pub fn py_copy_skill_to_team_dir(src: &str, dcc_name: Option<&str>) -> PyResult<String> {
    let dest = copy_skill_to_team_dir(Path::new(src), dcc_name)?;
    Ok(path_to_string(&dest))
}
