use std::path::{Component, Path, PathBuf};
use std::process::Command;

use sha2::Digest;

use super::file_ops::copy_dir_recursive;
use super::{InstallError, StepRollback, resolve_install_source_path};

pub(super) fn execute_git_clone(
    url: &str,
    ref_: Option<&str>,
    dest: &Path,
) -> Result<Option<StepRollback>, InstallError> {
    let commit = required_git_commit(ref_)?;
    if dest.exists() {
        return Err(InstallError::StepFailed {
            step: "git-clone".into(),
            message: format!("destination already exists: {}", dest.display()),
        });
    }

    let (_staging, staged) = stage_destination(dest, "git")?;
    let staged_string = staged.to_string_lossy().into_owned();
    run_git(
        &staged,
        "init",
        &["init", "--quiet", staged_string.as_str()],
    )?;
    let source = resolve_git_source(url, dest);
    run_git(
        &staged,
        "remote-add",
        &["remote", "add", "origin", source.as_str()],
    )?;
    run_git(
        &staged,
        "fetch",
        &["fetch", "--depth", "1", "origin", &commit],
    )?;
    run_git(
        &staged,
        "checkout",
        &["checkout", "--detach", "--quiet", "FETCH_HEAD"],
    )?;

    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(&staged)
        .output()
        .map_err(|error| InstallError::StepFailed {
            step: "git-verify".into(),
            message: format!("failed to launch git: {error}"),
        })?;
    if !output.status.success() {
        return Err(InstallError::StepFailed {
            step: "git-verify".into(),
            message: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        });
    }
    let actual = String::from_utf8_lossy(&output.stdout)
        .trim()
        .to_ascii_lowercase();
    if actual != commit {
        return Err(InstallError::StepFailed {
            step: "git-verify".into(),
            message: format!("commit mismatch: expected {commit}, got {actual}"),
        });
    }

    std::fs::rename(&staged, dest).map_err(|error| InstallError::StepFailed {
        step: "git-commit".into(),
        message: format!(
            "failed to commit staged checkout to {}: {error}",
            dest.display()
        ),
    })?;

    Ok(Some(StepRollback::RemovePath(dest.to_path_buf())))
}

fn required_git_commit(ref_: Option<&str>) -> Result<String, InstallError> {
    let reference = ref_.unwrap_or_default().trim();
    if reference.len() != 40 || !reference.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(InstallError::StepFailed {
            step: "git-integrity".into(),
            message: "a full 40-character commit object ID is required".into(),
        });
    }
    Ok(reference.to_ascii_lowercase())
}

fn resolve_git_source(url: &str, destination: &Path) -> String {
    if url.contains("://") || url.starts_with("git@") || Path::new(url).is_absolute() {
        return url.to_owned();
    }
    let resolved = lexical_normalize(&destination.join(url));
    let resolved = if resolved.is_absolute() {
        resolved
    } else {
        std::env::current_dir()
            .map(|directory| lexical_normalize(&directory.join(&resolved)))
            .unwrap_or(resolved)
    };
    resolved.to_string_lossy().into_owned()
}

fn lexical_normalize(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                let can_pop_normal = normalized
                    .components()
                    .next_back()
                    .is_some_and(|previous| matches!(previous, Component::Normal(_)));
                if can_pop_normal {
                    normalized.pop();
                } else if !normalized.has_root() {
                    normalized.push(component.as_os_str());
                }
            }
            Component::Normal(value) => normalized.push(value),
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(component.as_os_str()),
        }
    }
    normalized
}

fn run_git(dest: &Path, step: &str, args: &[&str]) -> Result<(), InstallError> {
    let mut command = Command::new("git");
    command.args(args);
    if step != "init" {
        command.current_dir(dest);
    }
    let output = command.output().map_err(|error| InstallError::StepFailed {
        step: format!("git-{step}"),
        message: format!("failed to launch git: {error}"),
    })?;
    if !output.status.success() {
        return Err(InstallError::StepFailed {
            step: format!("git-{step}"),
            message: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        });
    }
    Ok(())
}

pub(super) fn execute_zip_extract(
    url: &str,
    sha256: Option<&str>,
    dest: &Path,
) -> Result<Option<StepRollback>, InstallError> {
    let expected = required_archive_sha256(sha256)?;
    if dest.exists() {
        return Err(InstallError::StepFailed {
            step: "zip-extract".into(),
            message: format!("destination already exists: {}", dest.display()),
        });
    }

    let response = reqwest::blocking::get(url).map_err(|e| InstallError::StepFailed {
        step: "zip-download".into(),
        message: format!("failed to download {url}: {e}"),
    })?;
    if !response.status().is_success() {
        return Err(InstallError::StepFailed {
            step: "zip-download".into(),
            message: format!("archive download returned HTTP {}", response.status()),
        });
    }

    let bytes = response.bytes().map_err(|e| InstallError::StepFailed {
        step: "zip-download".into(),
        message: format!("failed to read response from {url}: {e}"),
    })?;
    let actual = sha2::Sha256::digest(&bytes);
    let actual_hex = actual
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    if !actual_hex.eq_ignore_ascii_case(&expected) {
        return Err(InstallError::StepFailed {
            step: "zip-checksum".into(),
            message: format!("SHA-256 mismatch: expected {expected}, got {actual_hex}"),
        });
    }

    let reader = std::io::Cursor::new(&bytes);
    let mut archive = zip::ZipArchive::new(reader).map_err(|e| InstallError::StepFailed {
        step: "zip-extract".into(),
        message: format!("failed to open zip archive: {e}"),
    })?;
    for index in 0..archive.len() {
        let entry = archive
            .by_index(index)
            .map_err(|e| InstallError::StepFailed {
                step: "zip-extract".into(),
                message: format!("failed to inspect ZIP archive entry: {e}"),
            })?;
        if entry.is_symlink() {
            return Err(InstallError::StepFailed {
                step: "zip-extract".into(),
                message: "ZIP archives must not contain symbolic links".into(),
            });
        }
    }

    let (_staging, staged) = stage_destination(dest, "zip")?;
    archive
        .extract(&staged)
        .map_err(|e| InstallError::StepFailed {
            step: "zip-extract".into(),
            message: format!("failed to extract to {}: {e}", staged.display()),
        })?;
    std::fs::rename(&staged, dest).map_err(|error| InstallError::StepFailed {
        step: "zip-commit".into(),
        message: format!(
            "failed to commit extracted archive to {}: {error}",
            dest.display()
        ),
    })?;

    Ok(Some(StepRollback::RemovePath(dest.to_path_buf())))
}

pub(super) fn required_archive_sha256(sha256: Option<&str>) -> Result<String, InstallError> {
    let value = sha256.unwrap_or_default().trim();
    let checksum = value.strip_prefix("sha256:").unwrap_or(value);
    if checksum.len() != 64 || !checksum.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(InstallError::StepFailed {
            step: "zip-integrity".into(),
            message: "exactly 64 hexadecimal SHA-256 digits are required".into(),
        });
    }
    Ok(checksum.to_ascii_lowercase())
}

pub(super) fn execute_path_copy(
    source: &Path,
    dest: &Path,
) -> Result<Option<StepRollback>, InstallError> {
    let source = resolve_install_source_path(source);
    if dest.exists() {
        return Err(InstallError::StepFailed {
            step: "path-copy".into(),
            message: format!("destination already exists: {}", dest.display()),
        });
    }
    if !source.exists() {
        return Err(InstallError::StepFailed {
            step: "path-copy".into(),
            message: format!("source does not exist: {}", source.display()),
        });
    }

    let (_staging, staged) = stage_destination(dest, "path")?;
    if source.is_dir() {
        copy_dir_recursive(&source, &staged)?;
    } else {
        std::fs::copy(&source, &staged)?;
    }
    std::fs::rename(&staged, dest).map_err(|error| InstallError::StepFailed {
        step: "path-commit".into(),
        message: format!(
            "failed to commit copied path to {}: {error}",
            dest.display()
        ),
    })?;

    Ok(Some(StepRollback::RemovePath(dest.to_path_buf())))
}

fn stage_destination(
    destination: &Path,
    kind: &str,
) -> Result<(tempfile::TempDir, PathBuf), InstallError> {
    let parent = destination.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)?;
    let staging = tempfile::Builder::new()
        .prefix(&format!(".dcc-mcp-install-{kind}-"))
        .tempdir_in(parent)
        .map_err(|error| InstallError::StepFailed {
            step: format!("{kind}-stage"),
            message: format!("failed to create a staging directory: {error}"),
        })?;
    let staged = staging.path().join("payload");
    Ok((staging, staged))
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::process::Command;

    use super::{execute_git_clone, resolve_git_source};

    fn run_git(directory: &Path, args: &[&str]) -> std::process::Output {
        Command::new("git")
            .args(args)
            .current_dir(directory)
            .output()
            .unwrap()
    }

    #[test]
    fn relative_git_sources_resolve_from_the_final_destination() {
        let root = tempfile::tempdir().unwrap();
        let destination = root.path().join("install").join("adapter");
        let source = resolve_git_source("../mirror.git", &destination);

        assert_eq!(
            source,
            root.path()
                .join("install")
                .join("mirror.git")
                .display()
                .to_string()
        );
    }

    #[test]
    fn relative_git_source_fetches_after_staging() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source");
        std::fs::create_dir_all(&source).unwrap();
        assert!(run_git(&source, &["init", "--quiet"]).status.success());
        std::fs::write(source.join("README.md"), "fixture\n").unwrap();
        assert!(run_git(&source, &["add", "README.md"]).status.success());
        assert!(
            run_git(
                &source,
                &[
                    "-c",
                    "user.name=Install Test",
                    "-c",
                    "user.email=install-test@example.invalid",
                    "commit",
                    "--quiet",
                    "-m",
                    "fixture",
                ],
            )
            .status
            .success()
        );
        let commit = String::from_utf8(run_git(&source, &["rev-parse", "HEAD"]).stdout)
            .unwrap()
            .trim()
            .to_owned();

        let destination = root.path().join("install").join("adapter");
        let relative_source = Path::new("../..").join("source");
        let relative_source = relative_source.to_string_lossy();
        execute_git_clone(&relative_source, Some(&commit), &destination).unwrap();

        assert_eq!(
            std::fs::read_to_string(destination.join("README.md"))
                .unwrap()
                .replace("\r\n", "\n"),
            "fixture\n"
        );
    }
}
