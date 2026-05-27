//! Relay-backed gateway discovery source (#1363).
//!
//! A tunnel relay is not a DCC backend by itself; it is a transport hop to a
//! DCC-MCP server behind NAT. This module keeps that distinction explicit by
//! polling relay admin `/tunnels` listings, converting active rows into
//! advisory [`ServiceEntry`] values, and probing the relayed REST health path
//! before the gateway can route calls through it.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use reqwest::Url;
use serde::Deserialize;
use tokio::sync::broadcast;
use uuid::Uuid;

use dcc_mcp_transport::discovery::types::{ServiceEntry, ServiceStatus};

use crate::gateway::http_registration::{MCP_URL_METADATA_KEY, REGISTRY_SOURCE_METADATA_KEY};

pub(crate) const SOURCE_RELAY: &str = "relay";
const RELAY_TUNNEL_ID_METADATA_KEY: &str = "relay_tunnel_id";
const RELAY_PUBLIC_URL_METADATA_KEY: &str = "relay_public_url";
const RELAY_ADMIN_URL_METADATA_KEY: &str = "relay_admin_url";
const RELAY_AGENT_VERSION_METADATA_KEY: &str = "relay_agent_version";
const RELAY_CAPABILITIES_METADATA_KEY: &str = "relay_capabilities";
const CAPABILITIES_FINGERPRINT_METADATA_KEY: &str = "capabilities_fingerprint";

#[derive(Debug, Clone)]
struct RelayInstanceRecord {
    entry: ServiceEntry,
    relay_url: String,
    tunnel_id: String,
    expires_at: SystemTime,
}

/// Result of refreshing one relay row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RelayUpsertOutcome {
    pub instance_id: Uuid,
    pub changed: bool,
}

/// In-memory view of DCC-MCP instances currently reachable through relays.
#[derive(Debug, Default)]
pub struct RelayInstanceRegistry {
    entries: HashMap<Uuid, RelayInstanceRecord>,
    tunnel_to_instance: HashMap<String, Uuid>,
}

#[derive(Debug, Clone)]
pub(crate) struct RelayPollerConfig {
    pub urls: Vec<String>,
    pub ttl: Duration,
    pub poll_interval: Duration,
    pub probe_timeout: Duration,
}

impl RelayInstanceRegistry {
    /// Add or refresh a probed relay row.
    pub fn upsert(
        &mut self,
        mut entry: ServiceEntry,
        relay_url: impl Into<String>,
        tunnel_id: impl Into<String>,
        ttl: Duration,
        now: SystemTime,
    ) -> RelayUpsertOutcome {
        entry.touch();
        let instance_id = entry.instance_id;
        let relay_url = relay_url.into();
        let tunnel_id = tunnel_id.into();
        let current_tunnel_key = tunnel_key(&relay_url, &tunnel_id);
        let changed = self.entries.get(&instance_id).is_none_or(|previous| {
            previous.relay_url != relay_url
                || previous.tunnel_id != tunnel_id
                || relay_entry_changed(&previous.entry, &entry)
        });

        if let Some(previous_record) = self.entries.get(&instance_id) {
            let previous_key = tunnel_key(&previous_record.relay_url, &previous_record.tunnel_id);
            if previous_key != current_tunnel_key {
                self.tunnel_to_instance.remove(&previous_key);
            }
        }
        if let Some(previous_instance_id) = self
            .tunnel_to_instance
            .insert(current_tunnel_key.clone(), instance_id)
            && previous_instance_id != instance_id
        {
            self.entries.remove(&previous_instance_id);
        }

        self.entries.insert(
            instance_id,
            RelayInstanceRecord {
                entry,
                relay_url,
                tunnel_id,
                expires_at: now + ttl,
            },
        );
        RelayUpsertOutcome {
            instance_id,
            changed,
        }
    }

    /// Drop relay rows for one relay URL that are no longer present in the
    /// latest `/tunnels` snapshot.
    pub fn remove_missing_for_relay(
        &mut self,
        relay_url: &str,
        active_tunnel_ids: &HashSet<String>,
    ) -> Vec<Uuid> {
        let removed: Vec<Uuid> = self
            .entries
            .iter()
            .filter_map(|(instance_id, record)| {
                (record.relay_url == relay_url && !active_tunnel_ids.contains(&record.tunnel_id))
                    .then_some(*instance_id)
            })
            .collect();
        for instance_id in &removed {
            if let Some(record) = self.entries.remove(instance_id) {
                self.tunnel_to_instance
                    .remove(&tunnel_key(&record.relay_url, &record.tunnel_id));
            }
        }
        removed
    }

    /// Remove expired rows and return their instance ids.
    pub fn prune_expired(&mut self, now: SystemTime) -> Vec<Uuid> {
        let expired: Vec<Uuid> = self
            .entries
            .iter()
            .filter_map(|(instance_id, record)| (record.expires_at <= now).then_some(*instance_id))
            .collect();
        for instance_id in &expired {
            if let Some(record) = self.entries.remove(instance_id) {
                self.tunnel_to_instance
                    .remove(&tunnel_key(&record.relay_url, &record.tunnel_id));
            }
        }
        expired
    }

    /// Live relay rows that can be routed to.
    pub fn live_entries(&self, now: SystemTime) -> Vec<ServiceEntry> {
        self.entries
            .values()
            .filter(|record| record.expires_at > now)
            .filter(|record| {
                !matches!(
                    record.entry.status,
                    ServiceStatus::ShuttingDown
                        | ServiceStatus::Unreachable
                        | ServiceStatus::Booting
                        | ServiceStatus::Stale
                )
            })
            .map(|record| record.entry.clone())
            .collect()
    }

    /// Operator-facing relay rows. Expired rows are omitted because this
    /// source has no durable backing store after a relay stops listing them.
    pub fn all_entries(&self, now: SystemTime) -> Vec<ServiceEntry> {
        self.entries
            .values()
            .filter(|record| record.expires_at > now)
            .map(|record| record.entry.clone())
            .collect()
    }
}

pub(crate) fn spawn_relay_poller(
    registry: Arc<parking_lot::RwLock<RelayInstanceRegistry>>,
    http_client: reqwest::Client,
    events_tx: Arc<broadcast::Sender<String>>,
    capability_index: Arc<crate::gateway::capability::CapabilityIndex>,
    config: RelayPollerConfig,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        if config.urls.is_empty() {
            return;
        }
        tracing::info!(
            relays = ?config.urls,
            "relay discovery poller started"
        );
        let mut interval = tokio::time::interval(config.poll_interval.max(Duration::from_secs(1)));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            interval.tick().await;
            poll_relays_once(
                &registry,
                &http_client,
                &events_tx,
                &capability_index,
                &config.urls,
                config.ttl,
                config.probe_timeout,
            )
            .await;
        }
    })
}

async fn poll_relays_once(
    registry: &Arc<parking_lot::RwLock<RelayInstanceRegistry>>,
    http_client: &reqwest::Client,
    events_tx: &broadcast::Sender<String>,
    capability_index: &crate::gateway::capability::CapabilityIndex,
    relay_urls: &[String],
    ttl: Duration,
    probe_timeout: Duration,
) {
    let now = SystemTime::now();
    for relay_url in relay_urls {
        let relay_key = normalise_relay_key(relay_url);
        let fetched = match fetch_relay_entries(http_client, relay_url, probe_timeout).await {
            Ok(entries) => entries,
            Err(err) => {
                tracing::debug!(relay = %relay_url, error = %err, "relay discovery poll failed");
                continue;
            }
        };
        let active_tunnel_ids: HashSet<String> = fetched
            .iter()
            .map(|entry| entry.tunnel_id.clone())
            .collect();
        {
            let mut guard = registry.write();
            let removed = guard.remove_missing_for_relay(&relay_key, &active_tunnel_ids);
            let mut changed = !removed.is_empty();
            for instance_id in removed {
                capability_index.remove_instance(instance_id);
            }
            for fetched_entry in fetched {
                let outcome = guard.upsert(
                    fetched_entry.entry,
                    relay_key.clone(),
                    fetched_entry.tunnel_id,
                    ttl,
                    now,
                );
                changed |= outcome.changed;
            }
            if changed {
                broadcast_resources_changed(events_tx);
            }
        }
    }
}

#[derive(Debug, Clone)]
struct FetchedRelayEntry {
    tunnel_id: String,
    entry: ServiceEntry,
}

async fn fetch_relay_entries(
    http_client: &reqwest::Client,
    relay_url: &str,
    probe_timeout: Duration,
) -> Result<Vec<FetchedRelayEntry>, String> {
    let tunnels_url = relay_tunnels_url(relay_url)?;
    let summaries = http_client
        .get(tunnels_url)
        .send()
        .await
        .map_err(|err| err.to_string())?
        .error_for_status()
        .map_err(|err| err.to_string())?
        .json::<Vec<RelayTunnelSummary>>()
        .await
        .map_err(|err| err.to_string())?;

    let mut entries = Vec::new();
    for summary in summaries {
        let entry = match summary.to_service_entry(relay_url) {
            Ok(Some(entry)) => entry,
            Ok(None) => continue,
            Err(err) => {
                tracing::debug!(
                    relay = %relay_url,
                    tunnel = %summary.tunnel_id,
                    error = %err,
                    "skipping invalid relay tunnel row"
                );
                continue;
            }
        };
        if probe_relay_entry(http_client, &entry, probe_timeout).await {
            entries.push(FetchedRelayEntry {
                tunnel_id: summary.tunnel_id,
                entry,
            });
        }
    }
    Ok(entries)
}

#[derive(Debug, Clone, Deserialize)]
struct RelayTunnelSummary {
    tunnel_id: String,
    #[serde(default)]
    instance_id: Option<String>,
    dcc: String,
    #[serde(default)]
    dcc_type: Option<String>,
    #[serde(default)]
    capabilities: Vec<String>,
    #[serde(default)]
    capabilities_fingerprint: Option<String>,
    #[serde(default)]
    adapter_version: Option<String>,
    #[serde(default)]
    scene: Option<String>,
    #[serde(default)]
    agent_version: Option<String>,
    #[serde(default)]
    public_url: Option<String>,
}

impl RelayTunnelSummary {
    fn to_service_entry(&self, relay_url: &str) -> Result<Option<ServiceEntry>, String> {
        let Some(public_url) = self
            .public_url
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            return Ok(None);
        };
        let Some(mcp_url) = mcp_url_from_public_url(public_url)? else {
            return Ok(None);
        };
        let parsed_mcp_url = Url::parse(&mcp_url).map_err(|err| err.to_string())?;
        let host = parsed_mcp_url
            .host_str()
            .ok_or_else(|| "relay public_url has no host".to_string())?
            .to_string();
        let port = parsed_mcp_url
            .port_or_known_default()
            .ok_or_else(|| "relay public_url has no port and unknown scheme".to_string())?;
        let instance_id = self
            .instance_id
            .as_deref()
            .or(Some(self.tunnel_id.as_str()))
            .and_then(|value| Uuid::parse_str(value.trim()).ok())
            .ok_or_else(|| format!("relay tunnel {} has no UUID instance id", self.tunnel_id))?;
        let dcc_type = self
            .dcc_type
            .as_deref()
            .unwrap_or(self.dcc.as_str())
            .trim()
            .to_string();
        if dcc_type.is_empty() {
            return Ok(None);
        }

        let mut entry = ServiceEntry::new(dcc_type, host, port);
        entry.instance_id = instance_id;
        entry.pid = None;
        entry.adapter_version = clean_optional(self.adapter_version.as_deref());
        entry.scene = clean_optional(self.scene.as_deref());
        entry.metadata.insert(
            REGISTRY_SOURCE_METADATA_KEY.to_string(),
            SOURCE_RELAY.to_string(),
        );
        entry
            .metadata
            .insert(MCP_URL_METADATA_KEY.to_string(), mcp_url);
        entry.metadata.insert(
            RELAY_TUNNEL_ID_METADATA_KEY.to_string(),
            self.tunnel_id.clone(),
        );
        entry.metadata.insert(
            RELAY_PUBLIC_URL_METADATA_KEY.to_string(),
            public_url.to_string(),
        );
        entry.metadata.insert(
            RELAY_ADMIN_URL_METADATA_KEY.to_string(),
            normalise_relay_key(relay_url),
        );
        if !self.capabilities.is_empty() {
            entry.metadata.insert(
                RELAY_CAPABILITIES_METADATA_KEY.to_string(),
                self.capabilities.join(","),
            );
        }
        if let Some(value) = clean_optional(self.capabilities_fingerprint.as_deref()) {
            entry
                .metadata
                .insert(CAPABILITIES_FINGERPRINT_METADATA_KEY.to_string(), value);
        }
        if let Some(value) = clean_optional(self.agent_version.as_deref()) {
            entry
                .metadata
                .insert(RELAY_AGENT_VERSION_METADATA_KEY.to_string(), value);
        }
        Ok(Some(entry))
    }
}

async fn probe_relay_entry(
    http_client: &reqwest::Client,
    entry: &ServiceEntry,
    timeout: Duration,
) -> bool {
    let mcp_url = crate::gateway::http_registration::entry_mcp_url(entry);
    let url = format!(
        "{}/v1/healthz",
        crate::gateway::backend_client::rest_base_from_mcp_url(&mcp_url)
    );
    matches!(
        tokio::time::timeout(timeout, http_client.get(url).send()).await,
        Ok(Ok(resp)) if resp.status().is_success()
    )
}

fn relay_tunnels_url(relay_url: &str) -> Result<String, String> {
    let mut url = Url::parse(relay_url.trim()).map_err(|err| err.to_string())?;
    let path = url.path().trim_end_matches('/');
    if path.is_empty() {
        url.set_path("/tunnels");
    } else if !path.ends_with("/tunnels") {
        url.set_path(&format!("{path}/tunnels"));
    }
    url.set_query(None);
    Ok(url.to_string())
}

fn mcp_url_from_public_url(public_url: &str) -> Result<Option<String>, String> {
    let mut url = Url::parse(public_url.trim()).map_err(|err| err.to_string())?;
    match url.scheme() {
        "ws" => url
            .set_scheme("http")
            .map_err(|_| "failed to rewrite ws relay URL to http".to_string())?,
        "wss" => url
            .set_scheme("https")
            .map_err(|_| "failed to rewrite wss relay URL to https".to_string())?,
        "http" | "https" => {}
        _ => return Ok(None),
    }
    let path = url.path().trim_end_matches('/');
    url.set_path(&format!("{path}/mcp"));
    Ok(Some(url.to_string()))
}

fn normalise_relay_key(relay_url: &str) -> String {
    relay_url.trim().trim_end_matches('/').to_string()
}

fn tunnel_key(relay_url: &str, tunnel_id: &str) -> String {
    format!("{}#{}", normalise_relay_key(relay_url), tunnel_id)
}

fn relay_entry_changed(previous: &ServiceEntry, next: &ServiceEntry) -> bool {
    previous.dcc_type != next.dcc_type
        || previous.host != next.host
        || previous.port != next.port
        || previous.adapter_version != next.adapter_version
        || previous.scene != next.scene
        || previous.metadata.get(MCP_URL_METADATA_KEY) != next.metadata.get(MCP_URL_METADATA_KEY)
        || previous.metadata.get(CAPABILITIES_FINGERPRINT_METADATA_KEY)
            != next.metadata.get(CAPABILITIES_FINGERPRINT_METADATA_KEY)
        || previous.metadata.get(RELAY_PUBLIC_URL_METADATA_KEY)
            != next.metadata.get(RELAY_PUBLIC_URL_METADATA_KEY)
        || previous.metadata.get(RELAY_AGENT_VERSION_METADATA_KEY)
            != next.metadata.get(RELAY_AGENT_VERSION_METADATA_KEY)
        || previous.metadata.get(RELAY_CAPABILITIES_METADATA_KEY)
            != next.metadata.get(RELAY_CAPABILITIES_METADATA_KEY)
}

fn clean_optional(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn broadcast_resources_changed(events_tx: &broadcast::Sender<String>) {
    if events_tx.receiver_count() == 0 {
        return;
    }
    let notif = serde_json::to_string(&serde_json::json!({
        "jsonrpc": "2.0",
        "method": "notifications/resources/list_changed",
        "params": {}
    }))
    .unwrap_or_default();
    let _ = events_tx.send(notif);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn relay_entry(instance_id: Uuid, tunnel_id: &str, dcc: &str) -> ServiceEntry {
        let mut entry = ServiceEntry::new(dcc, "relay.example", 443);
        entry.instance_id = instance_id;
        entry.pid = None;
        entry.metadata.insert(
            REGISTRY_SOURCE_METADATA_KEY.to_string(),
            SOURCE_RELAY.to_string(),
        );
        entry.metadata.insert(
            RELAY_TUNNEL_ID_METADATA_KEY.to_string(),
            tunnel_id.to_string(),
        );
        entry
    }

    #[test]
    fn tunnel_summary_builds_relay_service_entry() {
        let summary = RelayTunnelSummary {
            tunnel_id: "11111111111141118111111111111111".to_string(),
            instance_id: Some("22222222-2222-4222-8222-222222222222".to_string()),
            dcc: "maya".to_string(),
            dcc_type: Some("maya".to_string()),
            capabilities: vec!["scene.read".to_string()],
            capabilities_fingerprint: Some("fp-1".to_string()),
            adapter_version: Some("dcc_mcp_maya/1.2.3".to_string()),
            scene: Some("shot.ma".to_string()),
            agent_version: Some("agent/0.1".to_string()),
            public_url: Some(
                "wss://relay.example/tunnel/11111111111141118111111111111111".to_string(),
            ),
        };

        let entry = summary
            .to_service_entry("https://relay-admin.example")
            .unwrap()
            .unwrap();

        assert_eq!(
            entry.instance_id.to_string(),
            "22222222-2222-4222-8222-222222222222"
        );
        assert_eq!(entry.dcc_type, "maya");
        assert_eq!(entry.host, "relay.example");
        assert_eq!(entry.port, 443);
        assert_eq!(entry.adapter_version.as_deref(), Some("dcc_mcp_maya/1.2.3"));
        assert_eq!(entry.scene.as_deref(), Some("shot.ma"));
        assert_eq!(
            entry.metadata.get(MCP_URL_METADATA_KEY).map(String::as_str),
            Some("https://relay.example/tunnel/11111111111141118111111111111111/mcp")
        );
        assert_eq!(
            entry
                .metadata
                .get(REGISTRY_SOURCE_METADATA_KEY)
                .map(String::as_str),
            Some(SOURCE_RELAY)
        );
        assert_eq!(
            entry
                .metadata
                .get(CAPABILITIES_FINGERPRINT_METADATA_KEY)
                .map(String::as_str),
            Some("fp-1")
        );
    }

    #[test]
    fn registry_removes_missing_tunnels_for_one_relay() {
        let now = SystemTime::now();
        let first = Uuid::parse_str("11111111-1111-4111-8111-111111111111").unwrap();
        let second = Uuid::parse_str("22222222-2222-4222-8222-222222222222").unwrap();
        let mut registry = RelayInstanceRegistry::default();
        registry.upsert(
            relay_entry(first, "t1", "maya"),
            "https://relay-a.example",
            "t1",
            Duration::from_secs(30),
            now,
        );
        registry.upsert(
            relay_entry(second, "t2", "houdini"),
            "https://relay-b.example",
            "t2",
            Duration::from_secs(30),
            now,
        );

        let active = HashSet::from(["t3".to_string()]);
        let removed = registry.remove_missing_for_relay("https://relay-a.example", &active);

        assert_eq!(removed, vec![first]);
        let live = registry.live_entries(now);
        assert_eq!(live.len(), 1);
        assert_eq!(live[0].instance_id, second);
    }

    #[test]
    fn relay_tunnels_url_accepts_base_or_endpoint() {
        assert_eq!(
            relay_tunnels_url("https://relay.example/admin").unwrap(),
            "https://relay.example/admin/tunnels"
        );
        assert_eq!(
            relay_tunnels_url("https://relay.example/admin/tunnels").unwrap(),
            "https://relay.example/admin/tunnels"
        );
    }

    #[test]
    fn public_url_rewrites_ws_to_http_mcp_endpoint() {
        assert_eq!(
            mcp_url_from_public_url("ws://relay.example:9876/tunnel/abc")
                .unwrap()
                .unwrap(),
            "http://relay.example:9876/tunnel/abc/mcp"
        );
    }
}
