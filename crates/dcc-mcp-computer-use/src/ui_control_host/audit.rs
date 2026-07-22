use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;

use dcc_mcp_ui_control::host_protocol::{
    UiControlHostErrorCode, UiControlPolicyTier, UiControlSystemGrant, UiControlTaskGrant,
};
use serde_json::json;
use sha2::{Digest, Sha256};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

pub(super) fn audit_event(
    grant: &UiControlTaskGrant,
    action: &str,
    success: bool,
    policy_tier: UiControlPolicyTier,
    error_code: Option<UiControlHostErrorCode>,
) {
    audit_event_for_dcc(
        &grant.dcc_type,
        &session_correlation(&grant.task_grant_id),
        action,
        success,
        policy_tier,
        error_code,
    );
}

pub(super) fn audit_system_event(
    grant: &UiControlSystemGrant,
    action: &str,
    success: bool,
    policy_tier: UiControlPolicyTier,
    error_code: Option<UiControlHostErrorCode>,
) {
    audit_event_for_dcc(
        &grant.dcc_type,
        &session_correlation(&grant.system_grant_id),
        action,
        success,
        policy_tier,
        error_code,
    );
}

fn audit_event_for_dcc(
    dcc_type: &str,
    session_correlation: &str,
    action: &str,
    success: bool,
    policy_tier: UiControlPolicyTier,
    error_code: Option<UiControlHostErrorCode>,
) {
    let payload = json!({
        "event": "ui_control_operation",
        "tool": "dcc-mcp-ui-control-host",
        "dcc_type": dcc_type,
        "session_correlation": session_correlation,
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

fn session_correlation(grant_id: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    Sha256::digest(grant_id.as_bytes())[..8]
        .iter()
        .flat_map(|byte| {
            [
                HEX[usize::from(byte >> 4)] as char,
                HEX[usize::from(byte & 0x0f)] as char,
            ]
        })
        .collect()
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

#[cfg(test)]
mod tests {
    use super::session_correlation;

    #[test]
    fn audit_session_correlation_is_stable_and_does_not_expose_the_grant() {
        let correlation = session_correlation("adapter:maya:private-session:42");
        assert_eq!(correlation.len(), 16);
        assert_eq!(
            correlation,
            session_correlation("adapter:maya:private-session:42")
        );
        assert!(!correlation.contains("private-session"));
    }
}
