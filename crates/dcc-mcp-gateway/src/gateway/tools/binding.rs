//! Session-scoped instance resolution for gateway tool calls (RFC-0007 §3.1).
//!
//! Two halves:
//!
//! * **Resolve and pin** — [`tool_load_skill_for_session`] and friends pick the
//!   default instance before dispatching, so a call that omits `instance_id`
//!   keeps landing on the same process instead of failing on ambiguity.
//! * **Correctable** — [`tool_bind_instance`] / [`tool_unbind_instance`] let a
//!   caller that knows which Blender it means pin it for the session,
//!   optionally under a friendly alias.

use serde_json::{Value, json};

use super::super::state::GatewayState;
use super::tool_load_skill;

/// `bind_instance` — pin every later call for this session/dcc to one instance.
///
/// Accepts `instance_id` as a full UUID, a unique ≥4-char prefix, or — once
/// registered — an `alias`. Passing `alias` also makes that alias usable as the
/// `instance_id` argument of any later call.
pub async fn tool_bind_instance(
    gs: &GatewayState,
    args: &Value,
    session: Option<&str>,
) -> Result<String, String> {
    let Some(session) = session.map(str::trim).filter(|key| !key.is_empty()) else {
        return Err(serde_json::to_string_pretty(&json!({
            "success": false,
            "reason": "session_required",
            "message": "bind_instance needs a session to bind against. Send an `Mcp-Session-Id` header (MCP) or an `X-DCC-Session-Id` header (REST); without one there is nothing durable to pin to.",
            "hint": "The binding would be forgotten on the very next request, so it is refused instead of silently doing nothing.",
        }))
        .unwrap_or_else(|_| "session_required".to_string()));
    };

    let hint = args
        .get("instance_id")
        .or_else(|| args.get("instance"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            serde_json::to_string_pretty(&json!({
                "success": false,
                "reason": "instance_id_required",
                "message": "bind_instance requires `instance_id` (a full UUID, a unique >=4-char prefix, or a previously bound alias).",
            }))
            .unwrap_or_else(|_| "instance_id_required".to_string())
        })?;

    let dcc_filter = args
        .get("dcc_type")
        .or_else(|| args.get("dcc"))
        .and_then(Value::as_str);

    let resolved = gs
        .resolve_instance_async(Some(hint), dcc_filter)
        .await
        .map_err(|err| {
            serde_json::to_string_pretty(&json!({
                "success": false,
                "reason": "instance_not_found",
                "message": err.to_string(),
            }))
            .unwrap_or_else(|_| "instance_not_found".to_string())
        })?;

    let alias = args.get("alias").and_then(Value::as_str);
    let stored_alias =
        gs.instance_resolver
            .bind(session, &resolved.dcc_type, resolved.instance_id, alias);

    let mut out = json!({
        "success": true,
        "message": format!(
            "Bound '{}' for this session to {}",
            resolved.dcc_type,
            resolved.instance_id
        ),
        "instance": gs.instance_json(&resolved),
    });
    if let Some(alias) = stored_alias {
        out["alias"] = json!(alias);
    }
    serde_json::to_string_pretty(&out).map_err(|e| e.to_string())
}

/// `unbind_instance` — drop the pin for one `dcc_type` (or every DCC).
///
/// Sticky memory is cleared too, so the next unqualified call re-resolves from
/// scratch rather than restoring the binding that was just removed.
pub async fn tool_unbind_instance(
    gs: &GatewayState,
    args: &Value,
    session: Option<&str>,
) -> Result<String, String> {
    let Some(session) = session.map(str::trim).filter(|key| !key.is_empty()) else {
        return Err(serde_json::to_string_pretty(&json!({
            "success": false,
            "reason": "session_required",
            "message": "unbind_instance needs a session to clear. Send an `Mcp-Session-Id` header (MCP) or an `X-DCC-Session-Id` header (REST).",
        }))
        .unwrap_or_else(|_| "session_required".to_string()));
    };

    let dcc_type = args
        .get("dcc_type")
        .or_else(|| args.get("dcc"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());

    let removed = gs.instance_resolver.unbind(session, dcc_type);
    serde_json::to_string_pretty(&json!({
        "success": true,
        "message": match dcc_type {
            Some(dcc) => format!("Cleared {removed} instance binding(s) for '{dcc}' in this session"),
            None => format!("Cleared {removed} instance binding(s) in this session"),
        },
        "unbound": removed,
        "remaining": gs.instance_resolver.session_snapshot(session),
    }))
    .map_err(|e| e.to_string())
}

/// Read the `dcc_type` a skill-management call is scoped to, if any.
///
/// Auto-resolution is only safe when we know which DCC the caller means.
/// Without a filter, picking the newest instance across *all* DCC types would
/// silently send a Blender request to Maya, so we leave the call untouched and
/// let the existing ambiguity error stand.
fn skill_dcc_filter(args: &Value) -> Option<String> {
    args.get("dcc")
        .or_else(|| args.get("dcc_type"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|dcc| !dcc.is_empty())
        .map(str::to_string)
}

/// Resolve the default instance for a session-aware call and pin it in `args`.
///
/// Returns the `resolved_instance` JSON to echo, or `None` when the call
/// already names an instance (or cannot be scoped to one DCC) and must keep
/// its existing behaviour.
async fn pin_default_instance(
    gs: &GatewayState,
    args: &mut Value,
    session: Option<&str>,
) -> Option<serde_json::Value> {
    let session = session.map(str::trim).filter(|key| !key.is_empty())?;
    if args
        .get("instance_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .is_some_and(|value| !value.is_empty())
    {
        return None;
    }
    let dcc_filter = skill_dcc_filter(args)?;
    let resolved = gs
        .resolve_instance_for_session(None, Some(&dcc_filter), Some(session))
        .await
        .ok()?;
    let echo = resolved.to_json();
    if let Some(obj) = args.as_object_mut() {
        obj.insert(
            "instance_id".to_string(),
            serde_json::Value::String(resolved.entry.instance_id.to_string()),
        );
    }
    Some(echo)
}

/// Load a skill for a caller whose session identity is known (RFC-0007 §3.1).
///
/// With two Blender instances live and no `instance_id`, the old behaviour was
/// a hard error listing UUIDs. This wrapper resolves the instance first, pins
/// it for the rest of the conversation, and echoes which process was chosen.
pub async fn tool_load_skill_for_session(
    gs: &GatewayState,
    args: &Value,
    session: Option<&str>,
) -> (String, bool) {
    let mut pinned = args.clone();
    let echo = pin_default_instance(gs, &mut pinned, session).await;
    let (text, is_error) = tool_load_skill(gs, &pinned).await;
    match echo {
        Some(echo) if !is_error => (
            crate::gateway::instance_resolver::annotate_resolved_instance(&text, &echo),
            is_error,
        ),
        _ => (text, is_error),
    }
}

/// Unload a skill for a caller whose session identity is known.
///
/// Same contract as [`tool_load_skill_for_session`]: resolve and pin the
/// default instance instead of failing on ambiguity, then echo the choice.
pub async fn tool_unload_skill_for_session(
    gs: &GatewayState,
    args: &Value,
    session: Option<&str>,
) -> (String, bool) {
    let mut pinned = args.clone();
    let echo = pin_default_instance(gs, &mut pinned, session).await;
    let (text, is_error) =
        crate::gateway::aggregator::skill_mgmt_dispatch(gs, "unload_skill", &pinned).await;
    match echo {
        Some(echo) if !is_error => (
            crate::gateway::instance_resolver::annotate_resolved_instance(&text, &echo),
            is_error,
        ),
        _ => (text, is_error),
    }
}

/// Skill-management dispatch with session-scoped default-instance resolution.
///
/// Resolves (and thereby pins) the target instance before dispatching, so a
/// caller that omits `instance_id` keeps hitting the same process instead of
/// being asked to disambiguate every time.
pub async fn tool_skill_mgmt_for_session(
    gs: &GatewayState,
    tool: &str,
    args: &Value,
    session: Option<&str>,
) -> (String, bool) {
    let mut pinned = args.clone();
    let echo = pin_default_instance(gs, &mut pinned, session).await;
    let (text, is_error) = crate::gateway::aggregator::skill_mgmt_dispatch(gs, tool, &pinned).await;
    match echo {
        Some(echo) if !is_error => (
            crate::gateway::instance_resolver::annotate_resolved_instance(&text, &echo),
            is_error,
        ),
        _ => (text, is_error),
    }
}
