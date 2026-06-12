//! Diagnostic snapshot for `dcc-mcp-cli doctor`.
//!
//! Doctor is the read-only, no-launch view of the control plane: profile
//! selection, local registry inventory, daemon health, and gateway binary
//! discovery state.

use std::path::PathBuf;

use serde_json::{Map, Value, json};

use crate::application::gateway_profile::{GatewayProfileStore, GatewayTarget};
use crate::application::{gateway_ctrl, gateway_discovery, gateway_ensure, local_registry};

#[derive(Debug, Clone)]
pub struct DoctorRequest {
    pub profile_path: PathBuf,
    pub profile_store: GatewayProfileStore,
    pub gateway_target: GatewayTarget,
    pub registry_dir: Option<PathBuf>,
    pub server_bin: Option<PathBuf>,
    pub auto_gateway_enabled: bool,
    pub gateway_host: String,
    pub gateway_port: u16,
}

pub async fn run_doctor(request: DoctorRequest) -> anyhow::Result<Value> {
    let registry_dir = request
        .registry_dir
        .unwrap_or_else(gateway_ensure::default_registry_dir);
    let pidfile = gateway_ctrl::default_pidfile(&registry_dir);
    let gateway_status = gateway_ctrl::gateway_status(&gateway_ctrl::GatewayCtrlArgs {
        host: request.gateway_host.clone(),
        port: request.gateway_port,
        registry_dir: registry_dir.clone(),
        pidfile,
        start_opts: None,
    })
    .await;
    let local_inventory = match local_registry::list_local_instances(registry_dir.clone()) {
        Ok(value) => {
            let direct_control = direct_control_summary(&value);
            json!({
                "ok": true,
                "source": value.get("source").cloned().unwrap_or(Value::Null),
                "total": value.get("total").cloned().unwrap_or(Value::Null),
                "evicted": value.get("evicted").cloned().unwrap_or(Value::Null),
                "registry_dir": value.get("registry_dir").cloned().unwrap_or_else(|| json!(registry_dir.clone())),
                "direct_control": direct_control,
            })
        }
        Err(err) => json!({
            "ok": false,
            "registry_dir": registry_dir.clone(),
            "error": err.to_string(),
        }),
    };

    Ok(json!({
        "status": "ok",
        "cli": {
            "name": env!("CARGO_PKG_NAME"),
            "version": env!("CARGO_PKG_VERSION"),
        },
        "profile": request.profile_store.summary(
            &request.profile_path,
            Some(&request.gateway_target),
        ),
        "local": {
            "registry_dir": registry_dir,
            "inventory": local_inventory,
        },
        "gateway": {
            "auto_start_enabled": request.auto_gateway_enabled,
            "default_base_url": format!(
                "http://{}:{}",
                request.gateway_host,
                request.gateway_port,
            ),
            "status": gateway_status,
        },
        "server_binary": gateway_discovery::diagnose_gateway_bin(request.server_bin.as_ref()),
    }))
}

fn direct_control_summary(inventory: &Value) -> Value {
    let Some(instances) = inventory.get("instances").and_then(Value::as_array) else {
        return json!({
            "ready": 0,
            "not_ready": 0,
            "reasons": {},
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
