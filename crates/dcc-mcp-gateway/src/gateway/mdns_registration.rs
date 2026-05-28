//! mDNS-backed gateway instance source (#1362).
//!
//! mDNS is only an address-discovery mechanism. This registry stores rows
//! after the gateway has successfully probed the advertised MCP endpoint.

use std::collections::{HashMap, HashSet};
#[cfg(feature = "mdns")]
use std::net::IpAddr;
#[cfg(any(feature = "mdns", test))]
use std::time::Duration;
use std::time::SystemTime;

use dcc_mcp_transport::discovery::types::{ServiceEntry, ServiceStatus};
#[cfg(feature = "mdns")]
use thiserror::Error;
use uuid::Uuid;

#[cfg(feature = "mdns")]
use super::http_registration::{MCP_URL_METADATA_KEY, unix_secs};
#[cfg(any(feature = "mdns", test))]
use super::http_registration::{REGISTRY_SOURCE_METADATA_KEY, SOURCE_MDNS};

#[cfg(feature = "mdns")]
const MDNS_FULLNAME_METADATA_KEY: &str = "mdns_fullname";
#[cfg(feature = "mdns")]
const MDNS_HOSTNAME_METADATA_KEY: &str = "mdns_hostname";
#[cfg(feature = "mdns")]
const MDNS_AUTH_METADATA_KEY: &str = "mdns_auth";
#[cfg(feature = "mdns")]
const MDNS_ADAPTER_METADATA_KEY: &str = "mdns_adapter";

#[cfg(feature = "mdns")]
#[derive(Debug, Error)]
pub(crate) enum MdnsInstanceError {
    #[error("mDNS service is missing TXT field `{0}`")]
    MissingTxt(&'static str),
    #[error("mDNS service has invalid instance_id `{0}`")]
    InvalidInstanceId(String),
    #[error("mDNS service has invalid mcp_path `{0}`")]
    InvalidMcpPath(String),
    #[error("mDNS service produced invalid MCP URL `{0}`")]
    InvalidMcpUrl(String),
}

#[derive(Debug, Clone)]
struct MdnsInstanceRecord {
    entry: ServiceEntry,
    expires_at: SystemTime,
}

#[derive(Debug, Default, Clone)]
pub struct MdnsInstanceRegistry {
    entries: HashMap<Uuid, MdnsInstanceRecord>,
    fullname_index: HashMap<String, Uuid>,
}

impl MdnsInstanceRegistry {
    #[cfg(any(feature = "mdns", test))]
    pub(crate) fn upsert(
        &mut self,
        entry: ServiceEntry,
        fullname: String,
        ttl: Duration,
        now: SystemTime,
    ) {
        if let Some(previous) = self
            .fullname_index
            .insert(fullname.clone(), entry.instance_id)
            && previous != entry.instance_id
        {
            self.entries.remove(&previous);
        }
        self.entries.insert(
            entry.instance_id,
            MdnsInstanceRecord {
                entry,
                expires_at: now + ttl,
            },
        );
    }

    #[cfg(any(feature = "mdns", test))]
    pub(crate) fn remove_fullname(&mut self, fullname: &str) -> Option<ServiceEntry> {
        let id = self.fullname_index.remove(fullname)?;
        self.entries.remove(&id).map(|record| record.entry)
    }

    pub(crate) fn prune_expired(&mut self, now: SystemTime) -> usize {
        let expired: HashSet<Uuid> = self
            .entries
            .iter()
            .filter_map(|(id, record)| (record.expires_at <= now).then_some(*id))
            .collect();
        let count = expired.len();
        if count == 0 {
            return 0;
        }
        self.entries.retain(|id, _| !expired.contains(id));
        self.fullname_index
            .retain(|_, id| self.entries.contains_key(id));
        count
    }

    pub(crate) fn live_entries(&self, now: SystemTime) -> Vec<ServiceEntry> {
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

    pub(crate) fn all_entries(&self, now: SystemTime) -> Vec<ServiceEntry> {
        self.entries
            .values()
            .filter(|record| record.expires_at > now)
            .map(|record| record.entry.clone())
            .collect()
    }
}

#[cfg(feature = "mdns")]
pub(crate) fn entry_from_mdns_service(
    service: &dcc_mcp_transport::discovery::mdns::MdnsResolvedService,
    now: SystemTime,
) -> Result<ServiceEntry, MdnsInstanceError> {
    use dcc_mcp_transport::discovery::mdns::{
        DEFAULT_MCP_PATH, TXT_ADAPTER, TXT_AUTH, TXT_DCC, TXT_INSTANCE_ID, TXT_MCP_PATH,
        TXT_VERSION,
    };

    let dcc_type = required_txt(service, TXT_DCC)?;
    let instance_id_raw = required_txt(service, TXT_INSTANCE_ID)?;
    let instance_id = Uuid::parse_str(instance_id_raw)
        .map_err(|_| MdnsInstanceError::InvalidInstanceId(instance_id_raw.to_string()))?;
    let mcp_path = service.txt_value(TXT_MCP_PATH).unwrap_or(DEFAULT_MCP_PATH);
    if !mcp_path.starts_with('/') {
        return Err(MdnsInstanceError::InvalidMcpPath(mcp_path.to_string()));
    }

    let host = service
        .addresses
        .iter()
        .find(|ip| ip.is_ipv4())
        .or_else(|| service.addresses.first())
        .map(format_host_for_url)
        .unwrap_or_else(|| service.host.trim_end_matches('.').to_string());
    let mcp_url = format!("http://{}:{}{}", host, service.port, mcp_path);
    reqwest::Url::parse(&mcp_url).map_err(|_| MdnsInstanceError::InvalidMcpUrl(mcp_url.clone()))?;

    let mut entry = ServiceEntry::new(dcc_type, host, service.port);
    entry.instance_id = instance_id;
    entry.registered_at = now;
    entry.last_heartbeat = now;
    entry.adapter_version = service.txt_value(TXT_VERSION).map(str::to_string);
    entry.adapter_dcc = Some(dcc_type.to_string());
    entry.metadata.insert(
        REGISTRY_SOURCE_METADATA_KEY.to_string(),
        SOURCE_MDNS.to_string(),
    );
    entry
        .metadata
        .insert(MCP_URL_METADATA_KEY.to_string(), mcp_url);
    entry.metadata.insert(
        MDNS_FULLNAME_METADATA_KEY.to_string(),
        service.fullname.clone(),
    );
    entry
        .metadata
        .insert(MDNS_HOSTNAME_METADATA_KEY.to_string(), service.host.clone());
    if let Some(adapter) = service.txt_value(TXT_ADAPTER) {
        entry
            .metadata
            .insert(MDNS_ADAPTER_METADATA_KEY.to_string(), adapter.to_string());
    }
    if let Some(auth) = service.txt_value(TXT_AUTH) {
        entry
            .metadata
            .insert(MDNS_AUTH_METADATA_KEY.to_string(), auth.to_string());
    }
    entry.extras.insert(
        "source_meta".to_string(),
        serde_json::json!({
            "source": SOURCE_MDNS,
            "fullname": service.fullname,
            "hostname": service.host,
            "registered_at": unix_secs(now),
        }),
    );
    Ok(entry)
}

#[cfg(feature = "mdns")]
fn required_txt<'a>(
    service: &'a dcc_mcp_transport::discovery::mdns::MdnsResolvedService,
    key: &'static str,
) -> Result<&'a str, MdnsInstanceError> {
    service
        .txt_value(key)
        .filter(|value| !value.trim().is_empty())
        .ok_or(MdnsInstanceError::MissingTxt(key))
}

#[cfg(feature = "mdns")]
fn format_host_for_url(ip: &IpAddr) -> String {
    match ip {
        IpAddr::V4(addr) => addr.to_string(),
        IpAddr::V6(addr) => format!("[{addr}]"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: Uuid, source: &str) -> ServiceEntry {
        let mut entry = ServiceEntry::new("maya", "127.0.0.1", 8765);
        entry.instance_id = id;
        entry
            .metadata
            .insert(REGISTRY_SOURCE_METADATA_KEY.to_string(), source.to_string());
        entry
    }

    #[test]
    fn registry_prunes_expired_rows() {
        let id = Uuid::parse_str("11111111-1111-4111-8111-111111111111").unwrap();
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        let mut registry = MdnsInstanceRegistry::default();
        registry.upsert(
            entry(id, SOURCE_MDNS),
            "maya._dcc-mcp._tcp.local.".to_string(),
            Duration::from_secs(5),
            now,
        );

        assert_eq!(registry.live_entries(now).len(), 1);
        assert_eq!(registry.prune_expired(now + Duration::from_secs(6)), 1);
        assert!(
            registry
                .live_entries(now + Duration::from_secs(6))
                .is_empty()
        );
    }

    #[test]
    fn registry_removes_by_fullname() {
        let id = Uuid::parse_str("22222222-2222-4222-8222-222222222222").unwrap();
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        let mut registry = MdnsInstanceRegistry::default();
        registry.upsert(
            entry(id, SOURCE_MDNS),
            "maya._dcc-mcp._tcp.local.".to_string(),
            Duration::from_secs(30),
            now,
        );

        let removed = registry.remove_fullname("maya._dcc-mcp._tcp.local.");
        assert_eq!(removed.map(|entry| entry.instance_id), Some(id));
        assert!(registry.live_entries(now).is_empty());
    }

    #[cfg(feature = "mdns")]
    #[test]
    fn entry_from_mdns_service_maps_txt_to_service_entry() {
        use dcc_mcp_transport::discovery::mdns::{
            MdnsResolvedService, TXT_ADAPTER, TXT_AUTH, TXT_DCC, TXT_INSTANCE_ID, TXT_MCP_PATH,
            TXT_VERSION,
        };

        let id = Uuid::parse_str("33333333-3333-4333-8333-333333333333").unwrap();
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(321);
        let mut txt = HashMap::new();
        txt.insert(TXT_DCC.to_string(), "houdini".to_string());
        txt.insert(TXT_INSTANCE_ID.to_string(), id.to_string());
        txt.insert(TXT_MCP_PATH.to_string(), "/mcp".to_string());
        txt.insert(TXT_VERSION.to_string(), "0.17.37".to_string());
        txt.insert(TXT_ADAPTER.to_string(), "dcc-mcp-houdini".to_string());
        txt.insert(TXT_AUTH.to_string(), "bearer".to_string());

        let service = MdnsResolvedService {
            fullname: "houdini._dcc-mcp._tcp.local.".to_string(),
            host: "houdini.local.".to_string(),
            port: 9876,
            addresses: vec!["192.168.1.42".parse().unwrap()],
            txt,
        };

        let entry = entry_from_mdns_service(&service, now).unwrap();
        assert_eq!(entry.instance_id, id);
        assert_eq!(entry.dcc_type, "houdini");
        assert_eq!(entry.adapter_version.as_deref(), Some("0.17.37"));
        assert_eq!(
            entry.metadata[MCP_URL_METADATA_KEY],
            "http://192.168.1.42:9876/mcp"
        );
        assert_eq!(entry.metadata[REGISTRY_SOURCE_METADATA_KEY], SOURCE_MDNS);
        assert_eq!(entry.metadata[MDNS_AUTH_METADATA_KEY], "bearer");
    }
}
