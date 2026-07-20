use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use dcc_mcp_ui_control::host_protocol::{
    UiControlEnsureOutcome, UiControlHostErrorCode, UiControlSystemGrant, UiControlSystemOperation,
};

#[cfg(windows)]
use super::runtime_windows;
use super::{HostFailure, valid_wire_label};

#[allow(dead_code)]
const SYSTEM_GRANTS_FILE_ENV: &str = "DCC_MCP_UI_CONTROL_SYSTEM_GRANTS_FILE";
#[allow(dead_code)]
const MAX_SYSTEM_GRANTS_FILE_BYTES: u64 = 1024 * 1024;

#[allow(dead_code)]
pub(super) fn load_system_grants() -> Result<HashMap<String, UiControlSystemGrant>, String> {
    let Some(path) = std::env::var_os(SYSTEM_GRANTS_FILE_ENV) else {
        return Ok(HashMap::new());
    };
    let path = PathBuf::from(path);
    if !path.is_absolute() {
        return Err(format!("{SYSTEM_GRANTS_FILE_ENV} must be an absolute path"));
    }
    let metadata = std::fs::metadata(&path)
        .map_err(|error| format!("read operator system grant metadata: {error}"))?;
    if !metadata.is_file() || metadata.len() > MAX_SYSTEM_GRANTS_FILE_BYTES {
        return Err("operator system grant catalog must be a file no larger than 1 MiB".to_owned());
    }
    let bytes = std::fs::read(path)
        .map_err(|error| format!("read operator system grant catalog: {error}"))?;
    parse_system_grants(&bytes)
}

#[allow(dead_code)]
pub(super) fn parse_system_grants(
    bytes: &[u8],
) -> Result<HashMap<String, UiControlSystemGrant>, String> {
    let grants: Vec<UiControlSystemGrant> = serde_json::from_slice(bytes)
        .map_err(|error| format!("parse operator system grant catalog: {error}"))?;
    if grants.len() > 64 {
        return Err("operator system grant catalog exceeds 64 grants".to_owned());
    }
    let mut catalog = HashMap::with_capacity(grants.len());
    for grant in grants {
        if !valid_wire_label(&grant.system_grant_id, 256)
            || !valid_wire_label(&grant.dcc_type, 64)
            || grant.operations.is_empty()
            || grant.operations.len() > 64
        {
            return Err("operator system grant is empty or outside safety limits".to_owned());
        }
        let mut operation_ids = HashSet::with_capacity(grant.operations.len());
        for entry in &grant.operations {
            if !valid_wire_label(&entry.operation_id, 256)
                || !operation_ids.insert(entry.operation_id.as_str())
            {
                return Err(
                    "operator system operation ids must be explicit and unique within each grant"
                        .to_owned(),
                );
            }
            validate_system_operation(&entry.operation).map_err(|failure| failure.message)?;
        }
        let grant_id = grant.system_grant_id.clone();
        if catalog.insert(grant_id, grant).is_some() {
            return Err("operator system grant ids must be unique".to_owned());
        }
    }
    Ok(catalog)
}

pub(super) fn validate_system_operation(
    operation: &UiControlSystemOperation,
) -> Result<(), HostFailure> {
    match operation {
        UiControlSystemOperation::EnsureRegistryString {
            key,
            value_name,
            value,
        } => {
            validate_hkcu_key(key)?;
            validate_registry_value_name(value_name)?;
            if value.encode_utf16().count() > 32_767 || value.contains('\0') {
                return Err(invalid_system_operation());
            }
        }
        UiControlSystemOperation::EnsureRegistryDword {
            key, value_name, ..
        } => {
            validate_hkcu_key(key)?;
            validate_registry_value_name(value_name)?;
        }
        UiControlSystemOperation::EnsureFileSymlink { link, target }
        | UiControlSystemOperation::EnsureDirectorySymlink { link, target } => {
            if !valid_windows_absolute_path(link)
                || !valid_windows_absolute_path(target)
                || link.eq_ignore_ascii_case(target)
            {
                return Err(invalid_system_operation());
            }
        }
    }
    Ok(())
}

fn validate_hkcu_key(key: &str) -> Result<(), HostFailure> {
    let normalized = key.to_ascii_uppercase();
    if normalized.starts_with("HKLM\\")
        || normalized.starts_with("HKEY_LOCAL_MACHINE\\")
        || normalized.starts_with("HKCU\\")
        || normalized.starts_with("HKEY_CURRENT_USER\\")
        || key.starts_with(['\\', '/'])
        || key.contains(':')
    {
        return Err(HostFailure::new(
            UiControlHostErrorCode::Unsupported,
            "registry operations accept only an HKCU-relative subkey; HKLM and alternate hives are unsupported",
        ));
    }
    if matches!(
        normalized.as_str(),
        "SOFTWARE\\MICROSOFT\\WINDOWS\\CURRENTVERSION\\RUN"
            | "SOFTWARE\\MICROSOFT\\WINDOWS\\CURRENTVERSION\\RUNONCE"
            | "SOFTWARE\\MICROSOFT\\WINDOWS\\CURRENTVERSION\\POLICIES\\EXPLORER\\RUN"
    ) {
        return Err(HostFailure::new(
            UiControlHostErrorCode::HardDenied,
            "Windows startup-persistence registry keys are outside the UI Control system-operation boundary",
        ));
    }
    if key.is_empty()
        || key.len() > 512
        || key.contains('/')
        || key.chars().any(char::is_control)
        || key
            .split('\\')
            .any(|component| component.is_empty() || matches!(component, "." | ".."))
    {
        return Err(invalid_system_operation());
    }
    Ok(())
}

fn validate_registry_value_name(value_name: &str) -> Result<(), HostFailure> {
    if value_name.len() > 256
        || value_name.contains(['\\', '/'])
        || value_name.chars().any(char::is_control)
    {
        return Err(invalid_system_operation());
    }
    Ok(())
}

pub(super) fn valid_windows_absolute_path(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() >= 3
        && bytes.len() <= 32_767
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'\\' | b'/')
        && !value[2..].contains(':')
        && !value.chars().any(char::is_control)
        && !value[3..]
            .split(['\\', '/'])
            .any(|component| matches!(component, "." | ".."))
}

pub(super) fn invalid_system_operation() -> HostFailure {
    HostFailure::new(
        UiControlHostErrorCode::InvalidRequest,
        "the typed system operation is malformed or outside safety limits",
    )
}

#[cfg(windows)]
pub(super) fn run_system_operation(
    operation: &UiControlSystemOperation,
) -> Result<UiControlEnsureOutcome, HostFailure> {
    runtime_windows::execute_system_operation(operation)
}

#[cfg(not(windows))]
pub(super) fn run_system_operation(
    _operation: &UiControlSystemOperation,
) -> Result<UiControlEnsureOutcome, HostFailure> {
    Err(HostFailure::new(
        UiControlHostErrorCode::Unsupported,
        "typed system operations are supported only on Windows",
    ))
}
