//! EventBus — thread-safe event publish/subscribe system.

#[cfg(feature = "python-bindings")]
use pyo3::prelude::*;

use dashmap::DashMap;
use parking_lot::RwLock;
use std::sync::Arc;

/// Event subscriber ID for unsubscription.
type SubscriberId = u64;

/// Callback type alias for non-Python mode.
#[cfg(not(feature = "python-bindings"))]
type EventCallback = Box<dyn Fn() + Send + Sync>;

/// Thread-safe event bus.
#[cfg_attr(feature = "python-bindings", pyclass(name = "EventBus"))]
pub struct EventBus {
    next_id: Arc<RwLock<u64>>,
    #[cfg(feature = "python-bindings")]
    subscribers: Arc<DashMap<String, Vec<(SubscriberId, Py<PyAny>)>>>,
    #[cfg(not(feature = "python-bindings"))]
    subscribers: Arc<DashMap<String, Vec<(SubscriberId, EventCallback)>>>,
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

impl EventBus {
    pub fn new() -> Self {
        Self {
            next_id: Arc::new(RwLock::new(0)),
            subscribers: Arc::new(DashMap::new()),
        }
    }

    /// Get the count of all subscriptions.
    pub fn subscription_count(&self) -> usize {
        self.subscribers.iter().map(|r| r.value().len()).sum()
    }

    /// Check if any subscribers exist for the given event.
    pub fn has_subscribers(&self, event_name: &str) -> bool {
        self.subscribers
            .get(event_name)
            .map(|s| !s.is_empty())
            .unwrap_or(false)
    }

    /// Allocate a new subscriber ID.
    pub fn next_subscriber_id(&self) -> u64 {
        let mut id_guard = self.next_id.write();
        *id_guard += 1;
        *id_guard
    }
}

#[cfg(feature = "python-bindings")]
#[pymethods]
impl EventBus {
    #[new]
    fn py_new() -> Self {
        Self::new()
    }

    /// Subscribe to an event. Returns subscriber ID for unsubscription.
    fn subscribe(&self, event_name: String, callback: Py<PyAny>) -> u64 {
        let id = self.next_subscriber_id();

        self.subscribers
            .entry(event_name)
            .or_default()
            .push((id, callback));

        id
    }

    /// Unsubscribe from an event by subscriber ID.
    fn unsubscribe(&self, event_name: &str, subscriber_id: u64) -> bool {
        if let Some(mut subs) = self.subscribers.get_mut(event_name) {
            let before = subs.len();
            subs.retain(|(id, _)| *id != subscriber_id);
            return subs.len() < before;
        }
        false
    }

    /// Publish an event, calling all subscribers.
    #[pyo3(signature = (event_name, **kwargs))]
    fn publish(
        &self,
        py: Python,
        event_name: &str,
        kwargs: Option<&Bound<'_, pyo3::types::PyDict>>,
    ) {
        if let Some(subs) = self.subscribers.get(event_name) {
            for (_, callback) in subs.iter() {
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
    }

    fn __repr__(&self) -> String {
        format!("EventBus(subscriptions={})", self.subscription_count())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_bus_creation() {
        let bus = EventBus::new();
        assert_eq!(bus.subscription_count(), 0);
        assert!(!bus.has_subscribers("test"));
    }

    #[test]
    fn test_event_bus_id_allocation() {
        let bus = EventBus::new();
        assert_eq!(bus.next_subscriber_id(), 1);
        assert_eq!(bus.next_subscriber_id(), 2);
    }
}
