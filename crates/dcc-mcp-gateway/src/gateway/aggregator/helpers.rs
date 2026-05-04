use super::*;

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
    live_backends(gs)
        .await
        .into_iter()
        .find(|e| instance_short(&e.instance_id) == prefix)
}

pub(crate) async fn resolve_target(
    gs: &GatewayState,
    instance_id: Option<&str>,
    dcc_filter: Option<&str>,
) -> Result<ServiceEntry, String> {
    let candidates = live_backends(gs).await;

    // Exact or prefix match on instance_id.
    if let Some(iid) = instance_id {
        if let Some(e) = candidates.iter().find(|e| {
            let full = e.instance_id.to_string();
            full == iid || full.starts_with(iid) || instance_short(&e.instance_id) == iid
        }) {
            return Ok(e.clone());
        }
        return Err(format!("No live instance matches instance_id='{iid}'"));
    }

    // DCC-filtered auto-select when unambiguous.
    let filtered: Vec<&ServiceEntry> = candidates
        .iter()
        .filter(|e| dcc_filter.is_none_or(|f| e.dcc_type.eq_ignore_ascii_case(f)))
        .collect();

    match filtered.len() {
        0 => Err(match dcc_filter {
            Some(d) => format!("No live '{d}' instance."),
            None => "No live DCC instances.".to_string(),
        }),
        1 => Ok(filtered[0].clone()),
        _ => Err(format!(
            "Ambiguous target — {} instances live. Pass `instance_id` (or use `dcc` filter if only one of that type).",
            filtered.len()
        )),
    }
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
