//! EventBus — thread-safe event publish/subscribe system.

#[cfg(feature = "python-bindings")]
use pyo3::prelude::*;

use dashmap::DashMap;
use parking_lot::RwLock;
use std::sync::Arc;

/// Event subscriber ID for unsubscription.
type SubscriberId = u64;

/// Thread-safe event bus.
#[cfg_attr(feature = "python-bindings", pyclass(name = "EventBus"))]
pub struct EventBus {
    next_id: Arc<RwLock<u64>>,
    #[cfg(feature = "python-bindings")]
    subscribers: Arc<DashMap<String, Vec<(SubscriberId, Py<PyAny>)>>>,
    #[cfg(not(feature = "python-bindings"))]
    subscribers: Arc<DashMap<String, Vec<(SubscriberId, Box<dyn Fn() + Send + Sync>)>>>,
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
        let mut id_guard = self.next_id.write();
        *id_guard += 1;
        let id = *id_guard;

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
    fn publish(&self, py: Python, event_name: &str, kwargs: Option<&Bound<'_, pyo3::types::PyDict>>) {
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
        let count: usize = self.subscribers.iter().map(|r| r.value().len()).sum();
        format!("EventBus(subscriptions={})", count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_bus_creation() {
        let bus = EventBus::new();
        // Just verify it doesn't panic
        drop(bus);
    }
}
