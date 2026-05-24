//! EventBus — thread-safe event publish/subscribe system.
//!
//! PyO3 bindings live in `crate::python::events`.

#[cfg(feature = "python-bindings")]
use pyo3::prelude::*;

#[cfg(feature = "stub-gen")]
use pyo3_stub_gen_derive::gen_stub_pyclass;

use dashmap::DashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// Current event envelope schema version.
pub const EVENT_SCHEMA_VERSION: u32 = 1;

/// Events that support `before` hooks and veto decisions.
pub const VETOABLE_EVENTS: &[&str] = &[
    "skill.loading",
    "tool.dispatched",
    "resource.subscribed",
    "client.initialize",
];

/// Event subscriber ID for unsubscription.
pub(crate) type SubscriberId = u64;

/// Callback type for non-Python mode (placeholder for future pure-Rust subscribe API).
///
/// Wrapped in `Arc` so callbacks can be cloned out of the DashMap before
/// invocation — this avoids holding a read-lock while executing user code.
#[cfg(not(feature = "python-bindings"))]
type EventCallback = Arc<dyn Fn(&EventEnvelope) + Send + Sync>;

/// Callback type for non-Python veto hooks.
#[cfg(not(feature = "python-bindings"))]
type BeforeCallback = Arc<dyn Fn(&EventEnvelope) -> Option<EventVeto> + Send + Sync>;

/// Type alias for the subscriber storage map.
#[cfg(feature = "python-bindings")]
pub(crate) type SubscriberMap = Arc<DashMap<String, Vec<(SubscriberId, Py<PyAny>)>>>;
#[cfg(not(feature = "python-bindings"))]
type SubscriberMap = Arc<DashMap<String, Vec<(SubscriberId, EventCallback)>>>;

/// Type alias for veto hook storage.
#[cfg(feature = "python-bindings")]
pub(crate) type BeforeSubscriberMap = Arc<DashMap<String, Vec<(SubscriberId, Py<PyAny>)>>>;
#[cfg(not(feature = "python-bindings"))]
type BeforeSubscriberMap = Arc<DashMap<String, Vec<(SubscriberId, BeforeCallback)>>>;

/// Thread-safe event bus.
#[cfg_attr(feature = "stub-gen", gen_stub_pyclass)]
#[cfg_attr(
    feature = "python-bindings",
    pyo3::pyclass(name = "EventBus", from_py_object)
)]
#[derive(Clone)]
pub struct EventBus {
    pub(crate) next_id: Arc<AtomicU64>,
    pub(crate) next_event_id: Arc<AtomicU64>,
    pub(crate) subscribers: SubscriberMap,
    pub(crate) before_subscribers: BeforeSubscriberMap,
}

/// Structured event envelope shared by in-process subscribers and future webhooks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventEnvelope {
    pub schema_version: u32,
    pub name: String,
    pub id: String,
    pub timestamp_ns: u64,
    pub source: Value,
    pub correlation: Value,
    pub attributes: Value,
}

impl EventEnvelope {
    #[must_use]
    pub fn new(
        name: impl Into<String>,
        id: impl Into<String>,
        source: Value,
        correlation: Value,
        attributes: Value,
    ) -> Self {
        Self {
            schema_version: EVENT_SCHEMA_VERSION,
            name: name.into(),
            id: id.into(),
            timestamp_ns: timestamp_ns(),
            source: object_or_empty(source),
            correlation: object_or_empty(correlation),
            attributes: object_or_empty(attributes),
        }
    }

    #[must_use]
    pub fn to_value(&self) -> Value {
        serde_json::to_value(self).unwrap_or_else(|_| Value::Object(Map::new()))
    }
}

/// A veto decision returned by an EventBus `before` hook.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventVeto {
    pub code: String,
    pub reason: String,
}

impl EventVeto {
    #[must_use]
    pub fn new(reason: impl Into<String>) -> Self {
        Self::with_code("vetoed", reason)
    }

    #[must_use]
    pub fn with_code(code: impl Into<String>, reason: impl Into<String>) -> Self {
        let code = code.into();
        let reason = reason.into();
        Self {
            code: if code.trim().is_empty() {
                "vetoed".to_string()
            } else {
                code
            },
            reason: if reason.trim().is_empty() {
                "event vetoed".to_string()
            } else {
                reason
            },
        }
    }
}

impl std::fmt::Debug for EventBus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EventBus")
            .field("next_id", &self.next_id.load(Ordering::Relaxed))
            .field("next_event_id", &self.next_event_id.load(Ordering::Relaxed))
            .field(
                "subscriber_events",
                &self
                    .subscribers
                    .iter()
                    .map(|r| r.key().clone())
                    .collect::<Vec<_>>(),
            )
            .field(
                "before_events",
                &self
                    .before_subscribers
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
            next_event_id: Arc::new(AtomicU64::new(0)),
            subscribers: Arc::new(DashMap::new()),
            before_subscribers: Arc::new(DashMap::new()),
        }
    }

    /// Get the count of all subscriptions.
    #[must_use]
    pub fn subscription_count(&self) -> usize {
        self.subscribers
            .iter()
            .map(|r| r.value().len())
            .sum::<usize>()
            + self
                .before_subscribers
                .iter()
                .map(|r| r.value().len())
                .sum::<usize>()
    }

    /// Check if any subscribers exist for the given event.
    #[must_use]
    pub fn has_subscribers(&self, event_name: &str) -> bool {
        self.subscribers.iter().any(|entry| {
            !entry.value().is_empty() && subscription_matches(entry.key().as_str(), event_name)
        })
    }

    /// Check if any veto hooks exist for the given event.
    #[must_use]
    pub fn has_before_subscribers(&self, event_name: &str) -> bool {
        self.before_subscribers
            .get(event_name)
            .is_some_and(|entry| !entry.value().is_empty())
    }

    /// Check whether an event supports before/veto hooks.
    #[must_use]
    pub fn is_vetoable_event(event_name: &str) -> bool {
        is_vetoable_event(event_name)
    }

    /// Allocate a new subscriber ID (starts at 1, monotonically increasing).
    pub(crate) fn next_subscriber_id(&self) -> SubscriberId {
        self.next_id.fetch_add(1, Ordering::Relaxed) + 1
    }

    /// Remove a subscriber by ID from a specific event (shared logic).
    pub(crate) fn remove_subscriber(&self, event_name: &str, subscriber_id: SubscriberId) -> bool {
        if let Some(mut subs) = self.subscribers.get_mut(event_name) {
            let before = subs.len();
            subs.retain(|(id, _)| *id != subscriber_id);
            return subs.len() < before;
        }
        false
    }

    /// Remove a before hook by ID from a specific event.
    pub(crate) fn remove_before_subscriber(
        &self,
        event_name: &str,
        subscriber_id: SubscriberId,
    ) -> bool {
        if let Some(mut subs) = self.before_subscribers.get_mut(event_name) {
            let before = subs.len();
            subs.retain(|(id, _)| *id != subscriber_id);
            return subs.len() < before;
        }
        false
    }

    #[must_use]
    pub(crate) fn make_event(
        &self,
        event_name: &str,
        source: Value,
        correlation: Value,
        attributes: Value,
    ) -> EventEnvelope {
        EventEnvelope::new(
            event_name,
            self.next_event_id(),
            source,
            correlation,
            attributes,
        )
    }

    /// Emit a structured event and return the envelope that was delivered.
    #[must_use]
    pub fn emit(
        &self,
        event_name: &str,
        source: Value,
        correlation: Value,
        attributes: Value,
    ) -> EventEnvelope {
        let event = self.make_event(event_name, source, correlation, attributes);
        self.publish_event(&event);
        event
    }

    pub(crate) fn make_vetoable_event(
        &self,
        event_name: &str,
        source: Value,
        correlation: Value,
        attributes: Value,
    ) -> Result<EventEnvelope, EventVeto> {
        ensure_vetoable_event(event_name)?;
        Ok(self.make_event(event_name, source, correlation, attributes))
    }

    fn next_event_id(&self) -> String {
        let seq = self.next_event_id.fetch_add(1, Ordering::Relaxed) + 1;
        format!("ev_{:x}_{:x}", timestamp_ns(), seq)
    }
}

#[must_use]
pub fn is_vetoable_event(event_name: &str) -> bool {
    VETOABLE_EVENTS.contains(&event_name)
}

fn ensure_vetoable_event(event_name: &str) -> Result<(), EventVeto> {
    if is_vetoable_event(event_name) {
        Ok(())
    } else {
        Err(EventVeto::with_code(
            "unsupported_veto_event",
            format!("event '{event_name}' does not support before hooks"),
        ))
    }
}

#[must_use]
pub(crate) fn subscription_matches(pattern: &str, event_name: &str) -> bool {
    if pattern == event_name || pattern == "*" {
        return true;
    }
    let Some(prefix) = pattern.strip_suffix(".*") else {
        return false;
    };
    if prefix.is_empty() {
        return true;
    }
    event_name
        .strip_prefix(prefix)
        .is_some_and(|suffix| suffix.starts_with('.'))
}

fn object_or_empty(value: Value) -> Value {
    if value.is_object() {
        value
    } else {
        Value::Object(Map::new())
    }
}

fn timestamp_ns() -> u64 {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    duration.as_secs().saturating_mul(1_000_000_000) + u64::from(duration.subsec_nanos())
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
        self.subscribe_event(event_name, move |_| callback())
    }

    /// Subscribe a Rust closure to structured event envelopes.
    #[must_use]
    pub fn subscribe_event<F>(&self, event_name: String, callback: F) -> SubscriberId
    where
        F: Fn(&EventEnvelope) + Send + Sync + 'static,
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

    /// Register a veto hook for one of the whitelisted lifecycle events.
    ///
    /// The callback runs before the matching event is published. Returning
    /// `Some(EventVeto)` rejects the operation; returning `None` allows it.
    pub fn before<F>(&self, event_name: String, callback: F) -> Result<SubscriberId, EventVeto>
    where
        F: Fn(&EventEnvelope) -> Option<EventVeto> + Send + Sync + 'static,
    {
        ensure_vetoable_event(&event_name)?;
        let id = self.next_subscriber_id();
        self.before_subscribers
            .entry(event_name)
            .or_default()
            .push((id, Arc::new(callback)));
        Ok(id)
    }

    /// Remove a veto hook by subscriber ID.
    #[must_use]
    pub fn unsubscribe_before(&self, event_name: &str, subscriber_id: SubscriberId) -> bool {
        self.remove_before_subscriber(event_name, subscriber_id)
    }

    /// Build a vetoable event, run matching before hooks, and return the event
    /// for later publication when all hooks allow it.
    pub fn before_event(
        &self,
        event_name: &str,
        source: Value,
        correlation: Value,
        attributes: Value,
    ) -> Result<EventEnvelope, EventVeto> {
        let event = self.make_vetoable_event(event_name, source, correlation, attributes)?;
        self.check_before_event(&event)?;
        Ok(event)
    }

    /// Run before hooks against an existing event envelope.
    pub fn check_before_event(&self, event: &EventEnvelope) -> Result<(), EventVeto> {
        ensure_vetoable_event(&event.name)?;
        let callbacks: Vec<_> = self
            .before_subscribers
            .get(&event.name)
            .map(|entry| {
                entry
                    .value()
                    .iter()
                    .map(|(_, cb)| Clone::clone(cb))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        for callback in &callbacks {
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| callback(event))) {
                Ok(Some(veto)) => return Err(veto),
                Ok(None) => {}
                Err(e) => {
                    let msg = e
                        .downcast_ref::<&str>()
                        .copied()
                        .or_else(|| e.downcast_ref::<String>().map(|s| s.as_str()))
                        .unwrap_or("unknown panic");
                    return Err(EventVeto::with_code(
                        "before_hook_panic",
                        format!("before hook for '{}' panicked: {msg}", event.name),
                    ));
                }
            }
        }
        Ok(())
    }

    /// Publish an empty structured event, calling all matching subscribers.
    ///
    /// Callbacks are collected before invocation to avoid holding a DashMap
    /// read-lock while executing user code (which could attempt to
    /// subscribe/unsubscribe, causing a deadlock on the same shard).
    ///
    /// Each callback is wrapped in [`std::panic::catch_unwind`] so that a
    /// panicking subscriber does not prevent subsequent subscribers from
    /// being called.
    pub fn publish(&self, event_name: &str) {
        let event = self.make_event(
            event_name,
            Value::Object(Map::new()),
            Value::Object(Map::new()),
            Value::Object(Map::new()),
        );
        self.publish_event(&event);
    }

    /// Publish an existing structured event envelope.
    pub fn publish_event(&self, event: &EventEnvelope) {
        let callbacks: Vec<_> = self
            .subscribers
            .iter()
            .filter(|entry| subscription_matches(entry.key().as_str(), &event.name))
            .flat_map(|entry| {
                entry
                    .value()
                    .iter()
                    .map(|(_, cb)| Clone::clone(cb))
                    .collect::<Vec<_>>()
            })
            .collect();
        for callback in &callbacks {
            if let Err(e) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                callback(event);
            })) {
                let msg = e
                    .downcast_ref::<&str>()
                    .copied()
                    .or_else(|| e.downcast_ref::<String>().map(|s| s.as_str()))
                    .unwrap_or("unknown panic");
                tracing::error!("Panic in event subscriber for {}: {msg}", event.name);
            }
        }
    }
}

// PyO3 bindings live in `crate::python::events`.

#[cfg(all(test, not(feature = "python-bindings")))]
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
    fn test_event_bus_wildcard_subscribers_receive_matching_events() {
        let bus = EventBus::new();
        let names = Arc::new(std::sync::Mutex::new(Vec::new()));
        let seen = Arc::clone(&names);
        let _id = bus.subscribe_event("tool.*".to_string(), move |event| {
            seen.lock().unwrap().push(event.name.clone());
        });

        assert!(bus.has_subscribers("tool.completed"));
        assert!(!bus.has_subscribers("skill.loaded"));

        let _ = bus.emit(
            "tool.completed",
            serde_json::json!({"dcc_type": "maya"}),
            serde_json::json!({}),
            serde_json::json!({"tool_slug": "maya.scene__open"}),
        );
        bus.publish("skill.loaded");

        assert_eq!(
            names.lock().unwrap().as_slice(),
            &["tool.completed".to_string()]
        );
    }

    #[test]
    fn test_before_hook_rejects_non_vetoable_events() {
        let bus = EventBus::new();
        let err = bus
            .before("tool.completed".to_string(), |_| None)
            .expect_err("tool.completed is not vetoable");

        assert_eq!(err.code, "unsupported_veto_event");
        assert!(!bus.has_before_subscribers("tool.completed"));
    }

    #[test]
    fn test_before_hook_veto_stops_event_before_publish() {
        let bus = EventBus::new();
        let called = Arc::new(AtomicU64::new(0));
        let called_clone = Arc::clone(&called);
        let _before = bus
            .before("tool.dispatched".to_string(), move |event| {
                called_clone.fetch_add(1, Ordering::Relaxed);
                assert_eq!(event.attributes["tool_slug"], "maya_scene__open");
                Some(EventVeto::with_code(
                    "policy_denied",
                    "scene-open tools are blocked in review mode",
                ))
            })
            .unwrap();

        let event = bus.before_event(
            "tool.dispatched",
            serde_json::json!({"dcc_type": "maya"}),
            serde_json::json!({}),
            serde_json::json!({"tool_slug": "maya_scene__open"}),
        );

        let veto = event.expect_err("before hook should veto");
        assert_eq!(veto.code, "policy_denied");
        assert_eq!(called.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn test_event_bus_structured_envelope_contains_schema_and_payload() {
        let bus = EventBus::new();
        let captured = Arc::new(std::sync::Mutex::new(None));
        let captured_clone = Arc::clone(&captured);
        let _id = bus.subscribe_event("gateway.started".to_string(), move |event| {
            *captured_clone.lock().unwrap() = Some(event.clone());
        });

        let event = bus.emit(
            "gateway.started",
            serde_json::json!({"dcc_type": "custom"}),
            serde_json::json!({"request_id": "req-1"}),
            serde_json::json!({"port": 9765}),
        );

        let observed = captured.lock().unwrap().clone().unwrap();
        assert_eq!(event.schema_version, EVENT_SCHEMA_VERSION);
        assert_eq!(observed.name, "gateway.started");
        assert_eq!(observed.source["dcc_type"], "custom");
        assert_eq!(observed.correlation["request_id"], "req-1");
        assert_eq!(observed.attributes["port"], 9765);
        assert!(observed.id.starts_with("ev_"));
        assert!(observed.timestamp_ns > 0);
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
