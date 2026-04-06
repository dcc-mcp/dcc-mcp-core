//! Semantic-version management and backward-compatible routing for Actions.
//!
//! The [`VersionedRegistry`] stores multiple versions of the same action and exposes
//! a [`CompatibilityRouter`] that, given a client-side version constraint, selects the
//! best-matching registered version.
//!
//! # Version Constraint Syntax
//!
//! | Constraint | Meaning |
//! |-----------|---------|
//! | `=1.2.3`  | Exact match only |
//! | `>=1.2.0` | At least 1.2.0 (any higher version is acceptable) |
//! | `>1.2.0`  | Strictly greater than 1.2.0 |
//! | `<=2.0.0` | At most 2.0.0 |
//! | `<2.0.0`  | Strictly less than 2.0.0 |
//! | `^1.2.3`  | Compatible with 1.2.3 (same major, ≥ minor.patch) |
//! | `~1.2.3`  | Approximately 1.2.3 (same major.minor, ≥ patch) |
//! | `*`       | Any version |
//!
//! # Example
//!
//! ```rust
//! use dcc_mcp_actions::versioned::{VersionedRegistry, VersionConstraint};
//! use dcc_mcp_actions::registry::ActionMeta;
//!
//! let mut vr = VersionedRegistry::new();
//!
//! vr.register(ActionMeta { name: "create_sphere".into(), dcc: "maya".into(), version: "1.0.0".into(), ..Default::default() });
//! vr.register(ActionMeta { name: "create_sphere".into(), dcc: "maya".into(), version: "1.2.0".into(), ..Default::default() });
//! vr.register(ActionMeta { name: "create_sphere".into(), dcc: "maya".into(), version: "2.0.0".into(), ..Default::default() });
//!
//! // Client targeting >=1.0, <2.0 → should get 1.2.0
//! let router = vr.router();
//! let constraint: VersionConstraint = "^1.0.0".parse().unwrap();
//! let result = router.resolve("create_sphere", "maya", &constraint);
//! assert_eq!(result.unwrap().version, "1.2.0");
//! ```

use std::collections::HashMap;
use std::fmt;
use std::str::FromStr;

#[cfg(feature = "python-bindings")]
use pyo3::prelude::*;
use serde::{Deserialize, Serialize};

use crate::registry::ActionMeta;

#[cfg(test)]
mod tests;

// ── SemVer ──────────────────────────────────────────────────────────────────────

/// A semantic version consisting of major, minor, and patch components.
///
/// Only the numeric components are considered; pre-release labels (e.g. `-alpha`)
/// are stripped and ignored for comparison purposes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python-bindings",
    pyclass(name = "SemVer", get_all, from_py_object)
)]
pub struct SemVer {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
}

#[cfg(feature = "python-bindings")]
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

impl SemVer {
    /// Create a `SemVer` directly.
    #[must_use]
    pub const fn new(major: u64, minor: u64, patch: u64) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }

    /// Parse from a semver string like `"1.2.3"` or `"1.2"` or `"1"`.
    ///
    /// Missing components default to `0`. A leading `v` is allowed.
    ///
    /// # Errors
    /// Returns `VersionParseError` if the string is empty or a component is not numeric.
    pub fn parse(s: &str) -> Result<Self, VersionParseError> {
        let s = s.trim_start_matches('v').trim();
        if s.is_empty() {
            return Err(VersionParseError::EmptyString);
        }
        // Strip pre-release label (everything after `-`)
        let numeric = s.split('-').next().unwrap_or(s);
        let parts: Vec<&str> = numeric.splitn(3, '.').collect();
        let parse_component = |p: Option<&&str>| -> Result<u64, VersionParseError> {
            match p {
                None => Ok(0),
                Some(v) => v
                    .parse::<u64>()
                    .map_err(|_| VersionParseError::InvalidComponent(v.to_string())),
            }
        };
        let major = parse_component(parts.first())?;
        let minor = parse_component(parts.get(1))?;
        let patch = parse_component(parts.get(2))?;
        Ok(Self {
            major,
            minor,
            patch,
        })
    }
}

impl fmt::Display for SemVer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

impl FromStr for SemVer {
    type Err = VersionParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

// ── VersionParseError ────────────────────────────────────────────────────────────

/// Errors that can occur when parsing a version string or constraint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VersionParseError {
    /// The input string was empty.
    EmptyString,
    /// A version component could not be parsed as an integer.
    InvalidComponent(String),
    /// The constraint operator was not recognized.
    UnknownOperator(String),
}

impl fmt::Display for VersionParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyString => write!(f, "version string is empty"),
            Self::InvalidComponent(c) => write!(f, "invalid version component: '{c}'"),
            Self::UnknownOperator(op) => write!(f, "unknown constraint operator: '{op}'"),
        }
    }
}

impl std::error::Error for VersionParseError {}

// ── VersionConstraint ────────────────────────────────────────────────────────────

// ── VersionConstraint pyclass ────────────────────────────────────────────────

#[cfg(feature = "python-bindings")]
#[pyclass(name = "VersionConstraint")]
pub struct PyVersionConstraint {
    pub inner: VersionConstraint,
}

#[cfg(feature = "python-bindings")]
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

/// A version constraint that can be matched against a concrete [`SemVer`].
///
/// See module-level docs for syntax reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VersionConstraint {
    /// Any version (`*`).
    Any,
    /// Exact match (`=X.Y.Z`).
    Exact(SemVer),
    /// At least X.Y.Z (`>=X.Y.Z`).
    AtLeast(SemVer),
    /// Strictly greater than X.Y.Z (`>X.Y.Z`).
    GreaterThan(SemVer),
    /// At most X.Y.Z (`<=X.Y.Z`).
    AtMost(SemVer),
    /// Strictly less than X.Y.Z (`<X.Y.Z`).
    LessThan(SemVer),
    /// Caret — same major, at least minor.patch (`^X.Y.Z`).
    Caret(SemVer),
    /// Tilde — same major.minor, at least patch (`~X.Y.Z`).
    Tilde(SemVer),
}

impl VersionConstraint {
    /// Test whether `version` satisfies this constraint.
    #[must_use]
    pub fn matches(&self, version: SemVer) -> bool {
        match self {
            Self::Any => true,
            Self::Exact(v) => version == *v,
            Self::AtLeast(v) => version >= *v,
            Self::GreaterThan(v) => version > *v,
            Self::AtMost(v) => version <= *v,
            Self::LessThan(v) => version < *v,
            Self::Caret(v) => version.major == v.major && version >= *v,
            Self::Tilde(v) => {
                version.major == v.major && version.minor == v.minor && version.patch >= v.patch
            }
        }
    }
}

impl fmt::Display for VersionConstraint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Any => write!(f, "*"),
            Self::Exact(v) => write!(f, "={v}"),
            Self::AtLeast(v) => write!(f, ">={v}"),
            Self::GreaterThan(v) => write!(f, ">{v}"),
            Self::AtMost(v) => write!(f, "<={v}"),
            Self::LessThan(v) => write!(f, "<{v}"),
            Self::Caret(v) => write!(f, "^{v}"),
            Self::Tilde(v) => write!(f, "~{v}"),
        }
    }
}

impl FromStr for VersionConstraint {
    type Err = VersionParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();
        if s == "*" {
            return Ok(Self::Any);
        }
        if let Some(rest) = s.strip_prefix(">=") {
            return Ok(Self::AtLeast(SemVer::parse(rest)?));
        }
        if let Some(rest) = s.strip_prefix('>') {
            return Ok(Self::GreaterThan(SemVer::parse(rest)?));
        }
        if let Some(rest) = s.strip_prefix("<=") {
            return Ok(Self::AtMost(SemVer::parse(rest)?));
        }
        if let Some(rest) = s.strip_prefix('<') {
            return Ok(Self::LessThan(SemVer::parse(rest)?));
        }
        if let Some(rest) = s.strip_prefix('^') {
            return Ok(Self::Caret(SemVer::parse(rest)?));
        }
        if let Some(rest) = s.strip_prefix('~') {
            return Ok(Self::Tilde(SemVer::parse(rest)?));
        }
        if let Some(rest) = s.strip_prefix('=') {
            return Ok(Self::Exact(SemVer::parse(rest)?));
        }
        // Bare version string treated as exact
        Ok(Self::Exact(SemVer::parse(s)?))
    }
}

// ── VersionedRegistry ─────────────────────────────────────────────────────────────

/// Registry key: `(action_name, dcc_name)`.
type VersionKey = (String, String);

/// Multi-version action registry.
///
/// Allows multiple versions of the same `(action_name, dcc_name)` pair to coexist.
/// Older versions are kept until explicitly removed, enabling backward-compatible
/// resolution through the [`CompatibilityRouter`].
#[derive(Debug, Default, Clone)]
#[cfg_attr(
    feature = "python-bindings",
    pyclass(name = "VersionedRegistry", from_py_object)
)]
pub struct VersionedRegistry {
    /// `(action_name, dcc_name)` → sorted list of `(SemVer, ActionMeta)`
    store: HashMap<VersionKey, Vec<(SemVer, ActionMeta)>>,
}

impl VersionedRegistry {
    /// Create an empty `VersionedRegistry`.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register an action version.
    ///
    /// If the same `(name, dcc, version)` triple already exists it is overwritten
    /// in place, preserving the sort order. Otherwise the new entry is inserted and
    /// the list is re-sorted by version (ascending).
    pub fn register(&mut self, meta: ActionMeta) {
        let ver = SemVer::parse(&meta.version).unwrap_or(SemVer::new(0, 0, 0));
        let key: VersionKey = (meta.name.clone(), meta.dcc.clone());
        let entries = self.store.entry(key).or_default();
        // Replace existing entry with the same semver, or push a new one.
        if let Some(slot) = entries.iter_mut().find(|(v, _)| *v == ver) {
            slot.1 = meta;
        } else {
            entries.push((ver, meta));
            entries.sort_by_key(|(v, _)| *v);
        }
    }

    /// Remove all versions of `(name, dcc)` with a version that satisfies `constraint`.
    ///
    /// Returns the number of versions removed.
    pub fn remove(&mut self, name: &str, dcc: &str, constraint: &VersionConstraint) -> usize {
        let key: VersionKey = (name.to_owned(), dcc.to_owned());
        if let Some(entries) = self.store.get_mut(&key) {
            let before = entries.len();
            entries.retain(|(v, _)| !constraint.matches(*v));
            before - entries.len()
        } else {
            0
        }
    }

    /// List all versions registered for `(name, dcc)`, sorted ascending.
    #[must_use]
    pub fn versions(&self, name: &str, dcc: &str) -> Vec<SemVer> {
        let key: VersionKey = (name.to_owned(), dcc.to_owned());
        self.store
            .get(&key)
            .map(|v| v.iter().map(|(sv, _)| *sv).collect())
            .unwrap_or_default()
    }

    /// Get a specific version of an action, or `None` if not registered.
    #[must_use]
    pub fn get(&self, name: &str, dcc: &str, version: SemVer) -> Option<&ActionMeta> {
        let key: VersionKey = (name.to_owned(), dcc.to_owned());
        self.store
            .get(&key)
            .and_then(|entries| entries.iter().find(|(v, _)| *v == version).map(|(_, m)| m))
    }

    /// Get the latest (highest) version of an action.
    #[must_use]
    pub fn latest(&self, name: &str, dcc: &str) -> Option<&ActionMeta> {
        let key: VersionKey = (name.to_owned(), dcc.to_owned());
        self.store
            .get(&key)
            .and_then(|entries| entries.last().map(|(_, m)| m))
    }

    /// Return a view of all registered `(name, dcc)` keys.
    #[must_use]
    pub fn keys(&self) -> Vec<(String, String)> {
        self.store.keys().cloned().collect()
    }

    /// Total number of versioned entries (across all actions and versions).
    #[must_use]
    pub fn total_entries(&self) -> usize {
        self.store.values().map(|v| v.len()).sum()
    }

    /// Build a [`CompatibilityRouter`] that borrows this registry.
    #[must_use]
    pub fn router(&self) -> CompatibilityRouter<'_> {
        CompatibilityRouter { registry: self }
    }
}

#[cfg(feature = "python-bindings")]
#[pymethods]
impl VersionedRegistry {
    #[new]
    pub fn py_new() -> Self {
        Self::new()
    }

    /// Register an action version.
    ///
    /// If the same ``(name, dcc, version)`` triple already exists it is overwritten.
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

    /// Remove all versions of ``(name, dcc)`` that satisfy the constraint string.
    ///
    /// Returns the number of versions removed.
    #[pyo3(name = "remove")]
    fn py_remove(&mut self, name: &str, dcc: &str, constraint: &PyVersionConstraint) -> usize {
        self.remove(name, dcc, &constraint.inner)
    }

    /// Return all registered versions for ``(name, dcc)``, sorted ascending.
    #[pyo3(name = "versions")]
    fn py_versions(&self, name: &str, dcc: &str) -> Vec<String> {
        self.versions(name, dcc)
            .into_iter()
            .map(|v| v.to_string())
            .collect()
    }

    /// Return the highest registered version string, or ``None`` if not registered.
    #[pyo3(name = "latest_version")]
    fn py_latest_version(&self, name: &str, dcc: &str) -> Option<String> {
        self.latest(name, dcc).map(|m| m.version.clone())
    }

    /// Return all registered ``(name, dcc)`` keys.
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
    ///
    /// Returns the action metadata dict, or ``None`` if no version satisfies the
    /// constraint.
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
        Ok(self
            .router()
            .resolve(name, dcc, &c)
            .map(|m| build_action_meta_dict(py, m))
            .transpose()?)
    }

    /// Return all action metadata dicts that satisfy ``constraint``, sorted ascending.
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
#[cfg(feature = "python-bindings")]
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

// ── CompatibilityRouter ──────────────────────────────────────────────────────────

/// Routes a version constraint to the best-matching registered [`ActionMeta`].
///
/// The resolution strategy is: among all versions that satisfy `constraint`, pick
/// the **highest** one.  If no version satisfies the constraint, returns `None`.
pub struct CompatibilityRouter<'a> {
    registry: &'a VersionedRegistry,
}

impl<'a> CompatibilityRouter<'a> {
    /// Resolve the best-matching version for `(name, dcc)` given `constraint`.
    ///
    /// Returns `None` if the action is not registered or no version satisfies the
    /// constraint.
    #[must_use]
    pub fn resolve(
        &self,
        name: &str,
        dcc: &str,
        constraint: &VersionConstraint,
    ) -> Option<&'a ActionMeta> {
        let key: VersionKey = (name.to_owned(), dcc.to_owned());
        self.registry.store.get(&key).and_then(|entries| {
            entries
                .iter()
                .rfind(|(v, _)| constraint.matches(*v)) // highest because entries are sorted ascending
                .map(|(_, m)| m)
        })
    }

    /// Resolve all versions that satisfy `constraint`, sorted ascending.
    #[must_use]
    pub fn resolve_all(
        &self,
        name: &str,
        dcc: &str,
        constraint: &VersionConstraint,
    ) -> Vec<&'a ActionMeta> {
        let key: VersionKey = (name.to_owned(), dcc.to_owned());
        self.registry
            .store
            .get(&key)
            .map(|entries| {
                entries
                    .iter()
                    .filter(|(v, _)| constraint.matches(*v))
                    .map(|(_, m)| m)
                    .collect()
            })
            .unwrap_or_default()
    }
}
