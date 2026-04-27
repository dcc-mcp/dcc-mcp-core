//! PyO3 bindings for `SemVer` / `VersionConstraint` / `VersionedRegistry`.

use pyo3::prelude::*;
#[cfg(feature = "stub-gen")]
use pyo3_stub_gen_derive::{gen_stub_pyclass, gen_stub_pymethods};

use crate::registry::ActionMeta;
use crate::versioned::{SemVer, VersionConstraint, VersionedRegistry};

#[cfg_attr(feature = "stub-gen", gen_stub_pymethods)]
#[pymethods]
impl SemVer {
    #[new]
    #[pyo3(signature = (major, minor=0, patch=0))]
    fn py_new(major: u64, minor: u64, patch: u64) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }

    /// Parse a semver string such as "1.2.3", "v2.0", or "1.0.0-alpha".
    ///
    /// Raises ValueError if the string cannot be parsed.
    #[staticmethod]
    #[pyo3(name = "parse")]
    fn parse_str(s: &str) -> PyResult<Self> {
        Self::parse(s).map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))
    }

    /// Check whether this version satisfies a constraint string.
    ///
    /// Equivalent to ``VersionConstraint.parse(constraint_str).matches(self)``.
    fn matches_constraint(&self, constraint: &PyVersionConstraint) -> bool {
        constraint.inner.matches(*self)
    }

    fn __str__(&self) -> String {
        self.to_string()
    }

    fn __repr__(&self) -> String {
        format!("SemVer({}, {}, {})", self.major, self.minor, self.patch)
    }

    fn __eq__(&self, other: &Self) -> bool {
        self == other
    }

    fn __lt__(&self, other: &Self) -> bool {
        self < other
    }

    fn __le__(&self, other: &Self) -> bool {
        self <= other
    }

    fn __gt__(&self, other: &Self) -> bool {
        self > other
    }

    fn __ge__(&self, other: &Self) -> bool {
        self >= other
    }
}

#[cfg_attr(feature = "stub-gen", gen_stub_pyclass)]
#[pyclass(name = "VersionConstraint")]
pub struct PyVersionConstraint {
    pub inner: VersionConstraint,
}

#[cfg_attr(feature = "stub-gen", gen_stub_pymethods)]
#[pymethods]
impl PyVersionConstraint {
    #[staticmethod]
    fn parse(s: &str) -> PyResult<Self> {
        s.parse::<VersionConstraint>()
            .map(|inner| Self { inner })
            .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))
    }

    fn matches(&self, version: &SemVer) -> bool {
        self.inner.matches(*version)
    }

    fn __repr__(&self) -> String {
        format!("VersionConstraint({})", self.inner)
    }

    fn __str__(&self) -> String {
        self.inner.to_string()
    }
}

#[cfg_attr(feature = "stub-gen", gen_stub_pymethods)]
#[pymethods]
impl VersionedRegistry {
    #[new]
    pub fn py_new() -> Self {
        Self::new()
    }

    /// Register an action version.
    #[pyo3(name = "register_versioned")]
    #[pyo3(signature = (name, dcc, version, description="", category="", tags=None))]
    fn py_register_versioned(
        &mut self,
        name: String,
        dcc: String,
        version: String,
        description: &str,
        category: &str,
        tags: Option<Vec<String>>,
    ) {
        self.register(ActionMeta {
            name,
            dcc,
            version,
            description: description.to_owned(),
            category: category.to_owned(),
            tags: tags.unwrap_or_default(),
            ..Default::default()
        });
    }

    /// Remove all versions of `(name, dcc)` that satisfy the constraint string.
    #[pyo3(name = "remove")]
    fn py_remove(&mut self, name: &str, dcc: &str, constraint: &str) -> PyResult<usize> {
        let c = constraint
            .parse::<VersionConstraint>()
            .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))?;
        Ok(self.remove(name, dcc, &c))
    }

    /// Return all registered versions for `(name, dcc)`, sorted ascending.
    #[pyo3(name = "versions")]
    fn py_versions(&self, name: &str, dcc: &str) -> Vec<String> {
        self.versions(name, dcc)
            .into_iter()
            .map(|v| v.to_string())
            .collect()
    }

    /// Return the highest registered version string, or `None` if not registered.
    #[pyo3(name = "latest_version")]
    fn py_latest_version(&self, name: &str, dcc: &str) -> Option<String> {
        self.latest(name, dcc).map(|m| m.version.clone())
    }

    /// Return all registered `(name, dcc)` keys.
    #[pyo3(name = "keys")]
    fn py_keys(&self) -> Vec<(String, String)> {
        self.keys()
    }

    /// Return the total number of registered versioned entries.
    #[pyo3(name = "total_entries")]
    fn py_total_entries(&self) -> usize {
        self.store.values().map(|v| v.len()).sum()
    }

    /// Resolve the best-matching version given a constraint string.
    #[pyo3(name = "resolve")]
    fn py_resolve<'py>(
        &self,
        py: Python<'py>,
        name: &str,
        dcc: &str,
        constraint: &str,
    ) -> PyResult<Option<Bound<'py, pyo3::types::PyDict>>> {
        let c = constraint
            .parse::<VersionConstraint>()
            .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))?;
        self.router()
            .resolve(name, dcc, &c)
            .map(|m| build_action_meta_dict(py, m))
            .transpose()
    }

    /// Return all action metadata dicts that satisfy `constraint`, sorted ascending.
    #[pyo3(name = "resolve_all")]
    fn py_resolve_all<'py>(
        &self,
        py: Python<'py>,
        name: &str,
        dcc: &str,
        constraint: &str,
    ) -> PyResult<Vec<Bound<'py, pyo3::types::PyDict>>> {
        let c = constraint
            .parse::<VersionConstraint>()
            .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))?;
        self.router()
            .resolve_all(name, dcc, &c)
            .into_iter()
            .map(|m| build_action_meta_dict(py, m))
            .collect()
    }

    fn __repr__(&self) -> String {
        format!(
            "VersionedRegistry(entries={})",
            self.store.values().map(|v| v.len()).sum::<usize>()
        )
    }
}

/// Build a Python dict from an [`ActionMeta`] while holding the GIL.
fn build_action_meta_dict<'py>(
    py: Python<'py>,
    meta: &ActionMeta,
) -> PyResult<Bound<'py, pyo3::types::PyDict>> {
    use pyo3::types::PyDict;
    let d = PyDict::new(py);
    d.set_item("name", &meta.name)?;
    d.set_item("dcc", &meta.dcc)?;
    d.set_item("version", &meta.version)?;
    d.set_item("description", &meta.description)?;
    d.set_item("category", &meta.category)?;
    d.set_item("tags", &meta.tags)?;
    Ok(d)
}
