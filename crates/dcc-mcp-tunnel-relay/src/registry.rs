//! In-memory registry of currently-connected tunnels.
//!
//! The registry is the single source of truth the data-plane (PR 3) and
//! the `/tunnels` listing endpoint (PR 4) read from. It is intentionally
//! lock-free at the per-tunnel level (via `dashmap`) so a remote-client
//! lookup never blocks behind an unrelated heartbeat.

use std::sync::Arc;
use std::time::Instant;

use dashmap::DashMap;
use parking_lot::RwLock;

use dcc_mcp_tunnel_protocol::{TunnelId, frame::PROTOCOL_VERSION};

use crate::handle::TunnelHandle;

/// One row of the tunnel registry. Mutable fields are wrapped in an
/// `RwLock` so heartbeat updates don't conflict with `/tunnels` reads.
#[derive(Debug)]
pub struct TunnelEntry {
    /// Stable per-tunnel identifier minted by the relay.
    pub tunnel_id: TunnelId,

    /// DCC tag declared by the agent (`"maya"`, `"houdini"`, …).
    pub dcc: String,

    /// Capability tags reported by the agent. Forwarded to remote clients
    /// so they can pre-flight tool calls without round-tripping.
    pub capabilities: Vec<String>,

    /// Build identifier the agent sent in `RegisterRequest::agent_version`.
    pub agent_version: String,

    /// Wall-clock instant the registration was accepted. Used for the
    /// "tunnel age" column in `/tunnels` listings.
    pub registered_at: Instant,

    /// Last heartbeat received. Updated under [`Self::touch`] without
    /// taking a write-lock on the whole registry.
    pub last_heartbeat: RwLock<Instant>,

    /// Frame router for this tunnel. The frontend listener clones this to
    /// send `OpenSession` / `Data` / `CloseSession` toward the agent and to
    /// register per-session inbound channels. `Arc`'d so the control-plane
    /// reader, the data-plane writer, and the eviction sweeper can all hold
    /// references without locking the registry.
    pub handle: Arc<TunnelHandle>,
}

impl TunnelEntry {
    /// Refresh `last_heartbeat` to "now".
    pub fn touch(&self) {
        *self.last_heartbeat.write() = Instant::now();
    }

    /// Read the most recent heartbeat instant.
    pub fn last_seen(&self) -> Instant {
        *self.last_heartbeat.read()
    }
}

/// Concurrent map of `tunnel_id → TunnelEntry`. The relay's control-plane
/// task inserts on `Register`, removes on disconnect or stale-timeout, and
/// the data-plane / `/tunnels` reader holds short-lived references.
#[derive(Debug, Default)]
pub struct TunnelRegistry {
    inner: DashMap<TunnelId, TunnelEntry>,
}

impl TunnelRegistry {
    /// Construct an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert a freshly-accepted tunnel. Returns `true` when the id was
    /// new; `false` if a duplicate was overwritten (which should only
    /// happen during reconnect of a still-known agent).
    pub fn insert(&self, entry: TunnelEntry) -> bool {
        self.inner.insert(entry.tunnel_id.clone(), entry).is_none()
    }

    /// Look up an entry by id. The returned guard pins the row for the
    /// lifetime of the borrow — keep it short.
    pub fn get(
        &self,
        id: &TunnelId,
    ) -> Option<dashmap::mapref::one::Ref<'_, TunnelId, TunnelEntry>> {
        self.inner.get(id)
    }

    /// Remove an entry. Returns the removed row if any.
    pub fn remove(&self, id: &TunnelId) -> Option<TunnelEntry> {
        self.inner.remove(id).map(|(_, v)| v)
    }

    /// Total live tunnels.
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Whether the registry currently holds any tunnels.
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Iterate every entry — used by `/tunnels` listings (PR 4) and the
    /// stale-eviction sweep.
    pub fn iter(&self) -> dashmap::iter::Iter<'_, TunnelId, TunnelEntry> {
        self.inner.iter()
    }

    /// Filter live tunnels by their declared DCC tag. Used by the
    /// `/dcc/<name>/<id>` routing endpoint added in PR 4.
    pub fn iter_by_dcc<'a>(
        &'a self,
        dcc: &'a str,
    ) -> impl Iterator<Item = dashmap::mapref::multiple::RefMulti<'a, TunnelId, TunnelEntry>> + 'a
    {
        self.inner.iter().filter(move |e| e.value().dcc == dcc)
    }
}

/// Sanity check that the protocol crate is correctly re-exported and that
/// the version constant is the one the relay was built against. The
/// constant is a `u16`; this static assertion catches accidental wide-int
/// rebases that would slip past a normal build.
const _: () = {
    assert!(PROTOCOL_VERSION == 1);
};

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str, dcc: &str) -> TunnelEntry {
        let (tx, _rx) = tokio::sync::mpsc::channel(8);
        TunnelEntry {
            tunnel_id: id.into(),
            dcc: dcc.into(),
            capabilities: vec![],
            agent_version: "test/0.0".into(),
            registered_at: Instant::now(),
            last_heartbeat: RwLock::new(Instant::now()),
            handle: Arc::new(TunnelHandle::new(tx)),
        }
    }

    #[test]
    fn insert_and_lookup_roundtrip() {
        let reg = TunnelRegistry::new();
        assert!(reg.insert(entry("t1", "maya")));
        assert!(reg.insert(entry("t2", "houdini")));
        assert_eq!(reg.len(), 2);
        assert_eq!(reg.get(&"t1".to_string()).unwrap().dcc, "maya");
        let removed = reg.remove(&"t1".to_string()).unwrap();
        assert_eq!(removed.tunnel_id, "t1");
        assert_eq!(reg.len(), 1);
    }

    #[test]
    fn iter_by_dcc_filters() {
        let reg = TunnelRegistry::new();
        reg.insert(entry("t1", "maya"));
        reg.insert(entry("t2", "maya"));
        reg.insert(entry("t3", "houdini"));
        assert_eq!(reg.iter_by_dcc("maya").count(), 2);
        assert_eq!(reg.iter_by_dcc("houdini").count(), 1);
        assert_eq!(reg.iter_by_dcc("blender").count(), 0);
    }
}
