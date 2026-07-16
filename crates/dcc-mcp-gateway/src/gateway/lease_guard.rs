//! Active instance lease enforcement for gateway-routed calls.

use std::time::SystemTime;

use dcc_mcp_transport::discovery::types::{LeaseOwnerError, ServiceEntry};
use serde_json::Value;

pub(super) fn check_call_owner(
    entry: &ServiceEntry,
    meta: Option<&Value>,
) -> Result<(), LeaseOwnerError> {
    let request_owner = meta
        .and_then(|value| value.get("lease_owner"))
        .and_then(Value::as_str);
    entry.check_lease_owner(request_owner, SystemTime::now())
}

pub(super) fn check_raw_mcp_call_owner(
    entry: &ServiceEntry,
    body: &[u8],
) -> Result<(), LeaseOwnerError> {
    if entry.active_lease_owner(SystemTime::now()).is_none() {
        return Ok(());
    }
    let Ok(payload) = serde_json::from_slice::<Value>(body) else {
        return Ok(());
    };
    match payload {
        Value::Array(requests) => {
            for request in &requests {
                check_raw_request_owner(entry, request)?;
            }
            Ok(())
        }
        request => check_raw_request_owner(entry, &request),
    }
}

pub(super) fn check_raw_request_owner(
    entry: &ServiceEntry,
    request: &Value,
) -> Result<(), LeaseOwnerError> {
    if request.get("method").and_then(Value::as_str) != Some("tools/call") {
        return Ok(());
    }
    check_call_owner(entry, request.pointer("/params/_meta"))
}
