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
use serde_json::Value;
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::broadcast;
use uuid::Uuid;

use crate::protocol::ClientRoot;

/// Maximum number of recent log messages retained per session.
const SESSION_LOG_BUFFER_CAP: usize = 200;

/// Session-scoped MCP logging threshold.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SessionLogLevel {
    Debug,
    #[default]
    Info,
    Warning,
    Error,
}

impl SessionLogLevel {
    /// Parse MCP log level strings (case-insensitive).
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "debug" => Some(Self::Debug),
            "info" => Some(Self::Info),
            "warning" | "warn" => Some(Self::Warning),
            "error" => Some(Self::Error),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Debug => "debug",
            Self::Info => "info",
            Self::Warning => "warning",
            Self::Error => "error",
        }
    }

    /// Whether a message at `message_level` should be emitted under this threshold.
    pub fn allows(self, message_level: SessionLogLevel) -> bool {
        self.rank() <= message_level.rank()
    }

    fn rank(self) -> u8 {
        match self {
            Self::Debug => 10,
            Self::Info => 20,
            Self::Warning => 30,
            Self::Error => 40,
        }
    }
}

/// A retained per-session log message for error correlation (`details.log_tail`).
#[derive(Debug, Clone)]
pub struct SessionLogMessage {
    pub level: SessionLogLevel,
    pub logger: String,
    pub data: Value,
    pub request_id: Option<String>,
}

/// A single MCP session.
#[derive(Debug)]
pub struct McpSession {
    /// Unique session identifier (sent as `Mcp-Session-Id` header).
    pub id: String,
    /// Whether the session has been initialized (i.e. `initialize` was called).
    pub initialized: bool,
    /// The negotiated MCP protocol version for this session (e.g. "2025-03-26").
    ///
    /// Set during `initialize` via [`SessionManager::set_protocol_version`].
    /// Later handlers can branch on this to enable version-specific behaviour.
    pub protocol_version: Option<String>,
    /// Whether the client opted into delta tools notifications.
    pub supports_delta_tools: bool,
    /// Current minimum log level for MCP `notifications/message`.
    pub log_level: SessionLogLevel,
    /// Recent retained log lines emitted for this session.
    pub recent_logs: VecDeque<SessionLogMessage>,
    /// Whether the client advertised MCP roots capability.
    pub supports_roots: bool,
    /// Cached client-advertised roots from `roots/list`.
    pub client_roots: Vec<ClientRoot>,
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
            protocol_version: None,
            supports_delta_tools: false,
            log_level: SessionLogLevel::default(),
            recent_logs: VecDeque::with_capacity(SESSION_LOG_BUFFER_CAP),
            supports_roots: false,
            client_roots: Vec::new(),
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

    /// Store the negotiated protocol version on a session.
    ///
    /// Called during `initialize` after version negotiation.
    pub fn set_protocol_version(&self, session_id: &str, version: &str) -> bool {
        if let Some(mut s) = self.sessions.get_mut(session_id) {
            s.protocol_version = Some(version.to_owned());
            true
        } else {
            false
        }
    }

    /// Retrieve the negotiated protocol version for a session.
    pub fn get_protocol_version(&self, session_id: &str) -> Option<String> {
        self.sessions
            .get(session_id)
            .and_then(|s| s.protocol_version.clone())
    }

    /// Record whether the client opted into delta-tools notifications.
    pub fn set_supports_delta_tools(&self, session_id: &str, enabled: bool) -> bool {
        if let Some(mut s) = self.sessions.get_mut(session_id) {
            s.supports_delta_tools = enabled;
            true
        } else {
            false
        }
    }

    /// Whether the client for `session_id` opted into delta-tools notifications.
    pub fn supports_delta_tools(&self, session_id: &str) -> bool {
        self.sessions
            .get(session_id)
            .map(|s| s.supports_delta_tools)
            .unwrap_or(false)
    }

    /// Update the session's MCP message log threshold.
    pub fn set_log_level(&self, session_id: &str, level: SessionLogLevel) -> bool {
        if let Some(mut s) = self.sessions.get_mut(session_id) {
            s.log_level = level;
            true
        } else {
            false
        }
    }

    /// Return the session's effective log threshold.
    pub fn get_log_level(&self, session_id: &str) -> SessionLogLevel {
        self.sessions
            .get(session_id)
            .map(|s| s.log_level)
            .unwrap_or_default()
    }

    /// Retain a log message for later `details.log_tail` correlation.
    pub fn push_log_message(&self, session_id: &str, entry: SessionLogMessage) -> bool {
        if let Some(mut s) = self.sessions.get_mut(session_id) {
            s.recent_logs.push_back(entry);
            if s.recent_logs.len() > SESSION_LOG_BUFFER_CAP {
                s.recent_logs.pop_front();
            }
            true
        } else {
            false
        }
    }

    /// Return up to `limit` recent log entries correlated to `request_id`.
    pub fn tail_logs_for_request(
        &self,
        session_id: &str,
        request_id: &str,
        limit: usize,
    ) -> Vec<Value> {
        if request_id.is_empty() || limit == 0 {
            return Vec::new();
        }
        let Some(s) = self.sessions.get(session_id) else {
            return Vec::new();
        };

        let mut out: Vec<Value> = s
            .recent_logs
            .iter()
            .rev()
            .filter(|line| line.request_id.as_deref() == Some(request_id))
            .take(limit)
            .map(|line| {
                serde_json::json!({
                    "level": line.level.as_str(),
                    "logger": line.logger,
                    "data": line.data,
                    "request_id": line.request_id,
                })
            })
            .collect();
        out.reverse();
        out
    }

    /// Record whether the client advertised roots capability.
    pub fn set_supports_roots(&self, session_id: &str, enabled: bool) -> bool {
        if let Some(mut s) = self.sessions.get_mut(session_id) {
            s.supports_roots = enabled;
            true
        } else {
            false
        }
    }

    /// Whether the client for `session_id` supports roots/list.
    pub fn supports_roots(&self, session_id: &str) -> bool {
        self.sessions
            .get(session_id)
            .map(|s| s.supports_roots)
            .unwrap_or(false)
    }

    /// Replace cached roots for the session.
    pub fn set_client_roots(&self, session_id: &str, roots: Vec<ClientRoot>) -> bool {
        if let Some(mut s) = self.sessions.get_mut(session_id) {
            s.client_roots = roots;
            true
        } else {
            false
        }
    }

    /// Get cached roots for a session.
    pub fn get_client_roots(&self, session_id: &str) -> Vec<ClientRoot> {
        self.sessions
            .get(session_id)
            .map(|s| s.client_roots.clone())
            .unwrap_or_default()
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

    /// Snapshot of every active session id.
    ///
    /// Used by broadcast-style notifications (`prompts/list_changed`,
    /// `tools/list_changed`) that fan out to every subscriber. The
    /// returned vector is owned so the caller can iterate without
    /// holding a shard lock.
    pub fn all_ids(&self) -> Vec<String> {
        self.sessions.iter().map(|e| e.key().clone()).collect()
    }
}
