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
//! index hits are intersected with the current prefiltered set via a
//! `name → prefiltered_position` lookup so the correct entries are returned
//! regardless of which filter is active.

use super::scoring::FieldTokens;
use super::*;
use std::collections::{HashMap, HashSet};

impl SkillCatalog {
    /// Use the inverted index to prune `prefiltered` to only entries that
    /// contain at least one query token.
    ///
    /// Returns `(candidate_entries, original_indices)` where
    /// `original_indices[i]` is the position of `candidate_entries[i]` in
    /// the original `prefiltered` slice.
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
    pub(super) fn prune_with_index<'a>(
        &self,
        query: &str,
        prefiltered: &'a [SkillEntry],
    ) -> (Vec<&'a SkillEntry>, Vec<usize>) {
        let tokens = super::scoring::tokenize(query);
        if tokens.is_empty() || prefiltered.is_empty() {
            return (
                prefiltered.iter().collect(),
                (0..prefiltered.len()).collect(),
            );
        }

        // Build or rebuild the index from the full catalog if stale.
        {
            let mut guard = self.inverted_index.write();
            if guard.is_stale() {
                let entries: Vec<_> = self.entries.iter().map(|e| e.value().clone()).collect();
                let names_and_fields: Vec<(&str, &FieldTokens)> = entries
                    .iter()
                    .map(|e| (e.metadata.name.as_str(), &e.field_tokens))
                    .collect();
                let idx = InvertedIndex::build(&names_and_fields);
                guard.set(idx);
            }
        }

        // Read the index under a read lock.
        let guard = self.inverted_index.read();
        let idx = match &guard.index {
            Some(i) => i,
            None => {
                return (
                    prefiltered.iter().collect(),
                    (0..prefiltered.len()).collect(),
                );
            }
        };

        // Build a lookup: catalog-level doc_idx → prefiltered position.
        //
        // The index stores doc_idx relative to the full ordered snapshot of
        // `self.entries` at build time. The current prefiltered slice may be
        // a subset (tags/dcc filter) or a different ordering — we map back
        // through skill name, which is the stable identity.
        let mut name_to_prefiltered_pos: HashMap<&str, usize> =
            HashMap::with_capacity(prefiltered.len());
        for (i, entry) in prefiltered.iter().enumerate() {
            name_to_prefiltered_pos.insert(&entry.metadata.name, i);
        }

        // Collect candidate prefiltered positions that match any query token.
        let mut candidate_set: HashSet<usize> = HashSet::new();
        for token in &tokens {
            if let Some(postings) = idx.get(token) {
                for posting in postings {
                    if let Some(&pos) = name_to_prefiltered_pos.get(posting.name.as_str()) {
                        candidate_set.insert(pos);
                    }
                }
            }
        }

        if candidate_set.is_empty() {
            return (Vec::new(), Vec::new());
        }

        // Preserve the original ordering of prefiltered entries.
        let mut candidates: Vec<&SkillEntry> = Vec::with_capacity(candidate_set.len());
        let mut indices: Vec<usize> = Vec::with_capacity(candidate_set.len());
        for (i, entry) in prefiltered.iter().enumerate() {
            if candidate_set.contains(&i) {
                candidates.push(entry);
                indices.push(i);
            }
        }

        (candidates, indices)
    }
}
