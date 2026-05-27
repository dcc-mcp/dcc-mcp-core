//! HTTP-backed gateway instance registration (#1361).
//!
//! This is the in-memory registration source used by remote DCC adapters that
//! cannot write the gateway's local [`FileRegistry`].  It deliberately stores
//! rows as `ServiceEntry` values so gateway routing, capability refresh, and
//! resources continue to consume one canonical instance contract.

use std::collections::HashMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use dcc_mcp_transport::discovery::types::{ServiceEntry, ServiceStatus};
use reqwest::Url;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

pub(crate) const DEFAULT_TTL_SECS: u64 = 30;
pub(crate) const MAX_TTL_SECS: u64 = 24 * 60 * 60;
pub(crate) const MCP_URL_METADATA_KEY: &str = "mcp_url";
pub(crate) const REGISTRY_SOURCE_METADATA_KEY: &str = "dcc_mcp_registry_source";
pub(crate) const SOURCE_FILE: &str = "file";
pub(crate) const SOURCE_HTTP: &str = "http";
const CAPABILITIES_FINGERPRINT_METADATA_KEY: &str = "capabilities_fingerprint";
const HTTP_TTL_METADATA_KEY: &str = "dcc_mcp_http_ttl_secs";

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct HttpInstanceRegistrationRequest {
    pub instance_id: String,
    pub dcc_type: String,
    pub mcp_url: String,
    pub capabilities_fingerprint: Option<String>,
    pub adapter_version: Option<String>,
    pub scene: Option<String>,
    pub ttl_secs: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct HttpInstanceHeartbeatRequest {
    pub instance_id: String,
    pub capabilities_fingerprint: Option<String>,
    pub scene: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct HttpInstanceDeregisterRequest {
    pub instance_id: String,
}

#[derive(Debug, Clone)]
pub(crate) struct RegistrationOutcome {
    pub entry: ServiceEntry,
    pub heartbeat_interval_secs: u64,
}

#[derive(Debug, Error)]
pub(crate) enum RegistrationError {
    #[error("invalid {field}: {message}")]
    InvalidField {
        field: &'static str,
        message: String,
    },
    #[error("registered instance not found: {instance_id}")]
    NotFound { instance_id: String },
}

#[derive(Debug, Default)]
pub struct HttpInstanceRegistry {
    entries: HashMap<Uuid, HttpRegisteredInstance>,
}

#[derive(Debug, Clone)]
struct HttpRegisteredInstance {
    entry: ServiceEntry,
    ttl: Duration,
}

impl HttpInstanceRegistry {
    pub(crate) fn register(
        &mut self,
        request: HttpInstanceRegistrationRequest,
        now: SystemTime,
    ) -> Result<RegistrationOutcome, RegistrationError> {
        let registered = HttpRegisteredInstance::from_request(request, now)?;
        let outcome = registered.outcome();
        self.entries.insert(outcome.entry.instance_id, registered);
        Ok(outcome)
    }

    pub(crate) fn heartbeat(
        &mut self,
        request: HttpInstanceHeartbeatRequest,
        now: SystemTime,
    ) -> Result<RegistrationOutcome, RegistrationError> {
        let instance_id = parse_instance_id(&request.instance_id)?;
        let Some(registered) = self.entries.get_mut(&instance_id) else {
            return Err(RegistrationError::NotFound {
                instance_id: request.instance_id,
            });
        };

        registered.entry.last_heartbeat = now;
        registered.entry.status = ServiceStatus::Available;
        if let Some(scene) = clean_optional_string(request.scene) {
            registered.entry.scene = Some(scene);
        }
        upsert_optional_metadata(
            &mut registered.entry,
            CAPABILITIES_FINGERPRINT_METADATA_KEY,
            request.capabilities_fingerprint,
        );
        Ok(registered.outcome())
    }

    pub(crate) fn deregister(
        &mut self,
        request: HttpInstanceDeregisterRequest,
    ) -> Result<Option<ServiceEntry>, RegistrationError> {
        let instance_id = parse_instance_id(&request.instance_id)?;
        Ok(self
            .entries
            .remove(&instance_id)
            .map(|registered| registered.entry))
    }

    pub(crate) fn live_entries(&self, now: SystemTime) -> Vec<ServiceEntry> {
        self.entries
            .values()
            .filter(|registered| !registered.is_expired(now))
            .map(|registered| registered.entry.clone())
            .collect()
    }

    pub(crate) fn all_entries(&self, now: SystemTime) -> Vec<ServiceEntry> {
        self.live_entries(now)
    }

    pub(crate) fn prune_expired(&mut self, now: SystemTime) -> Vec<Uuid> {
        let expired: Vec<Uuid> = self
            .entries
            .iter()
            .filter_map(|(id, registered)| registered.is_expired(now).then_some(*id))
            .collect();
        for id in &expired {
            self.entries.remove(id);
        }
        expired
    }
}

impl HttpRegisteredInstance {
    fn from_request(
        request: HttpInstanceRegistrationRequest,
        now: SystemTime,
    ) -> Result<Self, RegistrationError> {
        let instance_id = parse_instance_id(&request.instance_id)?;
        let dcc_type = clean_required_string("dcc_type", request.dcc_type)?;
        let ParsedMcpUrl {
            canonical,
            host,
            port,
        } = parse_mcp_url(&request.mcp_url)?;
        let ttl_secs = request
            .ttl_secs
            .unwrap_or(DEFAULT_TTL_SECS)
            .clamp(1, MAX_TTL_SECS);

        let mut entry = ServiceEntry::new(dcc_type, host, port);
        entry.instance_id = instance_id;
        entry.pid = None;
        entry.registered_at = now;
        entry.last_heartbeat = now;
        entry.status = ServiceStatus::Available;
        entry.adapter_version = clean_optional_string(request.adapter_version);
        entry.scene = clean_optional_string(request.scene);
        entry
            .metadata
            .insert(MCP_URL_METADATA_KEY.to_string(), canonical);
        entry.metadata.insert(
            REGISTRY_SOURCE_METADATA_KEY.to_string(),
            SOURCE_HTTP.to_string(),
        );
        entry
            .metadata
            .insert(HTTP_TTL_METADATA_KEY.to_string(), ttl_secs.to_string());
        upsert_optional_metadata(
            &mut entry,
            CAPABILITIES_FINGERPRINT_METADATA_KEY,
            request.capabilities_fingerprint,
        );

        Ok(Self {
            entry,
            ttl: Duration::from_secs(ttl_secs),
        })
    }

    fn outcome(&self) -> RegistrationOutcome {
        RegistrationOutcome {
            entry: self.entry.clone(),
            heartbeat_interval_secs: heartbeat_interval_secs(self.ttl),
        }
    }

    fn is_expired(&self, now: SystemTime) -> bool {
        now.duration_since(self.entry.last_heartbeat)
            .map(|age| age > self.ttl)
            .unwrap_or(false)
    }
}

struct ParsedMcpUrl {
    canonical: String,
    host: String,
    port: u16,
}

fn parse_instance_id(raw: &str) -> Result<Uuid, RegistrationError> {
    Uuid::parse_str(raw.trim()).map_err(|err| RegistrationError::InvalidField {
        field: "instance_id",
        message: err.to_string(),
    })
}

fn parse_mcp_url(raw: &str) -> Result<ParsedMcpUrl, RegistrationError> {
    let trimmed = raw.trim();
    let url = Url::parse(trimmed).map_err(|err| RegistrationError::InvalidField {
        field: "mcp_url",
        message: err.to_string(),
    })?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(RegistrationError::InvalidField {
            field: "mcp_url",
            message: "scheme must be http or https".to_string(),
        });
    }
    if !url.path().trim_end_matches('/').ends_with("/mcp") {
        return Err(RegistrationError::InvalidField {
            field: "mcp_url",
            message: "path must point at the MCP endpoint and end with /mcp".to_string(),
        });
    }
    let host = url
        .host_str()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| RegistrationError::InvalidField {
            field: "mcp_url",
            message: "missing host".to_string(),
        })?
        .to_string();
    let port = url
        .port_or_known_default()
        .ok_or_else(|| RegistrationError::InvalidField {
            field: "mcp_url",
            message: "missing port and unknown default for scheme".to_string(),
        })?;
    Ok(ParsedMcpUrl {
        canonical: url.to_string(),
        host,
        port,
    })
}

fn clean_required_string(field: &'static str, value: String) -> Result<String, RegistrationError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        Err(RegistrationError::InvalidField {
            field,
            message: "must not be empty".to_string(),
        })
    } else {
        Ok(trimmed.to_string())
    }
}

fn clean_optional_string(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn upsert_optional_metadata(entry: &mut ServiceEntry, key: &str, value: Option<String>) {
    if let Some(value) = clean_optional_string(value) {
        entry.metadata.insert(key.to_string(), value);
    }
}

fn heartbeat_interval_secs(ttl: Duration) -> u64 {
    (ttl.as_secs() / 3).max(1)
}

pub(crate) fn entry_mcp_url(entry: &ServiceEntry) -> String {
    entry
        .metadata
        .get(MCP_URL_METADATA_KEY)
        .map(String::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| format!("http://{}:{}/mcp", entry.host, entry.port))
}

pub(crate) fn entry_registry_source(entry: &ServiceEntry) -> &str {
    entry
        .metadata
        .get(REGISTRY_SOURCE_METADATA_KEY)
        .map(String::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(SOURCE_FILE)
}

pub(crate) fn unix_secs(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_builds_service_entry_with_exact_mcp_url() {
        let mut registry = HttpInstanceRegistry::default();
        let now = UNIX_EPOCH + Duration::from_secs(123);
        let outcome = registry
            .register(
                HttpInstanceRegistrationRequest {
                    instance_id: "11111111-1111-4111-8111-111111111111".to_string(),
                    dcc_type: " maya ".to_string(),
                    mcp_url: "https://remote.example:9443/prefix/mcp".to_string(),
                    capabilities_fingerprint: Some("abc".to_string()),
                    adapter_version: Some("1.2.3".to_string()),
                    scene: Some("shot.ma".to_string()),
                    ttl_secs: Some(90),
                },
                now,
            )
            .unwrap();

        assert_eq!(outcome.entry.dcc_type, "maya");
        assert_eq!(outcome.entry.host, "remote.example");
        assert_eq!(outcome.entry.port, 9443);
        assert_eq!(
            entry_mcp_url(&outcome.entry),
            "https://remote.example:9443/prefix/mcp"
        );
        assert_eq!(entry_registry_source(&outcome.entry), SOURCE_HTTP);
        assert_eq!(outcome.heartbeat_interval_secs, 30);
        assert_eq!(outcome.entry.pid, None);
    }

    #[test]
    fn prunes_expired_http_rows() {
        let mut registry = HttpInstanceRegistry::default();
        let now = UNIX_EPOCH + Duration::from_secs(100);
        let id = "22222222-2222-4222-8222-222222222222";
        registry
            .register(
                HttpInstanceRegistrationRequest {
                    instance_id: id.to_string(),
                    dcc_type: "houdini".to_string(),
                    mcp_url: "http://127.0.0.1:8765/mcp".to_string(),
                    capabilities_fingerprint: None,
                    adapter_version: None,
                    scene: None,
                    ttl_secs: Some(2),
                },
                now,
            )
            .unwrap();

        assert_eq!(registry.live_entries(now + Duration::from_secs(2)).len(), 1);
        let expired = registry.prune_expired(now + Duration::from_secs(3));
        assert_eq!(expired, vec![Uuid::parse_str(id).unwrap()]);
        assert!(
            registry
                .live_entries(now + Duration::from_secs(3))
                .is_empty()
        );
    }

    #[test]
    fn rejects_non_mcp_url() {
        let mut registry = HttpInstanceRegistry::default();
        let err = registry
            .register(
                HttpInstanceRegistrationRequest {
                    instance_id: "33333333-3333-4333-8333-333333333333".to_string(),
                    dcc_type: "maya".to_string(),
                    mcp_url: "http://127.0.0.1:8765/v1/search".to_string(),
                    capabilities_fingerprint: None,
                    adapter_version: None,
                    scene: None,
                    ttl_secs: None,
                },
                SystemTime::now(),
            )
            .unwrap_err();

        assert!(err.to_string().contains("end with /mcp"));
    }
}
