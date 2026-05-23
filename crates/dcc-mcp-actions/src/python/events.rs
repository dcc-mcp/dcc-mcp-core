//! PyO3 bindings for `EventBus`.

use pyo3::prelude::*;
#[cfg(feature = "stub-gen")]
use pyo3_stub_gen_derive::gen_stub_pymethods;
use serde_json::{Map, Value};

use crate::events::{EventBus, EventEnvelope, SubscriberId, subscription_matches};

impl EventBus {
    fn matching_callbacks(&self, py: Python<'_>, event_name: &str) -> Vec<Py<PyAny>> {
        self.subscribers
            .iter()
            .filter(|entry| subscription_matches(entry.key().as_str(), event_name))
            .flat_map(|entry| {
                entry
                    .value()
                    .iter()
                    .map(|(_, cb)| cb.clone_ref(py))
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    /// Publish a structured event envelope to Python subscribers.
    pub fn publish_event(&self, event: &EventEnvelope) {
        let Some(()) = Python::try_attach(|py| {
            let callbacks = self.matching_callbacks(py, &event.name);
            if callbacks.is_empty() {
                return;
            }
            let event_value = event.to_value();
            let py_event = match dcc_mcp_pybridge::py_json::json_value_to_pyobject(py, &event_value)
            {
                Ok(value) => value,
                Err(err) => {
                    tracing::error!(
                        "Error converting event {} to Python object: {}",
                        event.name,
                        err
                    );
                    return;
                }
            };
            for callback in &callbacks {
                if let Err(err) = callback.call1(py, (py_event.clone_ref(py),)) {
                    tracing::error!("Error in event subscriber for {}: {}", event.name, err);
                }
            }
        }) else {
            tracing::warn!(
                "EventBus could not deliver '{}' because Python is not attached",
                event.name
            );
            return;
        };
    }
}

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

    /// Publish an event with legacy keyword payloads, calling all subscribers.
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
        let callbacks: Vec<Py<PyAny>> = self.matching_callbacks(py, event_name);
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

    /// Emit a structured RFC-0002 event envelope.
    #[pyo3(name = "emit")]
    #[pyo3(signature = (event_name, source=None, correlation=None, attributes=None))]
    fn py_emit(
        &self,
        py: Python<'_>,
        event_name: &str,
        source: Option<&Bound<'_, PyAny>>,
        correlation: Option<&Bound<'_, PyAny>>,
        attributes: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Py<PyAny>> {
        let source = object_arg(source)?;
        let correlation = object_arg(correlation)?;
        let attributes = object_arg(attributes)?;
        let event = self.emit(event_name, source, correlation, attributes);
        dcc_mcp_pybridge::py_json::json_value_to_pyobject(py, &event.to_value())
    }

    fn __repr__(&self) -> String {
        format!("EventBus(subscriptions={})", self.subscription_count())
    }
}

fn object_arg(value: Option<&Bound<'_, PyAny>>) -> PyResult<Value> {
    let Some(value) = value else {
        return Ok(Value::Object(Map::new()));
    };
    if value.is_none() {
        return Ok(Value::Object(Map::new()));
    }
    let value = dcc_mcp_pybridge::py_json::py_any_to_json_value(value)?;
    if value.is_object() {
        Ok(value)
    } else {
        Err(pyo3::exceptions::PyValueError::new_err(
            "event source, correlation, and attributes must be dict-like objects",
        ))
    }
}
