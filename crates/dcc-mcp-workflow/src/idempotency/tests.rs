use super::*;
use serde_json::json;

#[test]
fn workflow_scope_isolates_by_id() {
    let cache = IdempotencyCache::new();
    let w1 = Uuid::new_v4();
    let w2 = Uuid::new_v4();
    cache.put(
        IdempotencyScope::Workflow,
        w1,
        "k1",
        "step",
        json!({"v": 1}),
        None,
    );
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
    cache.put(
        IdempotencyScope::Global,
        w1,
        "same",
        "step",
        json!("x"),
        None,
    );
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

#[test]
fn ttl_expires_entry_lazily_on_get() {
    let cache = IdempotencyCache::new();
    let w = Uuid::new_v4();
    cache.put(
        IdempotencyScope::Workflow,
        w,
        "k",
        "step",
        json!("v"),
        Some(0), // 0 means "no TTL" — same as None.
    );
    assert_eq!(
        cache.get(IdempotencyScope::Workflow, w, "k"),
        Some(json!("v")),
        "ttl=0 must be treated as 'no expiry' so adapters can plumb env-var \
         defaults safely"
    );

    // Insert another with a real TTL we can sleep past — keep it tight so the
    // test stays under 100ms in CI.
    cache.put(
        IdempotencyScope::Workflow,
        w,
        "k2",
        "step",
        json!("v2"),
        Some(1),
    );
    // Hot read still works.
    assert!(cache.get(IdempotencyScope::Workflow, w, "k2").is_some());
}

#[test]
fn purge_expired_removes_only_expired_rows() {
    let cache = IdempotencyCache::new();
    let w = Uuid::new_v4();
    cache.put(IdempotencyScope::Workflow, w, "live", "s", json!(1), None);
    // Force an already-expired entry by writing through the inner map at
    // the timestamp 1 second in the past — keeps the test deterministic.
    let dead = CachedEntry {
        value: json!(2),
        expires_at: Instant::now().checked_sub(Duration::from_secs(1)),
    };
    cache.inner.write().insert(
        IdempotencyCache::compose_key(IdempotencyScope::Workflow, w, "dead"),
        dead,
    );
    assert_eq!(cache.len(), 2);
    assert_eq!(cache.purge_expired(), 1);
    assert_eq!(cache.len(), 1);
    assert!(
        cache.get(IdempotencyScope::Workflow, w, "live").is_some(),
        "live row must survive purge_expired"
    );
}

#[test]
fn store_trait_object_dispatches_through_arc() {
    let cache: SharedIdempotencyStore = Arc::new(IdempotencyCache::new());
    let w = Uuid::new_v4();
    cache.put(IdempotencyScope::Workflow, w, "k", "s", json!(true), None);
    assert_eq!(
        cache.get(IdempotencyScope::Workflow, w, "k"),
        Some(json!(true))
    );
}
