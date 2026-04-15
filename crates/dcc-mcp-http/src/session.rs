//! MCP session management per the 2025-03-26 spec.
//!
//! Sessions are identified by a cryptographically random UUID.
//! Each session owns an SSE broadcast channel so multiple GET connections
//! can receive server-pushed notifications.
//!
//! Sessions carry a `last_active` timestamp that is updated on every request
//! via [`SessionManager::touch`].  The background eviction task in
//! `McpHttpServer::start` calls [`SessionManager::evict_stale`] every 60 s to
//! remove idle sessions older than `McpHttpConfig::session_ttl_secs`.

use dashmap::DashMap;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::broadcast;
use uuid::Uuid;

/// A single MCP session.
#[derive(Debug)]
pub struct McpSession {
    /// Unique session identifier (sent as `Mcp-Session-Id` header).
    pub id: String,
    /// Whether the session has been initialized (i.e. `initialize` was called).
    pub initialized: bool,
    /// Broadcast channel for server-push SSE events.
    pub sse_tx: broadcast::Sender<String>,
    /// Wall-clock time of the last request handled for this session.
    /// Used by the TTL eviction logic.
    pub last_active: Instant,
}

impl Default for McpSession {
    fn default() -> Self {
        Self::new()
    }
}

impl McpSession {
    pub fn new() -> Self {
        let id = Uuid::new_v4().to_string();
        let (sse_tx, _) = broadcast::channel(256);
        Self {
            id,
            initialized: false,
            sse_tx,
            last_active: Instant::now(),
        }
    }

    /// Refresh the last-active timestamp to the current instant.
    pub fn touch(&mut self) {
        self.last_active = Instant::now();
    }

    /// Subscribe to SSE events for this session.
    pub fn subscribe(&self) -> broadcast::Receiver<String> {
        self.sse_tx.subscribe()
    }

    /// Broadcast an SSE event string to all current GET subscribers.
    pub fn push_event(&self, event: String) {
        // Ignore send errors — no active subscribers is fine.
        let _ = self.sse_tx.send(event);
    }
}

/// Thread-safe session store.
#[derive(Debug, Clone, Default)]
pub struct SessionManager {
    sessions: Arc<DashMap<String, McpSession>>,
}

impl SessionManager {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(DashMap::new()),
        }
    }

    /// Create a new session and return its ID.
    pub fn create(&self) -> String {
        let session = McpSession::new();
        let id = session.id.clone();
        self.sessions.insert(id.clone(), session);
        tracing::debug!("session created: {id}");
        id
    }

    /// Mark a session as initialized.
    pub fn mark_initialized(&self, session_id: &str) -> bool {
        if let Some(mut s) = self.sessions.get_mut(session_id) {
            s.initialized = true;
            true
        } else {
            false
        }
    }

    /// Whether the session exists and is initialized.
    pub fn is_initialized(&self, session_id: &str) -> bool {
        self.sessions
            .get(session_id)
            .map(|s| s.initialized)
            .unwrap_or(false)
    }

    /// Get an SSE subscriber for the session.
    pub fn subscribe(&self, session_id: &str) -> Option<broadcast::Receiver<String>> {
        self.sessions.get(session_id).map(|s| s.subscribe())
    }

    /// Push an event to all SSE subscribers of the session.
    pub fn push_event(&self, session_id: &str, event: String) {
        if let Some(s) = self.sessions.get(session_id) {
            s.push_event(event);
        }
    }

    /// Refresh the last-active timestamp for `session_id`.
    ///
    /// Returns `false` if the session does not exist.
    pub fn touch(&self, session_id: &str) -> bool {
        if let Some(mut s) = self.sessions.get_mut(session_id) {
            s.touch();
            true
        } else {
            false
        }
    }

    /// Evict sessions that have been idle for longer than `ttl`.
    ///
    /// Called periodically by the background task in `McpHttpServer::start`.
    /// Returns the number of sessions removed.
    pub fn evict_stale(&self, ttl: std::time::Duration) -> usize {
        let now = Instant::now();
        let stale: Vec<String> = self
            .sessions
            .iter()
            .filter(|e| now.duration_since(e.value().last_active) >= ttl)
            .map(|e| e.key().clone())
            .collect();
        let count = stale.len();
        for id in &stale {
            self.sessions.remove(id);
            tracing::debug!(session_id = %id, "session evicted (TTL expired)");
        }
        if count > 0 {
            tracing::info!(evicted = count, "evicted stale MCP sessions");
        }
        count
    }

    /// Remove and drop a session.
    pub fn remove(&self, session_id: &str) -> bool {
        let removed = self.sessions.remove(session_id).is_some();
        if removed {
            tracing::debug!("session removed: {session_id}");
        }
        removed
    }

    /// Whether a session exists.
    pub fn exists(&self, session_id: &str) -> bool {
        self.sessions.contains_key(session_id)
    }

    /// Total number of active sessions.
    pub fn count(&self) -> usize {
        self.sessions.len()
    }
}
