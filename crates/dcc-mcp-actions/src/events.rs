//! EventBus — thread-safe event publish/subscribe system.

#[cfg(feature = "python-bindings")]
use pyo3::prelude::*;

#[cfg(feature = "stub-gen")]
use pyo3_stub_gen_derive::{gen_stub_pyclass, gen_stub_pymethods};

use dashmap::DashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

/// Event subscriber ID for unsubscription.
type SubscriberId = u64;

/// Callback type for non-Python mode (placeholder for future pure-Rust subscribe API).
///
/// Wrapped in `Arc` so callbacks can be cloned out of the DashMap before
/// invocation — this avoids holding a read-lock while executing user code.
#[cfg(not(feature = "python-bindings"))]
type EventCallback = Arc<dyn Fn() + Send + Sync>;

/// Type alias for the subscriber storage map.
#[cfg(feature = "python-bindings")]
type SubscriberMap = Arc<DashMap<String, Vec<(SubscriberId, Py<PyAny>)>>>;
#[cfg(not(feature = "python-bindings"))]
type SubscriberMap = Arc<DashMap<String, Vec<(SubscriberId, EventCallback)>>>;

/// Thread-safe event bus.
#[cfg_attr(feature = "stub-gen", gen_stub_pyclass)]
#[cfg_attr(
    feature = "python-bindings",
    pyclass(name = "EventBus", from_py_object)
)]
#[derive(Clone)]
pub struct EventBus {
    next_id: Arc<AtomicU64>,
    subscribers: SubscriberMap,
}

impl std::fmt::Debug for EventBus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EventBus")
            .field("next_id", &self.next_id.load(Ordering::Relaxed))
            .field(
                "subscriber_events",
                &self
                    .subscribers
                    .iter()
                    .map(|r| r.key().clone())
                    .collect::<Vec<_>>(),
            )
            .finish()
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

impl EventBus {
    #[must_use]
    pub fn new() -> Self {
        Self {
            next_id: Arc::new(AtomicU64::new(0)),
            subscribers: Arc::new(DashMap::new()),
        }
    }

    /// Get the count of all subscriptions.
    #[must_use]
    pub fn subscription_count(&self) -> usize {
        self.subscribers.iter().map(|r| r.value().len()).sum()
    }

    /// Check if any subscribers exist for the given event.
    #[must_use]
    pub fn has_subscribers(&self, event_name: &str) -> bool {
        self.subscribers
            .get(event_name)
            .map(|s| !s.is_empty())
            .unwrap_or(false)
    }

    /// Allocate a new subscriber ID (starts at 1, monotonically increasing).
    fn next_subscriber_id(&self) -> SubscriberId {
        self.next_id.fetch_add(1, Ordering::Relaxed) + 1
    }

    /// Remove a subscriber by ID from a specific event (shared logic).
    fn remove_subscriber(&self, event_name: &str, subscriber_id: SubscriberId) -> bool {
        if let Some(mut subs) = self.subscribers.get_mut(event_name) {
            let before = subs.len();
            subs.retain(|(id, _)| *id != subscriber_id);
            return subs.len() < before;
        }
        false
    }
}

// ── Non-Python Rust API ──

#[cfg(not(feature = "python-bindings"))]
impl EventBus {
    /// Subscribe a Rust closure to an event. Returns subscriber ID for unsubscription.
    #[must_use]
    pub fn subscribe<F>(&self, event_name: String, callback: F) -> SubscriberId
    where
        F: Fn() + Send + Sync + 'static,
    {
        let id = self.next_subscriber_id();
        self.subscribers
            .entry(event_name)
            .or_default()
            .push((id, Arc::new(callback)));
        id
    }

    /// Unsubscribe from an event by subscriber ID.
    #[must_use]
    pub fn unsubscribe(&self, event_name: &str, subscriber_id: SubscriberId) -> bool {
        self.remove_subscriber(event_name, subscriber_id)
    }

    /// Publish an event, calling all subscribers.
    ///
    /// Callbacks are collected before invocation to avoid holding a DashMap
    /// read-lock while executing user code (which could attempt to
    /// subscribe/unsubscribe, causing a deadlock on the same shard).
    ///
    /// Each callback is wrapped in [`std::panic::catch_unwind`] so that a
    /// panicking subscriber does not prevent subsequent subscribers from
    /// being called.
    pub fn publish(&self, event_name: &str) {
        let callbacks: Vec<_> = self
            .subscribers
            .get(event_name)
            .map(|subs| subs.iter().map(|(_, cb)| Clone::clone(cb)).collect())
            .unwrap_or_default();
        for callback in &callbacks {
            if let Err(e) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                callback();
            })) {
                let msg = e
                    .downcast_ref::<&str>()
                    .copied()
                    .or_else(|| e.downcast_ref::<String>().map(|s| s.as_str()))
                    .unwrap_or("unknown panic");
                tracing::error!("Panic in event subscriber for {event_name}: {msg}");
            }
        }
    }
}

// ── Python bindings ──

// NOTE: subscribe() takes Py<PyAny>, publish() uses **kwargs — stubs may have imperfect types
#[cfg_attr(feature = "stub-gen", gen_stub_pymethods)]
#[cfg(feature = "python-bindings")]
#[pymethods]
impl EventBus {
    #[new]
    fn py_new() -> Self {
        Self::new()
    }

    /// Subscribe to an event. Returns subscriber ID for unsubscription.
    fn subscribe(&self, event_name: String, callback: Py<PyAny>) -> SubscriberId {
        let id = self.next_subscriber_id();
        self.subscribers
            .entry(event_name)
            .or_default()
            .push((id, callback));
        id
    }

    /// Unsubscribe from an event by subscriber ID.
    #[pyo3(signature = (event_name, subscriber_id))]
    fn unsubscribe(&self, event_name: &str, subscriber_id: SubscriberId) -> bool {
        self.remove_subscriber(event_name, subscriber_id)
    }

    /// Publish an event, calling all subscribers.
    ///
    /// Callbacks are collected before invocation to avoid holding a DashMap
    /// read-lock while executing user code (which could attempt to
    /// subscribe/unsubscribe, causing a deadlock on the same shard).
    #[pyo3(signature = (event_name, **kwargs))]
    fn publish(
        &self,
        py: Python,
        event_name: &str,
        kwargs: Option<&Bound<'_, pyo3::types::PyDict>>,
    ) {
        let callbacks: Vec<Py<PyAny>> = self
            .subscribers
            .get(event_name)
            .map(|subs| subs.iter().map(|(_, cb)| cb.clone_ref(py)).collect())
            .unwrap_or_default();
        for callback in &callbacks {
            let result = if let Some(kw) = kwargs {
                callback.call(py, (), Some(kw))
            } else {
                callback.call0(py)
            };
            if let Err(e) = result {
                tracing::error!("Error in event subscriber for {}: {}", event_name, e);
            }
        }
    }

    fn __repr__(&self) -> String {
        format!("EventBus(subscriptions={})", self.subscription_count())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Happy path ─────────────────────────────────────────────────────────────

    #[test]
    fn test_event_bus_creation() {
        let bus = EventBus::new();
        assert_eq!(bus.subscription_count(), 0);
        assert!(!bus.has_subscribers("test"));
    }

    #[test]
    fn test_event_bus_default() {
        let bus = EventBus::default();
        assert_eq!(bus.subscription_count(), 0);
    }

    #[test]
    fn test_event_bus_id_allocation() {
        let bus = EventBus::new();
        assert_eq!(bus.next_subscriber_id(), 1);
        assert_eq!(bus.next_subscriber_id(), 2);
        assert_eq!(bus.next_subscriber_id(), 3);
    }

    #[test]
    fn test_event_bus_subscribe_increments_count() {
        let bus = EventBus::new();
        assert_eq!(bus.subscription_count(), 0);
        let _id = bus.subscribe("evt".to_string(), || {});
        assert_eq!(bus.subscription_count(), 1);
        let _id2 = bus.subscribe("evt".to_string(), || {});
        assert_eq!(bus.subscription_count(), 2);
    }

    #[test]
    fn test_event_bus_subscribe_and_publish() {
        let bus = EventBus::new();
        let counter = Arc::new(AtomicU64::new(0));
        let counter_clone = Arc::clone(&counter);

        let id = bus.subscribe("test_event".to_string(), move || {
            counter_clone.fetch_add(1, Ordering::Relaxed);
        });
        assert!(id > 0);

        bus.publish("test_event");
        assert_eq!(counter.load(Ordering::Relaxed), 1);

        bus.publish("test_event");
        assert_eq!(counter.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn test_event_bus_publish_unknown_event_noop() {
        let bus = EventBus::new();
        // Should not panic, no-op
        bus.publish("nonexistent_event");
    }

    #[test]
    fn test_event_bus_has_subscribers_after_subscribe() {
        let bus = EventBus::new();
        assert!(!bus.has_subscribers("my_event"));
        let _id = bus.subscribe("my_event".to_string(), || {});
        assert!(bus.has_subscribers("my_event"));
    }

    #[test]
    fn test_event_bus_multiple_events_independent() {
        let bus = EventBus::new();
        let c1 = Arc::new(AtomicU64::new(0));
        let c2 = Arc::new(AtomicU64::new(0));
        let c1c = Arc::clone(&c1);
        let c2c = Arc::clone(&c2);

        let _ = bus.subscribe("event_a".to_string(), move || {
            c1c.fetch_add(1, Ordering::Relaxed);
        });
        let _ = bus.subscribe("event_b".to_string(), move || {
            c2c.fetch_add(1, Ordering::Relaxed);
        });

        bus.publish("event_a");
        assert_eq!(c1.load(Ordering::Relaxed), 1);
        assert_eq!(c2.load(Ordering::Relaxed), 0);

        bus.publish("event_b");
        assert_eq!(c1.load(Ordering::Relaxed), 1);
        assert_eq!(c2.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn test_event_bus_multiple_subscribers_same_event() {
        let bus = EventBus::new();
        let total = Arc::new(AtomicU64::new(0));

        for _ in 0..5 {
            let t = Arc::clone(&total);
            let _ = bus.subscribe("multi".to_string(), move || {
                t.fetch_add(1, Ordering::Relaxed);
            });
        }
        assert_eq!(bus.subscription_count(), 5);
        bus.publish("multi");
        assert_eq!(total.load(Ordering::Relaxed), 5);
    }

    // ── Unsubscribe paths ───────────────────────────────────────────────────────

    #[test]
    fn test_event_bus_unsubscribe() {
        let bus = EventBus::new();
        let counter = Arc::new(AtomicU64::new(0));
        let cc = Arc::clone(&counter);

        let id = bus.subscribe("evt".to_string(), move || {
            cc.fetch_add(1, Ordering::Relaxed);
        });

        bus.publish("evt");
        assert_eq!(counter.load(Ordering::Relaxed), 1);

        assert!(bus.unsubscribe("evt", id));
        bus.publish("evt");
        assert_eq!(counter.load(Ordering::Relaxed), 1); // unchanged

        // Double-unsubscribe returns false
        assert!(!bus.unsubscribe("evt", id));
        // Unsubscribe wrong event
        assert!(!bus.unsubscribe("nonexistent", 1));
    }

    #[test]
    fn test_event_bus_unsubscribe_one_of_many() {
        let bus = EventBus::new();
        let total = Arc::new(AtomicU64::new(0));

        let id1 = bus.subscribe("multi".to_string(), {
            let t = Arc::clone(&total);
            move || {
                t.fetch_add(10, Ordering::Relaxed);
            }
        });
        let _id2 = bus.subscribe("multi".to_string(), {
            let t = Arc::clone(&total);
            move || {
                t.fetch_add(1, Ordering::Relaxed);
            }
        });

        bus.publish("multi");
        assert_eq!(total.load(Ordering::Relaxed), 11);

        let _ = bus.unsubscribe("multi", id1);
        bus.publish("multi");
        // Only _id2 fires: adds 1 more → total = 12
        assert_eq!(total.load(Ordering::Relaxed), 12);
    }

    #[test]
    fn test_event_bus_unsubscribe_reduces_count() {
        let bus = EventBus::new();
        let id = bus.subscribe("evt".to_string(), || {});
        assert_eq!(bus.subscription_count(), 1);
        let _ = bus.unsubscribe("evt", id);
        assert_eq!(bus.subscription_count(), 0);
        assert!(!bus.has_subscribers("evt"));
    }

    // ── Panic safety ────────────────────────────────────────────────────────────

    #[test]
    fn test_event_bus_publish_panic_safety() {
        let bus = EventBus::new();
        let counter = Arc::new(AtomicU64::new(0));

        // First subscriber panics
        let _ = bus.subscribe("evt".to_string(), || {
            panic!("boom");
        });

        // Second subscriber should still be called
        let cc = Arc::clone(&counter);
        let _ = bus.subscribe("evt".to_string(), move || {
            cc.fetch_add(1, Ordering::Relaxed);
        });

        bus.publish("evt");
        assert_eq!(
            counter.load(Ordering::Relaxed),
            1,
            "second subscriber must run despite first panicking"
        );
    }

    #[test]
    fn test_event_bus_publish_all_panic_does_not_abort() {
        let bus = EventBus::new();
        for _ in 0..3 {
            let _ = bus.subscribe("boom_event".to_string(), || {
                panic!("every subscriber panics")
            });
        }
        // Should not abort the test
        bus.publish("boom_event");
    }

    // ── Clone / shared state ────────────────────────────────────────────────────

    #[test]
    fn test_event_bus_clone_shares_state() {
        let bus1 = EventBus::new();
        let bus2 = bus1.clone();

        let counter = Arc::new(AtomicU64::new(0));
        let cc = Arc::clone(&counter);
        let _ = bus1.subscribe("shared".to_string(), move || {
            cc.fetch_add(1, Ordering::Relaxed);
        });

        // Publishing via bus2 (clone) should fire subscriber registered on bus1
        bus2.publish("shared");
        assert_eq!(counter.load(Ordering::Relaxed), 1);
    }

    // ── Debug repr ──────────────────────────────────────────────────────────────

    #[test]
    fn test_event_bus_debug_format() {
        let bus = EventBus::new();
        let _id = bus.subscribe("alpha".to_string(), || {});
        let debug = format!("{bus:?}");
        assert!(debug.contains("EventBus"));
        assert!(debug.contains("alpha"));
    }

    // ── Concurrency ─────────────────────────────────────────────────────────────

    #[test]
    fn test_event_bus_concurrent_subscribe_and_publish() {
        use std::thread;
        let bus = Arc::new(EventBus::new());
        let total = Arc::new(AtomicU64::new(0));

        // Spawn subscribers
        let mut handles = vec![];
        for _ in 0..8 {
            let b = Arc::clone(&bus);
            let t = Arc::clone(&total);
            handles.push(thread::spawn(move || {
                let _ = b.subscribe("concurrent".to_string(), move || {
                    t.fetch_add(1, Ordering::Relaxed);
                });
            }));
        }
        for h in handles {
            h.join().unwrap();
        }

        // All 8 subscribers registered
        assert_eq!(bus.subscription_count(), 8);

        // Publish once → each should fire
        bus.publish("concurrent");
        assert_eq!(total.load(Ordering::Relaxed), 8);
    }
}
