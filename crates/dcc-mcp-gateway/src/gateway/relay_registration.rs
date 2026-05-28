//! Relay-backed gateway instance discovery (#1363).
//!
//! The tunnel relay's admin endpoint is read-only (`GET /tunnels`).  This
//! module translates those rows into the same `ServiceEntry` shape used by
//! file, HTTP, and mDNS discovery so routing and capability refresh can stay
//! source-agnostic.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, SystemTime};

use dcc_mcp_transport::discovery::types::{ServiceEntry, ServiceStatus};
use reqwest::Url;
use serde::{Deserialize, Serialize};
use serde_json::json;
use thiserror::Error;
use uuid::Uuid;

use super::http_registration::{
    MCP_URL_METADATA_KEY, REGISTRY_SOURCE_METADATA_KEY, SOURCE_RELAY, unix_secs,
};

const RELAY_TUNNEL_ID_METADATA_KEY: &str = "relay_tunnel_id";
const RELAY_ADMIN_URL_METADATA_KEY: &str = "relay_admin_url";
const RELAY_PUBLIC_BASE_URL_METADATA_KEY: &str = "relay_public_base_url";
const RELAY_AGENT_VERSION_METADATA_KEY: &str = "relay_agent_version";
const CAPABILITIES_FINGERPRINT_METADATA_KEY: &str = "capabilities_fingerprint";

/// One configured relay source for gateway discovery.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelaySourceConfig {
    /// Private/admin URL whose `/tunnels` endpoint returns live tunnel rows.
    pub admin_url: String,
    /// Public HTTP(S) base URL for the relay frontend that proxies
    /// `/tunnel/{id}/...` requests through the selected tunnel.
    pub public_base_url: String,
    /// Optional poll interval in seconds. Defaults to 5 when omitted or zero.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub poll_interval_secs: Option<u64>,
}

impl RelaySourceConfig {
    pub fn poll_interval(&self) -> Duration {
        Duration::from_secs(self.poll_interval_secs.unwrap_or(5).max(1))
    }

    pub fn source_key(&self) -> String {
        normalise_base_url(&self.admin_url)
    }
}

/// Tunnel row returned by `dcc-mcp-tunnel-relay`'s admin API.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct RelayTunnelSummary {
    pub tunnel_id: String,
    #[serde(alias = "dcc_type")]
    pub dcc: String,
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(default)]
    pub instance_id: Option<String>,
    #[serde(default)]
    pub capabilities_fingerprint: Option<String>,
    #[serde(default)]
    pub adapter_version: Option<String>,
    #[serde(default)]
    pub scene: Option<String>,
    #[serde(default)]
    pub public_url: Option<String>,
    pub agent_version: String,
    #[serde(default)]
    pub registered_at_ms_ago: u128,
    #[serde(default)]
    pub last_heartbeat_ms_ago: u128,
    #[serde(default)]
    pub session_count: usize,
}

#[derive(Debug, Error)]
pub enum RelayInstanceError {
    #[error("invalid relay admin_url: {0}")]
    InvalidAdminUrl(String),
    #[error("invalid relay public_base_url: {0}")]
    InvalidPublicBaseUrl(String),
    #[error("invalid relay mcp_url: {0}")]
    InvalidMcpUrl(String),
    #[error("missing tunnel_id")]
    MissingTunnelId,
    #[error("missing DCC type")]
    MissingDccType,
}

#[derive(Debug, Clone)]
struct RelayInstanceRecord {
    source_key: String,
    entry: ServiceEntry,
    expires_at: SystemTime,
}

/// In-memory registry of live relay-discovered instances.
#[derive(Debug, Default)]
pub struct RelayInstanceRegistry {
    entries: HashMap<(String, String), RelayInstanceRecord>,
}

impl RelayInstanceRegistry {
    pub(crate) fn upsert(
        &mut self,
        source_key: String,
        tunnel_id: String,
        entry: ServiceEntry,
        ttl: Duration,
        now: SystemTime,
    ) {
        let expires_at = now + ttl;
        self.entries.insert(
            (source_key.clone(), tunnel_id.clone()),
            RelayInstanceRecord {
                source_key,
                entry,
                expires_at,
            },
        );
    }

    pub(crate) fn retain_source_tunnels(
        &mut self,
        source_key: &str,
        live_tunnels: &HashSet<String>,
    ) -> usize {
        let before = self.entries.len();
        self.entries.retain(|(_, tunnel_id), rec| {
            source_key != rec.source_key || live_tunnels.contains(tunnel_id)
        });
        before.saturating_sub(self.entries.len())
    }

    pub(crate) fn prune_expired(&mut self, now: SystemTime) -> usize {
        let before = self.entries.len();
        self.entries.retain(|_, rec| rec.expires_at > now);
        before.saturating_sub(self.entries.len())
    }

    pub(crate) fn live_entries(&self, now: SystemTime) -> Vec<ServiceEntry> {
        self.entries
            .values()
            .filter(|rec| rec.expires_at > now)
            .map(|rec| rec.entry.clone())
            .collect()
    }

    pub(crate) fn all_entries(&self, now: SystemTime) -> Vec<ServiceEntry> {
        self.live_entries(now)
    }
}

pub(crate) fn entry_from_relay_tunnel(
    source: &RelaySourceConfig,
    summary: &RelayTunnelSummary,
    now: SystemTime,
) -> Result<ServiceEntry, RelayInstanceError> {
    let tunnel_id = clean_str(&summary.tunnel_id).ok_or(RelayInstanceError::MissingTunnelId)?;
    let dcc_type = clean_str(&summary.dcc).ok_or(RelayInstanceError::MissingDccType)?;
    let admin_url = normalise_admin_url(&source.admin_url)?;
    let tunnel_base_url = tunnel_base_url(source, summary, &tunnel_id)?;
    let mcp_url = append_path(&tunnel_base_url, "mcp")
        .map_err(|_| RelayInstanceError::InvalidMcpUrl(tunnel_base_url.clone()))?;
    Url::parse(&mcp_url).map_err(|_| RelayInstanceError::InvalidMcpUrl(mcp_url.clone()))?;

    let instance_id = summary
        .instance_id
        .as_deref()
        .and_then(|raw| Uuid::parse_str(raw).ok())
        .unwrap_or_else(|| relay_tunnel_uuid(&admin_url, &tunnel_id));
    let last_heartbeat = now
        .checked_sub(Duration::from_millis(
            summary.last_heartbeat_ms_ago.min(u64::MAX as u128) as u64,
        ))
        .unwrap_or(now);

    let url =
        Url::parse(&mcp_url).map_err(|_| RelayInstanceError::InvalidMcpUrl(mcp_url.clone()))?;
    let host = url.host_str().unwrap_or("127.0.0.1").to_string();
    let port = url.port_or_known_default().unwrap_or(0);
    let mut entry = ServiceEntry::new(dcc_type, host, port);
    entry.instance_id = instance_id;
    entry.pid = None;
    entry.scene = clean_opt(summary.scene.clone());
    entry.adapter_version = clean_opt(summary.adapter_version.clone());
    entry.last_heartbeat = last_heartbeat;
    entry.registered_at = now
        .checked_sub(Duration::from_millis(
            summary.registered_at_ms_ago.min(u64::MAX as u128) as u64,
        ))
        .unwrap_or(now);
    entry.status = ServiceStatus::Available;
    entry.metadata.insert(
        REGISTRY_SOURCE_METADATA_KEY.to_string(),
        SOURCE_RELAY.to_string(),
    );
    entry
        .metadata
        .insert(MCP_URL_METADATA_KEY.to_string(), mcp_url.clone());
    entry
        .metadata
        .insert(RELAY_TUNNEL_ID_METADATA_KEY.to_string(), tunnel_id.clone());
    entry
        .metadata
        .insert(RELAY_ADMIN_URL_METADATA_KEY.to_string(), admin_url.clone());
    entry.metadata.insert(
        RELAY_PUBLIC_BASE_URL_METADATA_KEY.to_string(),
        normalise_base_url(&source.public_base_url),
    );
    entry.metadata.insert(
        RELAY_AGENT_VERSION_METADATA_KEY.to_string(),
        summary.agent_version.clone(),
    );
    if let Some(fp) = clean_opt(summary.capabilities_fingerprint.clone()) {
        entry
            .metadata
            .insert(CAPABILITIES_FINGERPRINT_METADATA_KEY.to_string(), fp);
    }
    entry.extras.insert(
        "source_meta".to_string(),
        json!({
            "source": SOURCE_RELAY,
            "relay_admin_url": admin_url,
            "relay_public_base_url": normalise_base_url(&source.public_base_url),
            "tunnel_id": tunnel_id,
            "public_url": summary.public_url.clone(),
            "gateway_tunnel_url": tunnel_base_url,
            "agent_version": summary.agent_version.clone(),
            "capabilities": summary.capabilities.clone(),
            "session_count": summary.session_count,
            "last_heartbeat_unix_secs": unix_secs(last_heartbeat),
        }),
    );
    Ok(entry)
}

fn tunnel_base_url(
    source: &RelaySourceConfig,
    _summary: &RelayTunnelSummary,
    tunnel_id: &str,
) -> Result<String, RelayInstanceError> {
    append_path(
        &normalise_public_base_url(&source.public_base_url)?,
        &format!("tunnel/{tunnel_id}"),
    )
    .map_err(|_| RelayInstanceError::InvalidPublicBaseUrl(source.public_base_url.clone()))
}

fn normalise_admin_url(raw: &str) -> Result<String, RelayInstanceError> {
    Url::parse(raw)
        .map(|_| normalise_base_url(raw))
        .map_err(|_| RelayInstanceError::InvalidAdminUrl(raw.to_string()))
}

fn normalise_public_base_url(raw: &str) -> Result<String, RelayInstanceError> {
    Url::parse(raw)
        .map(|_| normalise_base_url(raw))
        .map_err(|_| RelayInstanceError::InvalidPublicBaseUrl(raw.to_string()))
}

fn normalise_base_url(raw: &str) -> String {
    raw.trim().trim_end_matches('/').to_string()
}

fn append_path(base: &str, path: &str) -> Result<String, ()> {
    let mut url = Url::parse(base).map_err(|_| ())?;
    let base_path = url.path().trim_end_matches('/');
    let path = path.trim_start_matches('/');
    let merged = if base_path.is_empty() || base_path == "/" {
        format!("/{path}")
    } else {
        format!("{base_path}/{path}")
    };
    url.set_path(&merged);
    url.set_query(None);
    url.set_fragment(None);
    Ok(url.to_string().trim_end_matches('/').to_string())
}

fn clean_opt(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn clean_str(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_string())
}

fn relay_tunnel_uuid(admin_url: &str, tunnel_id: &str) -> Uuid {
    Uuid::new_v5(
        &Uuid::NAMESPACE_URL,
        format!("dcc-mcp-relay:{admin_url}#{tunnel_id}").as_bytes(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relay_tunnel_maps_to_service_entry() {
        let source = RelaySourceConfig {
            admin_url: "http://127.0.0.1:9872".into(),
            public_base_url: "http://relay.example".into(),
            poll_interval_secs: None,
        };
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(123);
        let summary = RelayTunnelSummary {
            tunnel_id: "tun1".into(),
            dcc: "maya".into(),
            capabilities: vec!["scene.read".into()],
            instance_id: Some("2d86aa74-9b19-49a4-a166-92b3f47ed84a".into()),
            capabilities_fingerprint: Some("fp".into()),
            adapter_version: Some("adapter/1.0".into()),
            scene: Some("shot.ma".into()),
            public_url: None,
            agent_version: "agent/1.0".into(),
            registered_at_ms_ago: 1000,
            last_heartbeat_ms_ago: 250,
            session_count: 2,
        };
        let entry = entry_from_relay_tunnel(&source, &summary, now).unwrap();
        assert_eq!(
            entry.instance_id.to_string(),
            "2d86aa74-9b19-49a4-a166-92b3f47ed84a"
        );
        assert_eq!(entry.metadata[REGISTRY_SOURCE_METADATA_KEY], SOURCE_RELAY);
        assert_eq!(
            entry.metadata[MCP_URL_METADATA_KEY],
            "http://relay.example/tunnel/tun1/mcp"
        );
        assert_eq!(entry.scene.as_deref(), Some("shot.ma"));
        assert_eq!(entry.adapter_version.as_deref(), Some("adapter/1.0"));
    }

    #[test]
    fn configured_public_base_url_drives_gateway_rest_calls() {
        let source = RelaySourceConfig {
            admin_url: "http://127.0.0.1:9872".into(),
            public_base_url: "http://relay-gateway.example".into(),
            poll_interval_secs: None,
        };
        let summary = RelayTunnelSummary {
            tunnel_id: "tun1".into(),
            dcc: "houdini".into(),
            capabilities: vec![],
            instance_id: None,
            capabilities_fingerprint: None,
            adapter_version: None,
            scene: None,
            public_url: Some("wss://relay.example/tunnel/tun1".into()),
            agent_version: "agent/1.0".into(),
            registered_at_ms_ago: 0,
            last_heartbeat_ms_ago: 0,
            session_count: 0,
        };
        let entry = entry_from_relay_tunnel(&source, &summary, SystemTime::UNIX_EPOCH).unwrap();
        assert_eq!(
            entry.metadata[MCP_URL_METADATA_KEY],
            "http://relay-gateway.example/tunnel/tun1/mcp"
        );
    }

    #[test]
    fn registry_removes_absent_tunnels_for_source() {
        let source = RelaySourceConfig {
            admin_url: "http://relay-a".into(),
            public_base_url: "http://relay-a".into(),
            poll_interval_secs: None,
        };
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1);
        let summary = RelayTunnelSummary {
            tunnel_id: "tun1".into(),
            dcc: "maya".into(),
            capabilities: vec![],
            instance_id: None,
            capabilities_fingerprint: None,
            adapter_version: None,
            scene: None,
            public_url: None,
            agent_version: "agent/1.0".into(),
            registered_at_ms_ago: 0,
            last_heartbeat_ms_ago: 0,
            session_count: 0,
        };
        let entry = entry_from_relay_tunnel(&source, &summary, now).unwrap();
        let mut registry = RelayInstanceRegistry::default();
        registry.upsert(
            source.source_key(),
            "tun1".into(),
            entry,
            Duration::from_secs(30),
            now,
        );
        assert_eq!(registry.live_entries(now).len(), 1);
        assert_eq!(
            registry.retain_source_tunnels(&source.source_key(), &HashSet::new()),
            1
        );
        assert!(registry.live_entries(now).is_empty());
    }
}
