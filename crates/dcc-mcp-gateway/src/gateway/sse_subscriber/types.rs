use super::*;

/// Identifier for a client-side MCP session.
pub type ClientSessionId = String;

/// Identifier for a backend DCC server. Conventionally the backend's
/// MCP URL (`http://host:port/mcp`) — stable for the life of the
/// instance and sufficient for cancel forwarding.
pub type BackendId = String;

/// Gateway-owned routing entry for a single async job (issue #322).
///
/// Populated when the gateway forwards a `tools/call` and the backend
/// replies with a `job_id`; consulted on `notifications/cancelled` so
/// the cancel can be propagated to the exact backend that owns the
/// job. The `parent_job_id` link lets the gateway fan a cancel out
/// across backends when a workflow parent is cancelled (#318 cascade).
#[derive(Debug, Clone)]
pub struct JobRoute {
    /// Owning client session — used to route backend SSE notifications
    /// back to the originator (the pre-#322 behaviour).
    pub client_session_id: ClientSessionId,
    /// Backend that runs this job (stable for the job's lifetime —
    /// routes are sticky, no multi-backend failover, per #322).
    pub backend_id: BackendId,
    /// Tool name reported on dispatch, kept for cancel-payload logs.
    pub tool: String,
    /// Wall-clock time the route was created — drives TTL GC.
    pub created_at: DateTime<Utc>,
    /// Parent job id when this job was dispatched under a workflow
    /// (`_meta.dcc.parentJobId`). A cancel on the parent cascades to
    /// every child route, even across backends.
    pub parent_job_id: Option<String>,
}

/// Error returned when a new route cannot be admitted to the gateway
/// routing cache (issue #322).
#[derive(Debug, Clone)]
pub enum BindJobError {
    /// The owning session already holds `cap` live routes. The gateway
    /// surfaces this as a JSON-RPC `-32005 too_many_in_flight_jobs`
    /// error so AI clients can back off or cancel in-flight jobs.
    TooManyInFlight {
        session_id: ClientSessionId,
        live: usize,
        cap: usize,
    },
}

impl std::fmt::Display for BindJobError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BindJobError::TooManyInFlight {
                session_id,
                live,
                cap,
            } => write!(
                f,
                "too_many_in_flight_jobs: session {session_id} holds {live} live routes (cap {cap})"
            ),
        }
    }
}

impl std::error::Error for BindJobError {}

/// A notification buffered while its target mapping is still unknown.
#[derive(Debug, Clone)]
pub(crate) struct Pending {
    pub(crate) inserted_at: Instant,
    pub(crate) value: Value,
}

/// A single client's subscription to a backend resource (#732).
///
/// The gateway tracks `(backend_url, backend_uri)` → `{ClientRoute}` so
/// that a backend-emitted `notifications/resources/updated` can be fanned
/// out to every subscribing client session with the URI rewritten back
/// to the gateway-prefixed form the client originally subscribed with.
///
/// Stored inside a `DashSet`, so `Eq` / `Hash` must be total over the
/// fields that uniquely identify a subscription; two subscribers that
/// differ only in `client_uri` still count as distinct entries because
/// the outbound notification rewrites the URI per-route.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct ResourceSubscriberRoute {
    /// Owning client session — key into `client_sinks`.
    pub(crate) client_session_id: ClientSessionId,
    /// The gateway-prefixed URI the client originally subscribed with.
    /// Written back into `params.uri` on every outbound
    /// `notifications/resources/updated`, so the client sees the URI
    /// shape it originally requested (not the raw backend URI).
    pub(crate) client_uri: String,
}
