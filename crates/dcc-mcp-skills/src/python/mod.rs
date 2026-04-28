//! PyO3 bindings for `dcc-mcp-skills`.
//!
//! Per workspace convention (#501), every `#[pymethods]` /
//! `#[pyfunction]` block in this crate lives below `src/python/`.

pub(crate) mod catalog;
pub(crate) mod feedback;
pub(crate) mod gui_executable;
pub(crate) mod loader;
pub(crate) mod paths;
pub(crate) mod resolver;
pub(crate) mod scanner;
pub(crate) mod validator;
pub(crate) mod versioning;
pub(crate) mod watcher;
