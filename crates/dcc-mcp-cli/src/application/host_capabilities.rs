//! Read-only host capability summaries through the existing DCC control plane.
//! Instance selection mirrors `reload-skills`: resolve one full identity from
//! the inventory, then call the existing backend tool through the same
//! local-direct, gateway REST, or transport path as every other CLI tool call.

use std::time::Duration;

use serde_json::{Value, json};

use crate::application::control_plane::DccControlPlane;
use crate::application::instance_selection::{instance_field, select_instances};
use crate::domain::capabilities::{CapabilitiesRequest, HOST_CAPABILITIES_TOOL};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilitiesCommand {
    pub dcc_type: Option<String>,
    pub instance_id: Option<String>,
    pub keys: Vec<String>,
    pub force_refresh: bool,
    pub meta: Option<Value>,
}

impl CapabilitiesCommand {
    #[must_use]
    pub fn new(
        dcc_type: Option<String>,
        instance_id: Option<String>,
        keys: Vec<String>,
        force_refresh: bool,
        meta: Option<Value>,
    ) -> Self {
        Self {
            dcc_type,
            instance_id,
            keys: normalize_keys(keys),
            force_refresh,
            meta,
        }
    }

    pub async fn run(self, control: &DccControlPlane, timeout: Duration) -> anyhow::Result<Value> {
        let inventory = control.list_instances().await?;
        let target = resolve_target(
            &inventory,
            self.dcc_type.as_deref(),
            self.instance_id.as_deref(),
        )?;
        let dcc_type = require_field(&target, "dcc_type")?;
        let instance_id = require_field(&target, "instance_id")?;
        let request = CapabilitiesRequest {
            dcc_type: dcc_type.to_string(),
            instance_id: instance_id.to_string(),
            keys: self.keys,
            force_refresh: self.force_refresh,
        };
        let result = control
            .call(
                HOST_CAPABILITIES_TOOL.to_string(),
                Some(request.dcc_type.clone()),
                Some(request.instance_id.clone()),
                request.arguments(),
                self.meta.clone(),
                timeout,
            )
            .await?;
        Ok(attach_target(&result, &target))
    }
}

fn normalize_keys(keys: Vec<String>) -> Vec<String> {
    let mut keys: Vec<String> = keys
        .into_iter()
        .flat_map(|raw| {
            raw.split(',')
                .map(str::trim)
                .filter(|key| !key.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .collect();
    keys.sort_unstable();
    keys.dedup();
    keys
}

fn resolve_target(
    inventory: &Value,
    dcc_type: Option<&str>,
    instance_id: Option<&str>,
) -> anyhow::Result<Value> {
    let candidates = select_instances(inventory, dcc_type, instance_id)?;
    let target = match candidates.as_slice() {
        [] => anyhow::bail!(
            "no DCC instance matched --dcc-type={} --instance-id={}",
            dcc_type.unwrap_or("*"),
            instance_id.unwrap_or("*")
        ),
        [instance] => instance.clone(),
        many => {
            let listed = many
                .iter()
                .map(|instance| {
                    format!(
                        "instance_short={} instance_id={}",
                        instance
                            .get("instance_short")
                            .and_then(Value::as_str)
                            .unwrap_or("-"),
                        instance
                            .get("instance_id")
                            .and_then(Value::as_str)
                            .unwrap_or("-")
                    )
                })
                .collect::<Vec<_>>()
                .join("; ");
            anyhow::bail!(
                "multiple DCC instances matched capabilities; pass --instance-id. candidates: {listed}"
            )
        }
    };
    Ok(target)
}

fn require_field<'a>(target: &'a Value, key: &str) -> anyhow::Result<&'a str> {
    instance_field(target, key)
        .or_else(|| {
            (key == "dcc_type")
                .then(|| instance_field(target, "dcc"))
                .flatten()
        })
        .ok_or_else(|| anyhow::anyhow!("matched instance row is missing {key}"))
}

fn attach_target(result: &Value, target: &Value) -> Value {
    let Some(mut object) = result.as_object().cloned() else {
        return json!({
            "success": false,
            "error": "unexpected_host_capabilities_response",
            "backend_response": result,
            "dcc_type": target_field(target, "dcc_type", Some("dcc")),
            "instance_id": target_field(target, "instance_id", None),
            "instance_short": target_field(target, "instance_short", None),
            "backend_tool": HOST_CAPABILITIES_TOOL,
        });
    };
    object.insert(
        "dcc_type".to_string(),
        target_field(target, "dcc_type", Some("dcc")),
    );
    object.insert(
        "instance_id".to_string(),
        target_field(target, "instance_id", None),
    );
    object.insert(
        "instance_short".to_string(),
        target_field(target, "instance_short", None),
    );
    object.insert("backend_tool".to_string(), json!(HOST_CAPABILITIES_TOOL));
    Value::Object(object)
}

fn target_field(target: &Value, primary: &str, fallback: Option<&str>) -> Value {
    target
        .get(primary)
        .or_else(|| fallback.and_then(|key| target.get(key)))
        .cloned()
        .unwrap_or(Value::Null)
}

#[cfg(test)]
#[path = "host_capabilities_tests.rs"]
mod tests;
