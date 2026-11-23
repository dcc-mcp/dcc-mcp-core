use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::domain::install::{InstallPlan, InstallStepAction};

use super::pip::{pip_show_args, pip_show_version};

const RECEIPT_SCHEMA_VERSION: u8 = 1;
const MAX_RECEIPT_BYTES: u64 = 128 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct InstallReceipt {
    pub schema_version: u8,
    pub dcc_type: String,
    pub adapter_name: String,
    pub adapter_version: String,
    pub python: Option<String>,
    pub package: String,
    pub artifact_url: String,
    pub sha256: String,
    #[serde(default)]
    pub extras: Vec<String>,
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
        pip_python(plan).unwrap_or_default()
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

pub(super) fn pip_receipt_for_plan(plan: &InstallPlan) -> Option<InstallReceipt> {
    let InstallStepAction::PipInstall {
        package,
        version,
        python,
        artifact_url,
        sha256,
        ..
    } = plan.steps.iter().find_map(|step| step.action.as_ref())?
    else {
        return None;
    };
    Some(InstallReceipt {
        schema_version: RECEIPT_SCHEMA_VERSION,
        dcc_type: plan.dcc_type.clone(),
        adapter_name: plan.adapter.name.clone(),
        adapter_version: version.clone().unwrap_or_default(),
        python: python.clone(),
        package: package.clone(),
        artifact_url: artifact_url.clone().unwrap_or_default(),
        sha256: sha256
            .as_deref()
            .unwrap_or_default()
            .trim()
            .trim_start_matches("sha256:")
            .to_ascii_lowercase(),
        extras: normalized_extras(plan),
    })
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
    let receipt: InstallReceipt = serde_json::from_slice(&bytes)
        .map_err(|error| format!("install receipt is invalid JSON: {error}"))?;
    if receipt.schema_version != RECEIPT_SCHEMA_VERSION
        || receipt.dcc_type.trim().is_empty()
        || receipt.adapter_name.trim().is_empty()
        || receipt.adapter_version.trim().is_empty()
        || receipt.package.trim().is_empty()
        || !receipt.artifact_url.starts_with("https://")
        || receipt.sha256.len() != 64
        || !receipt.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err("install receipt has an incompatible identity".into());
    }
    Ok(Some(receipt))
}

pub(super) fn receipt_matches_pip(plan: &InstallPlan) -> Result<bool, String> {
    let Some(expected) = pip_receipt_for_plan(plan) else {
        return Ok(false);
    };
    let Some(actual) = read_receipt(&receipt_path_for_plan(plan))? else {
        return Ok(false);
    };
    if actual != expected {
        return Ok(false);
    }
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
    if !output.status.success() {
        return Ok(false);
    }
    if pip_show_version(&output.stdout).as_deref() != Some(expected.adapter_version.as_str()) {
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
    Ok(url == expected.artifact_url && sha256 == expected.sha256)
}

pub(super) fn commit_pip_receipt(plan: &InstallPlan) -> Result<bool, String> {
    let Some(receipt) = pip_receipt_for_plan(plan) else {
        return Ok(false);
    };
    let path = receipt_path_for_plan(plan);
    match read_receipt(&path)? {
        Some(existing) if existing == receipt => return Ok(true),
        _ => {}
    }
    let parent = path
        .parent()
        .ok_or_else(|| "install receipt has no parent directory".to_string())?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("failed to create install receipt directory: {error}"))?;
    let bytes = serde_json::to_vec_pretty(&receipt)
        .map_err(|error| format!("failed to serialize install receipt: {error}"))?;
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

fn replace_receipt_file(
    temporary: tempfile::NamedTempFile,
    destination: &Path,
) -> Result<(), String> {
    #[cfg(not(windows))]
    {
        temporary
            .persist(destination)
            .map_err(|error| format!("failed to commit install receipt: {error}"))?;
        return Ok(());
    }

    #[cfg(windows)]
    {
        let previous = destination
            .exists()
            .then(|| fs::read(destination))
            .transpose()
            .map_err(|error| format!("failed to back up install receipt: {error}"))?;
        if destination.exists() {
            fs::remove_file(destination)
                .map_err(|error| format!("failed to replace install receipt: {error}"))?;
        }
        match temporary.persist(destination) {
            Ok(_) => Ok(()),
            Err(error) => {
                if let Some(previous) = previous {
                    let _ = fs::write(destination, previous);
                }
                Err(format!("failed to commit install receipt: {error}"))
            }
        }
    }
}

fn default_receipt_directory() -> PathBuf {
    dirs::data_dir()
        .map(|path| path.join("dcc-mcp").join("install-receipts"))
        .unwrap_or_else(|| PathBuf::from("~/.local/share/dcc-mcp/install-receipts"))
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
        .collect::<String>();
    if sanitized.is_empty() {
        "adapter".into()
    } else {
        sanitized
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn receipt_filename_is_bounded_to_an_adapter_identity() {
        assert_eq!(safe_filename("DCC/Maya"), "dcc_maya");
        assert_eq!(safe_filename(""), "adapter");
    }
}
