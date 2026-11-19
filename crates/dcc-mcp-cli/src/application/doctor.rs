//! Diagnostic snapshot for `dcc-mcp-cli doctor`.
//!
//! Doctor is the read-only, no-launch view of the control plane: profile
//! selection, local registry inventory, daemon health, and gateway binary
//! discovery state.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde_json::{Map, Value, json};

use crate::application::adapter_import::{self, AdapterImportRequest, probe_adapters};
use crate::application::gateway_profile::{GatewayProfileStore, GatewayTarget};
use crate::application::{gateway_ctrl, gateway_discovery, gateway_ensure, local_registry};
use crate::domain::rest::Endpoint;
use dcc_mcp_transport::discovery::file_registry::registry_write_diagnostics;
use crate::infra::http::HttpGateway;

#[derive(Debug, Clone)]
pub struct DoctorContext {
    profile_path: PathBuf,
    profile_store: GatewayProfileStore,
    gateway_target: GatewayTarget,
    server_bin: Option<PathBuf>,
    auto_gateway_enabled: bool,
    require_gateway: bool,
    default_gateway_host: String,
    default_gateway_port: u16,
}

impl DoctorContext {
    pub fn new(
        profile_path: PathBuf,
        profile_store: GatewayProfileStore,
        gateway_target: GatewayTarget,
        server_bin: Option<PathBuf>,
        auto_gateway_enabled: bool,
        require_gateway: bool,
        endpoint: &Endpoint,
    ) -> anyhow::Result<Self> {
        let url = reqwest::Url::parse(&endpoint.base_url)?;
        let default_gateway_host = url
            .host_str()
            .ok_or_else(|| anyhow::anyhow!("gateway endpoint has no host"))?
            .to_string();
        let default_gateway_port = url
            .port_or_known_default()
            .ok_or_else(|| anyhow::anyhow!("gateway endpoint has no port"))?;
        Ok(Self {
            profile_path,
            profile_store,
            gateway_target,
            server_bin,
            auto_gateway_enabled,
            require_gateway,
            default_gateway_host,
            default_gateway_port,
        })
    }

    #[must_use]
    pub fn request(
        &self,
        registry_dir: Option<PathBuf>,
        gateway_host: Option<String>,
        gateway_port: Option<u16>,
    ) -> DoctorRequest {
        DoctorRequest {
            profile_path: self.profile_path.clone(),
            profile_store: self.profile_store.clone(),
            gateway_target: self.gateway_target.clone(),
            registry_dir,
            server_bin: self.server_bin.clone(),
            auto_gateway_enabled: self.auto_gateway_enabled,
            require_gateway: self.require_gateway,
            gateway_host: gateway_host.unwrap_or_else(|| self.default_gateway_host.clone()),
            gateway_port: gateway_port.unwrap_or(self.default_gateway_port),
            adapter_python: BTreeMap::new(),
            adapter_catalog: None,
            gateway_instances: None,
        }
    }

    /// Attach adapter import probe configuration to a request.
    #[must_use]
    pub fn request_with_adapter_probes(
        &self,
        registry_dir: Option<PathBuf>,
        gateway_host: Option<String>,
        gateway_port: Option<u16>,
        adapter_python: BTreeMap<String, PathBuf>,
        adapter_catalog: Option<PathBuf>,
    ) -> DoctorRequest {
        let mut request = self.request(registry_dir, gateway_host, gateway_port);
        request.adapter_python = adapter_python;
        request.adapter_catalog = adapter_catalog;
        request
    }
}

#[derive(Debug, Clone)]
pub struct DoctorRequest {
    pub profile_path: PathBuf,
    pub profile_store: GatewayProfileStore,
    pub gateway_target: GatewayTarget,
    pub registry_dir: Option<PathBuf>,
    pub server_bin: Option<PathBuf>,
    pub auto_gateway_enabled: bool,
    pub require_gateway: bool,
    pub gateway_host: String,
    pub gateway_port: u16,
    /// Explicit `<dcc_type>=<python>` interpreter overrides for adapter probes.
    pub adapter_python: BTreeMap<String, PathBuf>,
    /// Explicit adapter catalog path; `None` uses the bundled release catalog.
    pub adapter_catalog: Option<PathBuf>,
    /// Optional instance inventory from a live gateway. The diagnostic path
    /// keeps its no-launch contract, so callers pass a best-effort snapshot.
    pub gateway_instances: Option<Value>,
}

pub async fn run_doctor(request: DoctorRequest) -> anyhow::Result<Value> {
    let direct_local = request.gateway_target.is_local() && !request.require_gateway;
    let registry_dir = request
        .registry_dir
        .clone()
        .map(Ok)
        .unwrap_or_else(gateway_ensure::default_registry_dir)?;
    let pidfile = gateway_ctrl::default_pidfile(&registry_dir);
    let gateway_status = gateway_ctrl::gateway_status_readonly(&gateway_ctrl::GatewayCtrlArgs {
        host: request.gateway_host.clone(),
        port: request.gateway_port,
        registry_dir: registry_dir.clone(),
        pidfile,
        start_opts: None,
    })
    .await;
    let inventory = local_registry::snapshot_local_instances(registry_dir.clone());
    let gateway_instances = match &request.gateway_instances {
        Some(value) => Some(value.clone()),
        None => fetch_gateway_instances(&request.gateway_base_url()).await,
    };
    let registry_write_diagnostics = registry_write_diagnostics(&registry_dir);
    let adapter_imports = probe_adapters(&AdapterImportRequest {
        inventory: inventory.as_ref().ok().cloned(),
        python_overrides: request.adapter_python.clone(),
        catalog: request.adapter_catalog.clone(),
    });
    let local_inventory = match &inventory {
        Ok(value) => {
            let direct_control = direct_control_summary(value);
            let capabilities = gateway_instances
                .as_ref()
                .and_then(|payload| payload.get("instances").and_then(Value::as_array))
                .map(|instances| capability_summary(instances))
                .unwrap_or_else(|| {
                    capability_summary(value["instances"].as_array().map_or(&[], |v| v))
                });
            json!({
                "ok": true,
                "mode": "read_only_snapshot",
                "liveness": "not_checked",
                "source": value.get("source").cloned().unwrap_or(Value::Null),
                "total": value.get("total").cloned().unwrap_or(Value::Null),
                "evicted": value.get("evicted").cloned().unwrap_or(Value::Null),
                "registry_dir": value.get("registry_dir").cloned().unwrap_or_else(|| json!(registry_dir.clone())),
                "direct_control": direct_control,
                "capabilities": capabilities,
            })
        }
        Err(err) => inventory_error(err, &registry_dir),
    };

    Ok(json!({
        "status": if adapter_import::has_failures(&adapter_imports) {
            "degraded"
        } else {
            "ok"
        },
        "cli": {
            "name": env!("CARGO_PKG_NAME"),
            "version": env!("CARGO_PKG_VERSION"),
        },
        "profile": request.profile_store.summary(
            &request.profile_path,
            Some(&request.gateway_target),
        ),
        "control": {
            "route": if direct_local { "local_mcp_direct" } else { "gateway" },
            "gateway_stats_recorded": !direct_local,
            "stats_hint": if direct_local {
                "Use --require-gateway and _meta.agent_context.session_id when gateway stats are required evidence."
            } else {
                "Gateway-routed calls can be attributed with _meta.agent_context.session_id."
            },
        },
        "local": {
            "registry_dir": registry_dir,
            "inventory": local_inventory,
            "registry_write_diagnostics": registry_write_diagnostics,
        },
        "adapter_imports": adapter_imports,
        "gateway": {
            "auto_start_enabled": request.auto_gateway_enabled,
            "default_base_url": format!(
                "http://{}:{}",
                request.gateway_host,
                request.gateway_port,
            ),
            "capability_inventory_source": gateway_instances
                .as_ref()
                .map(|_| "gateway_v1_instances")
                .unwrap_or("unavailable"),
            "status": gateway_status,
        },
        "server_binary": gateway_discovery::diagnose_gateway_bin(request.server_bin.as_ref()),
    }))
}

impl DoctorRequest {
    fn gateway_base_url(&self) -> String {
        format!("http://{}:{}", self.gateway_host, self.gateway_port)
    }
}

async fn fetch_gateway_instances(base_url: &str) -> Option<Value> {
    let endpoint = Endpoint::new(base_url);
    HttpGateway::with_timeout(std::time::Duration::from_secs(2))
        .get_json(&endpoint.path("/v1/instances"))
        .await
        .ok()
}

fn inventory_error(error: &anyhow::Error, registry_dir: &std::path::Path) -> Value {
    let io = error
        .chain()
        .find_map(|cause| cause.downcast_ref::<std::io::Error>());
    let home = dirs::home_dir().map(|path| path.to_string_lossy().into_owned());
    let redact = |text: String| {
        let text = match &home {
            Some(home) if !home.is_empty() => text.replace(home, "<home>"),
            _ => text,
        };
        text.chars()
            .filter(|c| !c.is_control())
            .take(2048)
            .collect::<String>()
    };
    json!({
        "ok": false,
        "mode": "read_only_snapshot",
        "liveness": "unknown",
        "registry_dir": registry_dir,
        "error": redact(error.chain().take(8).map(|cause| cause.to_string().chars().take(240).collect::<String>()).collect::<Vec<_>>().join(": ")),
        "operation": "read_registry_snapshot",
        "path": redact(registry_dir.join("services.json").display().to_string()),
        "error_kind": io.map(|error| format!("{:?}", error.kind())).unwrap_or_else(|| "unknown".to_string()),
        "os_error": io.and_then(std::io::Error::raw_os_error),
    })
}

#[cfg(test)]
mod diagnostic_tests {
    use super::*;

    #[test]
    fn inventory_error_retains_os_cause_and_bounds_redacts_context() {
        let code = if cfg!(windows) { 5 } else { 13 };
        let path = dirs::home_dir().unwrap().join("registry");
        let error = anyhow::Error::new(std::io::Error::from_raw_os_error(code)).context(format!(
            "reading {}\n{}",
            path.display(),
            "x".repeat(4096)
        ));
        let report = inventory_error(&error, &path);
        assert_eq!(report["os_error"], code);
        assert_eq!(report["error_kind"], "PermissionDenied");
        assert_eq!(report["operation"], "read_registry_snapshot");
        assert!(report["path"].as_str().unwrap().starts_with("<home>"));
        assert!(report["error"].as_str().unwrap().chars().count() <= 2048);
        assert!(!report["error"].as_str().unwrap().contains('\n'));
        let short = inventory_error(
            &anyhow::Error::new(std::io::Error::from_raw_os_error(code))
                .context("reading snapshot"),
            &path,
        );
        assert!(
            short["error"]
                .as_str()
                .unwrap()
                .contains(&format!("os error {code}"))
        );
    }

    #[test]
    fn inventory_error_does_not_invent_an_errno() {
        let report = inventory_error(
            &anyhow::anyhow!("invalid registry"),
            std::path::Path::new("fixture"),
        );
        assert_eq!(report["error_kind"], "unknown");
        assert_eq!(report["os_error"], Value::Null);
        assert_eq!(report["liveness"], "unknown");
    }

    #[test]
    fn capability_summary_counts_only_indexed_registry_rows() {
        let inventory = json!({
            "instances": [
                {
                    "instance_id": "known-1",
                    "capabilities": {
                        "source": "capability_index",
                        "loaded_skill_count": 2,
                        "tool_count": 5
                    }
                },
                {
                    "instance_id": "known-2",
                    "capabilities": {
                        "source": "capability_index",
                        "loaded_skill_count": 0,
                        "tool_count": 0
                    }
                },
                {"instance_id": "legacy-1"}
            ]
        });
        let instances = inventory["instances"].as_array().unwrap();

        let summary = capability_summary(instances);

        assert_eq!(summary["reported"], 2);
        assert_eq!(summary["with_loaded_skills"], 1);
        assert_eq!(summary["with_tools"], 1);
        assert_eq!(summary["total_loaded_skills"], 2);
        assert_eq!(summary["total_tool_count"], 5);
        assert_eq!(summary["unknown_instances"], json!(["legacy-1"]));
    }

    #[test]
    fn direct_control_summary_includes_capability_projection() {
        let summary = direct_control_summary(&json!({"instances": []}));
        assert_eq!(summary["capabilities"]["reported"], 0);
        assert_eq!(summary["capabilities"]["unknown_instances"], json!([]));
    }

    #[tokio::test]
    async fn run_doctor_uses_gateway_instance_inventory_for_capabilities() {
        let dir = tempfile::tempdir().unwrap();
        let profiles = dir.path().join("profiles.json");
        let gateway_instances = json!({
            "instances": [
                {
                    "instance_id": "maya-1",
                    "capabilities": {
                        "source": "capability_index",
                        "loaded_skills": ["maya-modeling"],
                        "loaded_skill_count": 1,
                        "tool_count": 7
                    }
                }
            ]
        });
        let mut request = DoctorContext::new(
            profiles.clone(),
            GatewayProfileStore::load(&profiles).unwrap(),
            GatewayTarget::Local,
            None,
            false,
            false,
            &Endpoint::new("http://127.0.0.1:9765"),
        )
        .unwrap()
        .request(Some(dir.path().to_path_buf()), None, None);
        request.gateway_instances = Some(gateway_instances);

        let report = run_doctor(request).await.unwrap();

        assert_eq!(report["local"]["inventory"]["capabilities"]["reported"], 1);
        assert_eq!(
            report["local"]["inventory"]["capabilities"]["total_loaded_skills"],
            1
        );
        assert_eq!(
            report["local"]["inventory"]["capabilities"]["total_tool_count"],
            7
        );
        assert_eq!(
            report["gateway"]["capability_inventory_source"],
            "gateway_v1_instances"
        );
    }
}
fn direct_control_summary(inventory: &Value) -> Value {
    let Some(instances) = inventory.get("instances").and_then(Value::as_array) else {
        return json!({
            "ready": 0,
            "not_ready": 0,
            "reasons": {},
            "capabilities": capability_summary(&[]),
        });
    };

    let mut ready = 0_usize;
    let mut not_ready = 0_usize;
    let mut reasons = Map::new();
    let mut not_ready_instances = Vec::new();
    for instance in instances {
        let direct = instance.get("direct_control").unwrap_or(&Value::Null);
        if direct
            .get("ready")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            ready += 1;
            continue;
        }
        not_ready += 1;
        let reason = direct
            .get("reason")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        let current = reasons.get(reason).and_then(Value::as_u64).unwrap_or(0);
        reasons.insert(reason.to_string(), json!(current + 1));
        not_ready_instances.push(not_ready_instance_summary(instance, direct));
    }

    json!({
        "ready": ready,
        "not_ready": not_ready,
        "reasons": reasons,
        "not_ready_instances": not_ready_instances,
        "capabilities": capability_summary(instances),
    })
}

fn capability_summary(instances: &[Value]) -> Value {
    let mut reported = 0_usize;
    let mut with_loaded_skills = 0_usize;
    let mut with_tools = 0_usize;
    let mut total_loaded_skills = 0_usize;
    let mut total_tool_count = 0_usize;
    let mut unknown = Vec::new();

    for instance in instances {
        let Some(capabilities) = instance
            .get("capabilities")
            .filter(|value| value.is_object())
        else {
            if let Some(instance_id) = instance.get("instance_id").and_then(Value::as_str) {
                unknown.push(instance_id.to_owned());
            }
            continue;
        };
        if capabilities
            .get("source")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            != "capability_index"
        {
            if let Some(instance_id) = instance.get("instance_id").and_then(Value::as_str) {
                unknown.push(instance_id.to_owned());
            }
            continue;
        }

        reported += 1;
        let loaded_skill_count = capabilities
            .get("loaded_skill_count")
            .and_then(Value::as_u64)
            .unwrap_or(0) as usize;
        let tool_count = capabilities
            .get("tool_count")
            .and_then(Value::as_u64)
            .unwrap_or(0) as usize;
        total_loaded_skills += loaded_skill_count;
        total_tool_count += tool_count;
        if loaded_skill_count > 0 {
            with_loaded_skills += 1;
        }
        if tool_count > 0 {
            with_tools += 1;
        }
    }

    json!({
        "reported": reported,
        "with_loaded_skills": with_loaded_skills,
        "with_tools": with_tools,
        "total_loaded_skills": total_loaded_skills,
        "total_tool_count": total_tool_count,
        "unknown_instances": unknown,
    })
}

fn not_ready_instance_summary(instance: &Value, direct: &Value) -> Value {
    json!({
        "dcc_type": instance.get("dcc_type").cloned().unwrap_or(Value::Null),
        "instance_id": instance.get("instance_id").cloned().unwrap_or(Value::Null),
        "instance_short": instance.get("instance_short").cloned().unwrap_or(Value::Null),
        "display_name": instance.get("display_name").cloned().unwrap_or(Value::Null),
        "mcp_url": instance.get("mcp_url").cloned().unwrap_or(Value::Null),
        "reason": direct.get("reason").cloned().unwrap_or(Value::Null),
        "service_status": direct.get("service_status").cloned().unwrap_or(Value::Null),
        "dispatch_status": direct.get("dispatch_status").cloned().unwrap_or(Value::Null),
        "role": direct.get("role").cloned().unwrap_or(Value::Null),
        "diagnostics": direct.get("diagnostics").cloned().unwrap_or(Value::Null),
        "recommended_next_action": direct
            .get("recommended_next_action")
            .cloned()
            .unwrap_or(Value::Null),
    })
}

#[cfg(test)]
mod diagnostic_tests {
    use super::*;

    #[test]
    fn inventory_error_retains_os_cause_and_bounds_redacts_context() {
        let code = if cfg!(windows) { 5 } else { 13 };
        let path = dirs::home_dir().unwrap().join("registry");
        let error = anyhow::Error::new(std::io::Error::from_raw_os_error(code)).context(format!(
            "reading {}\n{}",
            path.display(),
            "x".repeat(4096)
        ));
        let report = inventory_error(&error, &path);
        assert_eq!(report["os_error"], code);
        assert_eq!(report["error_kind"], "PermissionDenied");
        assert_eq!(report["operation"], "read_registry_snapshot");
        assert!(report["path"].as_str().unwrap().starts_with("<home>"));
        assert!(report["error"].as_str().unwrap().chars().count() <= 2048);
        assert!(!report["error"].as_str().unwrap().contains('\n'));
        let short = inventory_error(
            &anyhow::Error::new(std::io::Error::from_raw_os_error(code))
                .context("reading snapshot"),
            &path,
        );
        assert!(
            short["error"]
                .as_str()
                .unwrap()
                .contains(&format!("os error {code}"))
        );
    }

    #[test]
    fn inventory_error_does_not_invent_an_errno() {
        let report = inventory_error(
            &anyhow::anyhow!("invalid registry"),
            std::path::Path::new("fixture"),
        );
        assert_eq!(report["error_kind"], "unknown");
        assert_eq!(report["os_error"], Value::Null);
        assert_eq!(report["liveness"], "unknown");
    }
}
