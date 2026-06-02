use super::*;
use dcc_mcp_transport::discovery::types::ServiceStatus;

pub(crate) fn is_fingerprint_eligible_instance(entry: &ServiceEntry) -> bool {
    entry.dcc_type != super::GATEWAY_SENTINEL_DCC_TYPE
        && !entry.dcc_type.eq_ignore_ascii_case("unknown")
        && !matches!(
            entry.status,
            ServiceStatus::ShuttingDown
                | ServiceStatus::Unreachable
                | ServiceStatus::Booting
                | ServiceStatus::Stale
        )
}

pub(crate) async fn live_backends(gs: &GatewayState) -> Vec<ServiceEntry> {
    let reg = gs.registry.read().await;
    gs.live_instances(&reg)
        .into_iter()
        .filter(|e| e.dcc_type != super::GATEWAY_SENTINEL_DCC_TYPE)
        .collect()
}

pub(crate) async fn targets_for_fanout(
    gs: &GatewayState,
    dcc_filter: Option<&str>,
) -> Vec<ServiceEntry> {
    live_backends(gs)
        .await
        .into_iter()
        .filter(|e| dcc_filter.is_none_or(|f| e.dcc_type.eq_ignore_ascii_case(f)))
        .collect()
}

pub(crate) async fn find_instance_by_prefix(
    gs: &GatewayState,
    prefix: &str,
) -> Option<ServiceEntry> {
    let reg = gs.registry.read().await;
    gs.resolve_instance(&reg, Some(prefix), None).ok()
}

pub(crate) async fn resolve_target(
    gs: &GatewayState,
    instance_id: Option<&str>,
    dcc_filter: Option<&str>,
) -> Result<ServiceEntry, String> {
    let reg = gs.registry.read().await;
    gs.resolve_instance(&reg, instance_id, dcc_filter)
        .map_err(|err| err.to_string())
}

pub(crate) fn to_text_result(res: Result<String, String>) -> (String, bool) {
    match res {
        Ok(text) => (text, false),
        Err(msg) => (msg, true),
    }
}

pub(crate) fn inject_instance_metadata(value: &mut Value, iid: &Uuid, dcc_type: &str) {
    if let Some(obj) = value.as_object_mut() {
        obj.insert("_instance_id".to_string(), Value::String(iid.to_string()));
        obj.insert(
            "_instance_short".to_string(),
            Value::String(instance_short(iid)),
        );
        obj.insert("_dcc_type".to_string(), Value::String(dcc_type.to_string()));
    }
}
