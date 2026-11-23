use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub(super) enum RemovalError {
    RequiresRestart { path: PathBuf, message: String },
    Failed { path: PathBuf, message: String },
}

#[derive(Debug)]
struct Backup {
    original: PathBuf,
    staged: PathBuf,
    _directory: tempfile::TempDir,
}

#[derive(Debug)]
pub(super) struct RemovalTransaction {
    backups: Vec<Backup>,
}

impl RemovalTransaction {
    pub(super) fn stage(paths: impl IntoIterator<Item = PathBuf>) -> Result<Self, RemovalError> {
        let mut transaction = Self {
            backups: Vec::new(),
        };
        for path in paths {
            match fs::symlink_metadata(&path) {
                Ok(_) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => return Err(failed(&path, error)),
            }
            let parent = path.parent().unwrap_or_else(|| Path::new("."));
            let directory = tempfile::Builder::new()
                .prefix(".dcc-mcp-uninstall-")
                .tempdir_in(parent)
                .map_err(|error| failed(&path, error))?;
            let staged = directory.path().join("payload");
            if let Err(error) = fs::rename(&path, &staged) {
                let result = classify(&path, error);
                if let Err(restore_error) = transaction.restore() {
                    return Err(RemovalError::Failed {
                        path,
                        message: format!("rollback failed after removal error: {restore_error}"),
                    });
                }
                return Err(result);
            }
            transaction.backups.push(Backup {
                original: path,
                staged,
                _directory: directory,
            });
        }
        Ok(transaction)
    }

    pub(super) fn commit(mut self) -> Result<(), RemovalError> {
        for index in (0..self.backups.len()).rev() {
            let backup = &self.backups[index];
            let original = backup.original.clone();
            if let Err(error) = remove_path(&backup.staged) {
                let result = classify(&original, error);
                if let Err(restore_error) = self.restore() {
                    return Err(RemovalError::Failed {
                        path: original,
                        message: format!("cleanup failed; rollback failed: {restore_error}"),
                    });
                }
                return Err(result);
            }
        }
        Ok(())
    }

    pub(super) fn restore(&mut self) -> Result<(), io::Error> {
        for backup in self.backups.iter().rev() {
            if fs::symlink_metadata(&backup.staged).is_err() {
                continue;
            }
            if fs::symlink_metadata(&backup.original).is_ok() {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    format!(
                        "rollback destination already exists: {}",
                        backup.original.display()
                    ),
                ));
            }
            fs::rename(&backup.staged, &backup.original)?;
        }
        Ok(())
    }
}

fn remove_path(path: &Path) -> Result<(), io::Error> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() {
        #[cfg(windows)]
        if std::os::windows::fs::FileTypeExt::is_symlink_dir(&metadata.file_type()) {
            return fs::remove_dir(path);
        }
        return fs::remove_file(path);
    }
    if metadata.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
}

fn classify(path: &Path, error: io::Error) -> RemovalError {
    if is_windows_lock_error(&error) {
        return RemovalError::RequiresRestart {
            path: path.to_path_buf(),
            message: error.to_string(),
        };
    }
    failed(path, error)
}

fn failed(path: &Path, error: io::Error) -> RemovalError {
    RemovalError::Failed {
        path: path.to_path_buf(),
        message: error.to_string(),
    }
}

fn is_windows_lock_error(error: &io::Error) -> bool {
    #[cfg(windows)]
    {
        error.kind() == io::ErrorKind::PermissionDenied
            || matches!(error.raw_os_error(), Some(5 | 32 | 33))
    }
    #[cfg(not(windows))]
    {
        let _ = error;
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transaction_restores_staged_paths() {
        let root = tempfile::tempdir().unwrap();
        let original = root.path().join("adapter");
        fs::create_dir(&original).unwrap();
        fs::write(original.join("payload.txt"), "payload").unwrap();

        let mut transaction = RemovalTransaction::stage([original.clone()]).unwrap();
        assert!(!original.exists());
        transaction.restore().unwrap();
        assert_eq!(
            fs::read_to_string(original.join("payload.txt")).unwrap(),
            "payload"
        );
    }

    #[test]
    fn transaction_commit_removes_staged_paths() {
        let root = tempfile::tempdir().unwrap();
        let original = root.path().join("adapter");
        fs::create_dir(&original).unwrap();
        fs::write(original.join("payload.txt"), "payload").unwrap();

        let transaction = RemovalTransaction::stage([original.clone()]).unwrap();
        transaction.commit().unwrap();
        assert!(!original.exists());
    }
}
