//! Shared local FileRegistry instance helpers for `dcc-mcp-cli`.
//!
//! Local inventory and direct MCP control must interpret registry rows the same
//! way. Keep ID matching, `mcp_url`, summaries, and gateway-sentinel filtering
//! in one place so list/search/call/wait-ready cannot drift.

use std::path::Path;

use anyhow::Context;
use dcc_mcp_transport::discovery::file_registry::FileRegistry;
use dcc_mcp_transport::discovery::types::{GATEWAY_SENTINEL_DCC_TYPE, ServiceEntry, ServiceStatus};
use serde_json::{Value, json};

const DISPATCH_STATUS_METADATA_KEY: &str = "dispatch_status";
const DISPATCH_STATUS_READY: &str = "ready";
const ROLE_METADATA_KEY: &str = "dcc_mcp_role";
const ROLE_PER_DCC_SIDECAR: &str = "per-dcc-sidecar";
const FAILURE_STAGE_METADATA_KEY: &str = "failure_stage";
const FAILURE_REASON_METADATA_KEY: &str = "failure_reason";
const FAILURE_AT_UNIX_METADATA_KEY: &str = "failure_at_unix";
const HOST_RPC_URI_METADATA_KEY: &str = "host_rpc_uri";
const HOST_RPC_SCHEME_METADATA_KEY: &str = "host_rpc_scheme";
const SIDECAR_PID_METADATA_KEY: &str = "sidecar_pid";
const GATEWAY_HEALTH_URL_METADATA_KEY: &str = "gateway_health_url";
const GATEWAY_RECOVERY_DRIVER_METADATA_KEY: &str = "gateway_recovery_driver";
const REGISTRATION_REFRESH_MODE_METADATA_KEY: &str = "registration_refresh_mode";
const GATEWAY_GUARDIAN_ACTIVE_METADATA_KEY: &str = "gateway_guardian_active";
const GATEWAY_GUARDIAN_FAILURES_METADATA_KEY: &str = "gateway_guardian_failures";
const GATEWAY_GUARDIAN_RESTARTS_METADATA_KEY: &str = "gateway_guardian_restarts";

pub(crate) fn live_dcc_entries(registry_dir: &Path) -> anyhow::Result<(Vec<ServiceEntry>, usize)> {
    let registry = FileRegistry::new(registry_dir.to_path_buf()).with_context(|| {
        format!(
            "opening local DCC FileRegistry at {}",
            registry_dir.display()
        )
    })?;
    let (entries, evicted) = registry
        .read_alive()
        .context("reading live local DCC instances")?;
    let mut entries: Vec<_> = entries
        .into_iter()
        .filter(|entry| entry.dcc_type != GATEWAY_SENTINEL_DCC_TYPE)
        .collect();
    entries.sort_by(|left, right| {
        (left.dcc_type.as_str(), left.instance_id)
            .cmp(&(right.dcc_type.as_str(), right.instance_id))
    });
    Ok((entries, evicted))
}

pub(crate) fn select_entries(
    registry_dir: &Path,
    dcc_type: Option<&str>,
    instance_hint: Option<&str>,
) -> anyhow::Result<Vec<ServiceEntry>> {
    let (entries, _) = live_dcc_entries(registry_dir)?;
    let hint = normalize_instance_hint(instance_hint)?;
    Ok(entries
        .into_iter()
        .filter(|entry| {
            dcc_type.is_none_or(|expected| entry.dcc_type.eq_ignore_ascii_case(expected))
        })
        .filter(|entry| {
            hint.as_deref()
                .is_none_or(|expected| instance_matches(entry, expected))
        })
        .collect())
}

pub(crate) fn select_routable_entries(
    registry_dir: &Path,
    dcc_type: Option<&str>,
    instance_hint: Option<&str>,
) -> anyhow::Result<Vec<ServiceEntry>> {
    let live_matches = select_entries(registry_dir, dcc_type, instance_hint)?;
    let routable_matches: Vec<_> = live_matches
        .iter()
        .filter(|entry| direct_control_ready(entry))
        .cloned()
        .collect();
    if routable_matches.is_empty() && !live_matches.is_empty() {
        anyhow::bail!(
            "local DCC instance matched the request but is not dispatch-ready. candidates: {}",
            live_matches
                .iter()
                .map(instance_readiness_summary)
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    Ok(routable_matches)
}

pub(crate) fn select_one_entry(
    registry_dir: &Path,
    dcc_type: Option<&str>,
    instance_hint: Option<&str>,
) -> anyhow::Result<ServiceEntry> {
    let matches = select_entries(registry_dir, dcc_type, instance_hint)?;
    match matches.as_slice() {
        [] => anyhow::bail!("no live local DCC instance matched the request"),
        [entry] => Ok(entry.clone()),
        many => anyhow::bail!(
            "local instance selection is ambiguous; provide --dcc-type and --instance-id. candidates: {}",
            many.iter()
                .map(|entry| format!("{}:{}", entry.dcc_type, instance_short(entry)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

pub(crate) fn select_one_routable_entry(
    registry_dir: &Path,
    dcc_type: Option<&str>,
    instance_hint: Option<&str>,
) -> anyhow::Result<ServiceEntry> {
    let live_matches = select_entries(registry_dir, dcc_type, instance_hint)?;
    let routable_matches: Vec<_> = live_matches
        .iter()
        .filter(|entry| direct_control_ready(entry))
        .cloned()
        .collect();
    match routable_matches.as_slice() {
        [] if live_matches.is_empty() => {
            anyhow::bail!("no live local DCC instance matched the request")
        }
        [] => anyhow::bail!(
            "local DCC instance matched the request but is not ready for direct local CLI control. candidates: {}",
            live_matches
                .iter()
                .map(instance_readiness_summary)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        [entry] => Ok(entry.clone()),
        many => anyhow::bail!(
            "local instance selection is ambiguous; provide --dcc-type and --instance-id. candidates: {}",
            many.iter()
                .map(|entry| format!("{}:{}", entry.dcc_type, instance_short(entry)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

pub(crate) fn instance_to_value(entry: ServiceEntry) -> anyhow::Result<Value> {
    let mut value = serde_json::to_value(&entry).context("serializing local registry entry")?;
    let Some(obj) = value.as_object_mut() else {
        anyhow::bail!("local registry entry did not serialize as an object");
    };

    obj.insert(
        "instance_short".to_string(),
        Value::String(instance_short(&entry)),
    );
    obj.insert("source".to_string(), Value::String("file".to_string()));
    obj.insert("display_id".to_string(), Value::String(entry.display_id()));
    if !obj.contains_key("mcp_url") {
        obj.insert("mcp_url".to_string(), Value::String(mcp_url(&entry)));
    }
    obj.insert("direct_control".to_string(), direct_control_report(&entry));
    Ok(value)
}

pub(crate) fn instance_summary(entry: &ServiceEntry) -> Value {
    json!({
        "dcc_type": entry.dcc_type,
        "instance_id": entry.instance_id.to_string(),
        "instance_short": instance_short(entry),
        "display_id": entry.display_id(),
        "display_name": entry.display_name,
        "pid": entry.pid,
        "mcp_url": mcp_url(entry),
        "status": entry.status.to_string(),
    })
}

pub(crate) fn local_tool_slug(entry: &ServiceEntry, backend_tool: &str) -> String {
    format!(
        "{}.{}.{}",
        entry.dcc_type,
        instance_short(entry),
        backend_tool
    )
}

pub(crate) fn instance_short(entry: &ServiceEntry) -> String {
    let mut short = entry.instance_id.simple().to_string();
    short.truncate(8);
    short
}

pub(crate) fn mcp_url(entry: &ServiceEntry) -> String {
    entry
        .metadata
        .get("mcp_url")
        .cloned()
        .or_else(|| {
            entry
                .extras
                .get("mcp_url")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_else(|| format!("http://{}:{}/mcp", entry.host, entry.port))
}

pub(crate) fn readyz_url(entry: &ServiceEntry) -> String {
    let mcp = mcp_url(entry);
    let trimmed = mcp.trim_end_matches('/');
    let base = trimmed.strip_suffix("/mcp").unwrap_or(trimmed);
    format!("{base}/v1/readyz")
}

pub(crate) fn direct_control_ready(entry: &ServiceEntry) -> bool {
    let is_sidecar = entry
        .metadata
        .get(ROLE_METADATA_KEY)
        .is_some_and(|role| role == ROLE_PER_DCC_SIDECAR);
    if !matches!(entry.status, ServiceStatus::Available | ServiceStatus::Busy) {
        return false;
    }
    match entry.metadata.get(DISPATCH_STATUS_METADATA_KEY) {
        Some(status) => status == DISPATCH_STATUS_READY,
        None => !is_sidecar,
    }
}

pub(crate) fn direct_control_report(entry: &ServiceEntry) -> Value {
    let role = entry.metadata.get(ROLE_METADATA_KEY).map(String::as_str);
    let dispatch_status = entry
        .metadata
        .get(DISPATCH_STATUS_METADATA_KEY)
        .map(String::as_str);
    let ready = direct_control_ready(entry);
    let reason = if ready {
        Value::Null
    } else if !matches!(entry.status, ServiceStatus::Available | ServiceStatus::Busy) {
        Value::String("service_status".to_string())
    } else {
        Value::String("dispatch_status".to_string())
    };
    let recommended_next_action = if ready {
        "Use this instance through the local MCP route."
    } else if !matches!(entry.status, ServiceStatus::Available | ServiceStatus::Busy) {
        "Run wait-ready for this instance and inspect startup logs if it stays unavailable."
    } else if role == Some(ROLE_PER_DCC_SIDECAR) {
        "Wait for this sidecar to report dispatch_status=ready before routing local CLI calls to it."
    } else {
        "Wait for dispatch_status=ready before routing local CLI calls to this instance."
    };

    json!({
        "ready": ready,
        "route": if ready { "local_mcp" } else { "none" },
        "reason": reason,
        "service_status": entry.status.to_string(),
        "dispatch_status": dispatch_status.unwrap_or("not_reported"),
        "role": role.unwrap_or("direct-mcp"),
        "diagnostics": direct_control_diagnostics(entry),
        "recommended_next_action": recommended_next_action,
    })
}

fn direct_control_diagnostics(entry: &ServiceEntry) -> Value {
    json!({
        "failure_stage": metadata_text(entry, FAILURE_STAGE_METADATA_KEY),
        "failure_reason": metadata_text(entry, FAILURE_REASON_METADATA_KEY),
        "failure_at_unix": metadata_text(entry, FAILURE_AT_UNIX_METADATA_KEY),
        "host_rpc_uri": metadata_text(entry, HOST_RPC_URI_METADATA_KEY),
        "host_rpc_scheme": metadata_text(entry, HOST_RPC_SCHEME_METADATA_KEY),
        "sidecar_pid": metadata_text(entry, SIDECAR_PID_METADATA_KEY),
        "gateway_health_url": metadata_text(entry, GATEWAY_HEALTH_URL_METADATA_KEY),
        "gateway_recovery_driver": metadata_text(entry, GATEWAY_RECOVERY_DRIVER_METADATA_KEY),
        "registration_refresh_mode": metadata_text(entry, REGISTRATION_REFRESH_MODE_METADATA_KEY),
        "gateway_guardian": {
            "active": metadata_text(entry, GATEWAY_GUARDIAN_ACTIVE_METADATA_KEY),
            "failures": metadata_text(entry, GATEWAY_GUARDIAN_FAILURES_METADATA_KEY),
            "restarts": metadata_text(entry, GATEWAY_GUARDIAN_RESTARTS_METADATA_KEY),
        },
        "logs": {
            "log_dir": metadata_first_text(entry, &["sidecar_log_dir", "stdio_log_dir", "log_dir"]),
            "stdout_path": metadata_first_text(entry, &["sidecar_stdout_path", "stdio_stdout_path", "stdout_path"]),
            "stderr_path": metadata_first_text(entry, &["sidecar_stderr_path", "stdio_stderr_path", "stderr_path"]),
        },
    })
}

fn metadata_text<'a>(entry: &'a ServiceEntry, key: &str) -> Option<&'a str> {
    entry
        .metadata
        .get(key)
        .map(String::as_str)
        .filter(|value| !value.trim().is_empty())
}

fn metadata_first_text<'a>(entry: &'a ServiceEntry, keys: &[&str]) -> Option<&'a str> {
    keys.iter().find_map(|key| metadata_text(entry, key))
}

fn instance_readiness_summary(entry: &ServiceEntry) -> String {
    let report = direct_control_report(entry);
    let dispatch = entry
        .metadata
        .get(DISPATCH_STATUS_METADATA_KEY)
        .map(String::as_str)
        .unwrap_or("not_reported");
    let role = entry
        .metadata
        .get(ROLE_METADATA_KEY)
        .map(String::as_str)
        .unwrap_or("direct-mcp");
    let failure = metadata_text(entry, FAILURE_STAGE_METADATA_KEY)
        .map(|stage| {
            let reason = metadata_text(entry, FAILURE_REASON_METADATA_KEY).unwrap_or("unknown");
            format!(" failure_stage={stage} failure_reason={reason}")
        })
        .unwrap_or_default();
    format!(
        "{}:{} status={} dispatch_status={} role={} direct_control_reason={}{}",
        entry.dcc_type,
        instance_short(entry),
        entry.status,
        dispatch,
        role,
        report
            .get("reason")
            .and_then(Value::as_str)
            .unwrap_or("unknown"),
        failure
    )
}

fn normalize_instance_hint(instance_hint: Option<&str>) -> anyhow::Result<Option<String>> {
    let Some(hint) = instance_hint
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Ok(None);
    };
    if hint.len() < 4 {
        anyhow::bail!("instance-id prefix must be at least 4 characters");
    }
    Ok(Some(hint.to_ascii_lowercase()))
}

fn instance_matches(entry: &ServiceEntry, expected: &str) -> bool {
    let id = entry.instance_id.to_string().to_ascii_lowercase();
    let simple = entry.instance_id.simple().to_string().to_ascii_lowercase();
    id == expected
        || simple.starts_with(expected)
        || instance_short(entry).eq_ignore_ascii_case(expected)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_tool_slug_round_trips_short_instance_id() {
        let entry = ServiceEntry::new("maya", "127.0.0.1", 18080);
        let slug = local_tool_slug(&entry, "maya_scene__get_session_info");

        assert!(slug.starts_with("maya."));
        assert!(slug.contains(&instance_short(&entry)));
        assert!(slug.ends_with(".maya_scene__get_session_info"));
    }

    #[test]
    fn live_dcc_entries_filters_gateway_sentinel() {
        let dir = tempfile::tempdir().unwrap();
        let registry = FileRegistry::new(dir.path()).unwrap();
        registry
            .register(ServiceEntry::new(
                GATEWAY_SENTINEL_DCC_TYPE,
                "127.0.0.1",
                9765,
            ))
            .unwrap();
        registry
            .register(ServiceEntry::new("maya", "127.0.0.1", 18080))
            .unwrap();

        let (entries, evicted) = live_dcc_entries(dir.path()).unwrap();

        assert_eq!(evicted, 0);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].dcc_type, "maya");
    }

    #[test]
    fn routable_selection_skips_booting_diagnostic_sidecar_rows() {
        let dir = tempfile::tempdir().unwrap();
        let registry = FileRegistry::new(dir.path()).unwrap();
        let mut diagnostic = ServiceEntry::new("maya", "127.0.0.1", 18080);
        diagnostic.status = ServiceStatus::Booting;
        diagnostic.metadata.insert(
            DISPATCH_STATUS_METADATA_KEY.to_string(),
            "unavailable".to_string(),
        );
        registry.register(diagnostic).unwrap();
        let mut ready = ServiceEntry::new("maya", "127.0.0.1", 18081);
        ready.metadata.insert(
            DISPATCH_STATUS_METADATA_KEY.to_string(),
            DISPATCH_STATUS_READY.to_string(),
        );
        let ready_id = ready.instance_id;
        registry.register(ready).unwrap();

        let entries = select_routable_entries(dir.path(), Some("maya"), None).unwrap();

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].instance_id, ready_id);

        let value = instance_to_value(entries[0].clone()).unwrap();
        assert_eq!(value["direct_control"]["ready"], true);
        assert_eq!(value["direct_control"]["route"], "local_mcp");
        assert_eq!(
            value["direct_control"]["recommended_next_action"],
            "Use this instance through the local MCP route."
        );
    }

    #[test]
    fn routable_selection_reports_unready_live_matches() {
        let dir = tempfile::tempdir().unwrap();
        let registry = FileRegistry::new(dir.path()).unwrap();
        let mut entry = ServiceEntry::new("maya", "127.0.0.1", 18080);
        entry.status = ServiceStatus::Booting;
        entry.metadata.insert(
            DISPATCH_STATUS_METADATA_KEY.to_string(),
            "unavailable".to_string(),
        );
        registry.register(entry).unwrap();

        let err = select_one_routable_entry(dir.path(), Some("maya"), None).unwrap_err();
        let message = err.to_string();

        assert!(message.contains("not ready for direct local CLI control"));
        assert!(message.contains("status=booting"));
        assert!(message.contains("dispatch_status=unavailable"));
        assert!(message.contains("direct_control_reason=service_status"));
    }

    #[test]
    fn routable_selection_includes_dispatch_ready_sidecar_rows() {
        let dir = tempfile::tempdir().unwrap();
        let registry = FileRegistry::new(dir.path()).unwrap();
        let mut sidecar = ServiceEntry::new("maya", "127.0.0.1", 18080);
        sidecar.metadata.insert(
            DISPATCH_STATUS_METADATA_KEY.to_string(),
            DISPATCH_STATUS_READY.to_string(),
        );
        sidecar.metadata.insert(
            ROLE_METADATA_KEY.to_string(),
            ROLE_PER_DCC_SIDECAR.to_string(),
        );
        let sidecar_id = sidecar.instance_id;
        registry.register(sidecar).unwrap();

        let entries = select_routable_entries(dir.path(), Some("maya"), None).unwrap();

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].instance_id, sidecar_id);

        let sidecar = select_entries(dir.path(), Some("maya"), None)
            .unwrap()
            .into_iter()
            .find(|entry| entry.instance_id == sidecar_id)
            .unwrap();
        let value = instance_to_value(sidecar).unwrap();
        assert_eq!(value["direct_control"]["ready"], true);
        assert_eq!(value["direct_control"]["reason"], Value::Null);
        assert_eq!(value["direct_control"]["route"], "local_mcp");
    }
}
