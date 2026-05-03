//! Resource-subscription routing (#732).
//!
//! Layered on top of the existing [`SubscriberManager`]: the per-backend
//! SSE subscriber already receives every notification a backend emits, so
//! no new network plumbing is required. This module adds a routing table
//! keyed by `(backend_url, backend_uri)` → set of subscribing client
//! sessions + the prefixed URI each client originally subscribed with,
//! and a `dispatch_resource_updated` helper the delivery path calls when
//! a `notifications/resources/updated` frame arrives.

use super::types::ResourceSubscriberRoute;
use super::*;
use dcc_mcp_jsonrpc::format_sse_event;

impl SubscriberManager {
    /// Register a `(client_session, client_uri)` subscription for the
    /// `(backend_url, backend_uri)` resource. Idempotent.
    ///
    /// Returns `true` when a new route was inserted, `false` when the
    /// same subscription already existed.
    pub fn bind_resource_subscription(
        &self,
        backend_url: &str,
        backend_uri: &str,
        client_session_id: &str,
        client_uri: &str,
    ) -> bool {
        let key = (backend_url.to_string(), backend_uri.to_string());
        let route = ResourceSubscriberRoute {
            client_session_id: client_session_id.to_string(),
            client_uri: client_uri.to_string(),
        };
        let set = self.inner.resource_subscriptions.entry(key).or_default();
        set.insert(route)
    }

    /// Remove a specific `(client_session, client_uri)` subscription for
    /// the `(backend_url, backend_uri)` resource. Returns `true` when
    /// the set is now empty so the caller can decide whether to forward
    /// a `resources/unsubscribe` to the backend.
    pub fn unbind_resource_subscription(
        &self,
        backend_url: &str,
        backend_uri: &str,
        client_session_id: &str,
        client_uri: &str,
    ) -> bool {
        let key = (backend_url.to_string(), backend_uri.to_string());
        let Some(set_ref) = self.inner.resource_subscriptions.get(&key) else {
            return false;
        };
        let route = ResourceSubscriberRoute {
            client_session_id: client_session_id.to_string(),
            client_uri: client_uri.to_string(),
        };
        set_ref.remove(&route);
        let empty = set_ref.is_empty();
        drop(set_ref);
        if empty {
            self.inner.resource_subscriptions.remove(&key);
        }
        empty
    }

    /// Forward a backend `notifications/resources/updated` to every
    /// subscribing client session, with the `params.uri` rewritten back
    /// to the gateway-prefixed form each client originally subscribed
    /// with.
    ///
    /// Returns `true` when at least one subscriber was matched (used by
    /// the delivery path to decide whether the event has already been
    /// consumed and should not be buffered).
    pub(super) fn dispatch_resource_updated(&self, value: &Value, backend_url: &str) -> bool {
        let Some(backend_uri) = value
            .get("params")
            .and_then(|p| p.get("uri"))
            .and_then(Value::as_str)
        else {
            return false;
        };
        let key = (backend_url.to_string(), backend_uri.to_string());
        let Some(set_ref) = self.inner.resource_subscriptions.get(&key) else {
            return false;
        };
        let routes: Vec<ResourceSubscriberRoute> = set_ref.iter().map(|e| e.clone()).collect();
        drop(set_ref);

        let mut dispatched_any = false;
        for route in routes {
            // Rewrite the URI so each subscriber sees the prefixed form
            // it originally requested — two clients may have subscribed
            // to the same backend resource through different encodings
            // and we must honour each one's wire contract.
            let mut outbound = value.clone();
            if let Some(params) = outbound.get_mut("params").and_then(Value::as_object_mut) {
                params.insert("uri".to_string(), Value::String(route.client_uri.clone()));
            }
            if let Some(sender) = self.inner.client_sinks.get(&route.client_session_id) {
                let event = format_sse_event(&outbound, None);
                let _ = sender.send(event);
                dispatched_any = true;
            } else {
                tracing::debug!(
                    session = %route.client_session_id,
                    backend = %backend_url,
                    uri = %backend_uri,
                    "gateway SSE: resources/updated subscriber has no live sink — dropping"
                );
            }
        }
        dispatched_any
    }

    /// Drop every subscription owned by `session_id`. Returns the set
    /// of `(backend_url, backend_uri)` pairs whose subscriber list
    /// became empty — callers use this to fire trailing
    /// `resources/unsubscribe` calls at the owning backends.
    pub fn forget_client_resource_subs(&self, session_id: &str) -> Vec<(String, String)> {
        let mut emptied: Vec<(String, String)> = Vec::new();
        let mut drop_keys: Vec<(String, String)> = Vec::new();
        for entry in self.inner.resource_subscriptions.iter() {
            let key = entry.key().clone();
            let set = entry.value();
            // DashSet::retain is not available; collect removals then apply.
            let to_remove: Vec<ResourceSubscriberRoute> = set
                .iter()
                .filter(|r| r.client_session_id == session_id)
                .map(|r| r.clone())
                .collect();
            for route in &to_remove {
                set.remove(route);
            }
            if set.is_empty() && !to_remove.is_empty() {
                emptied.push(key.clone());
                drop_keys.push(key);
            }
        }
        for key in drop_keys {
            self.inner.resource_subscriptions.remove(&key);
        }
        emptied
    }
}
