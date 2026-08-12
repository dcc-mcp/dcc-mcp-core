//! Package-handler port and infrastructure adapters.

use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

use dcc_mcp_catalog::{CatalogComponent, CatalogComponentKind, CatalogPackageFormat};

use crate::bundle::{install_staged_package, remove_installed_path};
use crate::error::MarketplaceError;

#[derive(Debug)]
pub(crate) struct InstallRequest<'a> {
    pub staging: &'a Path,
    pub destination: &'a Path,
    pub target_root: &'a Path,
    pub package_name: &'a str,
    pub target_id: &'a str,
    pub skill_roots: Option<&'a [String]>,
    pub components: &'a [CatalogComponent],
    pub force: bool,
}

#[derive(Debug)]
pub(crate) struct UninstallRequest<'a> {
    pub installed_path: &'a Path,
    pub target_root: &'a Path,
    pub components: &'a [CatalogComponent],
}

pub(crate) trait PackageHandler: Send + Sync {
    fn install(&self, request: InstallRequest<'_>) -> Result<PathBuf, MarketplaceError>;
    fn uninstall(&self, request: UninstallRequest<'_>) -> Result<(), MarketplaceError>;
}

pub(crate) fn handler_for(
    format: CatalogPackageFormat,
) -> Result<Box<dyn PackageHandler>, MarketplaceError> {
    match format {
        CatalogPackageFormat::CuaProfile => Ok(Box::new(CuaProfileHandler::default())),
        CatalogPackageFormat::Skill
        | CatalogPackageFormat::SkillBundle
        | CatalogPackageFormat::AgentPlugin => Ok(Box::new(SkillPackageHandler)),
        CatalogPackageFormat::Composite => Err(MarketplaceError::CommandFailed(
            "composite package installation is not supported yet".into(),
        )),
    }
}

struct SkillPackageHandler;

impl PackageHandler for SkillPackageHandler {
    fn install(&self, request: InstallRequest<'_>) -> Result<PathBuf, MarketplaceError> {
        install_staged_package(
            request.staging,
            request.destination,
            request.target_root,
            request.package_name,
            request.target_id,
            request.skill_roots,
            request.force,
        )
    }

    fn uninstall(&self, request: UninstallRequest<'_>) -> Result<(), MarketplaceError> {
        remove_installed_path(request.target_root, request.installed_path)
    }
}

#[derive(Default)]
struct CuaProfileHandler {
    executable: Option<PathBuf>,
}

impl PackageHandler for CuaProfileHandler {
    fn install(&self, request: InstallRequest<'_>) -> Result<PathBuf, MarketplaceError> {
        let component = one_profile_component(request.components)?;
        let profile_root = resolve_component_root(request.staging, &component.root)?;
        self.run(vec![
            OsString::from("profile"),
            OsString::from("validate"),
            profile_root.as_os_str().to_owned(),
        ])?;
        let mut install = vec![
            OsString::from("profile"),
            OsString::from("install"),
            profile_root.as_os_str().to_owned(),
        ];
        if request.force {
            install.push(OsString::from("--replace"));
        }
        self.run(install)?;
        Ok(PathBuf::from(format!("dcc-cua-profile:{}", component.id)))
    }

    fn uninstall(&self, request: UninstallRequest<'_>) -> Result<(), MarketplaceError> {
        let component = one_profile_component(request.components)?;
        self.run([
            OsString::from("profile"),
            OsString::from("uninstall"),
            OsString::from(&component.id),
            OsString::from("--confirm"),
        ])
    }
}

impl CuaProfileHandler {
    fn run(&self, args: impl IntoIterator<Item = OsString>) -> Result<(), MarketplaceError> {
        let executable = self
            .executable
            .clone()
            .or_else(resolve_dcc_cua_executable)
            .unwrap_or_else(|| PathBuf::from("dcc-cua"));
        let mut command = Command::new(&executable);
        command.args(args);
        let output = command.output().map_err(|error| {
            MarketplaceError::CommandFailed(format!(
                "failed to execute '{}': {error}",
                executable.display()
            ))
        })?;
        if output.status.success() {
            return Ok(());
        }
        Err(MarketplaceError::CommandFailed(format!(
            "dcc-cua command exited with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        )))
    }
}

fn resolve_dcc_cua_executable() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("DCC_MCP_CUA_BINARY").map(PathBuf::from) {
        return Some(path);
    }
    let sibling = std::env::current_exe()
        .ok()?
        .with_file_name(if cfg!(windows) {
            "dcc-cua.exe"
        } else {
            "dcc-cua"
        });
    sibling.is_file().then_some(sibling)
}

fn one_profile_component(
    components: &[CatalogComponent],
) -> Result<&CatalogComponent, MarketplaceError> {
    let profiles = components
        .iter()
        .filter(|component| component.kind == CatalogComponentKind::CuaProfile)
        .collect::<Vec<_>>();
    match profiles.as_slice() {
        [component] => Ok(component),
        [] => Err(MarketplaceError::CommandFailed(
            "cua-profile package requires one cua-profile component".into(),
        )),
        _ => Err(MarketplaceError::CommandFailed(
            "cua-profile package must not contain multiple cua-profile components".into(),
        )),
    }
}

fn resolve_component_root(staging: &Path, root: &str) -> Result<PathBuf, MarketplaceError> {
    if root == "." {
        return Ok(staging.to_path_buf());
    }
    let relative = Path::new(root);
    if root.trim().is_empty()
        || relative.is_absolute()
        || relative.components().any(|part| {
            matches!(
                part,
                Component::ParentDir
                    | Component::CurDir
                    | Component::RootDir
                    | Component::Prefix(_)
            )
        })
    {
        return Err(MarketplaceError::CommandFailed(format!(
            "marketplace component root '{root}' must be a safe relative path"
        )));
    }
    let direct = staging.join(relative);
    if direct.is_dir() {
        return Ok(direct);
    }
    let children = std::fs::read_dir(staging)
        .map_err(|error| MarketplaceError::ConfigIo(staging.display().to_string(), error))?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            entry
                .file_type()
                .ok()
                .filter(|kind| kind.is_dir())
                .map(|_| entry.path())
        })
        .collect::<Vec<_>>();
    if let [child] = children.as_slice() {
        let nested = child.join(relative);
        if nested.is_dir() {
            return Ok(nested);
        }
    }
    Err(MarketplaceError::CommandFailed(format!(
        "marketplace component root '{root}' does not exist in package"
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn component_root_rejects_parent_traversal() {
        let temp = tempfile::tempdir().unwrap();
        assert!(resolve_component_root(temp.path(), "../profile").is_err());
    }
}
