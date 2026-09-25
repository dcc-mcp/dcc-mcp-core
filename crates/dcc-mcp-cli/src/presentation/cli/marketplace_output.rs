use serde_json::Value;

use crate::application::control_plane::DccControlPlane;
use crate::domain::rest::ReloadSkillsRequest;

/// Refresh the live adapters affected by a marketplace mutation.
///
/// `dcc_type` selects one host when the package landed in a host-specific
/// directory. `None` refreshes every live instance, which is the correct
/// scope for a package installed into the shared host-neutral directory:
/// every host loads it, so every host has to re-scan. The instance selectors
/// match `dcc_type` exactly, so sending the pseudo-host `any` would match
/// nothing and fail the reload.
pub async fn reload_marketplace_value(
    control: &DccControlPlane,
    mut value: Value,
    dcc_type: Option<String>,
) -> (Value, bool) {
    let reload_failed = match control
        .reload_skills(ReloadSkillsRequest {
            dcc_type,
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
