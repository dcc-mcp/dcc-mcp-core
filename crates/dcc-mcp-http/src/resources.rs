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
//! | `artefact://…`            | varies             | Reserved for issue #349 — recognized but disabled by default. |
//!
//! # Adding a custom producer
//!
//! Implement [`ResourceProducer`] and register via
//! [`ResourceRegistry::add_producer`] **before** `McpHttpServer::start()`.
//! External producers can call [`ResourceRegistry::notify_updated`] to
//! emit `notifications/resources/updated` for a URI they own.

use std::sync::Arc;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use parking_lot::RwLock;
use serde_json::{Value, json};
use tokio::sync::broadcast;

use crate::protocol::{McpResource, ReadResourceResult, ResourceContents};

/// Content returned by a [`ResourceProducer`].
pub enum ProducerContent {
    /// UTF-8 text payload (stored in `text`). Typically `application/json`.
    Text {
        uri: String,
        mime_type: String,
        text: String,
    },
    /// Binary payload — serialized as base64 under `blob`.
    Blob {
        uri: String,
        mime_type: String,
        bytes: Vec<u8>,
    },
}

impl ProducerContent {
    fn into_contents(self) -> ResourceContents {
        match self {
            ProducerContent::Text {
                uri,
                mime_type,
                text,
            } => ResourceContents {
                uri,
                mime_type: Some(mime_type),
                text: Some(text),
                blob: None,
            },
            ProducerContent::Blob {
                uri,
                mime_type,
                bytes,
            } => ResourceContents {
                uri,
                mime_type: Some(mime_type),
                text: None,
                blob: Some(BASE64_STANDARD.encode(bytes)),
            },
        }
    }
}

/// Error type returned by [`ResourceProducer::read`].
#[derive(Debug, thiserror::Error)]
pub enum ResourceError {
    #[error("resource not found: {0}")]
    NotFound(String),
    #[error("resource not enabled: {0}")]
    NotEnabled(String),
    #[error("resource read failed: {0}")]
    Read(String),
}

pub type ResourceResult<T> = Result<T, ResourceError>;

/// A URI-scheme-keyed producer of MCP resources.
///
/// Implementations must be `Send + Sync` because the MCP server calls
/// them from any tokio worker thread.
pub trait ResourceProducer: Send + Sync {
    /// Human-readable URI scheme (e.g. `"scene"`, `"capture"`). Used to
    /// dispatch `resources/read` by scheme.
    fn scheme(&self) -> &str;

    /// Resources this producer surfaces in `resources/list`. May return an
    /// empty vector to hide the producer while keeping the scheme
    /// registered (useful for feature-flagged producers).
    fn list(&self) -> Vec<McpResource>;

    /// Read a resource by full URI.
    fn read(&self, uri: &str) -> ResourceResult<ProducerContent>;
}

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
}

impl ResourceRegistry {
    /// Construct a registry with the default built-in producers.
    pub fn new(enabled: bool, artefact_enabled: bool) -> Self {
        let (updated_tx, _) = broadcast::channel(64);
        let scene_snapshot = Arc::new(RwLock::new(None));
        let inner = Arc::new(ResourceRegistryInner {
            producers: RwLock::new(Vec::new()),
            subscriptions: RwLock::new(std::collections::HashMap::new()),
            updated_tx,
            scene_snapshot: scene_snapshot.clone(),
            enabled,
            artefact_enabled,
        });
        let registry = Self { inner };
        if enabled {
            registry.add_producer(Arc::new(SceneProducer {
                snapshot: scene_snapshot,
            }));
            registry.add_producer(Arc::new(CaptureProducer));
            registry.add_producer(Arc::new(AuditProducer::disabled()));
            // Always register the artefact producer so the scheme is
            // recognized; it returns NotEnabled when the flag is off so
            // #349 can wire the real backend without touching this file.
            registry.add_producer(Arc::new(ArtefactStubProducer {
                enabled: artefact_enabled,
            }));
        }
        registry
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

fn uri_scheme(uri: &str) -> Option<&str> {
    let idx = uri.find(':')?;
    Some(&uri[..idx])
}

// ── Built-in producers ─────────────────────────────────────────────────────

struct SceneProducer {
    snapshot: Arc<parking_lot::RwLock<Option<Value>>>,
}

impl ResourceProducer for SceneProducer {
    fn scheme(&self) -> &str {
        "scene"
    }

    fn list(&self) -> Vec<McpResource> {
        vec![McpResource {
            uri: "scene://current".to_string(),
            name: "Current Scene".to_string(),
            description: Some(
                "JSON summary of the current DCC scene (nodes, counts, metadata). \
                 Updated by the embedding adapter via ResourceRegistry::set_scene()."
                    .to_string(),
            ),
            mime_type: Some("application/json".to_string()),
        }]
    }

    fn read(&self, uri: &str) -> ResourceResult<ProducerContent> {
        if uri != "scene://current" {
            return Err(ResourceError::NotFound(uri.to_string()));
        }
        let snapshot = self.snapshot.read();
        let text = match snapshot.as_ref() {
            Some(v) => serde_json::to_string(v).map_err(|e| ResourceError::Read(e.to_string()))?,
            None => serde_json::to_string(&json!({
                "status": "no_scene_published",
                "hint": "embedding adapter should call ResourceRegistry::set_scene"
            }))
            .map_err(|e| ResourceError::Read(e.to_string()))?,
        };
        Ok(ProducerContent::Text {
            uri: uri.to_string(),
            mime_type: "application/json".to_string(),
            text,
        })
    }
}

struct CaptureProducer;

impl ResourceProducer for CaptureProducer {
    fn scheme(&self) -> &str {
        "capture"
    }

    fn list(&self) -> Vec<McpResource> {
        // Only surface capture://current_window when a real window backend
        // is available. Mock backend indicates no DCC window is present.
        let capturer = dcc_mcp_capture::Capturer::new_window_auto();
        if matches!(
            capturer.backend_kind(),
            dcc_mcp_capture::CaptureBackendKind::Mock
        ) {
            return Vec::new();
        }
        vec![McpResource {
            uri: "capture://current_window".to_string(),
            name: "DCC Window Snapshot".to_string(),
            description: Some(
                "PNG snapshot of the active DCC window. Read-only; each \
                 resources/read triggers a fresh capture."
                    .to_string(),
            ),
            mime_type: Some("image/png".to_string()),
        }]
    }

    fn read(&self, uri: &str) -> ResourceResult<ProducerContent> {
        if uri != "capture://current_window" {
            return Err(ResourceError::NotFound(uri.to_string()));
        }
        let capturer = dcc_mcp_capture::Capturer::new_window_auto();
        let cfg = dcc_mcp_capture::CaptureConfig::builder()
            .format(dcc_mcp_capture::CaptureFormat::Png)
            .build();
        let frame = capturer
            .capture(&cfg)
            .map_err(|e| ResourceError::Read(e.to_string()))?;
        Ok(ProducerContent::Blob {
            uri: uri.to_string(),
            mime_type: "image/png".to_string(),
            bytes: frame.data,
        })
    }
}

struct AuditProducer {
    log: Option<Arc<dcc_mcp_sandbox::AuditLog>>,
}

impl AuditProducer {
    fn new(log: Arc<dcc_mcp_sandbox::AuditLog>) -> Self {
        Self { log: Some(log) }
    }

    /// Stub producer used until `ResourceRegistry::wire_audit_log` is
    /// called. Returns an empty tail so callers can still list and read
    /// the resource without blowing up.
    fn disabled() -> Self {
        Self { log: None }
    }
}

impl ResourceProducer for AuditProducer {
    fn scheme(&self) -> &str {
        "audit"
    }

    fn list(&self) -> Vec<McpResource> {
        vec![McpResource {
            uri: "audit://recent".to_string(),
            name: "Recent Audit Entries".to_string(),
            description: Some(
                "Tail of the sandbox AuditLog. Supports ?limit=N (default \
                 100). Fires notifications/resources/updated on append."
                    .to_string(),
            ),
            mime_type: Some("application/json".to_string()),
        }]
    }

    fn read(&self, uri: &str) -> ResourceResult<ProducerContent> {
        let limit = parse_audit_limit(uri).unwrap_or(100);
        let entries = match &self.log {
            Some(log) => {
                let all = log.entries();
                let start = all.len().saturating_sub(limit);
                all[start..].to_vec()
            }
            None => Vec::new(),
        };
        let payload = json!({
            "limit": limit,
            "count": entries.len(),
            "entries": entries,
        });
        let text =
            serde_json::to_string(&payload).map_err(|e| ResourceError::Read(e.to_string()))?;
        Ok(ProducerContent::Text {
            uri: uri.to_string(),
            mime_type: "application/json".to_string(),
            text,
        })
    }
}

fn parse_audit_limit(uri: &str) -> Option<usize> {
    let q = uri.split_once('?')?.1;
    for kv in q.split('&') {
        if let Some(("limit", value)) = kv.split_once('=') {
            return value.parse().ok();
        }
    }
    None
}

/// Stub producer for `artefact://` — recognizes the scheme so that
/// `resources/read` can emit a descriptive error until issue #349 wires
/// the real artefact store.
struct ArtefactStubProducer {
    enabled: bool,
}

impl ResourceProducer for ArtefactStubProducer {
    fn scheme(&self) -> &str {
        "artefact"
    }

    fn list(&self) -> Vec<McpResource> {
        if !self.enabled {
            return Vec::new();
        }
        // When enabled, the real store (issue #349) will populate this;
        // the stub just returns an empty list.
        Vec::new()
    }

    fn read(&self, uri: &str) -> ResourceResult<ProducerContent> {
        if !self.enabled {
            return Err(ResourceError::NotEnabled(format!(
                "artefact resources not enabled (issue #349): {uri}"
            )));
        }
        Err(ResourceError::NotFound(uri.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scene_read_returns_placeholder_when_no_snapshot() {
        let reg = ResourceRegistry::new(true, false);
        let result = reg.read("scene://current").expect("scene read");
        let first = &result.contents[0];
        assert_eq!(first.uri, "scene://current");
        assert_eq!(first.mime_type.as_deref(), Some("application/json"));
        let text = first.text.as_deref().unwrap();
        assert!(text.contains("no_scene_published"));
    }

    #[test]
    fn scene_read_uses_published_snapshot() {
        let reg = ResourceRegistry::new(true, false);
        reg.set_scene(json!({"name": "my-scene", "nodes": 42}));
        let result = reg.read("scene://current").unwrap();
        let text = result.contents[0].text.as_deref().unwrap();
        assert!(text.contains("my-scene"));
        assert!(text.contains("42"));
    }

    #[test]
    fn list_includes_scene_and_audit_by_default() {
        let reg = ResourceRegistry::new(true, false);
        let uris: Vec<String> = reg.list().into_iter().map(|r| r.uri).collect();
        assert!(uris.iter().any(|u| u == "scene://current"));
        assert!(uris.iter().any(|u| u == "audit://recent"));
        // artefact hidden when disabled.
        assert!(!uris.iter().any(|u| u.starts_with("artefact://")));
    }

    #[test]
    fn artefact_read_returns_not_enabled_when_disabled() {
        let reg = ResourceRegistry::new(true, false);
        let err = reg.read("artefact://abc123").unwrap_err();
        assert!(matches!(err, ResourceError::NotEnabled(_)));
    }

    #[test]
    fn unknown_scheme_returns_not_found() {
        let reg = ResourceRegistry::new(true, false);
        let err = reg.read("bogus://x").unwrap_err();
        assert!(matches!(err, ResourceError::NotFound(_)));
    }

    #[test]
    fn subscribe_and_unsubscribe_tracks_session() {
        let reg = ResourceRegistry::new(true, false);
        assert!(reg.subscribe("sess1", "scene://current"));
        // Duplicate returns false.
        assert!(!reg.subscribe("sess1", "scene://current"));
        let subs = reg.sessions_subscribed_to("scene://current");
        assert_eq!(subs, vec!["sess1".to_string()]);
        assert!(reg.unsubscribe("sess1", "scene://current"));
        assert!(reg.sessions_subscribed_to("scene://current").is_empty());
    }

    #[test]
    fn audit_read_returns_empty_tail_by_default() {
        let reg = ResourceRegistry::new(true, false);
        let result = reg.read("audit://recent?limit=5").unwrap();
        let text = result.contents[0].text.as_deref().unwrap();
        assert!(text.contains("\"count\":0"));
        assert!(text.contains("\"limit\":5"));
    }

    #[tokio::test]
    async fn wire_audit_log_fires_update_on_record() {
        use dcc_mcp_sandbox::{AuditEntry, AuditLog, AuditOutcome};
        use std::time::Duration;

        let reg = ResourceRegistry::new(true, false);
        let log = Arc::new(AuditLog::new());
        reg.wire_audit_log(log.clone());

        let mut rx = reg.watch_updates();
        log.record(AuditEntry::new(
            None,
            "test",
            "{}",
            Duration::from_millis(1),
            AuditOutcome::Success,
        ));
        // Allow the forwarding task to run.
        let uri = tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .expect("update fired")
            .expect("recv ok");
        assert_eq!(uri, "audit://recent");

        // And the tail now includes the entry.
        let result = reg.read("audit://recent?limit=10").unwrap();
        let text = result.contents[0].text.as_deref().unwrap();
        assert!(text.contains("\"count\":1"));
    }

    #[test]
    fn disabled_artefact_hidden_from_list() {
        let reg = ResourceRegistry::new(true, false);
        assert!(!reg.list().iter().any(|r| r.uri.starts_with("artefact://")));
    }
}
