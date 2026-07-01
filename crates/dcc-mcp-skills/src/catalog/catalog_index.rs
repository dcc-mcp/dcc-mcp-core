//! Inverted-index integration for `SkillCatalog::search_skills`.
//!
//! Provides the `prune_with_index` method that uses the inverted index to
//! narrow the candidate set before BM25 scoring.

use super::scoring::tokenize;
use super::*;
use std::collections::HashSet;

impl SkillCatalog {
    /// Use the inverted index to prune `prefiltered` to only entries that
    /// contain at least one query token.
    ///
    /// Returns `(candidate_entries, original_indices)` where
    /// `original_indices[i]` is the position of `candidate_entries[i]` in
    /// the original `prefiltered` slice. This preserves the mapping needed
    /// for post-scoring lookups (e.g. to recover the original `SkillEntry`).
    ///
    /// When the index is stale or missing, this falls back to returning all
    /// prefiltered entries — the linear path is semantically identical, just
    /// slower.
    pub(super) fn prune_with_index<'a>(
        &self,
        query: &str,
        prefiltered: &'a [SkillEntry],
    ) -> (Vec<&'a SkillEntry>, Vec<usize>) {
        let tokens = tokenize(query);
        if tokens.is_empty() || prefiltered.is_empty() {
            return (
                prefiltered.iter().collect(),
                (0..prefiltered.len()).collect(),
            );
        }

        // Build or rebuild the index if stale.
        {
            let mut guard = self.inverted_index.write();
            if guard.is_stale() {
                let fields: Vec<_> = prefiltered.iter().map(|e| e.field_tokens.clone()).collect();
                let idx = InvertedIndex::build(&fields);
                guard.set(idx);
            }
        }

        // Read the index under a read lock.
        let guard = self.inverted_index.read();
        let idx = match &guard.index {
            Some(i) => i,
            None => {
                // Index not built yet (shouldn't happen after the set above,
                // but be defensive).
                return (
                    prefiltered.iter().collect(),
                    (0..prefiltered.len()).collect(),
                );
            }
        };

        // Collect all document indices that match any query token.
        let mut candidate_set: HashSet<usize> = HashSet::new();
        for token in &tokens {
            if let Some(postings) = idx.get(token) {
                for posting in postings {
                    if posting.doc_idx < prefiltered.len() {
                        candidate_set.insert(posting.doc_idx);
                    }
                }
            }
        }

        if candidate_set.is_empty() {
            // No document matches any query token — return empty.
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
