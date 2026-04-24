//! MCP Resources primitive (issue #350).
//!
//! Exposes live DCC state to MCP clients as readable, optionally
//! subscribable resources. The MCP 2025-03-26 `Resources` capability is
//! advertised when [`crate::McpHttpConfig::enable_resources`] is `true`.
//!
//! # URI schemes
//!
//! | Scheme                    | MIME type          | Notes |
//! |---------------------------|--------------------|-------|
//! | `scene://current`         | `application/json` | JSON summary of the current DCC scene, fed by the embedding adapter via [`ResourceRegistry::set_scene`]. |
//! | `capture://current_window`| `image/png`        | PNG snapshot of the DCC window (real backend on Windows; Mock elsewhere). |
//! | `audit://recent?limit=N`  | `application/json` | Last `N` entries from the `AuditLog`. `notifications/resources/updated` fires on append. |
//! | `artefact://…`            | varies             | Issue #349 — toggle via `McpHttpConfig::enable_artefact_resources`. |
//!
//! # Adding a custom producer
//!
//! Implement [`ResourceProducer`] and register via
//! [`ResourceRegistry::add_producer`] **before** `McpHttpServer::start()`.
//! External producers can call [`ResourceRegistry::notify_updated`] to
//! emit `notifications/resources/updated` for a URI they own.
//!
//! ## Maintainer layout
//!
//! This module is a **thin facade** that keeps `ResourceRegistry` and
//! re-exports the public surface. Implementation is split across
//! sibling files:
//!
//! | File | Responsibility |
//! |------|----------------|
//! | `resources_types.rs`     | `ProducerContent`, `ResourceError`, `ResourceResult`, `ResourceProducer` trait, `uri_scheme` helper |
//! | `resources_producers.rs` | Built-in producers (`SceneProducer`, `CaptureProducer`, `AuditProducer`, `ArtefactProducer`) + `parse_audit_limit` |
//! | `resources_tests.rs`     | Unit + `tokio::test` suite |

#[path = "resources_types.rs"]
mod types;

#[path = "resources_producers.rs"]
mod producers;

#[cfg(test)]
#[path = "resources_tests.rs"]
mod tests;

pub use types::{ProducerContent, ResourceError, ResourceProducer, ResourceResult};

use std::sync::Arc;

use dcc_mcp_artefact::{InMemoryArtefactStore, SharedArtefactStore};
use parking_lot::RwLock;
use serde_json::Value;
use tokio::sync::broadcast;

use self::producers::{ArtefactProducer, AuditProducer, CaptureProducer, SceneProducer};
use self::types::uri_scheme;
use crate::protocol::{McpResource, ReadResourceResult};

/// Thread-safe registry of resource producers and subscription state.
///
/// Owned by [`crate::handler::AppState`] via `Arc`.
#[derive(Clone)]
pub struct ResourceRegistry {
    inner: Arc<ResourceRegistryInner>,
}

struct ResourceRegistryInner {
    producers: RwLock<Vec<Arc<dyn ResourceProducer>>>,
    /// Per-session subscription set: `session_id -> Set<uri>`.
    subscriptions: RwLock<std::collections::HashMap<String, std::collections::HashSet<String>>>,
    /// Fan-out channel for `notifications/resources/updated`. Consumers
    /// are SSE sessions inside [`crate::handler`] that push the event out
    /// to the subscribed client.
    updated_tx: broadcast::Sender<String>,
    /// Scene snapshot injected by the embedding adapter. `None` means no
    /// scene has been published yet (producer returns an empty object).
    scene_snapshot: Arc<RwLock<Option<Value>>>,
    /// Enables `scene://` and `audit://` producers. Mirrored from
    /// [`crate::McpHttpConfig::enable_resources`].
    enabled: bool,
    /// Enables `artefact://` entries in `resources/list`. Mirrored from
    /// [`crate::McpHttpConfig::enable_artefact_resources`].
    artefact_enabled: bool,
    /// Backing store for `artefact://` resources (issue #349). Populated
    /// when `artefact_enabled` is `true`, wired into the producer at
    /// registration time and kept here so callers can hand it back to
    /// tools/workflow steps via [`ResourceRegistry::artefact_store`].
    artefact_store: Option<SharedArtefactStore>,
}

impl ResourceRegistry {
    /// Construct a registry with the default built-in producers.
    ///
    /// When `artefact_enabled` is `true`, an in-memory
    /// [`dcc_mcp_artefact::InMemoryArtefactStore`] is wired up by default.
    /// Callers that need persistence (the default for real servers) should
    /// use [`Self::new_with_artefact_store`].
    pub fn new(enabled: bool, artefact_enabled: bool) -> Self {
        let store: Option<SharedArtefactStore> = if artefact_enabled {
            Some(Arc::new(InMemoryArtefactStore::new()) as SharedArtefactStore)
        } else {
            None
        };
        Self::new_inner(enabled, artefact_enabled, store)
    }

    /// Construct a registry with a caller-supplied artefact store. Use
    /// this to plug in a [`dcc_mcp_artefact::FilesystemArtefactStore`]
    /// under the workspace's `.dcc-mcp/artefacts` directory.
    ///
    /// `artefact_enabled` must be `true` for the store to be surfaced in
    /// `resources/list` — pass `false` to retain the producer while
    /// hiding entries from agents.
    pub fn new_with_artefact_store(
        enabled: bool,
        artefact_enabled: bool,
        store: SharedArtefactStore,
    ) -> Self {
        Self::new_inner(enabled, artefact_enabled, Some(store))
    }

    fn new_inner(
        enabled: bool,
        artefact_enabled: bool,
        store: Option<SharedArtefactStore>,
    ) -> Self {
        let (updated_tx, _) = broadcast::channel(64);
        let scene_snapshot = Arc::new(RwLock::new(None));
        let inner = Arc::new(ResourceRegistryInner {
            producers: RwLock::new(Vec::new()),
            subscriptions: RwLock::new(std::collections::HashMap::new()),
            updated_tx,
            scene_snapshot: scene_snapshot.clone(),
            enabled,
            artefact_enabled,
            artefact_store: store.clone(),
        });
        let registry = Self { inner };
        if enabled {
            registry.add_producer(Arc::new(SceneProducer {
                snapshot: scene_snapshot,
            }));
            registry.add_producer(Arc::new(CaptureProducer));
            registry.add_producer(Arc::new(AuditProducer::disabled()));
            registry.add_producer(Arc::new(ArtefactProducer {
                enabled: artefact_enabled,
                store,
            }));
        }
        registry
    }

    /// The artefact store backing `artefact://` resources, if any.
    ///
    /// `None` when `enable_artefact_resources = false`. Useful for tools
    /// and workflow-step runners that need to hand back a
    /// [`dcc_mcp_artefact::FileRef`] inside a
    /// [`dcc_mcp_models::ToolResult`]'s `context`.
    pub fn artefact_store(&self) -> Option<SharedArtefactStore> {
        self.inner.artefact_store.clone()
    }

    /// Returns `true` when the Resources primitive is advertised in
    /// `initialize` (mirrors `McpHttpConfig::enable_resources`).
    pub fn is_enabled(&self) -> bool {
        self.inner.enabled
    }

    /// Register an additional producer.
    ///
    /// Must be called **before** `McpHttpServer::start()`. Panics never —
    /// duplicate schemes are allowed but `resources/read` dispatches to
    /// the first producer that matches.
    pub fn add_producer(&self, producer: Arc<dyn ResourceProducer>) {
        self.inner.producers.write().push(producer);
    }

    /// Publish a new scene snapshot for `scene://current`.
    ///
    /// Fires `notifications/resources/updated` for subscribed clients.
    pub fn set_scene(&self, snapshot: Value) {
        *self.inner.scene_snapshot.write() = Some(snapshot);
        self.notify_updated("scene://current");
    }

    /// Wire an [`dcc_mcp_sandbox::AuditLog`] so that `audit://recent`
    /// reflects its contents and fires `notifications/resources/updated`
    /// on append.
    pub fn wire_audit_log(&self, log: Arc<dcc_mcp_sandbox::AuditLog>) {
        // Replace the stub audit producer with a live one bound to this
        // AuditLog. Also spawn a background task that forwards append
        // notifications to subscribed sessions.
        {
            let mut producers = self.inner.producers.write();
            if let Some(slot) = producers.iter_mut().find(|p| p.scheme() == "audit") {
                *slot = Arc::new(AuditProducer::new(log.clone()));
            } else {
                producers.push(Arc::new(AuditProducer::new(log.clone())));
            }
        }
        // Subscribe synchronously so the receiver is installed before
        // `wire_audit_log` returns — otherwise a `record()` call that
        // fires immediately after wiring can race the spawned task and
        // be dropped on the floor.
        let mut rx = log.watch();
        let notifier = self.clone();
        tokio::spawn(async move {
            while rx.recv().await.is_ok() {
                notifier.notify_updated("audit://recent");
            }
        });
    }

    /// Enumerate all resources advertised in `resources/list`.
    pub fn list(&self) -> Vec<McpResource> {
        let mut out = Vec::new();
        for p in self.inner.producers.read().iter() {
            // Artefact producer hides entries when disabled.
            if p.scheme() == "artefact" && !self.inner.artefact_enabled {
                continue;
            }
            out.extend(p.list());
        }
        out
    }

    /// Read a resource by URI.
    pub fn read(&self, uri: &str) -> ResourceResult<ReadResourceResult> {
        let scheme = uri_scheme(uri)
            .ok_or_else(|| ResourceError::NotFound(format!("invalid URI (no scheme): {uri}")))?;
        let producer = {
            let producers = self.inner.producers.read();
            producers.iter().find(|p| p.scheme() == scheme).cloned()
        };
        let Some(producer) = producer else {
            return Err(ResourceError::NotFound(uri.to_string()));
        };
        let content = producer.read(uri)?;
        Ok(ReadResourceResult {
            contents: vec![content.into_contents()],
        })
    }

    /// Record a subscription for `session_id -> uri`.
    ///
    /// Returns `true` if the subscription was newly inserted.
    pub fn subscribe(&self, session_id: &str, uri: &str) -> bool {
        self.inner
            .subscriptions
            .write()
            .entry(session_id.to_string())
            .or_default()
            .insert(uri.to_string())
    }

    /// Remove a subscription. Returns `true` if a subscription was removed.
    pub fn unsubscribe(&self, session_id: &str, uri: &str) -> bool {
        let mut subs = self.inner.subscriptions.write();
        let Some(set) = subs.get_mut(session_id) else {
            return false;
        };
        let removed = set.remove(uri);
        if set.is_empty() {
            subs.remove(session_id);
        }
        removed
    }

    /// Sessions currently subscribed to `uri`.
    pub fn sessions_subscribed_to(&self, uri: &str) -> Vec<String> {
        self.inner
            .subscriptions
            .read()
            .iter()
            .filter(|(_, uris)| uris.contains(uri))
            .map(|(sid, _)| sid.clone())
            .collect()
    }

    /// Clear all subscriptions for a session (e.g. on `DELETE /mcp`).
    pub fn drop_session(&self, session_id: &str) {
        self.inner.subscriptions.write().remove(session_id);
    }

    /// Broadcast-subscribe to URI-update events. The item is the URI
    /// string. Consumed by the session broadcaster in `handler.rs`.
    pub fn watch_updates(&self) -> broadcast::Receiver<String> {
        self.inner.updated_tx.subscribe()
    }

    /// Emit `notifications/resources/updated` for `uri` on the internal
    /// broadcast channel. The HTTP layer (see `handler.rs`) forwards
    /// this to each subscribed session's SSE stream.
    pub fn notify_updated(&self, uri: &str) {
        let _ = self.inner.updated_tx.send(uri.to_string());
    }
}
