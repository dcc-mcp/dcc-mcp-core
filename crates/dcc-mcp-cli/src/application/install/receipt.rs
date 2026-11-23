use std::fs;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

use fs4::FileExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::domain::install::{InstallPlan, InstallStepAction};

use super::pip::{pip_show_args, pip_show_version};
use super::resolve_install_source_path;

mod identity;
mod ownership;
mod receipt_matching;
pub(super) use identity::*;
#[cfg(test)]
pub(super) use ownership::path_digest;
use ownership::{path_digest_for_receipt, path_digest_with_policy, replace_receipt_file};
pub(super) use receipt_matching::receipt_match_details;

#[cfg(test)]
#[path = "receipt_tests.rs"]
mod tests;

const RECEIPT_SCHEMA_VERSION: u8 = 1;
const MAX_RECEIPT_BYTES: u64 = 128 * 1024;
const OWNED_DIGEST_VERSION: u8 = 2;

pub(super) struct ReceiptLock {
    file: fs::File,
}

impl Drop for ReceiptLock {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.file);
    }
}

pub(super) fn lock_receipt_for_plan(plan: &InstallPlan) -> Result<Option<ReceiptLock>, String> {
    if receipt_for_plan(plan).is_none() {
        return Ok(None);
    }
    let path = receipt_path_for_plan(plan);
    validate_receipt_location(plan, &path)?;
    let parent = path
        .parent()
        .ok_or_else(|| "install receipt has no parent directory".to_string())?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("failed to create install receipt directory: {error}"))?;
    let lock_path = receipt_lock_path(plan, &path);
    lock_receipt_at(&lock_path).map(Some)
}

fn lock_receipt_at(lock_path: &Path) -> Result<ReceiptLock, String> {
    let file = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(lock_path)
        .map_err(|error| format!("failed to open install receipt lock: {error}"))?;
    FileExt::try_lock(&file).map_err(|error| format!("install receipt is busy: {error}"))?;
    Ok(ReceiptLock { file })
}

fn receipt_lock_path(plan: &InstallPlan, receipt_path: &Path) -> PathBuf {
    let pip_identity = receipt_actions(plan).filter_map(|action| match action {
        InstallStepAction::PipInstall {
            package, python, ..
        } => Some(format!(
            "{package}\n{}",
            python
                .as_deref()
                .and_then(python_environment_identity)
                .unwrap_or_else(|| python.as_deref().unwrap_or_default().to_string())
        )),
        _ => None,
    });
    let identity = pip_identity.collect::<Vec<_>>().join("\n");
    if !identity.is_empty() {
        let digest = Sha256::digest(identity.as_bytes());
        let suffix = digest
            .iter()
            .take(16)
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        return receipt_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(format!(".pip-{suffix}.lock"));
    }
    receipt_path.with_extension("lock")
}

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

pub(super) fn receipt_for_uninstall(
    plan: &InstallPlan,
) -> Result<Option<(PathBuf, InstallReceipt)>, String> {
    if owned_path_specs(plan)
        .iter()
        .any(|(_, path, _, _, _)| path.to_str().is_none())
    {
        return Err("install destination path is not representable in the receipt format".into());
    }
    let directory = receipt_path_for_plan(plan)
        .parent()
        .ok_or_else(|| "install receipt has no parent directory".to_string())?
        .to_path_buf();
    let mut candidate_paths = vec![receipt_path_for_plan(plan)];
    if let Some(path) = legacy_receipt_path_for_plan(plan) {
        candidate_paths.push(path);
    }
    if let Some(path) = legacy_non_pip_receipt_path_for_plan(plan) {
        candidate_paths.push(path);
    }
    let known_receipt_paths = candidate_paths.clone();
    match fs::read_dir(&directory) {
        Ok(entries) => {
            for entry in entries {
                let entry = entry
                    .map_err(|error| format!("failed to inspect install receipts: {error}"))?;
                let path = entry.path();
                if path
                    .extension()
                    .is_some_and(|extension| extension == "json")
                {
                    candidate_paths.push(path);
                }
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("failed to inspect install receipts: {error}")),
    }
    candidate_paths.sort();
    candidate_paths.dedup();

    let mut matches = Vec::new();
    let mut target_conflict = false;
    for path in candidate_paths {
        if let Err(error) = validate_receipt_location(plan, &path) {
            if known_receipt_paths.iter().any(|known| known == &path) {
                return Err(error);
            }
            continue;
        }
        let receipt = match read_receipt(&path) {
            Ok(receipt) => receipt,
            Err(error) if known_receipt_paths.iter().any(|known| known == &path) => {
                return Err(error);
            }
            Err(_) => continue,
        };
        let Some(receipt) = receipt else {
            continue;
        };
        if receipt_matches_uninstall_target(plan, &receipt) {
            matches.push((path, receipt));
        } else if receipt_has_install_identity(plan, &receipt) {
            target_conflict = true;
        }
    }
    if matches.is_empty() {
        if target_conflict {
            return Err("install receipt does not match the current install target".into());
        }
        return Ok(None);
    }
    if matches.len() == 1 {
        return Ok(matches.pop());
    }

    if matches[0].1.install_type != "pip" {
        let safe = matches
            .into_iter()
            .filter_map(
                |candidate| match receipt_uninstall_ownership_is_safe(plan, &candidate.1) {
                    Ok(true) => Some(Ok(candidate)),
                    Ok(false) => None,
                    Err(error) => Some(Err(error)),
                },
            )
            .collect::<Result<Vec<_>, _>>()?;
        return match safe.as_slice() {
            [candidate] => Ok(Some(candidate.clone())),
            [] => Err("no unambiguous install receipt matches the current adapter state".into()),
            _ => Err("multiple install receipts match the current adapter state".into()),
        };
    }
    let mut matching = Vec::new();
    let mut absent = Vec::new();
    for candidate in matches {
        match pip_receipt_state(&candidate.1)? {
            PipReceiptState::Matching => matching.push(candidate),
            PipReceiptState::Absent => absent.push(candidate),
            PipReceiptState::Drifted => {}
        }
    }
    match matching.as_slice() {
        [candidate] => return Ok(Some(candidate.clone())),
        [] => {}
        _ => {
            return Err(
                "multiple installed package receipts match the current adapter state".into(),
            );
        }
    }
    if let Some(current) = absent
        .iter()
        .position(|(path, _)| *path == receipt_path_for_plan(plan))
    {
        return Ok(Some(absent.remove(current)));
    }
    match absent.as_slice() {
        [candidate] => Ok(Some(candidate.clone())),
        [] => Err("no unambiguous install receipt matches the current adapter state".into()),
        _ => Err("multiple absent package receipts match the current adapter state".into()),
    }
}

fn receipt_matches_uninstall_target(plan: &InstallPlan, receipt: &InstallReceipt) -> bool {
    let Some(expected) = receipt_for_plan(plan) else {
        return false;
    };
    if receipt.schema_version != expected.schema_version
        || receipt.dcc_type != expected.dcc_type
        || receipt.adapter_name != expected.adapter_name
        || receipt.install_type != expected.install_type
    {
        return false;
    }
    if receipt.install_type == "pip" {
        receipt.package == expected.package
            && python_environments_match(receipt.python.as_deref(), expected.python.as_deref())
    } else {
        same_owned_path_set(&receipt.owned_paths, &owned_path_specs(plan))
    }
}

fn receipt_has_install_identity(plan: &InstallPlan, receipt: &InstallReceipt) -> bool {
    let Some(expected) = receipt_for_plan(plan) else {
        return false;
    };
    receipt.schema_version == expected.schema_version
        && receipt.dcc_type == expected.dcc_type
        && receipt.adapter_name == expected.adapter_name
        && receipt.install_type == expected.install_type
}

pub(super) fn receipt_uninstall_ownership_is_safe(
    plan: &InstallPlan,
    receipt: &InstallReceipt,
) -> Result<bool, String> {
    if !receipt_matches_uninstall_target(plan, receipt) {
        return Ok(false);
    }
    let expected = owned_path_specs(plan);
    if !same_owned_path_set(&receipt.owned_paths, &expected) {
        return Ok(false);
    }
    let mut seen = Vec::new();
    for owned in &receipt.owned_paths {
        if !seen.iter().all(|path: &String| path != &owned.path) {
            return Ok(false);
        }
        seen.push(owned.path.clone());
        let Some((action, path, nested, root, exclude_git)) = expected
            .iter()
            .find(|(_, path, _, _, _)| path.display().to_string() == owned.path)
            .map(|(action, path, nested, root, exclude_git)| {
                (action, path, nested, root, exclude_git)
            })
        else {
            return Ok(false);
        };
        match fs::symlink_metadata(path) {
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(format!("failed to inspect owned path: {error}")),
        }
        let digest = path_digest_for_receipt(
            path,
            *nested,
            *root,
            *exclude_git,
            receipt.owned_digest_version,
        )?;
        if digest.as_deref() != Some(owned.sha256.as_str()) {
            return Ok(false);
        }
        if matches!(action, InstallStepAction::GitClone { .. })
            && !git_repository_is_safe_to_remove(path, receipt_git_ref(receipt))?
        {
            return Ok(false);
        }
    }
    if receipt.install_type == "pip" && !pip_receipt_is_safe_to_uninstall(receipt)? {
        return Ok(false);
    }
    Ok(true)
}

fn same_owned_path_set(
    actual: &[ReceiptOwnedPath],
    expected: &[(InstallStepAction, PathBuf, bool, bool, bool)],
) -> bool {
    let mut actual_paths = actual
        .iter()
        .map(|owned| owned.path.clone())
        .collect::<Vec<_>>();
    let expected_paths = expected
        .iter()
        .map(|(_, path, _, _, _)| path.to_str().map(str::to_owned))
        .collect::<Option<Vec<_>>>();
    let Some(mut expected_paths) = expected_paths else {
        return false;
    };
    actual_paths.sort_unstable();
    expected_paths.sort_unstable();
    actual_paths == expected_paths && actual_paths.windows(2).all(|paths| paths[0] != paths[1])
}

fn git_repository_is_safe_to_remove(
    path: &Path,
    expected_ref: Option<&str>,
) -> Result<bool, String> {
    let Some(expected_ref) = expected_ref else {
        return Ok(false);
    };
    let status = Command::new("git")
        .arg("-C")
        .arg(path)
        .arg("--no-optional-locks")
        .args(["status", "--porcelain=v1", "--untracked-files=all"])
        .output()
        .map_err(|error| format!("failed to inspect Git ownership state: {error}"))?;
    if !status.status.success() || !status.stdout.is_empty() {
        return Ok(false);
    }
    let head = Command::new("git")
        .arg("-C")
        .arg(path)
        .arg("rev-parse")
        .arg("HEAD")
        .output()
        .map_err(|error| format!("failed to inspect Git HEAD: {error}"))?;
    if !head.status.success()
        || !String::from_utf8_lossy(&head.stdout)
            .trim()
            .eq_ignore_ascii_case(expected_ref)
    {
        return Ok(false);
    }
    let stashes = Command::new("git")
        .arg("-C")
        .arg(path)
        .arg("--no-optional-locks")
        .args(["stash", "list", "--format=%H"])
        .output()
        .map_err(|error| format!("failed to inspect Git stash state: {error}"))?;
    Ok(stashes.status.success() && stashes.stdout.is_empty())
}

fn receipt_git_ref(receipt: &InstallReceipt) -> Option<&str> {
    (receipt.install_type == "git")
        .then(|| {
            receipt
                .source
                .rsplit_once('@')
                .map(|(_, reference)| reference)
        })
        .flatten()
}

pub(super) fn receipt_pip_target(receipt: &InstallReceipt) -> Option<(String, String)> {
    (receipt.install_type == "pip")
        .then(|| Some((receipt.package.clone(), receipt.python.clone()?)))
        .flatten()
}

pub(super) fn receipt_owned_paths(receipt: &InstallReceipt) -> Vec<PathBuf> {
    receipt
        .owned_paths
        .iter()
        .map(|owned| PathBuf::from(&owned.path))
        .collect()
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

#[cfg(test)]
pub(super) fn receipt_matches_action(
    plan: &InstallPlan,
    action: &InstallStepAction,
) -> Result<bool, String> {
    Ok(receipt_match_details(plan, action)?.0)
}

pub(super) fn commit_install_receipt(
    plan: &InstallPlan,
    skipped_actions: &[(InstallStepAction, Option<ReceiptOwnershipDigest>)],
    acquired_actions: &[InstallStepAction],
) -> Result<bool, String> {
    if receipt_for_plan(plan).is_none() {
        return Ok(false);
    }
    let lock = lock_receipt_for_plan(plan)?.ok_or_else(|| {
        "install receipt lock is unavailable for a receipted install plan".to_string()
    })?;
    commit_install_receipt_with_lock(plan, skipped_actions, acquired_actions, &lock)
}

pub(super) fn commit_install_receipt_with_lock(
    plan: &InstallPlan,
    skipped_actions: &[(InstallStepAction, Option<ReceiptOwnershipDigest>)],
    acquired_actions: &[InstallStepAction],
    _receipt_lock: &ReceiptLock,
) -> Result<bool, String> {
    let Some(mut receipt) = receipt_for_plan(plan) else {
        return Ok(false);
    };
    let configured_path = receipt_path_for_plan(plan);
    validate_receipt_location(plan, &configured_path)?;
    let path = resolve_receipt_for_containment(&configured_path)?;
    validate_receipt_location(plan, &path)?;
    let parent = path
        .parent()
        .ok_or_else(|| "install receipt has no parent directory".to_string())?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("failed to create install receipt directory: {error}"))?;
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
    if !pip_receipt_matches_artifact(expected, &output.stdout) {
        return Ok(false);
    }
    let check = match Command::new(python).args(["-m", "pip", "check"]).output() {
        Ok(output) => output,
        Err(_) => return Ok(false),
    };
    if !check.status.success() {
        return Ok(false);
    }
    Ok(true)
}

fn pip_receipt_matches_artifact(expected: &InstallReceipt, stdout: &[u8]) -> bool {
    let output_text = String::from_utf8_lossy(stdout);
    let Some(location) = output_text
        .lines()
        .find_map(|line| line.strip_prefix("Location:").map(str::trim))
        .filter(|value| !value.is_empty())
    else {
        return false;
    };
    let Some((url, sha256)) = direct_url_identity(
        Path::new(location),
        &expected.package,
        Some(&expected.adapter_version),
    ) else {
        return false;
    };
    credential_free_url(&url) == expected.artifact_url && sha256 == expected.sha256
}

fn pip_receipt_is_safe_to_uninstall(expected: &InstallReceipt) -> Result<bool, String> {
    Ok(!matches!(
        pip_receipt_state(expected)?,
        PipReceiptState::Drifted
    ))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PipReceiptState {
    Absent,
    Matching,
    Drifted,
}

fn pip_receipt_state(expected: &InstallReceipt) -> Result<PipReceiptState, String> {
    let Some(python) = expected.python.as_deref().filter(|value| !value.is_empty()) else {
        return Ok(PipReceiptState::Drifted);
    };
    let output = Command::new(python)
        .args(pip_show_args(&expected.package))
        .output()
        .map_err(|error| format!("failed to inspect the installed package: {error}"))?;
    if !output.status.success() {
        return Ok(if pip_show_reports_missing_package(&output.stderr) {
            PipReceiptState::Absent
        } else {
            PipReceiptState::Drifted
        });
    }
    Ok(
        if pip_show_version(&output.stdout).as_deref() == Some(expected.adapter_version.as_str())
            && pip_receipt_matches_artifact(expected, &output.stdout)
        {
            PipReceiptState::Matching
        } else {
            PipReceiptState::Drifted
        },
    )
}

fn pip_show_reports_missing_package(stderr: &[u8]) -> bool {
    String::from_utf8_lossy(stderr)
        .to_ascii_lowercase()
        .contains("package(s) not found")
}

fn python_environments_match(actual: Option<&str>, expected: Option<&str>) -> bool {
    match (actual, expected) {
        (Some(actual), Some(expected)) => {
            python_environment_identity(actual) == python_environment_identity(expected)
        }
        _ => false,
    }
}

fn python_environment_identity(python: &str) -> Option<String> {
    let output = Command::new(python)
        .args(["-c", "import sys; print(sys.executable)"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8(output.stdout).ok()?;
    let resolved = value.trim();
    if resolved.is_empty() {
        return None;
    }
    fs::canonicalize(resolved)
        .ok()
        .map(|path| path.to_string_lossy().to_ascii_lowercase())
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
        if receipt_path.starts_with(resolve_receipt_for_containment(&destination)?) {
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

fn direct_url_identity(
    location: &Path,
    package: &str,
    version: Option<&str>,
) -> Option<(String, String)> {
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
        let prefix = version.map_or_else(
            || format!("{normalized_package}-"),
            |version| {
                format!(
                    "{normalized_package}-{}",
                    normalize_distribution_version(version)
                )
            },
        );
        let Some(suffix) = name.strip_prefix(&prefix) else {
            continue;
        };
        if !name.ends_with(".dist-info")
            || (version.is_some() && !suffix.starts_with(".dist-info") && !suffix.starts_with('-'))
        {
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

fn normalize_distribution_version(version: &str) -> String {
    version
        .chars()
        .map(|character| match character {
            '-' => '_',
            other => other.to_ascii_lowercase(),
        })
        .collect()
}
