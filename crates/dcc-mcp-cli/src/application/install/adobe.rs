use std::fs;
use std::path::Path;

use super::{InstallError, StepRollback};

pub(super) fn execute_plugin_link(
    source: &Path,
    dest: &Path,
    product: &str,
    extension_type: &str,
) -> Result<Option<StepRollback>, InstallError> {
    if !source.is_dir() {
        return Err(InstallError::StepFailed {
            step: "install-adobe-debug-link".into(),
            message: format!(
                "Adobe {product} {extension_type} source does not exist: {}",
                source.display()
            ),
        });
    }
    if let Ok(metadata) = fs::symlink_metadata(dest) {
        if !metadata.file_type().is_symlink() {
            return Err(InstallError::StepFailed {
                step: "install-adobe-debug-link".into(),
                message: format!(
                    "Adobe debug target already exists and is not a link: {}",
                    dest.display()
                ),
            });
        }
        let linked = fs::canonicalize(dest).map_err(|e| InstallError::StepFailed {
            step: "install-adobe-debug-link".into(),
            message: format!(
                "cannot resolve existing Adobe debug link {}: {e}",
                dest.display()
            ),
        })?;
        let expected = fs::canonicalize(source).map_err(|e| InstallError::StepFailed {
            step: "install-adobe-debug-link".into(),
            message: format!(
                "cannot resolve Adobe plugin source {}: {e}",
                source.display()
            ),
        })?;
        if linked == expected {
            return Ok(None);
        }
        return Err(InstallError::StepFailed {
            step: "install-adobe-debug-link".into(),
            message: format!(
                "Adobe debug target {} points to {}, expected {}",
                dest.display(),
                linked.display(),
                expected.display()
            ),
        });
    }
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    create_directory_link(source, dest).map_err(|error| InstallError::StepFailed {
        step: "install-adobe-debug-link".into(),
        message: format!(
            "failed to create Adobe debug link {} -> {}: {error}. On Windows, enable Developer Mode or grant symbolic-link privilege.",
            dest.display(), source.display()
        ),
    })?;
    Ok(Some(StepRollback::RemovePath(dest.to_path_buf())))
}

#[cfg(unix)]
fn create_directory_link(source: &Path, dest: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(source, dest)
}

#[cfg(windows)]
fn create_directory_link(source: &Path, dest: &Path) -> std::io::Result<()> {
    std::os::windows::fs::symlink_dir(source, dest)
}

#[cfg(not(any(unix, windows)))]
fn create_directory_link(_source: &Path, _dest: &Path) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "directory links are unsupported on this platform",
    ))
}

pub(super) fn verify_linked_plugin(
    dest: &Path,
    manifest: Option<&Path>,
) -> Result<(), InstallError> {
    let metadata = fs::symlink_metadata(dest).map_err(|e| InstallError::StepFailed {
        step: "verify".into(),
        message: format!(
            "Adobe plugin link is not readable at {}: {e}",
            dest.display()
        ),
    })?;
    if !metadata.file_type().is_symlink() {
        return Err(InstallError::StepFailed {
            step: "verify".into(),
            message: format!(
                "Adobe plugin target is not a directory link: {}",
                dest.display()
            ),
        });
    }
    if let Some(manifest) = manifest
        && !manifest.is_file()
    {
        return Err(InstallError::StepFailed {
            step: "verify".into(),
            message: format!(
                "Adobe plugin manifest does not exist: {}",
                manifest.display()
            ),
        });
    }
    Ok(())
}
