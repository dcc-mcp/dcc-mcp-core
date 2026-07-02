//! LRU cache for hot gateway search queries (PIP-2471).
//!
//! Agents often repeat discovery loops with the same query/filters;
//! this cache avoids redundant scoring at scale.
//!
//! ## Cache key
//!
//! The canonical key is a normalised concatenation of the search
//! parameters that affect ranking:
//! `(query, mode, dcc_type, dcc_types, instance_id, offset, limit,
//!   loaded_only, tags, tags_any, exclude_tags,
//!   scene_hint, min_score, skill_hint, or_queries)`
//!
//! ## Invalidation
//!
//! The cache is invalidated whenever the capability index changes:
//! on any instance join, leave, or capability fingerprint change.
//! Callers MUST call [`invalidate`] after any index mutation.

use std::num::NonZeroUsize;
use std::sync::Mutex;
use std::time::Instant;

use lru::LruCache;

/// Configuration for the search result cache.
#[derive(Debug, Clone)]
pub struct SearchCacheConfig {
    /// Maximum number of cached entries.
    pub capacity: NonZeroUsize,
    /// Entries older than this are evicted on access.
    pub ttl: std::time::Duration,
}

impl Default for SearchCacheConfig {
    fn default() -> Self {
        Self {
            // 256 is generous enough to absorb repeated agent loops
            // without holding stale data for long.
            capacity: NonZeroUsize::new(256).expect("256 > 0"),
            // 60s matches the typical agent loop cadence — longer
            // TTLs risk serving stale results after a backend refresh.
            ttl: std::time::Duration::from_secs(60),
        }
    }
}

/// A cached search result entry.
#[derive(Debug, Clone)]
pub struct CachedSearchResult {
    /// Serialized JSON response body (`{hits, total}`).
    pub body: Vec<u8>,
    /// When this entry was inserted.
    pub inserted_at: Instant,
    /// Index generation at insertion time (for cross-check).
    pub index_generation: String,
}

/// A deterministic cache key built from normalised search parameters.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SearchCacheKey {
    inner: String,
}

impl SearchCacheKey {
    /// Build a cache key from the fields that affect search ranking.
    ///
    /// Fields that are `None` or empty are omitted so that a query
    /// with no filters and the same query with an explicit `None`
    /// share the same cache entry.
    pub fn from_query(query: &crate::gateway::capability::SearchQuery) -> Self {
        let mut parts = Vec::new();
        parts.push(format!("q={}", query.query));

        // Sort and deduplicate multi-value fields for deterministic keys.
        if !query.dcc_types.is_empty() {
            let mut types: Vec<&str> = query.dcc_types.iter().map(String::as_str).collect();
            types.sort();
            parts.push(format!("dts={}", types.join(",")));
        }
        if let Some(ref dt) = query.dcc_type {
            parts.push(format!("dt={dt}"));
        }
        if let Some(iid) = query.instance_id {
            parts.push(format!("iid={iid}"));
        }
        if let Some(offset) = query.offset {
            parts.push(format!("o={offset}"));
        }
        if let Some(limit) = query.limit {
            parts.push(format!("l={limit}"));
        }
        if let Some(lo) = query.loaded_only {
            parts.push(format!("lo={lo}"));
        }
        if !query.tags.is_empty() {
            let mut tags: Vec<&str> = query.tags.iter().map(String::as_str).collect();
            tags.sort();
            parts.push(format!("t={}", tags.join(",")));
        }
        if !query.tags_any.is_empty() {
            let mut tags: Vec<&str> = query.tags_any.iter().map(String::as_str).collect();
            tags.sort();
            parts.push(format!("ta={}", tags.join(",")));
        }
        if !query.exclude_tags.is_empty() {
            let mut tags: Vec<&str> = query.exclude_tags.iter().map(String::as_str).collect();
            tags.sort();
            parts.push(format!("et={}", tags.join(",")));
        }
        // Fields that affect ranking — omitting these would let queries
        // with different scores/OR-clauses/scene-hints share a cache entry
        // and return wrong rank order.
        if let Some(ref sh) = query.scene_hint {
            parts.push(format!("sh={sh}"));
        }
        if let Some(ms) = query.min_score {
            parts.push(format!("ms={ms}"));
        }
        if let Some(ref sk) = query.skill_hint {
            parts.push(format!("sk={sk}"));
        }
        if !query.or_queries.is_empty() {
            let mut oqs: Vec<&str> = query.or_queries.iter().map(String::as_str).collect();
            oqs.sort();
            parts.push(format!("oq={}", oqs.join(",")));
        }
        parts.push(format!("m={:?}", query.mode));

        Self {
            inner: parts.join("|"),
        }
    }
}

/// Thread-safe LRU cache for search results.
///
/// Wrapped in `Arc<Mutex<...>>` so it can be shared across handlers
/// and invalidated from the refresh path.
pub struct SearchCache {
    cache: Mutex<LruCache<SearchCacheKey, CachedSearchResult>>,
    ttl: std::time::Duration,
    /// Monotonically increasing generation counter bumped on every
    /// invalidation — used to detect stale entries without holding
    /// the lock for the duration of a `get`.
    generation: Mutex<u64>,
}

impl SearchCache {
    /// Create a new cache with the given config.
    pub fn new(config: SearchCacheConfig) -> Self {
        Self {
            cache: Mutex::new(LruCache::new(config.capacity)),
            ttl: config.ttl,
            generation: Mutex::new(0),
        }
    }

    /// Look up a cached result.
    ///
    /// Returns `None` if the key is missing or the entry is expired.
    pub fn get(&self, key: &SearchCacheKey) -> Option<Vec<u8>> {
        self.get_with_index_gen(key, None)
    }

    /// Look up a cached result with index-generation validation.
    ///
    /// When `current_index_gen` is `Some(...)` and differs from the
    /// stored `index_generation`, the entry is treated as a miss
    /// and evicted — this handles index-change invalidation.
    pub fn get_with_index_gen(
        &self,
        key: &SearchCacheKey,
        current_index_gen: Option<&str>,
    ) -> Option<Vec<u8>> {
        let mut cache = self.cache.lock().unwrap();
        let entry = cache.get(key)?;
        if entry.inserted_at.elapsed() >= self.ttl {
            cache.pop(key);
            return None;
        }
        if let Some(current_gen) = current_index_gen
            && entry.index_generation != current_gen
        {
            cache.pop(key);
            return None;
        }
        Some(entry.body.clone())
    }

    /// Store a search result.
    pub fn put(&self, key: SearchCacheKey, body: Vec<u8>, index_generation: String) {
        let mut cache = self.cache.lock().unwrap();
        cache.put(
            key,
            CachedSearchResult {
                body,
                inserted_at: Instant::now(),
                index_generation,
            },
        );
    }

    /// Invalidate the entire cache.
    ///
    /// Bumps the generation counter so that concurrent `get` calls
    /// with an old generation see a miss. Returns the new generation.
    pub fn invalidate(&self) -> u64 {
        let mut cache = self.cache.lock().unwrap();
        cache.clear();
        let mut guard = self.generation.lock().unwrap();
        *guard = guard.wrapping_add(1);
        *guard
    }

    /// Return the current generation counter.
    pub fn generation(&self) -> u64 {
        *self.generation.lock().unwrap()
    }

    /// Return the number of cached entries.
    pub fn len(&self) -> usize {
        self.cache.lock().unwrap().len()
    }

    /// Return true if the cache is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl std::fmt::Debug for SearchCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let len = self.len();
        let generation = self.generation();
        f.debug_struct("SearchCache")
            .field("len", &len)
            .field("generation", &generation)
            .field("ttl", &self.ttl)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_query(text: &str) -> crate::gateway::capability::SearchQuery {
        crate::gateway::capability::SearchQuery {
            query: text.to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn cache_hit_and_miss() {
        let cache = SearchCache::new(SearchCacheConfig::default());

        let key = SearchCacheKey::from_query(&make_query("sphere"));
        assert!(cache.get(&key).is_none());

        cache.put(key.clone(), b"cached".to_vec(), "gen1".into());
        assert_eq!(cache.get(&key), Some(b"cached".to_vec()));
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn cache_invalidation() {
        let cache = SearchCache::new(SearchCacheConfig::default());

        let key = SearchCacheKey::from_query(&make_query("sphere"));
        cache.put(key.clone(), b"cached".to_vec(), "gen1".into());

        let new_gen = cache.invalidate();
        assert_ne!(new_gen, 0);
        assert!(cache.is_empty());
        assert!(cache.get(&key).is_none());
    }

    #[test]
    fn ttl_expiry() {
        let config = SearchCacheConfig {
            ttl: std::time::Duration::from_millis(1),
            ..Default::default()
        };
        let cache = SearchCache::new(config);

        let key = SearchCacheKey::from_query(&make_query("sphere"));
        cache.put(key.clone(), b"cached".to_vec(), "gen1".into());

        std::thread::sleep(std::time::Duration::from_millis(10));
        assert!(cache.get(&key).is_none());
    }

    #[test]
    fn deterministic_keys() {
        let q1 = make_query("sphere");
        let q2 = make_query("sphere");
        assert_eq!(
            SearchCacheKey::from_query(&q1).inner,
            SearchCacheKey::from_query(&q2).inner,
        );
    }

    #[test]
    fn different_queries_different_keys() {
        let k1 = SearchCacheKey::from_query(&make_query("sphere"));
        let k2 = SearchCacheKey::from_query(&make_query("cube"));
        assert_ne!(k1.inner, k2.inner);
    }

    #[test]
    fn filters_produce_different_keys() {
        let mut q1 = make_query("sphere");
        q1.dcc_type = Some("maya".into());
        let q2 = make_query("sphere");

        assert_ne!(
            SearchCacheKey::from_query(&q1).inner,
            SearchCacheKey::from_query(&q2).inner,
        );
    }

    #[test]
    fn multi_value_fields_are_sorted() {
        let mut q1 = make_query("render");
        q1.tags = vec!["geo".into(), "anim".into()];
        let mut q2 = make_query("render");
        q2.tags = vec!["anim".into(), "geo".into()];

        assert_eq!(
            SearchCacheKey::from_query(&q1).inner,
            SearchCacheKey::from_query(&q2).inner,
        );
    }
}
