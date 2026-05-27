//! Optional mDNS/DNS-SD discovery source for gateway backends (#1362).
//!
//! mDNS rows are advisory. The browser probes `/v1/healthz` before a service
//! is materialised into the shared live-instance view, and later security work
//! still owns trust and authorization for non-local sources.

use std::collections::HashMap;
#[cfg(feature = "mdns")]
use std::sync::Arc;
use std::time::{Duration, SystemTime};

#[cfg(feature = "mdns")]
use tokio::sync::broadcast;
use uuid::Uuid;

use dcc_mcp_transport::discovery::types::{ServiceEntry, ServiceStatus};

#[cfg(feature = "mdns")]
use dcc_mcp_transport::discovery::mdns::{DCC_MCP_MDNS_SERVICE_TYPE, entry_from_resolved_service};

#[derive(Debug, Clone)]
struct MdnsInstanceRecord {
    entry: ServiceEntry,
    fullname: String,
    expires_at: SystemTime,
}

/// In-memory view of mDNS-discovered DCC-MCP instances.
#[derive(Debug, Default)]
pub struct MdnsInstanceRegistry {
    entries: HashMap<Uuid, MdnsInstanceRecord>,
    fullname_to_instance: HashMap<String, Uuid>,
}

impl MdnsInstanceRegistry {
    /// Add or refresh a probed mDNS row.
    pub fn upsert(
        &mut self,
        mut entry: ServiceEntry,
        fullname: impl Into<String>,
        ttl: Duration,
        now: SystemTime,
    ) -> Uuid {
        entry.touch();
        let instance_id = entry.instance_id;
        let fullname = fullname.into();
        if let Some(previous_record) = self.entries.get(&instance_id)
            && previous_record.fullname != fullname
        {
            self.fullname_to_instance.remove(&previous_record.fullname);
        }
        if let Some(previous_instance_id) = self
            .fullname_to_instance
            .insert(fullname.clone(), instance_id)
            && previous_instance_id != instance_id
        {
            self.entries.remove(&previous_instance_id);
        }
        self.entries.insert(
            instance_id,
            MdnsInstanceRecord {
                entry,
                fullname,
                expires_at: now + ttl,
            },
        );
        instance_id
    }

    /// Remove an mDNS row by its DNS-SD full name.
    pub fn remove_fullname(&mut self, fullname: &str) -> Option<Uuid> {
        let instance_id = self.fullname_to_instance.remove(fullname)?;
        self.entries.remove(&instance_id);
        Some(instance_id)
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
                self.fullname_to_instance.remove(&record.fullname);
            }
        }
        expired
    }

    /// Live mDNS rows that can be routed to.
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

    /// Operator-facing mDNS rows. Expired rows are omitted because this source
    /// has no durable backing store to inspect after TTL expiry.
    pub fn all_entries(&self, now: SystemTime) -> Vec<ServiceEntry> {
        self.entries
            .values()
            .filter(|record| record.expires_at > now)
            .map(|record| record.entry.clone())
            .collect()
    }
}

#[cfg(feature = "mdns")]
pub(crate) fn spawn_mdns_browser(
    registry: Arc<parking_lot::RwLock<MdnsInstanceRegistry>>,
    http_client: reqwest::Client,
    events_tx: Arc<broadcast::Sender<String>>,
    capability_index: Arc<crate::gateway::capability::CapabilityIndex>,
    ttl: Duration,
    probe_timeout: Duration,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let daemon = match mdns_sd::ServiceDaemon::new() {
            Ok(daemon) => daemon,
            Err(err) => {
                tracing::warn!(error = %err, "mDNS discovery disabled: failed to create daemon");
                return;
            }
        };
        let receiver = match daemon.browse(DCC_MCP_MDNS_SERVICE_TYPE) {
            Ok(receiver) => receiver,
            Err(err) => {
                tracing::warn!(error = %err, "mDNS discovery disabled: failed to browse service");
                let _ = daemon.shutdown();
                return;
            }
        };

        tracing::info!(
            service = DCC_MCP_MDNS_SERVICE_TYPE,
            "mDNS discovery browser started"
        );

        while let Ok(event) = receiver.recv_async().await {
            match event {
                mdns_sd::ServiceEvent::ServiceResolved(resolved) => {
                    match entry_from_resolved_service(&resolved) {
                        Ok(entry) => {
                            let fullname = resolved.fullname.clone();
                            if probe_mdns_entry(&http_client, &entry, probe_timeout).await {
                                registry.write().upsert(
                                    entry.clone(),
                                    fullname,
                                    ttl,
                                    SystemTime::now(),
                                );
                                broadcast_resources_changed(&events_tx);
                                tracing::info!(
                                    instance = %entry.instance_id,
                                    dcc = %entry.dcc_type,
                                    "mDNS service resolved and probed"
                                );
                            } else {
                                tracing::debug!(
                                    instance = %entry.instance_id,
                                    dcc = %entry.dcc_type,
                                    "mDNS service ignored after failed /v1/healthz probe"
                                );
                            }
                        }
                        Err(err) => {
                            tracing::debug!(error = %err, "mDNS service ignored");
                        }
                    }
                }
                mdns_sd::ServiceEvent::ServiceRemoved(_, fullname) => {
                    if let Some(instance_id) = registry.write().remove_fullname(&fullname) {
                        capability_index.remove_instance(instance_id);
                        broadcast_resources_changed(&events_tx);
                        tracing::info!(
                            instance = %instance_id,
                            fullname = %fullname,
                            "mDNS service removed"
                        );
                    }
                }
                _ => {}
            }
        }

        let _ = daemon.stop_browse(DCC_MCP_MDNS_SERVICE_TYPE);
        let _ = daemon.shutdown();
    })
}

#[cfg(feature = "mdns")]
async fn probe_mdns_entry(
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

#[cfg(feature = "mdns")]
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

    fn mdns_entry(instance_id: Uuid, dcc: &str) -> ServiceEntry {
        let mut entry = ServiceEntry::new(dcc, "127.0.0.1", 8765);
        entry.instance_id = instance_id;
        entry.pid = None;
        entry
            .metadata
            .insert("dcc_mcp_registry_source".to_string(), "mdns".to_string());
        entry
    }

    #[test]
    fn registry_prunes_expired_mdns_rows() {
        let now = SystemTime::now();
        let instance_id = Uuid::parse_str("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee").unwrap();
        let mut registry = MdnsInstanceRegistry::default();

        registry.upsert(
            mdns_entry(instance_id, "maya"),
            "dcc-mcp-maya._dcc-mcp._tcp.local.",
            Duration::from_secs(1),
            now,
        );

        assert_eq!(registry.live_entries(now).len(), 1);
        let expired = registry.prune_expired(now + Duration::from_secs(2));
        assert_eq!(expired, vec![instance_id]);
        assert!(
            registry
                .live_entries(now + Duration::from_secs(2))
                .is_empty()
        );
    }

    #[test]
    fn registry_removes_by_fullname() {
        let now = SystemTime::now();
        let instance_id = Uuid::parse_str("bbbbbbbb-cccc-dddd-eeee-ffffffffffff").unwrap();
        let mut registry = MdnsInstanceRegistry::default();

        registry.upsert(
            mdns_entry(instance_id, "houdini"),
            "dcc-mcp-houdini._dcc-mcp._tcp.local.",
            Duration::from_secs(30),
            now,
        );

        assert_eq!(
            registry.remove_fullname("dcc-mcp-houdini._dcc-mcp._tcp.local."),
            Some(instance_id)
        );
        assert!(registry.live_entries(now).is_empty());
    }

    #[test]
    fn registry_replaces_stale_fullname_indexes() {
        let now = SystemTime::now();
        let first_instance_id = Uuid::parse_str("11111111-2222-3333-4444-555555555555").unwrap();
        let second_instance_id = Uuid::parse_str("66666666-7777-8888-9999-aaaaaaaaaaaa").unwrap();
        let mut registry = MdnsInstanceRegistry::default();

        registry.upsert(
            mdns_entry(first_instance_id, "maya"),
            "dcc-mcp-maya-old._dcc-mcp._tcp.local.",
            Duration::from_secs(30),
            now,
        );
        registry.upsert(
            mdns_entry(first_instance_id, "maya"),
            "dcc-mcp-maya-new._dcc-mcp._tcp.local.",
            Duration::from_secs(30),
            now,
        );

        assert_eq!(
            registry.remove_fullname("dcc-mcp-maya-old._dcc-mcp._tcp.local."),
            None
        );
        assert_eq!(registry.live_entries(now).len(), 1);

        registry.upsert(
            mdns_entry(second_instance_id, "maya"),
            "dcc-mcp-maya-new._dcc-mcp._tcp.local.",
            Duration::from_secs(30),
            now,
        );

        let live = registry.live_entries(now);
        assert_eq!(live.len(), 1);
        assert_eq!(live[0].instance_id, second_instance_id);
    }
}
