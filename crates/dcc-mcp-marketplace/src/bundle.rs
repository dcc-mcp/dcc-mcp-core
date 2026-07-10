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
    skill_roots: Option<&[String]>,
    force: bool,
) -> Result<PathBuf, MarketplaceError> {
    let all_skill_dirs = collect_skill_dirs(staging);
    if let Some(skill_roots) = skill_roots {
        let skill_dirs = select_configured_skill_dirs(staging, skill_roots)?;
        let bundle_dest = install_skill_bundle(&skill_dirs, dcc_root, package_name, dcc, force)?;
        remove_path(staging)?;
        return Ok(bundle_dest);
    }
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

fn select_configured_skill_dirs(
    staging: &Path,
    skill_roots: &[String],
) -> Result<Vec<PathBuf>, MarketplaceError> {
    if skill_roots.is_empty() {
        return Err(MarketplaceError::CommandFailed(
            "marketplace skillRoots must not be empty".into(),
        ));
    }

    let source_root = source_root(staging, skill_roots)?;
    let mut selected = Vec::new();
    for skill_root in skill_roots {
        let relative = safe_relative_skill_root(skill_root)?;
        let root = source_root.join(relative);
        if !root.is_dir() {
            return Err(MarketplaceError::CommandFailed(format!(
                "marketplace skillRoot '{skill_root}' does not exist in package"
            )));
        }
        let skill_dirs = collect_skill_dirs(&root);
        if skill_dirs.is_empty() {
            return Err(MarketplaceError::MissingSkill(root.display().to_string()));
        }
        for skill_dir in skill_dirs {
            if !selected.contains(&skill_dir) {
                selected.push(skill_dir);
            }
        }
    }
    Ok(selected)
}

fn source_root(staging: &Path, skill_roots: &[String]) -> Result<PathBuf, MarketplaceError> {
    if skill_roots
        .iter()
        .filter_map(|root| safe_relative_skill_root(root).ok())
        .any(|root| staging.join(root).is_dir())
    {
        return Ok(staging.to_path_buf());
    }

    let children: Vec<PathBuf> = fs::read_dir(staging)
        .map_err(|err| MarketplaceError::ConfigIo(staging.display().to_string(), err))?
        .flatten()
        .filter_map(|entry| {
            entry
                .file_type()
                .ok()
                .filter(|kind| kind.is_dir())
                .map(|_| entry.path())
        })
        .collect();
    let [child] = children.as_slice() else {
        return Ok(staging.to_path_buf());
    };
    Ok(child.clone())
}

fn safe_relative_skill_root(value: &str) -> Result<PathBuf, MarketplaceError> {
    let path = Path::new(value);
    if value.trim().is_empty()
        || path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir
                    | std::path::Component::CurDir
                    | std::path::Component::RootDir
                    | std::path::Component::Prefix(_)
            )
        })
    {
        return Err(MarketplaceError::CommandFailed(format!(
            "marketplace skillRoot '{value}' must be a safe relative path"
        )));
    }
    Ok(path.to_path_buf())
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

#[cfg(test)]
mod tests {
    use super::*;

    fn write_skill(path: &Path) {
        fs::create_dir_all(path).unwrap();
        fs::write(
            path.join("SKILL.md"),
            "---\nname: test\ndescription: Test\n---\n",
        )
        .unwrap();
    }

    #[test]
    fn configured_skill_roots_install_only_allowlisted_skills() {
        let tmp = tempfile::tempdir().unwrap();
        let staging = tmp.path().join("staging");
        write_skill(&staging.join("repo").join("skill").join("allowed"));
        write_skill(&staging.join("repo").join("examples").join("unwanted"));
        let dcc_root = tmp.path().join("marketplace").join("maya");
        fs::create_dir_all(&dcc_root).unwrap();
        let roots = vec!["skill".to_string()];

        let installed = install_staged_package(
            &staging,
            &dcc_root.join("package"),
            &dcc_root,
            "package",
            "maya",
            Some(&roots),
            false,
        )
        .unwrap();

        assert_eq!(installed, bundle_package_dir(&dcc_root, "package"));
        assert!(dcc_root.join("allowed").join("SKILL.md").is_file());
        assert!(!dcc_root.join("unwanted").exists());
        assert!(!staging.exists());
    }

    #[test]
    fn skill_roots_reject_parent_traversal() {
        assert!(safe_relative_skill_root("../examples").is_err());
    }
}
