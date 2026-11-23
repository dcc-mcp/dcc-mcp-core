use std::fs;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::domain::install::{InstallPlan, InstallStepAction};

use super::pip::{pip_show_args, pip_show_version};
use super::resolve_install_source_path;

const RECEIPT_SCHEMA_VERSION: u8 = 1;
const MAX_RECEIPT_BYTES: u64 = 128 * 1024;
const OWNED_DIGEST_VERSION: u8 = 2;

fn legacy_digest_version() -> u8 {
    1
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct ReceiptOwnedPath {
    pub path: String,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ReceiptOwnershipDigest {
    pub version: u8,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct InstallReceipt {
    pub schema_version: u8,
    #[serde(default = "legacy_digest_version")]
    pub owned_digest_version: u8,
    pub dcc_type: String,
    pub adapter_name: String,
    pub adapter_version: String,
    #[serde(default)]
    pub install_type: String,
    pub python: Option<String>,
    pub package: String,
    pub artifact_url: String,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub destination: Option<String>,
    pub sha256: String,
    #[serde(default)]
    pub extras: Vec<String>,
    #[serde(default)]
    pub actions: Vec<String>,
    #[serde(default)]
    pub owned_paths: Vec<ReceiptOwnedPath>,
}

pub(super) fn receipt_path_for_plan(plan: &InstallPlan) -> PathBuf {
    let directory = std::env::var_os("DCC_MCP_INSTALL_RECEIPT_DIR")
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(default_receipt_directory);
    let identity = format!(
        "{}\n{}\n{}",
        plan.dcc_type,
        plan.adapter.name,
        receipt_path_identity(plan)
    );
    let digest = Sha256::digest(identity.as_bytes());
    let suffix = digest
        .iter()
        .take(12)
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    directory.join(format!(
        "{}-{suffix}.json",
        safe_filename(&plan.adapter.name)
    ))
}

fn legacy_receipt_path_for_plan(plan: &InstallPlan) -> Option<PathBuf> {
    if !matches!(
        first_install_action(plan),
        Some(InstallStepAction::PipInstall { .. })
    ) {
        return None;
    }
    let python = pip_python(plan).unwrap_or_default();
    let directory = std::env::var_os("DCC_MCP_INSTALL_RECEIPT_DIR")
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(default_receipt_directory);
    let identity = format!("{}\n{}\n{}", plan.dcc_type, plan.adapter.name, python);
    let digest = Sha256::digest(identity.as_bytes());
    let suffix = digest
        .iter()
        .take(12)
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    legacy_safe_filename(&plan.adapter.name)
        .map(|name| directory.join(format!("{name}-{suffix}.json")))
}

fn legacy_non_pip_receipt_path_for_plan(plan: &InstallPlan) -> Option<PathBuf> {
    let action = first_install_action(plan)?;
    if matches!(action, InstallStepAction::PipInstall { .. }) {
        return None;
    }
    let directory = std::env::var_os("DCC_MCP_INSTALL_RECEIPT_DIR")
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(default_receipt_directory);
    let identity = receipt_actions(plan)
        .map(historical_action_identity)
        .collect::<Vec<_>>()
        .join("\n");
    let digest = Sha256::digest(
        format!("{}\n{}\n{}", plan.dcc_type, plan.adapter.name, identity).as_bytes(),
    );
    let suffix = digest
        .iter()
        .take(12)
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    legacy_safe_filename(&plan.adapter.name)
        .map(|name| directory.join(format!("{name}-{suffix}.json")))
}

pub(super) fn receipt_for_plan(plan: &InstallPlan) -> Option<InstallReceipt> {
    let action = first_install_action(plan)?;
    let adapter_version = receipt_version(plan, action)?;
    let mut receipt = InstallReceipt {
        schema_version: RECEIPT_SCHEMA_VERSION,
        owned_digest_version: OWNED_DIGEST_VERSION,
        dcc_type: plan.dcc_type.clone(),
        adapter_name: plan.adapter.name.clone(),
        adapter_version,
        install_type: String::new(),
        python: None,
        package: String::new(),
        artifact_url: String::new(),
        source: String::new(),
        destination: None,
        sha256: String::new(),
        extras: Vec::new(),
        actions: Vec::new(),
        owned_paths: Vec::new(),
    };
    match action {
        InstallStepAction::PipInstall {
            package,
            python,
            artifact_url,
            sha256,
            ..
        } => {
            if artifact_url
                .as_deref()
                .is_none_or(|value| value.trim().is_empty())
                || sha256
                    .as_deref()
                    .is_none_or(|value| value.trim().is_empty())
            {
                return None;
            }
            receipt.install_type = "pip".into();
            receipt.python = python.clone();
            receipt.package = package.clone();
            receipt.artifact_url = credential_free_url(artifact_url.as_deref().unwrap_or_default());
            receipt.source = receipt.artifact_url.clone();
            receipt.sha256 = normalized_sha256(sha256.as_deref());
            receipt.extras = normalized_extras(plan);
        }
        InstallStepAction::GitClone { url, ref_, dest } => {
            receipt.install_type = "git".into();
            receipt.source = format!(
                "{}@{}",
                credential_free_url(url),
                normalized_git_ref(ref_.as_deref())
            );
            receipt.destination = Some(dest.display().to_string());
        }
        InstallStepAction::ZipExtract { url, sha256, dest } => {
            receipt.install_type = "zip".into();
            receipt.source = credential_free_url(url);
            receipt.destination = Some(dest.display().to_string());
            receipt.sha256 = normalized_sha256(sha256.as_deref());
        }
        InstallStepAction::PathCopy { source, dest } => {
            receipt.install_type = "path".into();
            receipt.source = resolve_install_source_path(source).display().to_string();
            receipt.destination = Some(dest.display().to_string());
        }
        InstallStepAction::AdobePluginLink {
            source,
            dest,
            product,
            extension_type,
            ..
        } => {
            receipt.install_type = "adobe-plugin-link".into();
            receipt.source = format!("{}:{}:{}", product, extension_type, source.display());
            receipt.destination = Some(dest.display().to_string());
        }
        InstallStepAction::RegisterDcc { .. } | InstallStepAction::Verify => return None,
    }
    receipt.actions = receipt_actions(plan).map(action_identity).collect();
    Some(receipt)
}

fn plan_for_action(plan: &InstallPlan, action: &InstallStepAction) -> InstallPlan {
    let mut scoped = plan.clone();
    scoped
        .steps
        .retain(|step| step.action.as_ref() == Some(action));
    scoped
}

fn receipt_for_action(plan: &InstallPlan, action: &InstallStepAction) -> Option<InstallReceipt> {
    receipt_for_plan(&plan_for_action(plan, action))
}

pub(super) fn read_receipt(path: &Path) -> Result<Option<InstallReceipt>, String> {
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("failed to inspect install receipt: {error}")),
    };
    if !metadata.is_file() || metadata.len() > MAX_RECEIPT_BYTES {
        return Err("install receipt is not a bounded regular file".into());
    }
    let bytes =
        fs::read(path).map_err(|error| format!("failed to read install receipt: {error}"))?;
    let mut receipt: InstallReceipt = serde_json::from_slice(&bytes)
        .map_err(|error| format!("install receipt is invalid JSON: {error}"))?;
    if receipt.install_type.is_empty()
        && !receipt.package.trim().is_empty()
        && receipt.artifact_url.starts_with("https://")
    {
        receipt.install_type = "pip".into();
        receipt.source = receipt.artifact_url.clone();
    }
    receipt.artifact_url = credential_free_url(&receipt.artifact_url);
    receipt.source = normalize_receipt_source(&receipt.install_type, &receipt.source);
    receipt.actions = receipt
        .actions
        .iter()
        .map(|action| normalize_action_identity(action))
        .collect();
    if receipt.install_type == "pip" && receipt.owned_paths.is_empty() {
        receipt.owned_digest_version = OWNED_DIGEST_VERSION;
    }
    if receipt.schema_version != RECEIPT_SCHEMA_VERSION
        || receipt.dcc_type.trim().is_empty()
        || receipt.adapter_name.trim().is_empty()
        || receipt.adapter_version.trim().is_empty()
        || !matches!(receipt.owned_digest_version, 1 | OWNED_DIGEST_VERSION)
        || receipt.install_type.trim().is_empty()
        || receipt.source.trim().is_empty()
        || (!receipt.sha256.is_empty()
            && (receipt.sha256.len() != 64
                || !receipt.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())))
        || receipt.owned_paths.iter().any(|owned| {
            owned.path.trim().is_empty()
                || owned.sha256.len() != 64
                || !owned.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
        })
    {
        return Err("install receipt has an incompatible identity".into());
    }
    Ok(Some(receipt))
}

#[cfg(test)]
pub(super) fn receipt_matches_install(plan: &InstallPlan) -> Result<bool, String> {
    let mut found_action = false;
    for action in receipt_actions(plan) {
        found_action = true;
        if !receipt_matches_action(plan, action)? {
            return Ok(false);
        }
    }
    Ok(found_action)
}

pub(super) fn receipt_match_details(
    plan: &InstallPlan,
    action: &InstallStepAction,
) -> Result<(bool, Option<ReceiptOwnershipDigest>), String> {
    let Some(expected) = receipt_for_plan(plan) else {
        return Ok((false, None));
    };
    let Some(action_expected) = receipt_for_action(plan, action) else {
        return Ok((false, None));
    };
    let receipt_path = receipt_path_for_plan(plan);
    validate_receipt_location(plan, &receipt_path)?;
    for actual in read_receipts_for_action(plan, action)? {
        if !same_identity(&actual, &expected) && !same_identity(&actual, &action_expected) {
            continue;
        }
        return match action {
            InstallStepAction::PipInstall { .. } => Ok((
                pip_receipt_is_healthy_for_action(&action_expected, action)?,
                None,
            )),
            InstallStepAction::GitClone { ref_, dest, .. } => {
                if !git_checkout_is_healthy(dest, ref_.as_deref()) {
                    return Ok((false, None));
                }
                let digest = receipt_ownership_digest_for_action(&actual, action)?;
                Ok((digest.is_some(), digest))
            }
            InstallStepAction::ZipExtract { .. }
            | InstallStepAction::PathCopy { .. }
            | InstallStepAction::AdobePluginLink { .. } => {
                let digest = receipt_ownership_digest_for_action(&actual, action)?;
                Ok((digest.is_some(), digest))
            }
            InstallStepAction::RegisterDcc { .. } | InstallStepAction::Verify => Ok((false, None)),
        };
    }
    Ok((false, None))
}

#[cfg(test)]
pub(super) fn receipt_matches_action(
    plan: &InstallPlan,
    action: &InstallStepAction,
) -> Result<bool, String> {
    Ok(receipt_match_details(plan, action)?.0)
}

fn read_receipts_for_action(
    plan: &InstallPlan,
    action: &InstallStepAction,
) -> Result<Vec<InstallReceipt>, String> {
    let scoped = plan_for_action(plan, action);
    let paths = [
        Some(receipt_path_for_plan(plan)),
        Some(receipt_path_for_plan(&scoped)),
        legacy_receipt_path_for_plan(plan),
        legacy_receipt_path_for_plan(&scoped),
        legacy_non_pip_receipt_path_for_plan(plan),
        legacy_non_pip_receipt_path_for_plan(&scoped),
    ];
    let mut receipts = Vec::new();
    let mut seen_paths = Vec::new();
    for path in paths.into_iter().flatten() {
        if seen_paths.iter().any(|seen| seen == &path) {
            continue;
        }
        seen_paths.push(path.clone());
        if let Some(receipt) = read_receipt(&path)? {
            receipts.push(receipt);
        }
    }
    Ok(receipts)
}

pub(super) fn commit_install_receipt(
    plan: &InstallPlan,
    skipped_actions: &[(InstallStepAction, Option<ReceiptOwnershipDigest>)],
    acquired_actions: &[InstallStepAction],
) -> Result<bool, String> {
    let Some(mut receipt) = receipt_for_plan(plan) else {
        return Ok(false);
    };
    let configured_path = receipt_path_for_plan(plan);
    validate_receipt_location(plan, &configured_path)?;
    let path = resolve_receipt_for_containment(&configured_path)?;
    validate_receipt_location(plan, &path)?;
    receipt.owned_paths = owned_paths_for_plan(plan, skipped_actions, acquired_actions)?;
    if !skipped_candidate_matches(&receipt, skipped_actions)? {
        return Err("a skipped install destination changed before receipt commit".into());
    }
    let existing_receipt = read_receipt(&path)?;
    match existing_receipt {
        Some(existing) if existing == receipt => return Ok(true),
        Some(existing)
            if (!skipped_actions.is_empty() || !acquired_actions.is_empty())
                && same_receipt_scope(&existing, &receipt)
                && same_destination_set(&existing, &receipt)
                && skipped_ownership_matches(&existing, &mut receipt, skipped_actions)? => {}
        Some(existing) if receipt_can_be_extended(&existing, &receipt) => {}
        Some(existing) if same_install_target(&existing, &receipt) => {}
        Some(_) => {
            return Err("existing install receipt does not match the acquired state".into());
        }
        None => {}
    }
    let parent = path
        .parent()
        .ok_or_else(|| "install receipt has no parent directory".to_string())?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("failed to create install receipt directory: {error}"))?;
    let bytes = serde_json::to_vec_pretty(&receipt)
        .map_err(|error| format!("failed to serialize install receipt: {error}"))?;
    if bytes.len() as u64 > MAX_RECEIPT_BYTES {
        return Err("install receipt exceeds the bounded size limit".into());
    }
    let mut temporary = tempfile::NamedTempFile::new_in(parent)
        .map_err(|error| format!("failed to stage install receipt: {error}"))?;
    temporary
        .write_all(&bytes)
        .map_err(|error| format!("failed to write install receipt: {error}"))?;
    temporary
        .as_file()
        .sync_all()
        .map_err(|error| format!("failed to sync install receipt: {error}"))?;
    replace_receipt_file(temporary, &path).map(|()| true)
}

fn pip_receipt_is_healthy(expected: &InstallReceipt) -> Result<bool, String> {
    let Some(python) = expected.python.as_deref().filter(|value| !value.is_empty()) else {
        return Ok(false);
    };
    let output = match Command::new(python)
        .args(pip_show_args(&expected.package))
        .output()
    {
        Ok(output) => output,
        Err(_) => return Ok(false),
    };
    if !output.status.success()
        || pip_show_version(&output.stdout).as_deref() != Some(expected.adapter_version.as_str())
    {
        return Ok(false);
    }
    let check = match Command::new(python).args(["-m", "pip", "check"]).output() {
        Ok(output) => output,
        Err(_) => return Ok(false),
    };
    if !check.status.success() {
        return Ok(false);
    }
    let output_text = String::from_utf8_lossy(&output.stdout);
    let Some(location) = output_text
        .lines()
        .find_map(|line| line.strip_prefix("Location:").map(str::trim))
        .filter(|value| !value.is_empty())
    else {
        return Ok(false);
    };
    let Some((url, sha256)) = direct_url_identity(Path::new(location), &expected.package) else {
        return Ok(false);
    };
    Ok(credential_free_url(&url) == expected.artifact_url && sha256 == expected.sha256)
}

fn pip_receipt_is_healthy_for_action(
    base: &InstallReceipt,
    action: &InstallStepAction,
) -> Result<bool, String> {
    let InstallStepAction::PipInstall {
        package,
        python,
        artifact_url,
        sha256,
        ..
    } = action
    else {
        return Ok(false);
    };
    let mut expected = base.clone();
    expected.install_type = "pip".into();
    expected.package = package.clone();
    expected.python = python.clone();
    expected.artifact_url = credential_free_url(artifact_url.as_deref().unwrap_or_default());
    expected.source = expected.artifact_url.clone();
    expected.sha256 = normalized_sha256(sha256.as_deref());
    pip_receipt_is_healthy(&expected)
}

fn same_identity(actual: &InstallReceipt, expected: &InstallReceipt) -> bool {
    let mut actual = actual.clone();
    let mut expected = expected.clone();
    if actual.actions.is_empty()
        && (actual.owned_digest_version == 1
            || (actual.install_type == "pip" && actual.owned_paths.is_empty()))
        && !expected.actions.is_empty()
    {
        actual.actions = expected.actions.clone();
    }
    actual.owned_digest_version = expected.owned_digest_version;
    actual.owned_paths.clear();
    expected.owned_paths.clear();
    actual == expected
}

fn same_install_target(actual: &InstallReceipt, expected: &InstallReceipt) -> bool {
    if actual.adapter_version == expected.adapter_version {
        return false;
    }
    let mut actual = actual.clone();
    let mut expected = expected.clone();
    actual.adapter_version.clear();
    expected.adapter_version.clear();
    actual.owned_paths.clear();
    expected.owned_paths.clear();
    actual == expected
}

#[cfg(test)]
fn receipt_ownership_matches(plan: &InstallPlan, receipt: &InstallReceipt) -> Result<bool, String> {
    let expected = owned_path_specs(plan);
    let mut expected_paths = expected
        .iter()
        .map(|(_, path, _, _, _)| path.display().to_string())
        .collect::<Vec<_>>();
    let mut actual_paths = receipt
        .owned_paths
        .iter()
        .map(|owned| owned.path.clone())
        .collect::<Vec<_>>();
    expected_paths.sort();
    actual_paths.sort();
    if expected_paths != actual_paths || actual_paths.windows(2).any(|paths| paths[0] == paths[1]) {
        return Ok(false);
    }
    for owned in &receipt.owned_paths {
        let Some((path, allow_nested_symlink, allow_root_symlink, exclude_git)) = expected
            .iter()
            .find(|(_, path, _, _, _)| path.display().to_string() == owned.path)
            .map(|(_, path, nested, root, git)| (path, nested, root, git))
        else {
            return Ok(false);
        };
        if path_digest_for_receipt(
            path,
            *allow_nested_symlink,
            *allow_root_symlink,
            *exclude_git,
            receipt.owned_digest_version,
        )?
        .as_deref()
            != Some(owned.sha256.as_str())
        {
            return Ok(false);
        }
    }
    Ok(true)
}

fn receipt_ownership_digest_for_action(
    receipt: &InstallReceipt,
    action: &InstallStepAction,
) -> Result<Option<ReceiptOwnershipDigest>, String> {
    let Some((path, allow_nested_symlink, allow_root_symlink, exclude_git)) =
        owned_path_spec_for_action(action)
    else {
        return Ok(None);
    };
    let matching = receipt
        .owned_paths
        .iter()
        .filter(|owned| owned.path == path.display().to_string())
        .collect::<Vec<_>>();
    if matching.len() != 1 {
        return Ok(None);
    }
    let digest = path_digest_for_receipt(
        path,
        allow_nested_symlink,
        allow_root_symlink,
        exclude_git,
        receipt.owned_digest_version,
    )?;
    if digest.as_deref() != Some(matching[0].sha256.as_str()) {
        return Ok(None);
    }
    Ok(digest.map(|sha256| ReceiptOwnershipDigest {
        version: receipt.owned_digest_version,
        sha256,
    }))
}

fn receipt_can_be_extended(existing: &InstallReceipt, expected: &InstallReceipt) -> bool {
    if existing.owned_paths.len() >= expected.owned_paths.len()
        || !same_identity(existing, expected)
    {
        return false;
    }
    let mut existing_paths = existing
        .owned_paths
        .iter()
        .map(|owned| owned.path.clone())
        .collect::<Vec<_>>();
    existing_paths.sort();
    if existing_paths
        .windows(2)
        .any(|values| values[0] == values[1])
    {
        return false;
    }
    for owned in &existing.owned_paths {
        let Some(candidate) = expected
            .owned_paths
            .iter()
            .find(|candidate| candidate.path == owned.path)
        else {
            return false;
        };
        if candidate.sha256 != owned.sha256 {
            return false;
        }
    }
    true
}

fn same_destination_set(existing: &InstallReceipt, expected: &InstallReceipt) -> bool {
    if existing.owned_paths.len() != expected.owned_paths.len() {
        return false;
    }
    let mut existing_paths = existing
        .owned_paths
        .iter()
        .map(|owned| owned.path.clone())
        .collect::<Vec<_>>();
    let mut expected_paths = expected
        .owned_paths
        .iter()
        .map(|owned| owned.path.clone())
        .collect::<Vec<_>>();
    existing_paths.sort();
    expected_paths.sort();
    existing_paths == expected_paths
        && existing_paths
            .windows(2)
            .all(|values| values[0] != values[1])
}

fn same_receipt_scope(existing: &InstallReceipt, expected: &InstallReceipt) -> bool {
    existing.schema_version == expected.schema_version
        && existing.dcc_type == expected.dcc_type
        && existing.adapter_name == expected.adapter_name
        && existing.install_type == expected.install_type
}

fn skipped_ownership_matches(
    receipt: &InstallReceipt,
    candidate: &mut InstallReceipt,
    skipped_actions: &[(InstallStepAction, Option<ReceiptOwnershipDigest>)],
) -> Result<bool, String> {
    for (action, validated_digest) in skipped_actions {
        let Some((path, _, _, _)) = owned_path_spec_for_action(action) else {
            continue;
        };
        let path = path.display().to_string();
        let Some(validated_digest) = validated_digest else {
            continue;
        };
        let Some(current) = candidate
            .owned_paths
            .iter_mut()
            .find(|owned| owned.path == path)
        else {
            return Ok(false);
        };
        if validated_digest.version == candidate.owned_digest_version {
            if validated_digest.sha256 != current.sha256 {
                return Ok(false);
            }
            current.sha256 = validated_digest.sha256.clone();
        } else {
            let Some(previous) = receipt.owned_paths.iter().find(|owned| owned.path == path) else {
                return Ok(false);
            };
            if receipt_ownership_digest_for_action(receipt, action)?
                .is_none_or(|digest| digest.sha256 != previous.sha256)
            {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

fn skipped_candidate_matches(
    candidate: &InstallReceipt,
    skipped_actions: &[(InstallStepAction, Option<ReceiptOwnershipDigest>)],
) -> Result<bool, String> {
    for (action, validated) in skipped_actions {
        let Some(validated) = validated else {
            continue;
        };
        let Some((path, _, _, _)) = owned_path_spec_for_action(action) else {
            continue;
        };
        let path = path.display().to_string();
        let Some(current) = candidate
            .owned_paths
            .iter()
            .find(|owned| owned.path == path)
        else {
            return Ok(false);
        };
        if validated.version == candidate.owned_digest_version {
            if current.sha256 != validated.sha256 {
                return Ok(false);
            }
            continue;
        }
        let Some((path, allow_nested_symlink, allow_root_symlink, exclude_git)) =
            owned_path_spec_for_action(action)
        else {
            continue;
        };
        if path_digest_for_receipt(
            path,
            allow_nested_symlink,
            allow_root_symlink,
            exclude_git,
            validated.version,
        )?
        .as_deref()
            != Some(validated.sha256.as_str())
        {
            return Ok(false);
        }
    }
    Ok(true)
}

fn owned_path_spec_for_action(action: &InstallStepAction) -> Option<(&Path, bool, bool, bool)> {
    match action {
        InstallStepAction::GitClone { dest, .. } => Some((dest, true, false, true)),
        InstallStepAction::ZipExtract { dest, .. } | InstallStepAction::PathCopy { dest, .. } => {
            Some((dest, true, false, false))
        }
        InstallStepAction::AdobePluginLink { dest, .. } => Some((dest, false, true, false)),
        InstallStepAction::PipInstall { .. }
        | InstallStepAction::RegisterDcc { .. }
        | InstallStepAction::Verify => None,
    }
}

fn git_checkout_is_healthy(dest: &Path, ref_: Option<&str>) -> bool {
    let expected = normalized_git_ref(ref_);
    let root_output = match Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(dest)
        .output()
    {
        Ok(output) => output,
        Err(_) => return false,
    };
    if !root_output.status.success() {
        return false;
    }
    let Ok(root) = fs::canonicalize(String::from_utf8_lossy(&root_output.stdout).trim()) else {
        return false;
    };
    let Ok(destination) = fs::canonicalize(dest) else {
        return false;
    };
    if root != destination {
        return false;
    }
    let output = match Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(dest)
        .output()
    {
        Ok(output) => output,
        Err(_) => return false,
    };
    output.status.success()
        && String::from_utf8_lossy(&output.stdout)
            .trim()
            .eq_ignore_ascii_case(&expected)
}

fn validate_receipt_location(plan: &InstallPlan, receipt_path: &Path) -> Result<(), String> {
    let receipt_path = resolve_receipt_for_containment(receipt_path)?;
    for (_, destination, _, _, _) in owned_path_specs(plan) {
        if receipt_path.starts_with(resolve_for_containment(&destination)?) {
            return Err(format!(
                "install receipt must not be stored inside an owned destination: {}",
                destination.display()
            ));
        }
    }
    Ok(())
}

fn resolve_receipt_for_containment(path: &Path) -> Result<PathBuf, String> {
    let name = last_path_component(path)
        .ok_or_else(|| format!("cannot resolve receipt path: {}", path.display()))?;
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let mut resolved = resolve_for_containment(parent)?;
    resolved.push(name);
    Ok(resolved)
}

fn resolve_for_containment(path: &Path) -> Result<PathBuf, String> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|error| format!("failed to resolve install receipt path: {error}"))?
            .join(path)
    };
    let mut existing = absolute.clone();
    let mut suffix = Vec::new();
    loop {
        match fs::symlink_metadata(&existing) {
            Ok(_) => break,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let name = last_path_component(&existing).ok_or_else(|| {
                    format!(
                        "cannot resolve path for receipt containment: {}",
                        path.display()
                    )
                })?;
                suffix.push(name.to_owned());
                existing.pop();
            }
            Err(error) => {
                return Err(format!(
                    "failed to inspect receipt containment path: {error}"
                ));
            }
        }
    }
    let mut resolved = fs::canonicalize(&existing)
        .map_err(|error| format!("failed to resolve receipt containment path: {error}"))?;
    for component in suffix.iter().rev() {
        match Path::new(component).components().next() {
            Some(Component::CurDir) => {}
            Some(Component::ParentDir) => {
                resolved.pop();
            }
            Some(Component::Normal(_)) => resolved.push(component),
            Some(Component::Prefix(_) | Component::RootDir) | None => {
                return Err("invalid receipt containment path component".into());
            }
        }
    }
    Ok(resolved)
}

fn last_path_component(path: &Path) -> Option<std::ffi::OsString> {
    match path.components().next_back()? {
        Component::CurDir => Some(".".into()),
        Component::ParentDir => Some("..".into()),
        Component::Normal(value) => Some(value.to_owned()),
        Component::Prefix(_) | Component::RootDir => None,
    }
}

fn owned_paths_for_plan(
    plan: &InstallPlan,
    skipped_actions: &[(InstallStepAction, Option<ReceiptOwnershipDigest>)],
    acquired_actions: &[InstallStepAction],
) -> Result<Vec<ReceiptOwnedPath>, String> {
    let has_execution_state = !skipped_actions.is_empty() || !acquired_actions.is_empty();
    let mut owned = Vec::new();
    for (_action, path, allow_nested_symlink, allow_root_symlink, exclude_git) in
        owned_path_specs(plan).into_iter().filter_map(
            |(action, path, allow_nested_symlink, allow_root_symlink, exclude_git)| {
                if has_execution_state
                    && !skipped_actions
                        .iter()
                        .any(|(skipped, _)| skipped == &action)
                    && !acquired_actions.iter().any(|acquired| acquired == &action)
                {
                    return None;
                }
                Some((
                    action,
                    path,
                    allow_nested_symlink,
                    allow_root_symlink,
                    exclude_git,
                ))
            },
        )
    {
        let digest =
            path_digest_with_policy(&path, allow_nested_symlink, allow_root_symlink, exclude_git)?
                .ok_or_else(|| {
                    format!(
                        "receipt destination is missing after install: {}",
                        path.display()
                    )
                })?;
        owned.push(ReceiptOwnedPath {
            path: path.display().to_string(),
            sha256: digest,
        });
    }
    Ok(owned)
}

fn owned_path_specs(plan: &InstallPlan) -> Vec<(InstallStepAction, PathBuf, bool, bool, bool)> {
    let mut paths = plan
        .steps
        .iter()
        .filter_map(|step| match step.action.as_ref()? {
            action @ InstallStepAction::GitClone { dest, .. } => {
                Some((action.clone(), dest.clone(), true, false, true))
            }
            InstallStepAction::ZipExtract { dest, .. }
            | InstallStepAction::PathCopy { dest, .. } => Some((
                step.action.as_ref()?.clone(),
                dest.clone(),
                true,
                false,
                false,
            )),
            InstallStepAction::AdobePluginLink { dest, .. } => Some((
                step.action.as_ref()?.clone(),
                dest.clone(),
                false,
                true,
                false,
            )),
            InstallStepAction::PipInstall { .. }
            | InstallStepAction::RegisterDcc { .. }
            | InstallStepAction::Verify => None,
        })
        .collect::<Vec<_>>();
    paths.sort_by(|left, right| left.1.cmp(&right.1));
    paths.dedup_by(|left, right| left.1 == right.1);
    paths
}

fn direct_url_identity(location: &Path, package: &str) -> Option<(String, String)> {
    let normalized_package = package
        .chars()
        .map(|character| match character {
            '-' | '.' => '_',
            other => other.to_ascii_lowercase(),
        })
        .collect::<String>();
    for entry in fs::read_dir(location).ok()?.flatten() {
        let file_type = entry.file_type().ok()?;
        if !file_type.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
        if !name.starts_with(&format!("{normalized_package}-")) || !name.ends_with(".dist-info") {
            continue;
        }
        let metadata = fs::read_to_string(entry.path().join("direct_url.json")).ok()?;
        let document: serde_json::Value = serde_json::from_str(&metadata).ok()?;
        let url = document.get("url")?.as_str()?.to_owned();
        let sha256 = document
            .get("archive_info")
            .and_then(|archive| archive.get("hashes"))
            .and_then(|hashes| hashes.get("sha256"))
            .and_then(serde_json::Value::as_str)
            .or_else(|| {
                document
                    .get("archive_info")
                    .and_then(|archive| archive.get("hash"))
                    .and_then(serde_json::Value::as_str)
                    .and_then(|value| value.strip_prefix("sha256="))
            })?
            .to_ascii_lowercase();
        return Some((url, sha256));
    }
    None
}

#[cfg(test)]
fn path_digest(path: &Path, allow_symlink: bool) -> Result<Option<String>, String> {
    path_digest_with_policy(path, allow_symlink, allow_symlink, false)
}

fn path_digest_with_policy(
    path: &Path,
    allow_nested_symlink: bool,
    allow_root_symlink: bool,
    exclude_git: bool,
) -> Result<Option<String>, String> {
    if let Err(error) = fs::symlink_metadata(path) {
        if error.kind() == std::io::ErrorKind::NotFound {
            return Ok(None);
        }
        return Err(format!("failed to inspect owned path: {error}"));
    }
    let mut hasher = Sha256::new();
    hasher.update(b"dcc-mcp-owned-path-v2\0");
    let root_is_symlink = fs::symlink_metadata(path)
        .map_err(|error| format!("failed to inspect owned path: {error}"))?
        .file_type()
        .is_symlink();
    if root_is_symlink && !allow_root_symlink {
        return Err("receipt ownership found a symlink root".into());
    }
    digest_path(
        path,
        Path::new(""),
        allow_nested_symlink || root_is_symlink,
        exclude_git,
        &mut hasher,
    )?;
    Ok(Some(digest_hex(hasher.finalize())))
}

fn path_digest_for_receipt(
    path: &Path,
    allow_nested_symlink: bool,
    allow_root_symlink: bool,
    exclude_git: bool,
    digest_version: u8,
) -> Result<Option<String>, String> {
    if digest_version == 1 {
        return legacy_path_digest(path);
    }
    path_digest_with_policy(path, allow_nested_symlink, allow_root_symlink, exclude_git)
}

fn legacy_path_digest(path: &Path) -> Result<Option<String>, String> {
    if let Err(error) = fs::symlink_metadata(path) {
        if error.kind() == std::io::ErrorKind::NotFound {
            return Ok(None);
        }
        return Err(format!("failed to inspect owned path: {error}"));
    }
    let mut hasher = Sha256::new();
    legacy_digest_path(path, path, &mut hasher)?;
    Ok(Some(digest_hex(hasher.finalize())))
}

fn legacy_digest_path(path: &Path, root: &Path, hasher: &mut Sha256) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("failed to inspect owned path: {error}"))?;
    if metadata.file_type().is_symlink() {
        return Err("legacy receipt ownership found a symlink".into());
    }
    if metadata.is_file() {
        let relative = path.strip_prefix(root).unwrap_or(path);
        hasher.update(relative.to_string_lossy().as_bytes());
        hasher.update([0]);
        hasher.update(
            fs::read(path).map_err(|error| {
                format!("failed to read owned path for legacy receipt: {error}")
            })?,
        );
        return Ok(());
    }
    if !metadata.is_dir() {
        return Err("legacy receipt ownership found an unsupported entry".into());
    }
    let mut entries = Vec::new();
    for entry in
        fs::read_dir(path).map_err(|error| format!("failed to enumerate owned path: {error}"))?
    {
        entries.push(
            entry
                .map_err(|error| format!("failed to read owned path entry: {error}"))?
                .path(),
        );
    }
    entries.sort();
    for entry in entries {
        let relative = entry.strip_prefix(root).unwrap_or(&entry);
        hasher.update(relative.to_string_lossy().replace('\\', "/").as_bytes());
        hasher.update([0]);
        legacy_digest_path(&entry, root, hasher)?;
    }
    Ok(())
}

fn digest_hex<D>(digest: D) -> String
where
    D: AsRef<[u8]>,
{
    digest
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn digest_path(
    path: &Path,
    relative: &Path,
    allow_nested_symlink: bool,
    exclude_git: bool,
    hasher: &mut Sha256,
) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("failed to inspect owned path: {error}"))?;
    if metadata.file_type().is_symlink() {
        if !allow_nested_symlink {
            return Err("receipt ownership found a symlink".into());
        }
        let target = fs::read_link(path)
            .map_err(|error| format!("failed to read owned symlink: {error}"))?;
        hasher.update(b"symlink\0");
        update_digest_path(hasher, relative);
        update_digest_os_str(hasher, target.as_os_str());
        return Ok(());
    }
    if metadata.is_file() {
        hasher.update(b"file\0");
        update_digest_path(hasher, relative);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            hasher.update(metadata.permissions().mode().to_le_bytes());
        }
        #[cfg(not(unix))]
        hasher.update(0_u32.to_le_bytes());
        stream_digest_file(path, metadata.len(), hasher)?;
        return Ok(());
    }
    if metadata.is_dir() {
        hasher.update(b"directory\0");
        update_digest_path(hasher, relative);
    } else {
        return Err("receipt ownership found an unsupported filesystem entry".into());
    }
    let mut entries = Vec::new();
    for entry in
        fs::read_dir(path).map_err(|error| format!("failed to enumerate owned path: {error}"))?
    {
        let entry = entry.map_err(|error| format!("failed to read owned path entry: {error}"))?;
        entries.push(entry.path());
    }
    entries.sort_by(|left, right| {
        left.file_name()
            .unwrap_or_default()
            .cmp(right.file_name().unwrap_or_default())
    });
    for entry in entries {
        if exclude_git
            && relative.as_os_str().is_empty()
            && entry
                .file_name()
                .is_some_and(|name| name == std::ffi::OsStr::new(".git"))
        {
            continue;
        }
        let child_relative = relative.join(entry.file_name().unwrap_or_default());
        digest_path(
            &entry,
            &child_relative,
            allow_nested_symlink,
            exclude_git,
            hasher,
        )?;
    }
    Ok(())
}

fn update_digest_bytes(hasher: &mut Sha256, bytes: &[u8]) {
    hasher.update((bytes.len() as u64).to_le_bytes());
    hasher.update(bytes);
}

fn update_digest_path(hasher: &mut Sha256, path: &Path) {
    for component in path.components() {
        hasher.update([match component {
            Component::CurDir => 0,
            Component::ParentDir => 1,
            Component::RootDir => 2,
            Component::Prefix(_) => 3,
            Component::Normal(_) => 4,
        }]);
        update_digest_os_str(hasher, component.as_os_str());
    }
    hasher.update([255]);
}

fn update_digest_os_str(hasher: &mut Sha256, value: &std::ffi::OsStr) {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        update_digest_bytes(hasher, value.as_bytes());
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        let bytes = value
            .encode_wide()
            .flat_map(|unit| unit.to_le_bytes())
            .collect::<Vec<_>>();
        update_digest_bytes(hasher, &bytes);
    }
    #[cfg(not(any(unix, windows)))]
    update_digest_bytes(hasher, value.to_string_lossy().as_bytes());
}

fn stream_digest_file(path: &Path, expected_len: u64, hasher: &mut Sha256) -> Result<(), String> {
    let mut file = fs::File::open(path)
        .map_err(|error| format!("failed to open owned path for reading: {error}"))?;
    hasher.update(expected_len.to_le_bytes());
    let mut buffer = [0_u8; 64 * 1024];
    let mut total = 0_u64;
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|error| format!("failed to read owned path: {error}"))?;
        if count == 0 {
            break;
        }
        total += count as u64;
        hasher.update(&buffer[..count]);
    }
    if total != expected_len {
        return Err("owned path changed while its receipt digest was computed".into());
    }
    Ok(())
}

fn replace_receipt_file(
    temporary: tempfile::NamedTempFile,
    destination: &Path,
) -> Result<(), String> {
    #[cfg(not(windows))]
    {
        temporary
            .persist(destination)
            .map_err(|error| format!("failed to commit install receipt: {error}"))?;
        Ok(())
    }

    #[cfg(windows)]
    {
        let temporary = temporary.into_temp_path();
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_ATTRIBUTE_TEMPORARY, GetFileAttributesW, INVALID_FILE_ATTRIBUTES,
            MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW, SetFileAttributesW,
        };
        let from = temporary
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        let attributes = unsafe { GetFileAttributesW(from.as_ptr()) };
        if attributes != INVALID_FILE_ATTRIBUTES
            && attributes & FILE_ATTRIBUTE_TEMPORARY != 0
            && unsafe { SetFileAttributesW(from.as_ptr(), attributes & !FILE_ATTRIBUTE_TEMPORARY) }
                == 0
        {
            return Err(format!(
                "failed to prepare install receipt for commit: {}",
                std::io::Error::last_os_error()
            ));
        }
        let to = destination
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        let ok = unsafe {
            MoveFileExW(
                from.as_ptr(),
                to.as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        };
        if ok == 0 {
            Err(format!(
                "failed to commit install receipt: {}",
                std::io::Error::last_os_error()
            ))
        } else {
            Ok(())
        }
    }
}

fn default_receipt_directory() -> PathBuf {
    dirs::data_dir()
        .map(|path| path.join("dcc-mcp").join("install-receipts"))
        .unwrap_or_else(|| PathBuf::from("~/.local/share/dcc-mcp/install-receipts"))
}

fn first_install_action(plan: &InstallPlan) -> Option<&InstallStepAction> {
    plan.steps
        .iter()
        .find_map(|step| match step.action.as_ref()? {
            InstallStepAction::PipInstall { .. }
            | InstallStepAction::GitClone { .. }
            | InstallStepAction::ZipExtract { .. }
            | InstallStepAction::PathCopy { .. }
            | InstallStepAction::AdobePluginLink { .. } => step.action.as_ref(),
            _ => None,
        })
}

fn receipt_path_identity(plan: &InstallPlan) -> String {
    receipt_actions(plan)
        .map(|action| match action {
            InstallStepAction::GitClone { dest, .. } => {
                format!("git\n{}", dest.display())
            }
            InstallStepAction::ZipExtract { dest, .. } => {
                format!("zip\n{}", dest.display())
            }
            InstallStepAction::PathCopy { dest, .. } => {
                format!("path\n{}", dest.display())
            }
            InstallStepAction::AdobePluginLink { dest, .. } => {
                format!("adobe-plugin-link\n{}", dest.display())
            }
            _ => action_identity(action),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn action_identity(action: &InstallStepAction) -> String {
    match action {
        InstallStepAction::PipInstall {
            package,
            python,
            artifact_url,
            sha256,
            ..
        } => format!(
            "pip\n{}\n{}\n{}\n{}",
            package,
            python.as_deref().unwrap_or_default(),
            credential_free_url(artifact_url.as_deref().unwrap_or_default()),
            normalized_sha256(sha256.as_deref())
        ),
        InstallStepAction::GitClone { url, ref_, dest } => {
            format!(
                "git\n{}\n{}\n{}",
                credential_free_url(url),
                normalized_git_ref(ref_.as_deref()),
                dest.display()
            )
        }
        InstallStepAction::ZipExtract { url, sha256, dest } => {
            format!(
                "zip\n{}\n{}\n{}",
                credential_free_url(url),
                normalized_sha256(sha256.as_deref()),
                dest.display()
            )
        }
        InstallStepAction::PathCopy { source, dest } => {
            format!(
                "path\n{}\n{}",
                resolve_install_source_path(source).display(),
                dest.display()
            )
        }
        InstallStepAction::AdobePluginLink {
            source,
            dest,
            product,
            extension_type,
            ..
        } => format!(
            "adobe-plugin-link\n{}\n{}\n{product}\n{extension_type}",
            source.display(),
            dest.display()
        ),
        InstallStepAction::RegisterDcc { .. } | InstallStepAction::Verify => String::new(),
    }
}

fn historical_action_identity(action: &InstallStepAction) -> String {
    match action {
        InstallStepAction::GitClone { url, ref_, dest } => format!(
            "git\n{url}\n{}\n{}",
            normalized_git_ref(ref_.as_deref()),
            dest.display()
        ),
        InstallStepAction::ZipExtract { url, sha256, dest } => format!(
            "zip\n{url}\n{}\n{}",
            normalized_sha256(sha256.as_deref()),
            dest.display()
        ),
        _ => action_identity(action),
    }
}

fn receipt_version(plan: &InstallPlan, action: &InstallStepAction) -> Option<String> {
    plan.version
        .clone()
        .or_else(|| plan.adapter.version.clone())
        .filter(|version| !version.trim().is_empty())
        .or_else(|| match action {
            InstallStepAction::GitClone { ref_, .. } => Some(normalized_git_ref(ref_.as_deref())),
            InstallStepAction::ZipExtract { sha256, .. } => {
                Some(normalized_sha256(sha256.as_deref()))
            }
            InstallStepAction::PathCopy { .. } | InstallStepAction::AdobePluginLink { .. } => {
                Some("unversioned".into())
            }
            _ => None,
        })
        .filter(|version| !version.trim().is_empty())
}

fn normalized_git_ref(value: Option<&str>) -> String {
    value.unwrap_or_default().trim().to_ascii_lowercase()
}

fn credential_free_url(value: &str) -> String {
    let Ok(mut url) = reqwest::Url::parse(value) else {
        if let Some((user_host, repository)) = value.split_once(':')
            && let Some(host) = user_host.rsplit_once('@').map(|(_, host)| host)
            && !host.is_empty()
            && !repository.is_empty()
        {
            return format!("ssh://{host}/{repository}");
        }
        return value
            .split(['?', '#'])
            .next()
            .unwrap_or_default()
            .to_string();
    };
    let _ = url.set_username("");
    let _ = url.set_password(None);
    url.set_query(None);
    url.set_fragment(None);
    url.to_string()
}

fn normalize_receipt_source(install_type: &str, source: &str) -> String {
    match install_type {
        "git" => source.rsplit_once('@').map_or_else(
            || credential_free_url(source),
            |(url, reference)| format!("{}@{reference}", credential_free_url(url)),
        ),
        "zip" | "pip" => credential_free_url(source),
        _ => source.to_string(),
    }
}

fn normalize_action_identity(identity: &str) -> String {
    let mut lines = identity.split('\n').map(str::to_string).collect::<Vec<_>>();
    match lines.first().map(String::as_str) {
        Some("git") | Some("zip") if lines.len() >= 4 => {
            lines[1] = credential_free_url(&lines[1]);
        }
        Some("pip") if lines.len() >= 5 => {
            lines[3] = credential_free_url(&lines[3]);
        }
        _ => {}
    }
    lines.join("\n")
}

fn receipt_actions(plan: &InstallPlan) -> impl Iterator<Item = &InstallStepAction> {
    plan.steps
        .iter()
        .filter_map(|step| match step.action.as_ref()? {
            InstallStepAction::PipInstall { .. }
            | InstallStepAction::GitClone { .. }
            | InstallStepAction::ZipExtract { .. }
            | InstallStepAction::PathCopy { .. }
            | InstallStepAction::AdobePluginLink { .. } => step.action.as_ref(),
            InstallStepAction::RegisterDcc { .. } | InstallStepAction::Verify => None,
        })
}

fn normalized_extras(plan: &InstallPlan) -> Vec<String> {
    let mut extras = plan
        .steps
        .iter()
        .find_map(|step| match step.action.as_ref()? {
            InstallStepAction::PipInstall { extras, .. } => extras.clone(),
            _ => None,
        })
        .unwrap_or_default();
    extras.sort_unstable();
    extras.dedup();
    extras
}

fn normalized_sha256(value: Option<&str>) -> String {
    let value = value.unwrap_or_default().trim();
    value
        .strip_prefix("sha256:")
        .unwrap_or(value)
        .to_ascii_lowercase()
}

fn pip_python(plan: &InstallPlan) -> Option<&str> {
    plan.steps
        .iter()
        .find_map(|step| match step.action.as_ref()? {
            InstallStepAction::PipInstall { python, .. } => python.as_deref(),
            _ => None,
        })
}

fn safe_filename(value: &str) -> String {
    let sanitized = value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.') {
                character.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .take(64)
        .collect::<String>();
    if sanitized.is_empty() {
        "adapter".into()
    } else {
        sanitized
    }
}

fn legacy_safe_filename(value: &str) -> Option<String> {
    let sanitized = value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.') {
                character.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect::<String>();
    if sanitized.is_empty() {
        Some("adapter".into())
    } else if sanitized.len() > 225 {
        None
    } else {
        Some(sanitized)
    }
}

#[cfg(test)]
#[path = "receipt_tests.rs"]
mod tests;
