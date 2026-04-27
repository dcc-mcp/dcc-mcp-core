//! PyO3 bindings for `EventBus`.

use pyo3::prelude::*;
#[cfg(feature = "stub-gen")]
use pyo3_stub_gen_derive::gen_stub_pymethods;

use crate::events::{EventBus, SubscriberId};

// NOTE: subscribe() takes Py<PyAny>, publish() uses **kwargs — stubs may have imperfect types
#[cfg_attr(feature = "stub-gen", gen_stub_pymethods)]
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
