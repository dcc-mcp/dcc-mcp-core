use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::add_repo::collect_skill_dirs;
use crate::error::MarketplaceError;
use crate::service::{
    copy_dir_recursive, path_component, promote_single_nested_skill_directory, remove_path,
    write_atomic,
};

const BUNDLE_DIR: &str = ".packages";
const BUNDLE_MANIFEST: &str = "installed-skills.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MarketplaceBundleManifest {
    skills: Vec<String>,
}

pub(crate) fn install_staged_package(
    staging: &Path,
    dest: &Path,
    dcc_root: &Path,
    package_name: &str,
    dcc: &str,
    force: bool,
) -> Result<PathBuf, MarketplaceError> {
    let all_skill_dirs = collect_skill_dirs(staging);
    if all_skill_dirs.len() > 1 {
        let skill_dirs = select_bundle_skill_dirs(staging, &all_skill_dirs);
        let bundle_dest = install_skill_bundle(&skill_dirs, dcc_root, package_name, dcc, force)?;
        remove_path(staging)?;
        return Ok(bundle_dest);
    }

    promote_single_nested_skill_directory(staging)?;

    let skill_md = staging.join("SKILL.md");
    if !skill_md.is_file() {
        return Err(MarketplaceError::MissingSkill(
            skill_md.display().to_string(),
        ));
    }

    if dest.exists() {
        if !force {
            return Err(MarketplaceError::AlreadyInstalled {
                name: package_name.to_string(),
                dcc: dcc.to_string(),
                path: dest.display().to_string(),
            });
        }
        remove_path(dest)?;
    }
    fs::rename(staging, dest)
        .map_err(|err| MarketplaceError::ConfigIo(dest.display().to_string(), err))?;
    Ok(dest.to_path_buf())
}

pub(crate) fn bundle_package_dir(dcc_root: &Path, package_name: &str) -> PathBuf {
    dcc_root.join(BUNDLE_DIR).join(package_name)
}

pub(crate) fn remove_installed_path(dcc_root: &Path, dest: &Path) -> Result<(), MarketplaceError> {
    if let Some(manifest) = read_bundle_manifest(dest)? {
        for skill in manifest.skills {
            let target = dcc_root.join(path_component("skill name", &skill)?);
            if target.exists() {
                remove_path(&target)?;
            }
        }
    }
    remove_path(dest)
}

fn select_bundle_skill_dirs(root: &Path, skill_dirs: &[PathBuf]) -> Vec<PathBuf> {
    let preferred: Vec<PathBuf> = skill_dirs
        .iter()
        .filter(|dir| is_preferred_bundle_skill_dir(root, dir))
        .cloned()
        .collect();
    if preferred.is_empty() {
        skill_dirs.to_vec()
    } else {
        preferred
    }
}

fn is_preferred_bundle_skill_dir(root: &Path, skill_dir: &Path) -> bool {
    let rel = skill_dir.strip_prefix(root).unwrap_or(skill_dir);
    if rel.components().any(|component| {
        component
            .as_os_str()
            .to_str()
            .is_some_and(|name| matches!(name, "example" | "examples"))
    }) {
        return false;
    }
    skill_dir
        .parent()
        .and_then(Path::file_name)
        .and_then(|name| name.to_str())
        .is_some_and(|name| matches!(name, "skill" | "skills"))
}

fn install_skill_bundle(
    skill_dirs: &[PathBuf],
    dcc_root: &Path,
    package_name: &str,
    dcc: &str,
    force: bool,
) -> Result<PathBuf, MarketplaceError> {
    let bundle_dest = bundle_package_dir(dcc_root, package_name);
    if bundle_dest.exists() {
        if !force {
            return Err(MarketplaceError::AlreadyInstalled {
                name: package_name.to_string(),
                dcc: dcc.to_string(),
                path: bundle_dest.display().to_string(),
            });
        }
        remove_path(&bundle_dest)?;
    }

    let mut plan = Vec::new();
    let mut names = Vec::new();
    for skill_dir in skill_dirs {
        let skill_name = skill_dir_name(skill_dir)?;
        if names.contains(&skill_name) {
            return Err(MarketplaceError::CommandFailed(format!(
                "duplicate skill name '{skill_name}' in marketplace package"
            )));
        }
        let target = dcc_root.join(&skill_name);
        if target.exists() && !force {
            return Err(MarketplaceError::AlreadyInstalled {
                name: skill_name,
                dcc: dcc.to_string(),
                path: target.display().to_string(),
            });
        }
        names.push(skill_name.clone());
        plan.push((skill_dir.clone(), target));
    }

    for (_, target) in &plan {
        if target.exists() {
            remove_path(target)?;
        }
    }

    let mut copied = Vec::new();
    for (skill_dir, target) in &plan {
        if let Err(err) = copy_dir_recursive(skill_dir, target) {
            cleanup_paths(&copied);
            return Err(err);
        }
        copied.push(target.clone());
    }

    if let Err(err) = write_bundle_manifest(&bundle_dest, names) {
        cleanup_paths(&copied);
        let _ = remove_path(&bundle_dest);
        return Err(err);
    }

    Ok(bundle_dest)
}

fn skill_dir_name(skill_dir: &Path) -> Result<String, MarketplaceError> {
    let Some(name) = skill_dir.file_name().and_then(|name| name.to_str()) else {
        return Err(MarketplaceError::CommandFailed(format!(
            "invalid skill directory '{}'",
            skill_dir.display()
        )));
    };
    path_component("skill name", name)
}

fn write_bundle_manifest(bundle_dest: &Path, skills: Vec<String>) -> Result<(), MarketplaceError> {
    fs::create_dir_all(bundle_dest)
        .map_err(|err| MarketplaceError::ConfigIo(bundle_dest.display().to_string(), err))?;
    let text = serde_json::to_string_pretty(&MarketplaceBundleManifest { skills })
        .expect("MarketplaceBundleManifest serialization should not fail");
    write_atomic(&bundle_dest.join(BUNDLE_MANIFEST), &text)
}

fn read_bundle_manifest(
    bundle_dest: &Path,
) -> Result<Option<MarketplaceBundleManifest>, MarketplaceError> {
    let path = bundle_dest.join(BUNDLE_MANIFEST);
    if !path.is_file() {
        return Ok(None);
    }
    let text = fs::read_to_string(&path)
        .map_err(|err| MarketplaceError::ConfigIo(path.display().to_string(), err))?;
    serde_json::from_str(&text)
        .map(Some)
        .map_err(|err| MarketplaceError::ConfigParse(path.display().to_string(), err))
}

fn cleanup_paths(paths: &[PathBuf]) {
    for path in paths {
        let _ = remove_path(path);
    }
}
