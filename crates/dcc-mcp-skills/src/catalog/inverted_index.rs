//! Optional token index utility — token → posting list + term frequency.
//!
//! The production skill catalog does not prune the canonical fuzzy scorer with
//! this exact-token index because doing so would discard typo/prefix matches.
//! It remains available for callers that explicitly need exact-token lookup.
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
//! Tokenless queries use the linear scan. A stale index is rebuilt before it
//! participates in candidate selection and the flag is cleared only after a
//! successful rebuild.

use super::scoring::FieldTokens;
use dashmap::DashMap;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

type DocumentTokens = (usize, HashMap<String, usize>);

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
    documents: Arc<DashMap<String, DocumentTokens>>,
    next_doc_idx: Arc<AtomicUsize>,
}

impl InvertedIndex {
    /// Build an inverted index from a slice of skill names and their
    /// `FieldTokens`. The names provide stable document identity so the
    /// posting list remains correct across different `tags`/`dcc` filters.
    ///
    /// Complexity: O(total tokens across all fields). The posting lists are
    /// built by scanning every token in every field for every document.
    pub fn build(names_and_fields: &[(&str, &FieldTokens)]) -> Self {
        let index = Self {
            index: Arc::new(DashMap::new()),
            documents: Arc::new(DashMap::new()),
            next_doc_idx: Arc::new(AtomicUsize::new(0)),
        };
        for (name, fields) in names_and_fields {
            index.upsert(name, fields);
        }
        index
    }

    /// Add or replace one searchable document without rebuilding the catalog.
    pub fn upsert(&self, name: &str, fields: &FieldTokens) {
        self.remove(name);
        let doc_idx = self.next_doc_idx.fetch_add(1, Ordering::Relaxed);
        let token_tfs = doc_token_tfs(fields);
        for (token, tf) in &token_tfs {
            let mut postings = self.index.entry(token.clone()).or_default();
            postings.push(Posting {
                name: name.to_string(),
                doc_idx,
                tf: *tf,
            });
            postings.sort_by_key(|posting| posting.doc_idx);
        }
        self.documents
            .insert(name.to_string(), (doc_idx, token_tfs));
    }

    /// Remove one searchable document and all of its postings.
    pub fn remove(&self, name: &str) {
        let Some((_, (_, token_tfs))) = self.documents.remove(name) else {
            return;
        };
        for token in token_tfs.keys() {
            let remove_token = if let Some(mut postings) = self.index.get_mut(token) {
                postings.retain(|posting| posting.name != name);
                postings.is_empty()
            } else {
                false
            };
            if remove_token {
                self.index.remove(token);
            }
        }
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
}
