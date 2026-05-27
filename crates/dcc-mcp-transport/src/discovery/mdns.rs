//! mDNS / DNS-SD helpers for DCC-MCP LAN discovery (#1362).
//!
//! This module is deliberately feature-gated. FileRegistry remains the default
//! discovery path; mDNS only provides advisory addresses that higher layers
//! must probe and authenticate before trusting.

use std::collections::HashMap;
use std::net::IpAddr;
use std::time::Duration;

use mdns_sd::{ResolvedService, ServiceDaemon, ServiceInfo};
use uuid::Uuid;

use crate::error::{TransportError, TransportResult};
use crate::ipc::TransportAddress;

use super::types::{ServiceEntry, ServiceStatus};

/// DNS-SD service type used by DCC-MCP instances on a LAN.
pub const DCC_MCP_MDNS_SERVICE_TYPE: &str = "_dcc-mcp._tcp.local.";
/// Registry source metadata value for mDNS-discovered rows.
pub const DCC_MCP_MDNS_SOURCE: &str = "mdns";
/// Metadata key storing the registry source (`file`, `http`, `mdns`, ...).
pub const REGISTRY_SOURCE_METADATA_KEY: &str = "dcc_mcp_registry_source";
/// Metadata key storing the MCP endpoint URL derived from the resolved service.
pub const MCP_URL_METADATA_KEY: &str = "mcp_url";

const DEFAULT_MCP_PATH: &str = "/mcp";
const DEFAULT_AUTH_SCHEME: &str = "none";

/// Input used when advertising a DCC-MCP HTTP endpoint over DNS-SD.
#[derive(Debug, Clone)]
pub struct MdnsAdvertisement {
    pub dcc_type: String,
    pub instance_id: Uuid,
    pub host: String,
    pub port: u16,
    pub mcp_path: String,
    pub version: String,
    pub adapter: String,
    pub auth_scheme: String,
}

impl MdnsAdvertisement {
    /// Build an advertisement with the canonical DCC-MCP TXT keys.
    #[must_use]
    pub fn new(
        dcc_type: impl Into<String>,
        instance_id: Uuid,
        host: impl Into<String>,
        port: u16,
        version: impl Into<String>,
        adapter: impl Into<String>,
    ) -> Self {
        Self {
            dcc_type: dcc_type.into(),
            instance_id,
            host: host.into(),
            port,
            mcp_path: DEFAULT_MCP_PATH.to_string(),
            version: version.into(),
            adapter: adapter.into(),
            auth_scheme: DEFAULT_AUTH_SCHEME.to_string(),
        }
    }

    /// Override the advertised MCP path. The path is normalised to start with `/`.
    #[must_use]
    pub fn with_mcp_path(mut self, path: impl Into<String>) -> Self {
        let raw = path.into();
        self.mcp_path = if raw.starts_with('/') {
            raw
        } else {
            format!("/{raw}")
        };
        self
    }

    /// Override the advertised auth scheme (`none`, `bearer`, `oauth`, ...).
    #[must_use]
    pub fn with_auth_scheme(mut self, scheme: impl Into<String>) -> Self {
        self.auth_scheme = scheme.into();
        self
    }
}

/// RAII handle for an advertised DCC-MCP DNS-SD service.
pub struct MdnsAdvertiser {
    daemon: ServiceDaemon,
    fullname: String,
}

impl MdnsAdvertiser {
    /// Advertise a DCC-MCP service until this handle is dropped.
    pub fn start(ad: MdnsAdvertisement) -> TransportResult<Self> {
        let daemon = ServiceDaemon::new()
            .map_err(|err| TransportError::Internal(format!("creating mDNS daemon: {err}")))?;
        let info = build_service_info(&ad)?;
        let fullname = info.get_fullname().to_string();
        daemon
            .register(info)
            .map_err(|err| TransportError::Internal(format!("registering mDNS service: {err}")))?;
        Ok(Self { daemon, fullname })
    }

    /// Fully-qualified DNS-SD service name.
    #[must_use]
    pub fn fullname(&self) -> &str {
        &self.fullname
    }
}

impl Drop for MdnsAdvertiser {
    fn drop(&mut self) {
        if let Ok(rx) = self.daemon.unregister(&self.fullname) {
            let _ = rx.recv_timeout(Duration::from_millis(500));
        }
        if let Ok(rx) = self.daemon.shutdown() {
            let _ = rx.recv_timeout(Duration::from_millis(500));
        }
    }
}

/// Build a DNS-SD service record from DCC-MCP metadata.
pub fn build_service_info(ad: &MdnsAdvertisement) -> TransportResult<ServiceInfo> {
    if ad.dcc_type.trim().is_empty() {
        return Err(TransportError::Internal(
            "mDNS advertisement requires non-empty dcc_type".to_string(),
        ));
    }
    if ad.port == 0 {
        return Err(TransportError::Internal(
            "mDNS advertisement requires a bound MCP port".to_string(),
        ));
    }

    let instance_name = format!(
        "dcc-mcp-{}-{}",
        dns_label_fragment(&ad.dcc_type),
        instance_short(&ad.instance_id)
    );
    let host_name = format!("{instance_name}.local.");
    let properties = [
        ("dcc", ad.dcc_type.as_str()),
        ("instance_id", &ad.instance_id.to_string()),
        ("version", ad.version.as_str()),
        ("mcp_path", ad.mcp_path.as_str()),
        ("adapter", ad.adapter.as_str()),
        ("auth", ad.auth_scheme.as_str()),
    ];

    let host = ad.host.trim();
    let mut info = if is_unspecified_host(host) {
        ServiceInfo::new(
            DCC_MCP_MDNS_SERVICE_TYPE,
            &instance_name,
            &host_name,
            (),
            ad.port,
            &properties[..],
        )
    } else {
        ServiceInfo::new(
            DCC_MCP_MDNS_SERVICE_TYPE,
            &instance_name,
            &host_name,
            host,
            ad.port,
            &properties[..],
        )
    }
    .map_err(|err| TransportError::Internal(format!("building mDNS service info: {err}")))?;

    if is_unspecified_host(host) {
        info = info.enable_addr_auto();
    }
    Ok(info)
}

/// Convert a resolved DNS-SD service into a ServiceEntry.
///
/// The returned row is still advisory. Gateway callers must probe the derived
/// endpoint and apply auth policy before making it routable.
pub fn entry_from_resolved_service(resolved: &ResolvedService) -> TransportResult<ServiceEntry> {
    let dcc_type = required_txt(resolved, "dcc")?;
    let instance_id = required_txt(resolved, "instance_id")?;
    let instance_id = Uuid::parse_str(instance_id).map_err(|err| {
        TransportError::Internal(format!("mDNS service has invalid instance_id: {err}"))
    })?;
    let mcp_path = resolved
        .get_property_val_str("mcp_path")
        .filter(|path| !path.trim().is_empty())
        .unwrap_or(DEFAULT_MCP_PATH);
    let mcp_path = if mcp_path.starts_with('/') {
        mcp_path.to_string()
    } else {
        format!("/{mcp_path}")
    };
    let address = choose_address(resolved).ok_or_else(|| {
        TransportError::Internal("mDNS service resolved without usable IP address".to_string())
    })?;
    let host = address.to_string();
    let mcp_url = format!(
        "http://{}:{}{}",
        url_host(&address),
        resolved.port,
        mcp_path
    );

    let mut entry = ServiceEntry::new(dcc_type.to_string(), &host, resolved.port);
    entry.instance_id = instance_id;
    entry.pid = None;
    entry.transport_address = Some(TransportAddress::tcp(&host, resolved.port));
    entry.status = ServiceStatus::Available;
    entry.adapter_version = resolved
        .get_property_val_str("version")
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string);
    entry.display_name = Some(resolved.fullname.clone());
    entry.metadata = metadata_from_resolved(resolved, &mcp_url, &mcp_path);
    Ok(entry)
}

fn metadata_from_resolved(
    resolved: &ResolvedService,
    mcp_url: &str,
    mcp_path: &str,
) -> HashMap<String, String> {
    let mut metadata = HashMap::new();
    metadata.insert(
        REGISTRY_SOURCE_METADATA_KEY.to_string(),
        DCC_MCP_MDNS_SOURCE.to_string(),
    );
    metadata.insert(MCP_URL_METADATA_KEY.to_string(), mcp_url.to_string());
    metadata.insert("mdns_fullname".to_string(), resolved.fullname.clone());
    metadata.insert("mdns_service_type".to_string(), resolved.ty_domain.clone());
    metadata.insert("mdns_host".to_string(), resolved.host.clone());
    metadata.insert("mdns_mcp_path".to_string(), mcp_path.to_string());

    for key in ["auth", "adapter", "version"] {
        if let Some(value) = resolved
            .get_property_val_str(key)
            .filter(|value| !value.trim().is_empty())
        {
            metadata.insert(format!("mdns_{key}"), value.to_string());
        }
    }
    metadata
}

fn required_txt<'a>(resolved: &'a ResolvedService, key: &str) -> TransportResult<&'a str> {
    resolved
        .get_property_val_str(key)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| TransportError::Internal(format!("mDNS service missing TXT field '{key}'")))
}

fn choose_address(resolved: &ResolvedService) -> Option<IpAddr> {
    let mut addresses: Vec<IpAddr> = resolved
        .addresses
        .iter()
        .map(|scoped| scoped.to_ip_addr())
        .collect();
    addresses.sort_by_key(|ip| {
        (
            !matches!(ip, IpAddr::V4(_)),
            ip.is_loopback(),
            ip.to_string(),
        )
    });
    addresses.into_iter().next()
}

fn url_host(ip: &IpAddr) -> String {
    match ip {
        IpAddr::V4(ip) => ip.to_string(),
        IpAddr::V6(ip) => format!("[{ip}]"),
    }
}

fn is_unspecified_host(host: &str) -> bool {
    host.is_empty()
        || host == "0.0.0.0"
        || host == "::"
        || host.parse::<IpAddr>().is_ok_and(|ip| ip.is_unspecified())
}

fn dns_label_fragment(value: &str) -> String {
    let mut out = String::new();
    let mut previous_dash = false;
    for ch in value.chars().flat_map(char::to_lowercase) {
        let valid = ch.is_ascii_alphanumeric();
        if valid {
            out.push(ch);
            previous_dash = false;
        } else if !previous_dash {
            out.push('-');
            previous_dash = true;
        }
        if out.len() >= 32 {
            break;
        }
    }
    let trimmed = out.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "dcc".to_string()
    } else {
        trimmed
    }
}

fn instance_short(instance_id: &Uuid) -> String {
    instance_id.simple().to_string()[..8].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};

    #[test]
    fn service_info_contains_required_txt_fields() {
        let instance_id = Uuid::parse_str("11111111-2222-3333-4444-555555555555").unwrap();
        let ad = MdnsAdvertisement::new(
            "maya",
            instance_id,
            "127.0.0.1",
            8765,
            "0.17.37",
            "dcc-mcp-server",
        );

        let info = build_service_info(&ad).unwrap();

        assert_eq!(info.get_type(), DCC_MCP_MDNS_SERVICE_TYPE);
        assert!(info.get_fullname().starts_with("dcc-mcp-maya-11111111."));
        assert_eq!(info.get_port(), 8765);
        assert_eq!(info.get_property_val_str("dcc"), Some("maya"));
        assert_eq!(
            info.get_property_val_str("instance_id"),
            Some(instance_id.to_string().as_str())
        );
        assert_eq!(info.get_property_val_str("mcp_path"), Some("/mcp"));
        assert_eq!(info.get_property_val_str("auth"), Some("none"));
    }

    #[test]
    fn wildcard_host_enables_auto_addresses() {
        let ad = MdnsAdvertisement::new(
            "houdini",
            Uuid::new_v4(),
            "0.0.0.0",
            8765,
            "0.17.37",
            "dcc-mcp-server",
        );

        let info = build_service_info(&ad).unwrap();

        assert!(info.is_addr_auto());
        assert!(info.get_addresses().is_empty());
    }

    #[test]
    fn resolved_service_becomes_advisory_service_entry() {
        let instance_id = Uuid::parse_str("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee").unwrap();
        let instance_id_string = instance_id.to_string();
        let properties = [
            ("dcc", "photoshop"),
            ("instance_id", instance_id_string.as_str()),
            ("version", "0.17.37"),
            ("mcp_path", "/mcp"),
            ("adapter", "dcc-mcp-server"),
            ("auth", "none"),
        ];
        let info = ServiceInfo::new(
            DCC_MCP_MDNS_SERVICE_TYPE,
            "dcc-mcp-photoshop-aaaaaaaa",
            "dcc-mcp-photoshop-aaaaaaaa.local.",
            IpAddr::V4(Ipv4Addr::new(192, 168, 1, 42)),
            8765,
            &properties[..],
        )
        .unwrap();

        let entry = entry_from_resolved_service(&info.as_resolved_service()).unwrap();

        assert_eq!(entry.dcc_type, "photoshop");
        assert_eq!(entry.instance_id, instance_id);
        assert_eq!(entry.pid, None);
        assert_eq!(entry.host, "192.168.1.42");
        assert_eq!(entry.metadata[REGISTRY_SOURCE_METADATA_KEY], "mdns");
        assert_eq!(
            entry.metadata[MCP_URL_METADATA_KEY],
            "http://192.168.1.42:8765/mcp"
        );
        assert_eq!(entry.adapter_version.as_deref(), Some("0.17.37"));
    }
}
