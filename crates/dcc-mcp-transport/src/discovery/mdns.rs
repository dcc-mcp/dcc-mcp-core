//! Optional mDNS / DNS-SD discovery for LAN-local DCC MCP endpoints.
//!
//! The feature is deliberately advisory: this module only advertises and
//! discovers endpoint coordinates. Gateways must still probe and apply their
//! configured auth policy before routing traffic.

use std::collections::HashMap;
use std::net::IpAddr;

use mdns_sd::{Receiver, ServiceDaemon, ServiceEvent, ServiceInfo};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

pub const DCC_MCP_SERVICE_TYPE: &str = "_dcc-mcp._tcp.local.";
pub const DEFAULT_MCP_PATH: &str = "/mcp";
pub const TXT_DCC: &str = "dcc";
pub const TXT_INSTANCE_ID: &str = "instance_id";
pub const TXT_VERSION: &str = "version";
pub const TXT_MCP_PATH: &str = "mcp_path";
pub const TXT_ADAPTER: &str = "adapter";
pub const TXT_AUTH: &str = "auth";

#[derive(Debug, Error)]
pub enum MdnsDiscoveryError {
    #[error("invalid mDNS service info: {0}")]
    ServiceInfo(String),
    #[error("mDNS daemon error: {0}")]
    Daemon(String),
}

impl From<mdns_sd::Error> for MdnsDiscoveryError {
    fn from(value: mdns_sd::Error) -> Self {
        Self::Daemon(value.to_string())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MdnsAdvertisement {
    pub dcc_type: String,
    pub instance_id: Uuid,
    pub host_name: String,
    pub port: u16,
    pub version: Option<String>,
    pub adapter: Option<String>,
    pub auth: Option<String>,
    pub mcp_path: String,
}

impl MdnsAdvertisement {
    pub fn new(
        dcc_type: impl Into<String>,
        instance_id: Uuid,
        host_name: impl Into<String>,
        port: u16,
    ) -> Self {
        Self {
            dcc_type: dcc_type.into(),
            instance_id,
            host_name: host_name.into(),
            port,
            version: None,
            adapter: None,
            auth: None,
            mcp_path: DEFAULT_MCP_PATH.to_string(),
        }
    }

    pub fn with_version(mut self, version: Option<String>) -> Self {
        self.version = version;
        self
    }

    pub fn with_adapter(mut self, adapter: Option<String>) -> Self {
        self.adapter = adapter;
        self
    }

    pub fn with_auth(mut self, auth: Option<String>) -> Self {
        self.auth = auth;
        self
    }

    pub fn with_mcp_path(mut self, mcp_path: impl Into<String>) -> Self {
        let mcp_path = mcp_path.into();
        self.mcp_path = if mcp_path.starts_with('/') {
            mcp_path
        } else {
            format!("/{mcp_path}")
        };
        self
    }

    pub fn instance_name(&self) -> String {
        let mut short = self.instance_id.simple().to_string();
        short.truncate(8);
        format!("dcc-mcp-{}-{short}", self.dcc_type)
    }

    pub fn service_host_name(&self) -> String {
        normalise_local_host_name(&self.host_name)
    }

    pub fn txt_properties(&self) -> Vec<(String, String)> {
        let mut props = vec![
            (TXT_DCC.to_string(), self.dcc_type.clone()),
            (TXT_INSTANCE_ID.to_string(), self.instance_id.to_string()),
            (TXT_MCP_PATH.to_string(), self.mcp_path.clone()),
        ];
        if let Some(version) = &self.version {
            props.push((TXT_VERSION.to_string(), version.clone()));
        }
        if let Some(adapter) = &self.adapter {
            props.push((TXT_ADAPTER.to_string(), adapter.clone()));
        }
        if let Some(auth) = &self.auth {
            props.push((TXT_AUTH.to_string(), auth.clone()));
        }
        props
    }

    pub fn service_info(&self) -> Result<ServiceInfo, MdnsDiscoveryError> {
        let properties = self.txt_properties();
        let property_refs: Vec<(&str, &str)> = properties
            .iter()
            .map(|(key, value)| (key.as_str(), value.as_str()))
            .collect();
        ServiceInfo::new(
            DCC_MCP_SERVICE_TYPE,
            &self.instance_name(),
            &self.service_host_name(),
            "",
            self.port,
            &property_refs[..],
        )
        .map(ServiceInfo::enable_addr_auto)
        .map_err(|err| MdnsDiscoveryError::ServiceInfo(err.to_string()))
    }
}

pub struct MdnsAdvertiser {
    daemon: ServiceDaemon,
    fullname: String,
}

impl MdnsAdvertiser {
    pub fn start(advertisement: MdnsAdvertisement) -> Result<Self, MdnsDiscoveryError> {
        let daemon = ServiceDaemon::new()?;
        let service_info = advertisement.service_info()?;
        let fullname = service_info.get_fullname().to_string();
        daemon.register(service_info)?;
        Ok(Self { daemon, fullname })
    }

    pub fn fullname(&self) -> &str {
        &self.fullname
    }
}

impl Drop for MdnsAdvertiser {
    fn drop(&mut self) {
        let _ = self.daemon.unregister(&self.fullname);
        let _ = self.daemon.shutdown();
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MdnsBrowseEvent {
    Resolved(MdnsResolvedService),
    Removed {
        service_type: String,
        fullname: String,
    },
    Ignored,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MdnsResolvedService {
    pub fullname: String,
    pub host: String,
    pub port: u16,
    pub addresses: Vec<IpAddr>,
    pub txt: HashMap<String, String>,
}

impl MdnsResolvedService {
    pub fn txt_value(&self, key: &str) -> Option<&str> {
        self.txt.get(&key.to_ascii_lowercase()).map(String::as_str)
    }
}

impl From<&mdns_sd::ResolvedService> for MdnsResolvedService {
    fn from(value: &mdns_sd::ResolvedService) -> Self {
        let mut txt = HashMap::new();
        for prop in value.get_properties().iter() {
            txt.insert(prop.key().to_ascii_lowercase(), prop.val_str().to_string());
        }
        let mut addresses: Vec<IpAddr> = value
            .get_addresses()
            .iter()
            .map(mdns_sd::ScopedIp::to_ip_addr)
            .collect();
        addresses.sort_by_key(|ip| ip.to_string());
        Self {
            fullname: value.get_fullname().to_string(),
            host: value.get_hostname().trim_end_matches('.').to_string(),
            port: value.get_port(),
            addresses,
            txt,
        }
    }
}

pub struct MdnsBrowser {
    daemon: ServiceDaemon,
    receiver: Receiver<ServiceEvent>,
}

impl MdnsBrowser {
    pub fn start() -> Result<Self, MdnsDiscoveryError> {
        let daemon = ServiceDaemon::new()?;
        let receiver = daemon.browse(DCC_MCP_SERVICE_TYPE)?;
        Ok(Self { daemon, receiver })
    }

    pub async fn recv_async(&self) -> Option<MdnsBrowseEvent> {
        match self.receiver.recv_async().await.ok()? {
            ServiceEvent::ServiceResolved(service) => Some(MdnsBrowseEvent::Resolved(
                MdnsResolvedService::from(&*service),
            )),
            ServiceEvent::ServiceRemoved(service_type, fullname) => {
                Some(MdnsBrowseEvent::Removed {
                    service_type,
                    fullname,
                })
            }
            _ => Some(MdnsBrowseEvent::Ignored),
        }
    }
}

impl Drop for MdnsBrowser {
    fn drop(&mut self) {
        let _ = self.daemon.stop_browse(DCC_MCP_SERVICE_TYPE);
        let _ = self.daemon.shutdown();
    }
}

fn normalise_local_host_name(value: &str) -> String {
    let trimmed = value.trim().trim_end_matches('.');
    if trimmed.ends_with(".local") {
        format!("{trimmed}.")
    } else {
        format!("{trimmed}.local.")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advertisement_txt_matches_contract() {
        let id = Uuid::parse_str("11111111-1111-4111-8111-111111111111").unwrap();
        let props = MdnsAdvertisement::new("maya", id, "maya-host", 8765)
            .with_version(Some("0.17.37".to_string()))
            .with_adapter(Some("dcc-mcp-server".to_string()))
            .with_auth(Some("bearer".to_string()))
            .txt_properties();
        let map: HashMap<_, _> = props.into_iter().collect();

        assert_eq!(map[TXT_DCC], "maya");
        assert_eq!(map[TXT_INSTANCE_ID], id.to_string());
        assert_eq!(map[TXT_VERSION], "0.17.37");
        assert_eq!(map[TXT_MCP_PATH], "/mcp");
        assert_eq!(map[TXT_ADAPTER], "dcc-mcp-server");
        assert_eq!(map[TXT_AUTH], "bearer");
    }

    #[test]
    fn service_info_uses_dcc_mcp_service_type() {
        let id = Uuid::parse_str("22222222-2222-4222-8222-222222222222").unwrap();
        let info = MdnsAdvertisement::new("blender", id, "blender-host.local", 8877)
            .service_info()
            .unwrap();

        assert_eq!(info.get_type(), DCC_MCP_SERVICE_TYPE);
        assert_eq!(info.get_port(), 8877);
        assert_eq!(info.get_property_val_str(TXT_DCC), Some("blender"));
        assert_eq!(info.get_property_val_str(TXT_MCP_PATH), Some("/mcp"));
    }
}
