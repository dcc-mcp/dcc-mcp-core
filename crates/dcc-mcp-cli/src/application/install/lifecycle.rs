use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug)]
pub(super) enum RemovalError {
    RequiresRestart {
        path: PathBuf,
        message: String,
        recovery: Vec<RecoveryEntry>,
    },
    Failed {
        path: PathBuf,
        message: String,
        recovery: Vec<RecoveryEntry>,
    },
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(super) struct RecoveryEntry {
    pub(super) original: PathBuf,
    pub(super) recovery_dir: PathBuf,
}

#[derive(Debug)]
struct Backup {
    original: PathBuf,
    staged: PathBuf,
    directory: Option<tempfile::TempDir>,
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
            let directory = match tempfile::Builder::new()
                .prefix(".dcc-mcp-uninstall-")
                .tempdir_in(parent)
            {
                Ok(directory) => directory,
                Err(error) => {
                    let result = transaction.finish_stage_error(failed(&path, error));
                    return Err(result);
                }
            };
            let staged = directory.path().join("payload");
            if let Err(error) = fs::rename(&path, &staged) {
                let result = classify(&path, error);
                if let Err(restore_error) = transaction.restore() {
                    return Err(transaction.rollback_failure(
                        path,
                        format!("rollback failed after removal error: {restore_error}"),
                    ));
                }
                return Err(result);
            }
            transaction.backups.push(Backup {
                original: path,
                staged,
                directory: Some(directory),
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
                return Err(self.with_recovery(result));
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

    fn finish_stage_error(&mut self, error: RemovalError) -> RemovalError {
        match self.restore() {
            Ok(()) => error,
            Err(restore_error) => self.rollback_failure(
                error_path(&error),
                format!(
                    "{}; rollback failed: {restore_error}",
                    error_message(&error)
                ),
            ),
        }
    }

    fn rollback_failure(&mut self, path: PathBuf, message: String) -> RemovalError {
        let recovery = self.retain_backups();
        RemovalError::Failed {
            path,
            message,
            recovery,
        }
    }

    fn with_recovery(&mut self, error: RemovalError) -> RemovalError {
        let recovery = self.retain_backups();
        match error {
            RemovalError::RequiresRestart { path, message, .. } => RemovalError::RequiresRestart {
                path,
                message,
                recovery,
            },
            RemovalError::Failed { path, message, .. } => RemovalError::Failed {
                path,
                message,
                recovery,
            },
        }
    }

    fn retain_backups(&mut self) -> Vec<RecoveryEntry> {
        self.backups
            .iter_mut()
            .filter_map(|backup| {
                backup.directory.take().map(|directory| RecoveryEntry {
                    original: backup.original.clone(),
                    recovery_dir: directory.keep(),
                })
            })
            .collect()
    }

    pub(super) fn retain_recovery(&mut self) -> Vec<RecoveryEntry> {
        self.retain_backups()
    }
}

pub(super) fn resume_pending(
    operation: &str,
    entries: &[RecoveryEntry],
) -> Result<(), RemovalError> {
    for entry in entries {
        let payload = entry.recovery_dir.join("payload");
        let result = match operation {
            "restore" => {
                let payload_exists = match fs::symlink_metadata(&payload) {
                    Ok(_) => true,
                    Err(error) if error.kind() == io::ErrorKind::NotFound => false,
                    Err(error) => {
                        return Err(classify_with_recovery(&payload, error, entries.to_vec()));
                    }
                };
                let original_exists = match fs::symlink_metadata(&entry.original) {
                    Ok(_) => true,
                    Err(error) if error.kind() == io::ErrorKind::NotFound => false,
                    Err(error) => {
                        return Err(classify_with_recovery(
                            &entry.original,
                            error,
                            entries.to_vec(),
                        ));
                    }
                };
                if !payload_exists && original_exists {
                    Ok(())
                } else if original_exists {
                    Err(io::Error::new(
                        io::ErrorKind::AlreadyExists,
                        format!(
                            "rollback destination already exists: {}",
                            entry.original.display()
                        ),
                    ))
                } else if !payload_exists {
                    Err(io::Error::new(
                        io::ErrorKind::NotFound,
                        format!("pending recovery payload is missing: {}", payload.display()),
                    ))
                } else {
                    fs::rename(&payload, &entry.original)
                }
            }
            "cleanup" => match fs::symlink_metadata(&payload) {
                Ok(_) => remove_path(&payload),
                Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
                Err(error) => Err(error),
            },
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unknown pending uninstall operation: {operation}"),
            )),
        };
        if let Err(error) = result {
            return Err(classify_with_recovery(
                &entry.original,
                error,
                entries.to_vec(),
            ));
        }
        if let Err(error) = fs::remove_dir(&entry.recovery_dir)
            && error.kind() != io::ErrorKind::NotFound
        {
            return Err(classify_with_recovery(
                &entry.recovery_dir,
                error,
                entries.to_vec(),
            ));
        }
    }
    Ok(())
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
            recovery: Vec::new(),
        };
    }
    failed(path, error)
}

fn failed(path: &Path, error: io::Error) -> RemovalError {
    RemovalError::Failed {
        path: path.to_path_buf(),
        message: error.to_string(),
        recovery: Vec::new(),
    }
}

fn classify_with_recovery(
    path: &Path,
    error: io::Error,
    recovery: Vec<RecoveryEntry>,
) -> RemovalError {
    match classify(path, error) {
        RemovalError::RequiresRestart { path, message, .. } => RemovalError::RequiresRestart {
            path,
            message,
            recovery,
        },
        RemovalError::Failed { path, message, .. } => RemovalError::Failed {
            path,
            message,
            recovery,
        },
    }
}

fn error_path(error: &RemovalError) -> PathBuf {
    match error {
        RemovalError::RequiresRestart { path, .. } | RemovalError::Failed { path, .. } => {
            path.clone()
        }
    }
}

fn error_message(error: &RemovalError) -> &str {
    match error {
        RemovalError::RequiresRestart { message, .. } | RemovalError::Failed { message, .. } => {
            message
        }
    }
}

fn is_windows_lock_error(error: &io::Error) -> bool {
    #[cfg(windows)]
    {
        matches!(error.raw_os_error(), Some(32 | 33))
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

    #[test]
    fn pending_restore_resumes_from_a_retained_recovery_entry() {
        let root = tempfile::tempdir().unwrap();
        let original = root.path().join("adapter");
        fs::create_dir(&original).unwrap();
        fs::write(original.join("payload.txt"), "payload").unwrap();

        let mut transaction = RemovalTransaction::stage([original.clone()]).unwrap();
        let entries = transaction.retain_recovery();
        resume_pending("restore", &entries).unwrap();

        assert_eq!(
            fs::read_to_string(original.join("payload.txt")).unwrap(),
            "payload"
        );
    }

    #[test]
    fn pending_cleanup_resumes_without_recreating_the_original_path() {
        let root = tempfile::tempdir().unwrap();
        let original = root.path().join("adapter");
        fs::create_dir(&original).unwrap();
        fs::write(original.join("payload.txt"), "payload").unwrap();

        let mut transaction = RemovalTransaction::stage([original.clone()]).unwrap();
        let entries = transaction.retain_recovery();
        resume_pending("cleanup", &entries).unwrap();

        assert!(!original.exists());
        assert!(!entries[0].recovery_dir.exists());
    }
}
