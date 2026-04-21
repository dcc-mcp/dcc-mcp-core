//! In-process idempotency cache keyed by `(scope, rendered_key)`.
//!
//! A step with an `idempotency_key` template renders the key against the
//! workflow context at dispatch time; before invoking the caller, the
//! executor consults this cache. A hit short-circuits the step and reuses
//! the cached output; a miss runs the step and stores the result on
//! success.
//!
//! `IdempotencyScope::Workflow` keys are siloed by workflow id so two
//! parallel workflows with identical keys do not collide. `Global` keys
//! share a single namespace across the process.

use std::collections::HashMap;

use parking_lot::RwLock;
use serde_json::Value;
use uuid::Uuid;

use crate::policy::IdempotencyScope;

/// Process-local idempotency cache.
#[derive(Debug, Default, Clone)]
pub struct IdempotencyCache {
    inner: std::sync::Arc<RwLock<HashMap<String, Value>>>,
}

impl IdempotencyCache {
    /// Construct an empty cache.
    pub fn new() -> Self {
        Self::default()
    }

    fn compose_key(&self, scope: IdempotencyScope, workflow_id: Uuid, rendered: &str) -> String {
        match scope {
            IdempotencyScope::Global => format!("G:{rendered}"),
            IdempotencyScope::Workflow => format!("W:{workflow_id}:{rendered}"),
        }
    }

    /// Look up a cached output.
    pub fn get(&self, scope: IdempotencyScope, workflow_id: Uuid, rendered: &str) -> Option<Value> {
        let k = self.compose_key(scope, workflow_id, rendered);
        self.inner.read().get(&k).cloned()
    }

    /// Record a successful output.
    pub fn put(&self, scope: IdempotencyScope, workflow_id: Uuid, rendered: &str, output: Value) {
        let k = self.compose_key(scope, workflow_id, rendered);
        self.inner.write().insert(k, output);
    }

    /// Number of entries. Testing helper.
    pub fn len(&self) -> usize {
        self.inner.read().len()
    }

    /// Whether cache is empty. Testing helper.
    pub fn is_empty(&self) -> bool {
        self.inner.read().is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn workflow_scope_isolates_by_id() {
        let cache = IdempotencyCache::new();
        let w1 = Uuid::new_v4();
        let w2 = Uuid::new_v4();
        cache.put(IdempotencyScope::Workflow, w1, "k1", json!({"v": 1}));
        assert_eq!(
            cache.get(IdempotencyScope::Workflow, w1, "k1"),
            Some(json!({"v": 1}))
        );
        assert_eq!(cache.get(IdempotencyScope::Workflow, w2, "k1"), None);
    }

    #[test]
    fn global_scope_crosses_workflows() {
        let cache = IdempotencyCache::new();
        let w1 = Uuid::new_v4();
        let w2 = Uuid::new_v4();
        cache.put(IdempotencyScope::Global, w1, "same", json!("x"));
        assert_eq!(
            cache.get(IdempotencyScope::Global, w2, "same"),
            Some(json!("x"))
        );
    }

    #[test]
    fn miss_returns_none() {
        let cache = IdempotencyCache::new();
        let w = Uuid::new_v4();
        assert!(cache.get(IdempotencyScope::Workflow, w, "k").is_none());
    }
}
