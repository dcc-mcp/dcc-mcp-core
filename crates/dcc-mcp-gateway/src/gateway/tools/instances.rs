//! Instance lease management — `acquire_dcc_instance` / `release_dcc_instance`.

use serde_json::{Value, json};

use super::super::state::GatewayState;
use dcc_mcp_transport::discovery::types::ServiceKey;

/// `acquire_dcc_instance` — reserve an idle DCC instance for a workflow/client.
pub async fn tool_acquire_instance(gs: &GatewayState, args: &Value) -> Result<String, String> {
    let dcc_type = args
        .get("dcc_type")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Provide dcc_type".to_string())?;
    let owner = args
        .get("lease_owner")
        .and_then(|v| v.as_str())
        .filter(|value| !value.is_empty() && *value == value.trim())
        .ok_or_else(|| {
            serde_json::to_string_pretty(&json!({
                "success": false,
                "reason": "lease_owner_required",
                "message": "Acquiring a lease requires a non-empty lease_owner coordination label without surrounding whitespace.",
            }))
            .unwrap_or_else(|_| "lease_owner_required".to_string())
        })?;
    let instance_id = args.get("instance_id").and_then(|v| v.as_str());
    let current_job_id = args
        .get("current_job_id")
        .and_then(|v| v.as_str())
        .map(str::to_owned);
    let ttl_secs = args
        .get("ttl_secs")
        .and_then(|v| v.as_u64())
        .unwrap_or(3600)
        .max(1);

    let reg = gs.registry.read().await;
    let resolved_instance_id = if instance_id.is_some() {
        Some(
            gs.resolve_instance(&reg, instance_id, Some(dcc_type))
                .map_err(|err| err.to_string())?
                .instance_id
                .to_string(),
        )
    } else {
        None
    };
    let Some(entry) = reg
        .acquire_lease(
            dcc_type,
            resolved_instance_id.as_deref(),
            owner,
            current_job_id,
            Some(std::time::Duration::from_secs(ttl_secs)),
        )
        .map_err(|e| e.to_string())?
    else {
        return Err(format!(
            "No idle '{dcc_type}' instance is available for lease. \
             Release a busy instance or start another DCC process."
        ));
    };

    serde_json::to_string_pretty(&json!({
        "success": true,
        "message": format!("Leased {dcc_type} instance {}", entry.instance_id),
        "instance": gs.instance_json(&entry),
    }))
    .map_err(|e| e.to_string())
}

/// `release_dcc_instance` — release a previously acquired instance lease.
pub async fn tool_release_instance(gs: &GatewayState, args: &Value) -> Result<String, String> {
    let instance_id = args
        .get("instance_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Provide instance_id".to_string())?;
    let reg = gs.registry.read().await;

    let entry = gs
        .resolve_instance(&reg, Some(instance_id), None)
        .map_err(|err| err.to_string())?;
    let key = ServiceKey {
        dcc_type: entry.dcc_type.clone(),
        instance_id: entry.instance_id,
    };

    let Some(row) = reg.get(&key) else {
        return Err(serde_json::to_string_pretty(&json!({
            "success": false,
            "reason": "unknown_instance",
            "message": format!("No FileRegistry row for instance_id {instance_id} after resolve"),
        }))
        .unwrap_or_else(|_| "unknown_instance".to_string()));
    };

    let Some(current) = row.active_lease_owner(std::time::SystemTime::now()) else {
        return Err(serde_json::to_string_pretty(&json!({
            "success": false,
            "reason": "no_active_lease",
            "message": "This instance has no active pool lease in the shared registry.",
            "hint": "Call acquire_dcc_instance first (same lease_owner string you plan to pass to release). release_dcc_instance only clears pool metadata in services.json — it does not close Maya or drop MCP connections.",
            "instance_id": entry.instance_id.to_string(),
            "instance": gs.instance_json(&entry),
        }))
        .unwrap_or_else(|_| "no_active_lease".to_string()));
    };
    let owner = args
        .get("lease_owner")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            serde_json::to_string_pretty(&json!({
                "success": false,
                "reason": "lease_owner_required",
                "message": "Releasing an active lease requires the same lease_owner used to acquire it.",
                "instance_id": instance_id,
            }))
            .unwrap_or_else(|_| "lease_owner_required".to_string())
        })?;
    if owner != current {
        return Err(serde_json::to_string_pretty(&json!({
            "success": false,
            "reason": "lease_owner_mismatch",
            "message": "lease_owner does not match the active lease holder.",
            "hint": "Pass the exact lease_owner used in acquire_dcc_instance.",
            "instance_id": entry.instance_id.to_string(),
        }))
        .unwrap_or_else(|_| "lease_owner_mismatch".to_string()));
    }

    let Some(released) = reg
        .release_lease(&key, Some(owner))
        .map_err(|e| e.to_string())?
    else {
        return Err(serde_json::to_string_pretty(&json!({
            "success": false,
            "reason": "release_rejected",
            "message": "Registry refused to clear the lease after pre-flight checks — possible concurrent mutation; retry once.",
            "instance_id": entry.instance_id.to_string(),
        }))
        .unwrap_or_else(|_| "release_rejected".to_string()));
    };

    serde_json::to_string_pretty(&json!({
        "success": true,
        "message": format!("Released lease for instance {}", released.instance_id),
        "instance": gs.instance_json(&released),
    }))
    .map_err(|e| e.to_string())
}
