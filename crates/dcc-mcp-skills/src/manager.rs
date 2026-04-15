//! [`SkillsManager`] — session-isolated, dual-cache skill discovery.
//!
//! Wraps [`SkillCatalog`] with two independent caches so that repeated
//! calls with the same parameters avoid redundant filesystem scans:
//!
//! | Cache | Key | Purpose |
//! |-------|-----|---------|
//! | `by_paths` | sorted extra-paths + DCC name | Shared across all sessions that use the same paths |
//! | `by_config` | paths + config overrides fingerprint | Session-level isolation when config differs |
//!
//! # Typical usage
//!
//! ```no_run
//! use dcc_mcp_skills::{SkillCatalog, manager::SkillsManager};
//! use dcc_mcp_actions::ActionRegistry;
//! use std::sync::Arc;
//! use std::time::Duration;
//!
//! let registry = Arc::new(ActionRegistry::new());
//! let catalog  = Arc::new(SkillCatalog::new(registry));
//! let manager  = SkillsManager::new(catalog.clone());
//!
//! // First call: runs a real filesystem scan and caches the result (TTL = 60 s).
//! let count = manager.discover_cached(None, Some("maya"), Duration::from_secs(60));
//!
//! // Second call (same paths, within TTL): returns cached count, no scan.
//! let count2 = manager.discover_cached(None, Some("maya"), Duration::from_secs(60));
//! assert_eq!(count, count2);
//! ```

use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
    time::{Duration, Instant},
};

use dcc_mcp_models::SkillScope;

use crate::catalog::SkillCatalog;

// ── Cache entry ───────────────────────────────────────────────────────────

struct CacheEntry {
    /// When this result was computed.
    discovered_at: Instant,
    /// Number of skills discovered in that scan.
    count: usize,
    /// TTL at the time of caching (used to decide whether to honour it).
    ttl: Duration,
}

impl CacheEntry {
    fn is_fresh(&self) -> bool {
        self.discovered_at.elapsed() < self.ttl
    }
}

// ── SkillsManager ─────────────────────────────────────────────────────────

/// Session-isolated, dual-cache skill discovery manager.
///
/// See the [module documentation](self) for full details.
pub struct SkillsManager {
    catalog: Arc<SkillCatalog>,

    /// Cache keyed by `paths_fingerprint(extra_paths, dcc_name)`.
    by_paths: RwLock<HashMap<String, CacheEntry>>,

    /// Cache keyed by `config_fingerprint(extra_paths, dcc_name, overrides)`.
    ///
    /// Useful when two sessions use the same base paths but different config
    /// overrides (e.g. different DCC versions or feature flags).
    by_config: RwLock<HashMap<String, CacheEntry>>,
}

impl SkillsManager {
    /// Create a new manager wrapping the given catalog.
    pub fn new(catalog: Arc<SkillCatalog>) -> Self {
        Self {
            catalog,
            by_paths: RwLock::new(HashMap::new()),
            by_config: RwLock::new(HashMap::new()),
        }
    }

    /// Access the underlying catalog directly.
    pub fn catalog(&self) -> &Arc<SkillCatalog> {
        &self.catalog
    }

    /// Discover skills, using the **paths cache** to skip redundant scans.
    ///
    /// - If a fresh cache entry exists for `(extra_paths, dcc_name)`, returns
    ///   the cached count without touching the filesystem.
    /// - Otherwise runs a full scan, stores the result, and returns the count.
    pub fn discover_cached(
        &self,
        extra_paths: Option<&[String]>,
        dcc_name: Option<&str>,
        ttl: Duration,
    ) -> usize {
        let key = Self::paths_fingerprint(extra_paths, dcc_name);

        // Fast path: check read lock first
        if let Ok(cache) = self.by_paths.read() {
            if let Some(entry) = cache.get(&key) {
                if entry.is_fresh() {
                    tracing::debug!(key = %key, "SkillsManager: paths cache hit");
                    return entry.count;
                }
            }
        }

        // Cache miss — run the real scan
        let count = self.catalog.discover(extra_paths, dcc_name);

        if let Ok(mut cache) = self.by_paths.write() {
            cache.insert(
                key,
                CacheEntry {
                    discovered_at: Instant::now(),
                    count,
                    ttl,
                },
            );
        }
        count
    }

    /// Like [`discover_cached`](Self::discover_cached) but accepts `config_overrides`
    /// that are mixed into the cache key, providing **session-level isolation**.
    ///
    /// Two sessions with different `config_overrides` will never share a cache
    /// entry even if they scan the same paths.
    pub fn discover_with_config(
        &self,
        extra_paths: Option<&[String]>,
        dcc_name: Option<&str>,
        config_overrides: &[(&str, &str)],
        ttl: Duration,
    ) -> usize {
        let key = Self::config_fingerprint(extra_paths, dcc_name, config_overrides);

        if let Ok(cache) = self.by_config.read() {
            if let Some(entry) = cache.get(&key) {
                if entry.is_fresh() {
                    tracing::debug!(key = %key, "SkillsManager: config cache hit");
                    return entry.count;
                }
            }
        }

        let count = self.catalog.discover(extra_paths, dcc_name);

        if let Ok(mut cache) = self.by_config.write() {
            cache.insert(
                key,
                CacheEntry {
                    discovered_at: Instant::now(),
                    count,
                    ttl,
                },
            );
        }
        count
    }

    /// Discover skills from scope-tagged paths, using the paths cache.
    pub fn discover_scoped_cached(
        &self,
        scoped_paths: &[(SkillScope, Vec<String>)],
        dcc_name: Option<&str>,
        ttl: Duration,
    ) -> usize {
        // Build a combined fingerprint from all scoped paths
        let mut all_paths: Vec<String> = scoped_paths
            .iter()
            .flat_map(|(scope, paths)| {
                paths
                    .iter()
                    .map(move |p| format!("{}:{}", scope.label(), p))
            })
            .collect();
        all_paths.sort_unstable();
        let key = format!("scoped:{}:{}", all_paths.join("|"), dcc_name.unwrap_or(""));

        if let Ok(cache) = self.by_paths.read() {
            if let Some(entry) = cache.get(&key) {
                if entry.is_fresh() {
                    tracing::debug!(key = %key, "SkillsManager: scoped paths cache hit");
                    return entry.count;
                }
            }
        }

        let count = self.catalog.discover_scoped(scoped_paths, dcc_name);

        if let Ok(mut cache) = self.by_paths.write() {
            cache.insert(
                key,
                CacheEntry {
                    discovered_at: Instant::now(),
                    count,
                    ttl,
                },
            );
        }
        count
    }

    /// Evict all stale cache entries (both caches).
    ///
    /// Call periodically (e.g. every minute) to keep memory usage bounded.
    pub fn evict_stale(&self) {
        if let Ok(mut cache) = self.by_paths.write() {
            cache.retain(|_, e| e.is_fresh());
        }
        if let Ok(mut cache) = self.by_config.write() {
            cache.retain(|_, e| e.is_fresh());
        }
    }

    /// Clear **all** cache entries, forcing the next discovery to rescan.
    pub fn clear_cache(&self) {
        if let Ok(mut c) = self.by_paths.write() {
            c.clear();
        }
        if let Ok(mut c) = self.by_config.write() {
            c.clear();
        }
    }

    // ── Fingerprint helpers ───────────────────────────────────────────────

    fn paths_fingerprint(extra_paths: Option<&[String]>, dcc_name: Option<&str>) -> String {
        let mut parts: Vec<&str> = extra_paths
            .unwrap_or(&[])
            .iter()
            .map(String::as_str)
            .collect();
        parts.sort_unstable();
        format!("{}::{}", parts.join("|"), dcc_name.unwrap_or(""))
    }

    fn config_fingerprint(
        extra_paths: Option<&[String]>,
        dcc_name: Option<&str>,
        overrides: &[(&str, &str)],
    ) -> String {
        let base = Self::paths_fingerprint(extra_paths, dcc_name);
        let mut kv: Vec<String> = overrides.iter().map(|(k, v)| format!("{k}={v}")).collect();
        kv.sort_unstable();
        format!("{base}::{}", kv.join(","))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_paths_fingerprint_stable() {
        let a = SkillsManager::paths_fingerprint(
            Some(&["b".to_string(), "a".to_string()]),
            Some("maya"),
        );
        let b = SkillsManager::paths_fingerprint(
            Some(&["a".to_string(), "b".to_string()]),
            Some("maya"),
        );
        assert_eq!(a, b, "fingerprint must be order-independent");
    }

    #[test]
    fn test_config_fingerprint_differs_by_override() {
        let base = SkillsManager::config_fingerprint(None, Some("maya"), &[]);
        let with_override =
            SkillsManager::config_fingerprint(None, Some("maya"), &[("version", "2025")]);
        assert_ne!(base, with_override);
    }
}
