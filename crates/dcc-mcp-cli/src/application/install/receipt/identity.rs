use std::path::PathBuf;

use super::super::resolve_install_source_path;
use crate::domain::install::{InstallPlan, InstallStepAction};

pub(crate) fn default_receipt_directory() -> PathBuf {
    dirs::data_dir()
        .map(|path| path.join("dcc-mcp").join("install-receipts"))
        .unwrap_or_else(|| PathBuf::from("~/.local/share/dcc-mcp/install-receipts"))
}

pub(crate) fn first_install_action(plan: &InstallPlan) -> Option<&InstallStepAction> {
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

pub(crate) fn receipt_path_identity(plan: &InstallPlan) -> String {
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

pub(crate) fn action_identity(action: &InstallStepAction) -> String {
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

pub(crate) fn historical_action_identity(action: &InstallStepAction) -> String {
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

pub(crate) fn receipt_version(plan: &InstallPlan, action: &InstallStepAction) -> Option<String> {
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

pub(crate) fn normalized_git_ref(value: Option<&str>) -> String {
    value.unwrap_or_default().trim().to_ascii_lowercase()
}

pub(crate) fn credential_free_url(value: &str) -> String {
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

pub(crate) fn normalize_receipt_source(install_type: &str, source: &str) -> String {
    match install_type {
        "git" => source.rsplit_once('@').map_or_else(
            || credential_free_url(source),
            |(url, reference)| format!("{}@{reference}", credential_free_url(url)),
        ),
        "zip" | "pip" => credential_free_url(source),
        _ => source.to_string(),
    }
}

pub(crate) fn normalize_action_identity(identity: &str) -> String {
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

pub(crate) fn receipt_actions(plan: &InstallPlan) -> impl Iterator<Item = &InstallStepAction> {
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

pub(crate) fn normalized_extras(plan: &InstallPlan) -> Vec<String> {
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

pub(crate) fn normalized_sha256(value: Option<&str>) -> String {
    let value = value.unwrap_or_default().trim();
    value
        .strip_prefix("sha256:")
        .unwrap_or(value)
        .to_ascii_lowercase()
}

pub(crate) fn pip_python(plan: &InstallPlan) -> Option<&str> {
    plan.steps
        .iter()
        .find_map(|step| match step.action.as_ref()? {
            InstallStepAction::PipInstall { python, .. } => python.as_deref(),
            _ => None,
        })
}

pub(crate) fn safe_filename(value: &str) -> String {
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

pub(crate) fn legacy_safe_filename(value: &str) -> Option<String> {
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
