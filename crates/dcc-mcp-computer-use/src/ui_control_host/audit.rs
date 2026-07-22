use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;

use dcc_mcp_ui_control::host_protocol::{
    UiControlHostErrorCode, UiControlPolicyTier, UiControlSystemGrant, UiControlTaskGrant,
};
use serde_json::json;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

pub(super) fn audit_event(
    grant: &UiControlTaskGrant,
    action: &str,
    success: bool,
    policy_tier: UiControlPolicyTier,
    error_code: Option<UiControlHostErrorCode>,
) {
    audit_event_for_dcc(&grant.dcc_type, action, success, policy_tier, error_code);
}

pub(super) fn audit_system_event(
    grant: &UiControlSystemGrant,
    action: &str,
    success: bool,
    policy_tier: UiControlPolicyTier,
    error_code: Option<UiControlHostErrorCode>,
) {
    audit_event_for_dcc(&grant.dcc_type, action, success, policy_tier, error_code);
}

fn audit_event_for_dcc(
    dcc_type: &str,
    action: &str,
    success: bool,
    policy_tier: UiControlPolicyTier,
    error_code: Option<UiControlHostErrorCode>,
) {
    let payload = json!({
        "event": "ui_control_operation",
        "tool": "dcc-mcp-ui-control-host",
        "dcc_type": dcc_type,
        "action": action,
        "success": success,
        "error": error_code,
        "message": if success { "DCC UI Control host operation succeeded" } else { "DCC UI Control host operation rejected" },
        "detail": format!("action={action} tier={policy_tier:?}"),
    });
    let timestamp = OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_owned());
    let level = if success { "INFO" } else { "WARN" };
    let line = format!(
        "{timestamp} {level} dcc_mcp_ui_control_host.audit: {}\n",
        payload
    );
    eprint!("{line}");
    let path = audit_log_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = file.write_all(line.as_bytes());
    }
}

fn audit_log_path() -> PathBuf {
    let directory = std::env::var_os("DCC_MCP_LOG_DIR")
        .map(PathBuf::from)
        .or_else(|| dcc_mcp_paths::get_log_dir().ok().map(PathBuf::from))
        .unwrap_or_else(std::env::temp_dir);
    directory.join(format!(
        "dcc-mcp-ui-control-host.{}.log",
        std::process::id()
    ))
}
