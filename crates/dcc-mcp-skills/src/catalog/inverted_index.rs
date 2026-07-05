//! Inverted index for `search_skills` — token → posting list + term frequency.
//!
//! At 10k+ skills the full linear scan in `score_skills_with_tokens` becomes
//! the dominant cost. The inverted index maps every token that appears in any
//! skill's `FieldTokens` to the set of document indices that contain it,
//! together with the per-document term frequency. When a query arrives, the
//! scorer only visits documents that intersect the query's posting lists
//! instead of iterating over the entire catalog.
//!
//! # Index lifecycle
//!
//! - **Built lazily** on the first `search_skills` call that carries a query.
//!   The build walks every `SkillEntry` in the catalog and is O(total tokens).
//! - **Invalidated** on every mutation that changes a skill's searchable text:
//!   `register`, `remove`, `add_skill`, `remove_skill`, `rediscover`,
//!   `load_skill_object` (via `refresh_tokens`), and `load_skill_metadata`
//!   (via `refresh_tokens`).
//! - **Re-built** on the next search after invalidation.
//! - The `stale` flag is cheap (`AtomicBool`); invalidation never blocks on
//!   the index lock.
//!
//! # Data structure
//!
//! ```text
//! token → { posting_list: Vec<(doc_idx, term_freq)>, doc_freq: usize }
//! ```
//!
//! A "document" is a skill entry identified by its positional index in the
//! ordered snapshot of all entries. Term frequency is the total count of that
//! token across all nine `FieldTokens` sub-fields.
//!
//! # Fallback path
//!
//! When the index is stale (e.g. because it hasn't been rebuilt yet after a
//! mutation), `search_skills` falls back to the existing linear scan. The
//! stale flag is checked at the top of the query path; if set, the index is
//! not used and the linear path is taken. The flag is only cleared after a
//! successful rebuild. This guarantees correctness at all times — stale
//! index = slower, but never wrong results.

use super::scoring::FieldTokens;
use dashmap::DashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// A single posting: skill name + term frequency in that document.
///
/// Uses skill name as the stable document identity so the posting list is
/// independent of the per-query prefiltered slice ordering (PIP-2469 P0).
#[derive(Debug, Clone)]
pub struct Posting {
    pub name: String,
    pub doc_idx: usize,
    #[allow(dead_code)]
    pub tf: usize,
}

/// The inverted index: token → posting list.
///
/// Uses `DashMap` internally so the build phase can insert in parallel
/// (one thread per entry), but the read path is single-threaded so the
/// outer `Arc` provides cheap sharing.
#[derive(Debug, Clone)]
pub struct InvertedIndex {
    /// Maps a token string to its posting list. The posting list is sorted
    /// by `doc_idx` for deterministic iteration.
    index: Arc<DashMap<String, Vec<Posting>>>,
}

impl InvertedIndex {
    /// Build an inverted index from a slice of skill names and their
    /// `FieldTokens`. The names provide stable document identity so the
    /// posting list remains correct across different `tags`/`dcc` filters.
    ///
    /// Complexity: O(total tokens across all fields). The posting lists are
    /// built by scanning every token in every field for every document.
    pub fn build(names_and_fields: &[(&str, &FieldTokens)]) -> Self {
        let index = Arc::new(DashMap::<String, Vec<Posting>>::new());

        // Accumulate per-document token → tf maps first, then merge into
        // the global index. This avoids locking the DashMap shard on every
        // single token insertion.
        let per_doc: Vec<_> = names_and_fields
            .iter()
            .map(|(name, ft)| (name.to_string(), doc_token_tfs(ft)))
            .collect();

        for (doc_idx, (name, doc_tf)) in per_doc.iter().enumerate() {
            for (token, tf) in doc_tf.iter() {
                index.entry(token.clone()).or_default().push(Posting {
                    name: name.clone(),
                    doc_idx,
                    tf: *tf,
                });
            }
        }

        // Sort each posting list by doc_idx for deterministic iteration.
        for mut entry in index.iter_mut() {
            entry.value_mut().sort_by_key(|p| p.doc_idx);
        }

        InvertedIndex { index }
    }

    /// Return the posting list for `token`, if present.
    #[inline]
    pub fn get(&self, token: &str) -> Option<impl Iterator<Item = Posting> + '_> {
        self.index
            .get(token)
            .map(|entry| entry.value().clone().into_iter())
    }

    /// Return the document frequency (number of docs containing `token`).
    #[allow(dead_code)]
    #[inline]
    pub fn doc_freq(&self, token: &str) -> usize {
        self.index.get(token).map(|entry| entry.len()).unwrap_or(0)
    }

    /// Return the total number of unique tokens in the index.
    #[allow(dead_code)]
    #[inline]
    pub fn token_count(&self) -> usize {
        self.index.len()
    }
}

/// The index guard held by `SkillCatalog`. Wraps the built index plus a
/// stale flag so the catalog can invalidate without locking the index.
#[derive(Debug)]
pub(crate) struct IndexGuard {
    pub index: Option<Arc<InvertedIndex>>,
    pub stale: Arc<AtomicBool>,
}

impl Default for IndexGuard {
    fn default() -> Self {
        Self {
            index: None,
            stale: Arc::new(AtomicBool::new(true)),
        }
    }
}

impl IndexGuard {
    /// Mark the index as stale so the next query rebuilds it.
    pub fn invalidate(&self) {
        self.stale.store(true, Ordering::Release);
    }

    /// Check whether the index is stale.
    pub fn is_stale(&self) -> bool {
        self.stale.load(Ordering::Acquire)
    }

    /// Replace the index and clear the stale flag atomically.
    pub fn set(&mut self, index: InvertedIndex) {
        self.index = Some(Arc::new(index));
        self.stale.store(false, Ordering::Release);
    }
}

// ── helpers ─────────────────────────────────────────────────────────────

/// Collect (token, term_frequency) for every token in a single document's
/// `FieldTokens`. A token that appears in multiple fields has its TF summed.
fn doc_token_tfs(ft: &FieldTokens) -> std::collections::HashMap<String, usize> {
    let mut tf = std::collections::HashMap::new();
    for token in ft
        .name
        .iter()
        .chain(&ft.tags)
        .chain(&ft.hint)
        .chain(&ft.aliases)
        .chain(&ft.description)
        .chain(&ft.tool_names)
        .chain(&ft.tool_aliases)
        .chain(&ft.tool_descriptions)
        .chain(&ft.dcc)
    {
        *tf.entry(token.clone()).or_default() += 1;
    }
    tf
}

// ── tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::super::scoring::FieldTokens;
    use super::*;

    #[test]
    fn test_build_empty() {
        let idx = InvertedIndex::build(&[]);
        assert_eq!(idx.token_count(), 0);
        assert_eq!(idx.doc_freq("any"), 0);
    }

    #[test]
    fn test_build_single_doc() {
        let ft = FieldTokens {
            name: vec!["polygon".to_string(), "bevel".to_string()],
            description: vec!["polygon".to_string(), "tools".to_string()],
            ..Default::default()
        };

        let idx = InvertedIndex::build(&[("test-skill", &ft)]);
        assert!(idx.token_count() >= 3);
        assert_eq!(idx.doc_freq("polygon"), 1);
        assert_eq!(idx.doc_freq("bevel"), 1);
        assert_eq!(idx.doc_freq("tools"), 1);
        assert_eq!(idx.doc_freq("nonexistent"), 0);

        let postings: Vec<_> = idx.get("polygon").unwrap().collect();
        assert_eq!(postings.len(), 1);
        assert_eq!(postings[0].name, "test-skill");
        assert_eq!(postings[0].doc_idx, 0);
        assert_eq!(postings[0].tf, 2, "polygon appears in name + description");
    }

    #[test]
    fn test_build_multi_doc() {
        let ft0 = FieldTokens {
            name: vec!["polygon".to_string()],
            tags: vec!["modeling".to_string()],
            ..Default::default()
        };

        let ft1 = FieldTokens {
            name: vec!["render".to_string()],
            tags: vec!["modeling".to_string()],
            ..Default::default()
        };

        let idx = InvertedIndex::build(&[("skill-a", &ft0), ("skill-b", &ft1)]);
        assert_eq!(idx.doc_freq("polygon"), 1);
        assert_eq!(idx.doc_freq("render"), 1);
        assert_eq!(idx.doc_freq("modeling"), 2, "shared token");
        assert_eq!(idx.doc_freq("absent"), 0);

        // Posting lists must be sorted by doc_idx.
        let posts: Vec<_> = idx.get("modeling").unwrap().collect();
        assert_eq!(posts.len(), 2);
        assert!(posts.windows(2).all(|w| w[0].doc_idx < w[1].doc_idx));
    }

    #[test]
    fn test_index_guard_stale_default() {
        let guard = IndexGuard::default();
        assert!(guard.is_stale());
        assert!(guard.index.is_none());
    }

    #[test]
    fn test_index_guard_set_and_invalidate() {
        let mut guard = IndexGuard::default();
        let idx = InvertedIndex::build(&[]);
        guard.set(idx);
        assert!(!guard.is_stale());
        assert!(guard.index.is_some());

        guard.invalidate();
        assert!(guard.is_stale());
        // index ref still alive but stale flag says "don't use".
    }
}
