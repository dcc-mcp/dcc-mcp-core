//! Idempotency cache abstraction shared across workflow executors.
//!
//! A step with an `idempotency_key` template renders the key against the
//! workflow context at dispatch time; before invoking the caller, the
//! executor consults the configured [`IdempotencyStore`]. A hit
//! short-circuits the step and reuses the cached output; a miss runs the
//! step and stores the result on success.
//!
//! [`IdempotencyScope::Workflow`] keys are siloed by workflow id so two
//! parallel workflows with identical keys do not collide. `Global` keys
//! share a single namespace across the process / database.
//!
//! Two implementations ship in this crate:
//!
//! * [`IdempotencyCache`] — process-local `RwLock<HashMap<…>>`, the
//!   historical default. No persistence; entries die with the executor.
//! * `crate::sqlite::SqliteIdempotencyStore` — durable, keyed on the same
//!   SQLite connection used by [`crate::sqlite::WorkflowStorage`]. Gated
//!   behind the `job-persist-sqlite` Cargo feature. Survives server
//!   restarts so that a re-run of the same spec short-circuits steps that
//!   were already completed in the previous run.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::RwLock;
use serde_json::Value;
use uuid::Uuid;

use crate::policy::IdempotencyScope;

/// Convenient type alias: a shared, dyn-dispatched idempotency store.
pub type SharedIdempotencyStore = Arc<dyn IdempotencyStore>;

/// Pluggable idempotency cache. Implementations are responsible for
/// honouring the optional per-entry TTL passed to [`IdempotencyStore::put`].
pub trait IdempotencyStore: Send + Sync + std::fmt::Debug {
    /// Look up a cached, non-expired output for `(scope, workflow_id, key)`.
    fn get(&self, scope: IdempotencyScope, workflow_id: Uuid, key: &str) -> Option<Value>;

    /// Record a successful output. `step_id` is recorded for diagnostics
    /// and downstream resume tooling. `ttl_secs = None` means the entry
    /// lives until its scope is purged (workflow-scoped: forever within
    /// the workflow row; global: forever or until [`Self::purge_expired`]
    /// removes it).
    fn put(
        &self,
        scope: IdempotencyScope,
        workflow_id: Uuid,
        key: &str,
        step_id: &str,
        output: Value,
        ttl_secs: Option<u64>,
    );

    /// Remove every row whose `expires_at` is in the past. Returns the
    /// number of rows removed. Implementations without TTL support may
    /// return 0.
    fn purge_expired(&self) -> usize {
        0
    }
}

#[derive(Debug, Clone)]
struct CachedEntry {
    value: Value,
    expires_at: Option<Instant>,
}

/// Process-local idempotency cache. Default for executors that don't opt
/// into a persistent backend.
#[derive(Debug, Default, Clone)]
pub struct IdempotencyCache {
    inner: Arc<RwLock<HashMap<String, CachedEntry>>>,
}

impl IdempotencyCache {
    /// Construct an empty cache.
    pub fn new() -> Self {
        Self::default()
    }

    fn compose_key(scope: IdempotencyScope, workflow_id: Uuid, rendered: &str) -> String {
        match scope {
            IdempotencyScope::Global => format!("G:{rendered}"),
            IdempotencyScope::Workflow => format!("W:{workflow_id}:{rendered}"),
        }
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

impl IdempotencyStore for IdempotencyCache {
    fn get(&self, scope: IdempotencyScope, workflow_id: Uuid, rendered: &str) -> Option<Value> {
        let k = Self::compose_key(scope, workflow_id, rendered);
        let now = Instant::now();
        let guard = self.inner.read();
        let entry = guard.get(&k)?;
        if let Some(exp) = entry.expires_at
            && exp <= now
        {
            return None;
        }
        Some(entry.value.clone())
    }

    fn put(
        &self,
        scope: IdempotencyScope,
        workflow_id: Uuid,
        rendered: &str,
        _step_id: &str,
        output: Value,
        ttl_secs: Option<u64>,
    ) {
        let k = Self::compose_key(scope, workflow_id, rendered);
        let expires_at = ttl_secs
            .filter(|n| *n > 0)
            .and_then(|n| Instant::now().checked_add(Duration::from_secs(n)));
        self.inner.write().insert(
            k,
            CachedEntry {
                value: output,
                expires_at,
            },
        );
    }

    fn purge_expired(&self) -> usize {
        let now = Instant::now();
        let mut guard = self.inner.write();
        let before = guard.len();
        guard.retain(|_, entry| match entry.expires_at {
            Some(exp) => exp > now,
            None => true,
        });
        before - guard.len()
    }
}

#[cfg(test)]
mod tests;
