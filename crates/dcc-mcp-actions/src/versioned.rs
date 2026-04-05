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

    #[staticmethod]
    fn parse_str(s: &str) -> PyResult<Self> {
        Self::parse(s).map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))
    }

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

    fn py_register(&mut self, name: String, dcc: String, version: String) {
        self.register(ActionMeta {
            name,
            dcc,
            version,
            ..Default::default()
        });
    }

    fn py_remove(&mut self, name: &str, dcc: &str, constraint: &PyVersionConstraint) -> usize {
        self.remove(name, dcc, &constraint.inner)
    }

    fn py_versions(&self, name: &str, dcc: &str) -> Vec<SemVer> {
        self.versions(name, dcc)
    }

    fn py_latest_version(&self, name: &str, dcc: &str) -> Option<String> {
        self.latest(name, dcc).map(|m| m.version.clone())
    }

    fn py_keys(&self) -> Vec<(String, String)> {
        self.keys()
    }

    fn py_total_entries(&self) -> usize {
        self.store.values().map(|v| v.len()).sum()
    }

    fn py_resolve(
        &self,
        name: &str,
        dcc: &str,
        constraint: &PyVersionConstraint,
    ) -> Option<String> {
        self.router()
            .resolve(name, dcc, &constraint.inner)
            .map(|m| m.version.clone())
    }

    fn __repr__(&self) -> String {
        format!(
            "VersionedRegistry(entries={})",
            self.store.values().map(|v| v.len()).sum::<usize>()
        )
    }
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

// ── Tests ────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── SemVer parsing ───────────────────────────────────────────────────────────

    mod test_semver_parse {
        use super::*;

        #[test]
        fn test_parse_full_triple() {
            let v = SemVer::parse("1.2.3").unwrap();
            assert_eq!(v, SemVer::new(1, 2, 3));
        }

        #[test]
        fn test_parse_with_v_prefix() {
            let v = SemVer::parse("v2.0.0").unwrap();
            assert_eq!(v, SemVer::new(2, 0, 0));
        }

        #[test]
        fn test_parse_two_components() {
            let v = SemVer::parse("3.1").unwrap();
            assert_eq!(v, SemVer::new(3, 1, 0));
        }

        #[test]
        fn test_parse_one_component() {
            let v = SemVer::parse("4").unwrap();
            assert_eq!(v, SemVer::new(4, 0, 0));
        }

        #[test]
        fn test_parse_strips_prerelease_label() {
            let v = SemVer::parse("1.0.0-alpha").unwrap();
            assert_eq!(v, SemVer::new(1, 0, 0));
        }

        #[test]
        fn test_parse_strips_prerelease_complex() {
            let v = SemVer::parse("2.1.0-beta.3").unwrap();
            assert_eq!(v, SemVer::new(2, 1, 0));
        }

        #[test]
        fn test_parse_empty_returns_error() {
            assert_eq!(SemVer::parse(""), Err(VersionParseError::EmptyString));
        }

        #[test]
        fn test_parse_invalid_component_returns_error() {
            let err = SemVer::parse("1.x.0").unwrap_err();
            assert!(matches!(err, VersionParseError::InvalidComponent(_)));
        }

        #[test]
        fn test_ordering_major() {
            assert!(SemVer::new(2, 0, 0) > SemVer::new(1, 9, 9));
        }

        #[test]
        fn test_ordering_minor() {
            assert!(SemVer::new(1, 3, 0) > SemVer::new(1, 2, 99));
        }

        #[test]
        fn test_ordering_patch() {
            assert!(SemVer::new(1, 0, 5) > SemVer::new(1, 0, 4));
        }

        #[test]
        fn test_display() {
            assert_eq!(SemVer::new(1, 2, 3).to_string(), "1.2.3");
        }

        #[test]
        fn test_fromstr_trait() {
            let v: SemVer = "3.14.15".parse().unwrap();
            assert_eq!(v, SemVer::new(3, 14, 15));
        }
    }

    // ── VersionConstraint parsing & matching ─────────────────────────────────────

    mod test_constraints {
        use super::*;

        fn ver(s: &str) -> SemVer {
            SemVer::parse(s).unwrap()
        }

        fn constraint(s: &str) -> VersionConstraint {
            s.parse().unwrap()
        }

        #[test]
        fn test_any_matches_everything() {
            let c = constraint("*");
            assert!(c.matches(ver("0.0.1")));
            assert!(c.matches(ver("99.99.99")));
        }

        #[test]
        fn test_exact_matches_only_same() {
            let c = constraint("=1.2.3");
            assert!(c.matches(ver("1.2.3")));
            assert!(!c.matches(ver("1.2.4")));
            assert!(!c.matches(ver("1.2.2")));
        }

        #[test]
        fn test_bare_version_is_exact() {
            let c = constraint("1.2.3");
            assert!(c.matches(ver("1.2.3")));
            assert!(!c.matches(ver("1.2.4")));
        }

        #[test]
        fn test_at_least() {
            let c = constraint(">=1.2.0");
            assert!(c.matches(ver("1.2.0")));
            assert!(c.matches(ver("1.2.1")));
            assert!(c.matches(ver("2.0.0")));
            assert!(!c.matches(ver("1.1.9")));
        }

        #[test]
        fn test_greater_than() {
            let c = constraint(">1.2.0");
            assert!(c.matches(ver("1.2.1")));
            assert!(!c.matches(ver("1.2.0")));
            assert!(!c.matches(ver("1.1.9")));
        }

        #[test]
        fn test_at_most() {
            let c = constraint("<=2.0.0");
            assert!(c.matches(ver("2.0.0")));
            assert!(c.matches(ver("1.9.9")));
            assert!(!c.matches(ver("2.0.1")));
        }

        #[test]
        fn test_less_than() {
            let c = constraint("<2.0.0");
            assert!(c.matches(ver("1.9.9")));
            assert!(!c.matches(ver("2.0.0")));
        }

        #[test]
        fn test_caret_same_major() {
            let c = constraint("^1.2.0");
            assert!(c.matches(ver("1.2.0")));
            assert!(c.matches(ver("1.5.3")));
            assert!(c.matches(ver("1.99.0")));
            assert!(!c.matches(ver("2.0.0")));
            assert!(!c.matches(ver("1.1.9")));
        }

        #[test]
        fn test_caret_major_zero() {
            // ^0.2.0 should only allow same major (0), minor >= 2
            let c = constraint("^0.2.0");
            assert!(c.matches(ver("0.2.0")));
            assert!(c.matches(ver("0.3.0")));
            assert!(!c.matches(ver("1.0.0")));
        }

        #[test]
        fn test_tilde_same_major_minor() {
            let c = constraint("~1.2.3");
            assert!(c.matches(ver("1.2.3")));
            assert!(c.matches(ver("1.2.9")));
            assert!(!c.matches(ver("1.3.0")));
            assert!(!c.matches(ver("2.2.3")));
        }

        #[test]
        fn test_constraint_display_round_trip() {
            for s in [
                "*", "=1.2.3", ">=1.0.0", ">2.0.0", "<=3.0.0", "<1.0.0", "^1.2.3", "~1.2.3",
            ] {
                let c: VersionConstraint = s.parse().unwrap();
                assert_eq!(c.to_string(), s, "round-trip failed for '{s}'");
            }
        }
    }

    // ── VersionedRegistry ────────────────────────────────────────────────────────

    mod test_versioned_registry {
        use super::*;

        fn meta(name: &str, dcc: &str, version: &str) -> ActionMeta {
            ActionMeta {
                name: name.into(),
                dcc: dcc.into(),
                version: version.into(),
                ..Default::default()
            }
        }

        #[test]
        fn test_register_and_versions() {
            let mut vr = VersionedRegistry::new();
            vr.register(meta("action", "maya", "1.0.0"));
            vr.register(meta("action", "maya", "1.2.0"));
            vr.register(meta("action", "maya", "2.0.0"));

            let versions = vr.versions("action", "maya");
            assert_eq!(
                versions,
                vec![
                    SemVer::new(1, 0, 0),
                    SemVer::new(1, 2, 0),
                    SemVer::new(2, 0, 0)
                ]
            );
        }

        #[test]
        fn test_register_same_version_overwrites() {
            let mut vr = VersionedRegistry::new();
            let mut m = meta("act", "maya", "1.0.0");
            m.description = "old".into();
            vr.register(m);

            let mut m2 = meta("act", "maya", "1.0.0");
            m2.description = "new".into();
            vr.register(m2);

            let versions = vr.versions("act", "maya");
            assert_eq!(versions.len(), 1);
            assert_eq!(
                vr.get("act", "maya", SemVer::new(1, 0, 0))
                    .unwrap()
                    .description,
                "new"
            );
        }

        #[test]
        fn test_register_independent_dccs() {
            let mut vr = VersionedRegistry::new();
            vr.register(meta("action", "maya", "1.0.0"));
            vr.register(meta("action", "blender", "2.0.0"));

            assert_eq!(vr.versions("action", "maya"), vec![SemVer::new(1, 0, 0)]);
            assert_eq!(vr.versions("action", "blender"), vec![SemVer::new(2, 0, 0)]);
        }

        #[test]
        fn test_latest_returns_highest() {
            let mut vr = VersionedRegistry::new();
            vr.register(meta("x", "maya", "1.0.0"));
            vr.register(meta("x", "maya", "3.0.0"));
            vr.register(meta("x", "maya", "2.0.0"));

            assert_eq!(vr.latest("x", "maya").unwrap().version, "3.0.0");
        }

        #[test]
        fn test_latest_returns_none_for_unknown() {
            let vr = VersionedRegistry::new();
            assert!(vr.latest("unknown", "maya").is_none());
        }

        #[test]
        fn test_get_specific_version() {
            let mut vr = VersionedRegistry::new();
            vr.register(meta("a", "maya", "1.0.0"));
            vr.register(meta("a", "maya", "2.0.0"));

            assert!(vr.get("a", "maya", SemVer::new(1, 0, 0)).is_some());
            assert!(vr.get("a", "maya", SemVer::new(2, 0, 0)).is_some());
            assert!(vr.get("a", "maya", SemVer::new(3, 0, 0)).is_none());
        }

        #[test]
        fn test_remove_by_constraint() {
            let mut vr = VersionedRegistry::new();
            vr.register(meta("a", "maya", "1.0.0"));
            vr.register(meta("a", "maya", "1.5.0"));
            vr.register(meta("a", "maya", "2.0.0"));

            let constraint: VersionConstraint = "^1.0.0".parse().unwrap();
            let removed = vr.remove("a", "maya", &constraint);
            assert_eq!(removed, 2); // 1.0.0 and 1.5.0 removed
            assert_eq!(vr.versions("a", "maya"), vec![SemVer::new(2, 0, 0)]);
        }

        #[test]
        fn test_total_entries() {
            let mut vr = VersionedRegistry::new();
            vr.register(meta("a", "maya", "1.0.0"));
            vr.register(meta("a", "maya", "2.0.0"));
            vr.register(meta("b", "maya", "1.0.0"));
            assert_eq!(vr.total_entries(), 3);
        }

        #[test]
        fn test_keys_contains_all_pairs() {
            let mut vr = VersionedRegistry::new();
            vr.register(meta("a", "maya", "1.0.0"));
            vr.register(meta("b", "blender", "1.0.0"));

            let mut keys = vr.keys();
            keys.sort();
            assert!(keys.contains(&("a".to_string(), "maya".to_string())));
            assert!(keys.contains(&("b".to_string(), "blender".to_string())));
        }
    }

    // ── CompatibilityRouter ──────────────────────────────────────────────────────

    mod test_router {
        use super::*;

        fn meta(name: &str, dcc: &str, version: &str) -> ActionMeta {
            ActionMeta {
                name: name.into(),
                dcc: dcc.into(),
                version: version.into(),
                description: format!("{version} description"),
                ..Default::default()
            }
        }

        fn registry_with_versions() -> VersionedRegistry {
            let mut vr = VersionedRegistry::new();
            vr.register(meta("create_sphere", "maya", "1.0.0"));
            vr.register(meta("create_sphere", "maya", "1.2.0"));
            vr.register(meta("create_sphere", "maya", "1.5.0"));
            vr.register(meta("create_sphere", "maya", "2.0.0"));
            vr
        }

        #[test]
        fn test_resolve_any_returns_latest() {
            let vr = registry_with_versions();
            let result = vr
                .router()
                .resolve("create_sphere", "maya", &VersionConstraint::Any);
            assert_eq!(result.unwrap().version, "2.0.0");
        }

        #[test]
        fn test_resolve_caret_picks_highest_compatible() {
            let vr = registry_with_versions();
            let c: VersionConstraint = "^1.0.0".parse().unwrap();
            let result = vr.router().resolve("create_sphere", "maya", &c);
            assert_eq!(result.unwrap().version, "1.5.0");
        }

        #[test]
        fn test_resolve_at_least() {
            let vr = registry_with_versions();
            let c: VersionConstraint = ">=1.2.0".parse().unwrap();
            let result = vr.router().resolve("create_sphere", "maya", &c);
            assert_eq!(result.unwrap().version, "2.0.0");
        }

        #[test]
        fn test_resolve_tilde_picks_patch() {
            let mut vr = VersionedRegistry::new();
            vr.register(meta("a", "maya", "1.2.0"));
            vr.register(meta("a", "maya", "1.2.5"));
            vr.register(meta("a", "maya", "1.3.0"));

            let c: VersionConstraint = "~1.2.0".parse().unwrap();
            let result = vr.router().resolve("a", "maya", &c);
            assert_eq!(result.unwrap().version, "1.2.5");
        }

        #[test]
        fn test_resolve_exact() {
            let vr = registry_with_versions();
            let c: VersionConstraint = "=1.2.0".parse().unwrap();
            let result = vr.router().resolve("create_sphere", "maya", &c);
            assert_eq!(result.unwrap().version, "1.2.0");
        }

        #[test]
        fn test_resolve_returns_none_when_no_match() {
            let vr = registry_with_versions();
            let c: VersionConstraint = ">=3.0.0".parse().unwrap();
            assert!(vr.router().resolve("create_sphere", "maya", &c).is_none());
        }

        #[test]
        fn test_resolve_unknown_action_returns_none() {
            let vr = registry_with_versions();
            assert!(
                vr.router()
                    .resolve("nonexistent", "maya", &VersionConstraint::Any)
                    .is_none()
            );
        }

        #[test]
        fn test_resolve_all_with_caret() {
            let vr = registry_with_versions();
            let c: VersionConstraint = "^1.0.0".parse().unwrap();
            let results = vr.router().resolve_all("create_sphere", "maya", &c);
            let versions: Vec<&str> = results.iter().map(|m| m.version.as_str()).collect();
            assert_eq!(versions, vec!["1.0.0", "1.2.0", "1.5.0"]);
        }

        #[test]
        fn test_resolve_all_none_matching() {
            let vr = registry_with_versions();
            let c: VersionConstraint = ">=10.0.0".parse().unwrap();
            assert!(
                vr.router()
                    .resolve_all("create_sphere", "maya", &c)
                    .is_empty()
            );
        }

        #[test]
        fn test_resolve_less_than() {
            let vr = registry_with_versions();
            let c: VersionConstraint = "<1.5.0".parse().unwrap();
            let result = vr.router().resolve("create_sphere", "maya", &c);
            // highest below 1.5.0 is 1.2.0
            assert_eq!(result.unwrap().version, "1.2.0");
        }

        #[test]
        fn test_resolve_dcc_isolation() {
            let mut vr = VersionedRegistry::new();
            vr.register(meta("a", "maya", "1.0.0"));
            vr.register(meta("a", "blender", "2.0.0"));

            let c = VersionConstraint::Any;
            assert_eq!(
                vr.router().resolve("a", "maya", &c).unwrap().version,
                "1.0.0"
            );
            assert_eq!(
                vr.router().resolve("a", "blender", &c).unwrap().version,
                "2.0.0"
            );
        }
    }
}
