use serde_json::Value;

use crate::application::control_plane::DccControlPlane;
use crate::domain::rest::ReloadSkillsRequest;

/// Refresh a live adapter after a marketplace mutation and attach the result.
pub async fn reload_marketplace_value(
    control: &DccControlPlane,
    mut value: Value,
    dcc_type: String,
) -> (Value, bool) {
    let reload_failed = match control
        .reload_skills(ReloadSkillsRequest {
            dcc_type: Some(dcc_type),
            instance_id: None,
        })
        .await
    {
        Ok(result) => {
            let reloaded = result.get("ok").and_then(Value::as_bool).unwrap_or(false);
            value["reload_required"] = Value::Bool(!reloaded);
            value["reload"] = result;
            !reloaded
        }
        Err(err) => {
            value["reload"] = serde_json::json!({
                "ok": false,
                "error": err.to_string(),
            });
            true
        }
    };
    (value, reload_failed)
}
