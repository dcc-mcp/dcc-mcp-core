//! PyO3 bindings for `EventBus`.

use pyo3::prelude::*;
use pyo3::types::PyDict;
#[cfg(feature = "stub-gen")]
use pyo3_stub_gen_derive::gen_stub_pymethods;
use serde_json::{Map, Value};

use crate::events::{
    EventBus, EventEnvelope, EventVeto, SubscriberId, VETOABLE_EVENTS, is_vetoable_event,
    subscription_matches,
};

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

    fn matching_before_callbacks(&self, py: Python<'_>, event_name: &str) -> Vec<Py<PyAny>> {
        self.before_subscribers
            .get(event_name)
            .map(|entry| {
                entry
                    .value()
                    .iter()
                    .map(|(_, cb)| cb.clone_ref(py))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    }

    /// Build a vetoable event, run Python before hooks, and return the event
    /// for later publication when all hooks allow it.
    pub fn before_event(
        &self,
        event_name: &str,
        source: Value,
        correlation: Value,
        attributes: Value,
    ) -> Result<EventEnvelope, EventVeto> {
        let event = self.make_vetoable_event(event_name, source, correlation, attributes)?;
        if !self.has_before_subscribers(event_name) {
            return Ok(event);
        }

        let Some(result) = Python::try_attach(|py| {
            let callbacks = self.matching_before_callbacks(py, &event.name);
            if callbacks.is_empty() {
                return Ok(());
            }
            let event_value = event.to_value();
            let py_event = dcc_mcp_pybridge::py_json::json_value_to_pyobject(py, &event_value)
                .map_err(|err| {
                    EventVeto::with_code(
                        "before_hook_error",
                        format!(
                            "failed to convert event '{}' for before hooks: {err}",
                            event.name
                        ),
                    )
                })?;

            for callback in &callbacks {
                let result = callback
                    .call1(py, (py_event.clone_ref(py),))
                    .map_err(|err| {
                        EventVeto::with_code(
                            "before_hook_error",
                            format!("before hook for '{}' raised: {err}", event.name),
                        )
                    })?;
                if let Some(veto) = veto_from_py_result(py, result)? {
                    return Err(veto);
                }
            }
            Ok(())
        }) else {
            return Err(EventVeto::with_code(
                "before_hook_unavailable",
                format!("Python is not attached; cannot evaluate before hooks for '{event_name}'"),
            ));
        };

        result?;
        Ok(event)
    }

    /// Publish a structured event envelope to Python and Rust-native subscribers.
    pub fn publish_event(&self, event: &EventEnvelope) {
        // Deliver to Python subscribers.
        match Python::try_attach(|py| {
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
        }) {
            Some(()) => {}
            None => {
                tracing::warn!(
                    "EventBus could not deliver '{}' because Python is not attached",
                    event.name
                );
                // Still deliver to native subscribers even if Python is absent.
            }
        }

        // Deliver to Rust-native subscribers.
        self.publish_to_native_subscribers(event);
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

    /// Register a before hook for a vetoable event.
    ///
    /// The callback receives the structured event envelope. Return ``None`` or
    /// ``False`` to allow the operation, or return ``EventBus.veto(...)``, a
    /// ``{"reason": "...", "code": "..."}`` dict, or a string reason to veto.
    fn before(&self, event_name: String, callback: Py<PyAny>) -> PyResult<SubscriberId> {
        if !is_vetoable_event(&event_name) {
            return Err(pyo3::exceptions::PyValueError::new_err(format!(
                "event '{event_name}' does not support before hooks"
            )));
        }
        let id = self.next_subscriber_id();
        self.before_subscribers
            .entry(event_name)
            .or_default()
            .push((id, callback));
        Ok(id)
    }

    /// Unsubscribe from an event by subscriber ID.
    #[pyo3(signature = (event_name, subscriber_id))]
    fn unsubscribe(&self, event_name: &str, subscriber_id: SubscriberId) -> bool {
        self.remove_subscriber(event_name, subscriber_id)
    }

    /// Unsubscribe a before hook by subscriber ID.
    #[pyo3(signature = (event_name, subscriber_id))]
    fn unsubscribe_before(&self, event_name: &str, subscriber_id: SubscriberId) -> bool {
        self.remove_before_subscriber(event_name, subscriber_id)
    }

    /// Return a veto payload for before-hook callbacks.
    #[staticmethod]
    #[pyo3(signature = (reason, code = "vetoed"))]
    fn veto(py: Python<'_>, reason: &str, code: &str) -> PyResult<Py<PyAny>> {
        let dict = PyDict::new(py);
        dict.set_item("reason", reason)?;
        dict.set_item("code", code)?;
        Ok(dict.into())
    }

    /// Return the event names that accept before hooks.
    #[staticmethod]
    fn vetoable_events() -> Vec<&'static str> {
        VETOABLE_EVENTS.to_vec()
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

fn veto_from_py_result(py: Python<'_>, value: Py<PyAny>) -> Result<Option<EventVeto>, EventVeto> {
    let bound = value.bind(py);
    if bound.is_none() {
        return Ok(None);
    }
    if let Ok(flag) = bound.extract::<bool>()
        && !flag
    {
        return Ok(None);
    }
    if let Ok(reason) = bound.extract::<String>() {
        return Ok(Some(EventVeto::new(reason)));
    }
    if let Ok(dict) = bound.cast::<PyDict>() {
        let reason = py_dict_string(dict, "reason")?.unwrap_or_else(|| "event vetoed".to_string());
        let code = py_dict_string(dict, "code")?.unwrap_or_else(|| "vetoed".to_string());
        return Ok(Some(EventVeto::with_code(code, reason)));
    }
    match bound.is_truthy() {
        Ok(false) => Ok(None),
        Ok(true) => Ok(Some(EventVeto::with_code(
            "invalid_before_hook_result",
            "before hook returned an unsupported truthy value; return None/False to allow or a string/dict to veto",
        ))),
        Err(err) => Err(EventVeto::with_code(
            "before_hook_error",
            format!("failed to interpret before hook result: {err}"),
        )),
    }
}

fn py_dict_string(dict: &Bound<'_, PyDict>, key: &str) -> Result<Option<String>, EventVeto> {
    dict.get_item(key)
        .map_err(|err| {
            EventVeto::with_code(
                "before_hook_error",
                format!("invalid veto payload field '{key}': {err}"),
            )
        })?
        .map(|value| {
            value.extract::<String>().map_err(|err| {
                EventVeto::with_code(
                    "before_hook_error",
                    format!("veto payload field '{key}' must be a string: {err}"),
                )
            })
        })
        .transpose()
}
