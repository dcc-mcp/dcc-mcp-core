//! Shared registry contract for ActionRegistry / SkillCatalog /
//! WorkflowCatalog (issue #489).
//!
//! # Design
//!
//! Three components:
//! - [`RegistryEntry`] — marker trait every stored value must implement.
//! - [`Registry<V>`] — shared CRUD + search contract, using `&self` (interior
//!   mutability) so trait objects work behind `Arc`.
//! - [`DefaultRegistry<V>`] — thread-safe `HashMap`-backed default impl for
//!   use cases that don't need specialised secondary indexes.
//!
//! `SkillCatalog` and `ActionRegistry` keep their existing `DashMap` storage;
//! `WorkflowCatalog` keeps its `Vec` with interior mutability via
//! `parking_lot::RwLock`. Each simply adds an `impl Registry<V>` block that
//! delegates to the existing methods.

use std::collections::HashMap;
use std::sync::RwLock;

// ── RegistryEntry ────────────────────────────────────────────────────────────

/// Every value stored in a [`Registry`] must implement this trait.
///
/// - [`key`](RegistryEntry::key) is the stable lookup key (must be unique
///   within a given registry).
/// - [`search_tags`](RegistryEntry::search_tags) returns tokens used for
///   free-text substring matching in [`Registry::search`].
pub trait RegistryEntry: Clone + Send + Sync {
    /// Stable lookup key — must be unique within the registry.
    fn key(&self) -> String;

    /// Tokens used by [`Registry::search`] for substring matching.
    fn search_tags(&self) -> Vec<String>;
}

// ── SearchQuery ──────────────────────────────────────────────────────────────

/// Parameters for [`Registry::search`].
#[derive(Debug, Clone, Default)]
pub struct SearchQuery {
    /// Free-text query string (case-insensitive substring match against
    /// [`RegistryEntry::search_tags`]).
    pub query: String,
    /// Optional cap on the number of results returned.
    pub limit: Option<usize>,
}

// ── Registry<V> ──────────────────────────────────────────────────────────────

/// Shared registry contract: insert, look up, list, remove, count, search.
///
/// All methods take `&self` — implementations use interior mutability
/// (`DashMap`, `RwLock`, etc.) so the trait is usable behind `Arc<dyn Registry<V>>`.
pub trait Registry<V: RegistryEntry> {
    /// Insert (or overwrite) an entry.
    fn register(&self, entry: V);

    /// Retrieve a clone of the entry with the given key, or `None`.
    fn get(&self, key: &str) -> Option<V>;

    /// Return clones of all entries in an unspecified order.
    fn list(&self) -> Vec<V>;

    /// Remove the entry with the given key.
    ///
    /// Returns `true` if an entry was removed, `false` if the key was unknown.
    fn remove(&self, key: &str) -> bool;

    /// Number of entries currently stored.
    fn count(&self) -> usize;

    /// Free-text search over [`RegistryEntry::search_tags`] (case-insensitive
    /// substring match).  Results are unordered unless the implementation
    /// applies additional ranking.
    fn search(&self, query: &SearchQuery) -> Vec<V>;
}

// ── DefaultRegistry<V> ───────────────────────────────────────────────────────

/// Thread-safe `HashMap`-backed [`Registry`] implementation.
///
/// Suitable for registries that do not need ordered iteration or secondary
/// indexes. Uses [`std::sync::RwLock`] for interior mutability — zero extra
/// crate dependencies.
///
/// `WorkflowCatalog` currently preserves insertion order via a `Vec` and
/// therefore implements [`Registry`] directly rather than delegating to
/// `DefaultRegistry`.
#[derive(Debug)]
pub struct DefaultRegistry<V: RegistryEntry> {
    inner: RwLock<HashMap<String, V>>,
}

impl<V: RegistryEntry> Default for DefaultRegistry<V> {
    fn default() -> Self {
        Self {
            inner: RwLock::new(HashMap::new()),
        }
    }
}

impl<V: RegistryEntry> DefaultRegistry<V> {
    /// Create an empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

impl<V: RegistryEntry> Registry<V> for DefaultRegistry<V> {
    fn register(&self, entry: V) {
        self.inner
            .write()
            .expect("DefaultRegistry lock poisoned")
            .insert(entry.key(), entry);
    }

    fn get(&self, key: &str) -> Option<V> {
        self.inner
            .read()
            .expect("DefaultRegistry lock poisoned")
            .get(key)
            .cloned()
    }

    fn list(&self) -> Vec<V> {
        self.inner
            .read()
            .expect("DefaultRegistry lock poisoned")
            .values()
            .cloned()
            .collect()
    }

    fn remove(&self, key: &str) -> bool {
        self.inner
            .write()
            .expect("DefaultRegistry lock poisoned")
            .remove(key)
            .is_some()
    }

    fn count(&self) -> usize {
        self.inner
            .read()
            .expect("DefaultRegistry lock poisoned")
            .len()
    }

    fn search(&self, query: &SearchQuery) -> Vec<V> {
        let q = query.query.to_ascii_lowercase();
        let guard = self.inner.read().expect("DefaultRegistry lock poisoned");
        let mut results: Vec<V> = guard
            .values()
            .filter(|v| {
                v.search_tags()
                    .iter()
                    .any(|tag| tag.to_ascii_lowercase().contains(&q))
            })
            .cloned()
            .collect();
        if let Some(limit) = query.limit {
            results.truncate(limit);
        }
        results
    }
}

// ── Shared test harness (enabled by the `testing` feature or in `cfg(test)`) ─

#[cfg(any(test, feature = "testing"))]
pub mod testing {
    //! Shared parameterised test harness for [`super::Registry`] impls.
    //!
    //! Gate with the crate's `testing` feature so downstream crates can import
    //! [`assert_registry_contract`] in their own `#[cfg(test)]` modules without
    //! pulling in test-only code in production builds.
    //!
    //! # Usage (downstream crate)
    //!
    //! ```toml
    //! # Cargo.toml [dev-dependencies]
    //! dcc-mcp-models = { workspace = true, features = ["testing"] }
    //! ```
    //!
    //! ```rust,ignore
    //! use dcc_mcp_models::registry::testing::assert_registry_contract;
    //! ```

    use super::{Registry, RegistryEntry, SearchQuery};

    /// Exercise the full CRUD + search contract for any [`Registry`] impl.
    ///
    /// Panics with a descriptive message on any contract violation.
    pub fn assert_registry_contract<R, V>(make: impl Fn() -> R, sample: V)
    where
        R: Registry<V>,
        V: RegistryEntry + PartialEq + std::fmt::Debug,
    {
        let r = make();

        // Empty on construction.
        assert_eq!(r.count(), 0, "new registry should be empty");
        assert!(r.list().is_empty(), "new registry list should be empty");

        // Register one entry.
        r.register(sample.clone());
        assert_eq!(r.count(), 1, "count after one register should be 1");

        // Get by key.
        let fetched = r.get(&sample.key());
        assert_eq!(
            fetched,
            Some(sample.clone()),
            "get should return the registered entry"
        );

        // List contains the entry.
        let listing = r.list();
        assert!(
            listing.contains(&sample),
            "list should contain the registered entry; got {listing:?}"
        );

        // Remove.
        assert!(
            r.remove(&sample.key()),
            "remove should return true for existing key"
        );
        assert_eq!(r.count(), 0, "count should be 0 after remove");
        assert!(
            r.get(&sample.key()).is_none(),
            "get should return None after remove"
        );

        // Remove non-existent → false.
        assert!(
            !r.remove(&sample.key()),
            "remove of missing key should return false"
        );

        // Search finds the entry.
        r.register(sample.clone());
        let tags = sample.search_tags();
        assert!(
            !tags.is_empty(),
            "search_tags must not be empty for a valid RegistryEntry"
        );
        let results = r.search(&SearchQuery {
            query: tags[0].clone(),
            limit: None,
        });
        assert!(
            !results.is_empty(),
            "search by first search_tag should find the entry; tag={:?}",
            tags[0]
        );
    }
}
