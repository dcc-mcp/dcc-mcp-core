//! Inverted-index integration for `SkillCatalog::search_skills`.
//!
//! Provides the `prune_with_index` method that uses the inverted index to
//! narrow the candidate set before BM25 scoring.
//!
//! # Index scope (PIP-2469 P0 fix)
//!
//! The index is built from the **full catalog** (`self.entries`), not from the
//! per-query `prefiltered` slice. This gives every skill a stable `doc_idx`
//! that is independent of the current `tags`/`dcc` filter. During pruning,
//! index hits are returned as stable skill names and intersected while the
//! caller applies its current `tags`/`dcc` filters.

use super::*;
use std::collections::HashSet;

impl SkillCatalog {
    pub(super) fn sync_search_index(&self, name: &str) {
        let mut index = self.inverted_index.write();
        if let Some(entry) = self.entries.get(name) {
            index.upsert(name, &entry.field_tokens);
        } else {
            index.remove(name);
        }
    }

    /// Return the stable names of catalog entries matching any query token.
    ///
    /// `None` means the query cannot use the index and callers must retain the
    /// linear path. `Some(empty)` is a valid indexed result with no matches.
    ///
    /// # Index lifecycle
    ///
    /// - **Built** from the full catalog (`self.entries`) on first query after
    ///   any mutation. The `stale` flag is set by `add_skill`, `remove_skill`,
    ///   `register`, `remove`, `rediscover`, `load_skill_object`, and
    ///   `load_skill_metadata`.
    /// - **Invalidated** only by catalog mutation, **not** by filter changes.
    ///   This is correct because the index maps stable catalog-level doc_idx,
    ///   and pruning maps those back through the current prefiltered set.
    pub(super) fn candidate_names_with_index(&self, query: &str) -> Option<HashSet<String>> {
        let tokens = super::scoring::tokenize(query);
        if tokens.is_empty() {
            return None;
        }

        // Build or rebuild the index from the full catalog if stale.
        {
            let mut guard = self.inverted_index.write();
            if guard.is_stale() {
                // Keep DashMap read guards alive while the index borrows names
                // and token fields. This avoids a deep SkillEntry clone for
                // every catalog row on each rebuild.
                let entries: Vec<_> = self.entries.iter().collect();
                let names_and_fields: Vec<_> = entries
                    .iter()
                    .map(|e| (e.value().metadata.name.as_str(), &e.value().field_tokens))
                    .collect();
                let idx = InvertedIndex::build(&names_and_fields);
                guard.set(idx);
            }
        }

        // Read the index under a read lock.
        let guard = self.inverted_index.read();
        let idx = match &guard.index {
            Some(i) => i,
            None => return None,
        };

        let mut candidates = HashSet::new();
        for token in &tokens {
            if let Some(postings) = idx.get(token) {
                for posting in postings {
                    candidates.insert(posting.name);
                }
            }
        }

        Some(candidates)
    }
}
