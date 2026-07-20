//! Crash-recoverable staged updates for a binary and its sibling runtimes.

use std::collections::HashSet;
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::{Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

use fs4::{FileExt, TryLockError};
use serde::{Deserialize, Serialize};

use super::{UpdateError, sha256_bytes, sha256_file, staging_dir};

const PENDING_SET_DIR: &str = "pending-set";
const COMMITTED_SET_PREFIX: &str = "pending-set.committed-";
const MANIFEST_FILE: &str = "manifest.json";
const JOURNAL_FILE: &str = "journal.json";
const TRANSACTION_LOCK_FILE: &str = "transaction.lock";
const REMOVED_CAPTURE_HELPER: &str = "dcc-mcp-capture-helper.exe";
const UPDATE_LOCK_TIMEOUT: Duration = Duration::from_secs(10);
const UPDATE_LOCK_BACKOFF: Duration = Duration::from_millis(10);

static PROCESS_INSTALLATION_LOCKS: OnceLock<(Mutex<HashSet<String>>, Condvar)> = OnceLock::new();

struct ProcessInstallationGuard {
    key: String,
}

impl Drop for ProcessInstallationGuard {
    fn drop(&mut self) {
        let (locks, wake) = process_installation_locks();
        let mut held = locks.lock().unwrap_or_else(|error| error.into_inner());
        held.remove(&self.key);
        wake.notify_all();
    }
}

struct InstallationLock {
    file: File,
    _process: ProcessInstallationGuard,
}

impl Drop for InstallationLock {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.file);
    }
}

/// Installation target for one member of a staged update set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum UpdateTarget {
    /// Replace the executable that staged the update.
    CurrentExecutable,
    /// Replace one exact file beside the current executable.
    Sibling { file_name: String },
}

/// One already-downloaded and checksummed member of an update set.
#[derive(Debug, Clone, Copy)]
pub struct UpdateSetSource<'a> {
    pub downloaded: &'a Path,
    pub target: &'a UpdateTarget,
    pub expected_sha256: &'a str,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct UpdateSetManifest {
    format_version: u8,
    bound_executable: PathBuf,
    components: Vec<StagedComponent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StagedComponent {
    staged_file: String,
    target: UpdateTarget,
    sha256: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum JournalState {
    Applying,
    Committed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct UpdateJournal {
    state: JournalState,
    entries: Vec<JournalEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct JournalEntry {
    target: PathBuf,
    prepared: PathBuf,
    backup: PathBuf,
    had_original: bool,
    backed_up: bool,
    installed: bool,
}

/// Stage a complete update set and bind it to this executable installation.
pub fn stage_update_set(
    binary_name: &str,
    sources: &[UpdateSetSource<'_>],
) -> Result<(), UpdateError> {
    let current_exe = std::env::current_exe().map_err(|_| UpdateError::NoExePath)?;
    stage_update_set_for(binary_name, &current_exe, sources)
}

/// Stage a complete update set for an explicit executable path.
///
/// This is public for deterministic installer and regression testing. Normal
/// binaries should call [`stage_update_set`].
#[doc(hidden)]
pub fn stage_update_set_for(
    binary_name: &str,
    current_exe: &Path,
    sources: &[UpdateSetSource<'_>],
) -> Result<(), UpdateError> {
    stage_update_set_for_with_timeout(binary_name, current_exe, sources, UPDATE_LOCK_TIMEOUT)
}

fn stage_update_set_for_with_timeout(
    binary_name: &str,
    current_exe: &Path,
    sources: &[UpdateSetSource<'_>],
    lock_timeout: Duration,
) -> Result<(), UpdateError> {
    let bound_executable = canonical_existing(current_exe)?;
    let root = installation_staging_dir(binary_name, &bound_executable)?;
    let _lock = acquire_installation_lock(&root, &bound_executable, lock_timeout)?;
    cleanup_committed_sets(&root);

    if sources.is_empty() {
        return Err(UpdateError::Stage("update set must not be empty".into()));
    }
    let mut targets = HashSet::new();
    let mut has_current_executable = false;
    for source in sources {
        validate_target(source.target)?;
        let target_key = target_key(source.target);
        if !targets.insert(target_key) {
            return Err(UpdateError::Stage(
                "update set contains duplicate installation targets".into(),
            ));
        }
        has_current_executable |= matches!(source.target, UpdateTarget::CurrentExecutable);
        validate_expected_sha(source.expected_sha256)?;
        if !source.downloaded.is_file() {
            return Err(UpdateError::Stage(format!(
                "update component is missing: {}",
                source.downloaded.display()
            )));
        }
        let actual = sha256_file(source.downloaded)?;
        if !actual.eq_ignore_ascii_case(source.expected_sha256) {
            return Err(UpdateError::ChecksumMismatch {
                expected: source.expected_sha256.to_ascii_lowercase(),
                actual,
            });
        }
    }
    if !has_current_executable {
        return Err(UpdateError::Stage(
            "update set must include the current executable".into(),
        ));
    }

    let nonce = unique_nonce()?;
    let temp_dir = root.join(format!("{PENDING_SET_DIR}.{nonce}.tmp"));
    std::fs::create_dir(&temp_dir)?;

    let stage_result = (|| {
        let mut components = Vec::with_capacity(sources.len());
        for (index, source) in sources.iter().enumerate() {
            let staged_file = format!("component-{index}.bin");
            let staged_path = temp_dir.join(&staged_file);
            std::fs::copy(source.downloaded, &staged_path)?;
            let actual = sha256_file(&staged_path)?;
            if !actual.eq_ignore_ascii_case(source.expected_sha256) {
                return Err(UpdateError::ChecksumMismatch {
                    expected: source.expected_sha256.to_ascii_lowercase(),
                    actual,
                });
            }
            components.push(StagedComponent {
                staged_file,
                target: source.target.clone(),
                sha256: actual,
            });
        }
        let manifest = UpdateSetManifest {
            format_version: 1,
            bound_executable,
            components,
        };
        write_json_atomic(&temp_dir.join(MANIFEST_FILE), &manifest)?;
        Ok::<(), UpdateError>(())
    })();
    if let Err(error) = stage_result {
        let _ = std::fs::remove_dir_all(&temp_dir);
        return Err(error);
    }

    let pending = root.join(PENDING_SET_DIR);
    let previous = root.join(format!("{PENDING_SET_DIR}.previous"));
    let _ = std::fs::remove_dir_all(&previous);
    if pending.exists() {
        std::fs::rename(&pending, &previous)?;
    }
    if let Err(error) = std::fs::rename(&temp_dir, &pending) {
        if previous.exists() {
            let _ = std::fs::rename(&previous, &pending);
        }
        let _ = std::fs::remove_dir_all(&temp_dir);
        return Err(error.into());
    }
    let _ = std::fs::remove_dir_all(previous);
    tracing::info!(path = %pending.display(), "complete update set staged");
    Ok(())
}

/// Apply a complete staged update set to this executable installation.
pub fn apply_staged_update_set(binary_name: &str) -> Result<bool, UpdateError> {
    let current_exe = std::env::current_exe().map_err(|_| UpdateError::NoExePath)?;
    apply_staged_update_set_for(binary_name, &current_exe)
}

/// Apply a complete staged update set to an explicit executable path.
#[doc(hidden)]
pub fn apply_staged_update_set_for(
    binary_name: &str,
    current_exe: &Path,
) -> Result<bool, UpdateError> {
    apply_staged_update_set_for_with_timeout(binary_name, current_exe, UPDATE_LOCK_TIMEOUT)
}

fn apply_staged_update_set_for_with_timeout(
    binary_name: &str,
    current_exe: &Path,
    lock_timeout: Duration,
) -> Result<bool, UpdateError> {
    let current_exe = canonical_existing(current_exe)?;
    let root = installation_staging_dir(binary_name, &current_exe)?;
    let _lock = acquire_installation_lock(&root, &current_exe, lock_timeout)?;
    cleanup_committed_sets(&root);

    let pending = root.join(PENDING_SET_DIR);
    if !pending.is_dir() {
        return Ok(false);
    }
    let manifest: UpdateSetManifest = read_json(&pending.join(MANIFEST_FILE))?;
    if manifest.format_version != 1 {
        return Err(UpdateError::Stage(
            "unsupported staged update-set format".into(),
        ));
    }
    if !paths_equal(&current_exe, &manifest.bound_executable) {
        return Err(UpdateError::Stage(format!(
            "staged update belongs to a different executable: {}",
            manifest.bound_executable.display()
        )));
    }

    let journal_path = pending.join(JOURNAL_FILE);
    if journal_path.is_file() {
        let journal: UpdateJournal = read_json(&journal_path)?;
        if journal.state == JournalState::Committed {
            finish_committed(&pending, &journal)?;
            return Ok(false);
        }
        rollback(&journal)?;
        let _ = std::fs::remove_file(&journal_path);
    }

    let install_dir = current_exe
        .parent()
        .ok_or_else(|| UpdateError::Stage("current executable has no parent directory".into()))?;
    let mut entries = Vec::with_capacity(manifest.components.len());
    let mut seen_targets = HashSet::new();
    for component in &manifest.components {
        validate_target(&component.target)?;
        validate_expected_sha(&component.sha256)?;
        let source = pending.join(&component.staged_file);
        if !source.is_file() {
            return Err(UpdateError::Stage(format!(
                "staged update component is missing: {}",
                source.display()
            )));
        }
        let actual = sha256_file(&source)?;
        if !actual.eq_ignore_ascii_case(&component.sha256) {
            return Err(UpdateError::ChecksumMismatch {
                expected: component.sha256.clone(),
                actual,
            });
        }
        let target = match &component.target {
            UpdateTarget::CurrentExecutable => current_exe.clone(),
            UpdateTarget::Sibling { file_name } => install_dir.join(file_name),
        };
        if !seen_targets.insert(path_key(&target)) {
            return Err(UpdateError::Stage(
                "staged update contains duplicate resolved targets".into(),
            ));
        }
        let file_name = target
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| UpdateError::Stage("update target has no filename".into()))?;
        let prepared = target.with_file_name(format!("{file_name}.dcc-mcp-new"));
        let backup = target.with_file_name(format!("{file_name}.dcc-mcp-backup"));
        let had_original = target.exists();
        let _ = std::fs::remove_file(&prepared);
        std::fs::copy(&source, &prepared)?;
        let prepared_sha = sha256_file(&prepared)?;
        if !prepared_sha.eq_ignore_ascii_case(&component.sha256) {
            let _ = std::fs::remove_file(&prepared);
            return Err(UpdateError::ChecksumMismatch {
                expected: component.sha256.clone(),
                actual: prepared_sha,
            });
        }
        entries.push(JournalEntry {
            target,
            prepared,
            backup,
            // Persist the pre-transaction state before the first rename. If
            // the process exits after the initial journal write, recovery
            // must not mistake an untouched original for a newly installed
            // file and delete it.
            had_original,
            backed_up: false,
            installed: false,
        });
    }
    entries.sort_by_key(|entry| paths_equal(&entry.target, &current_exe));

    let mut journal = UpdateJournal {
        state: JournalState::Applying,
        entries,
    };
    write_json_atomic(&journal_path, &journal)?;
    let result = apply_entries(&journal_path, &mut journal);
    if let Err(error) = result {
        if let Err(rollback_error) = rollback(&journal) {
            return Err(UpdateError::Stage(format!(
                "update failed ({error}); rollback also failed ({rollback_error})"
            )));
        }
        let _ = std::fs::remove_file(&journal_path);
        return Err(error);
    }
    journal.state = JournalState::Committed;
    write_json_atomic(&journal_path, &journal)?;
    finish_committed(&pending, &journal)?;
    tracing::info!(path = %current_exe.display(), "staged update set applied successfully");
    Ok(true)
}

/// Install one verified runtime beside the current executable.
///
/// This is used to reconcile a missing or stale UI Control host after the
/// first raw-server upgrade from an updater that predates update sets.
pub fn install_verified_sibling(
    binary_name: &str,
    current_exe: &Path,
    downloaded: &Path,
    sibling_file_name: &str,
    expected_sha256: &str,
) -> Result<(), UpdateError> {
    install_verified_sibling_with_timeout(
        binary_name,
        current_exe,
        downloaded,
        sibling_file_name,
        expected_sha256,
        UPDATE_LOCK_TIMEOUT,
    )
}

fn install_verified_sibling_with_timeout(
    binary_name: &str,
    current_exe: &Path,
    downloaded: &Path,
    sibling_file_name: &str,
    expected_sha256: &str,
    lock_timeout: Duration,
) -> Result<(), UpdateError> {
    validate_sibling_name(sibling_file_name)?;
    validate_expected_sha(expected_sha256)?;
    let current_exe = canonical_existing(current_exe)?;
    let root = installation_staging_dir(binary_name, &current_exe)?;
    let _lock = acquire_installation_lock(&root, &current_exe, lock_timeout)?;
    cleanup_committed_sets(&root);

    let actual = sha256_file(downloaded)?;
    if !actual.eq_ignore_ascii_case(expected_sha256) {
        return Err(UpdateError::ChecksumMismatch {
            expected: expected_sha256.to_ascii_lowercase(),
            actual,
        });
    }
    let target = current_exe
        .parent()
        .ok_or_else(|| UpdateError::Stage("current executable has no parent directory".into()))?
        .join(sibling_file_name);
    let prepared = target.with_file_name(format!("{sibling_file_name}.dcc-mcp-new"));
    let backup = target.with_file_name(format!("{sibling_file_name}.dcc-mcp-backup"));

    if backup.exists() && !target.exists() {
        std::fs::rename(&backup, &target)?;
    }
    let _ = std::fs::remove_file(&prepared);
    std::fs::copy(downloaded, &prepared)?;
    let prepared_sha = sha256_file(&prepared)?;
    if !prepared_sha.eq_ignore_ascii_case(expected_sha256) {
        let _ = std::fs::remove_file(&prepared);
        return Err(UpdateError::ChecksumMismatch {
            expected: expected_sha256.to_ascii_lowercase(),
            actual: prepared_sha,
        });
    }
    let _ = std::fs::remove_file(&backup);
    let had_original = target.exists();
    if had_original {
        std::fs::rename(&target, &backup)?;
    }
    if let Err(error) = std::fs::rename(&prepared, &target) {
        if had_original {
            let _ = std::fs::rename(&backup, &target);
        }
        let _ = std::fs::remove_file(&prepared);
        return Err(error.into());
    }
    let installed_sha = sha256_file(&target)?;
    if !installed_sha.eq_ignore_ascii_case(expected_sha256) {
        let _ = std::fs::remove_file(&target);
        if had_original {
            let _ = std::fs::rename(&backup, &target);
        }
        return Err(UpdateError::ChecksumMismatch {
            expected: expected_sha256.to_ascii_lowercase(),
            actual: installed_sha,
        });
    }
    let _ = std::fs::remove_file(backup);
    Ok(())
}

fn apply_entries(journal_path: &Path, journal: &mut UpdateJournal) -> Result<(), UpdateError> {
    for index in 0..journal.entries.len() {
        let target = journal.entries[index].target.clone();
        let prepared = journal.entries[index].prepared.clone();
        let backup = journal.entries[index].backup.clone();
        if backup.exists() {
            if !backup.is_file() {
                return Err(UpdateError::Stage(format!(
                    "update backup path is not a regular file: {}",
                    backup.display()
                )));
            }
            std::fs::remove_file(&backup)?;
        }
        if journal.entries[index].had_original {
            std::fs::rename(&target, &backup)?;
            journal.entries[index].backed_up = true;
            write_json_atomic(journal_path, journal)?;
        }
        std::fs::rename(&prepared, &target)?;
        journal.entries[index].installed = true;
        write_json_atomic(journal_path, journal)?;
    }
    Ok(())
}

fn rollback(journal: &UpdateJournal) -> Result<(), UpdateError> {
    for entry in &journal.entries {
        if entry.backed_up && !entry.backup.is_file() {
            return Err(UpdateError::Stage(format!(
                "cannot roll back update because the recorded backup is missing: {}",
                entry.backup.display()
            )));
        }
    }
    for entry in journal.entries.iter().rev() {
        if entry.installed && entry.target.exists() {
            let _ = std::fs::remove_file(&entry.prepared);
            std::fs::rename(&entry.target, &entry.prepared)?;
        }
        // The backup itself is authoritative when `had_original` is true.
        // A crash can occur after target -> backup but before `backed_up` is
        // flushed to the journal.
        if entry.had_original && entry.backup.is_file() {
            if entry.target.exists() {
                std::fs::remove_file(&entry.target)?;
            }
            std::fs::rename(&entry.backup, &entry.target)?;
        } else if !entry.had_original && entry.target.exists() {
            std::fs::remove_file(&entry.target)?;
        }
        let _ = std::fs::remove_file(&entry.prepared);
    }
    Ok(())
}

fn finish_committed(pending: &Path, journal: &UpdateJournal) -> Result<(), UpdateError> {
    let root = pending
        .parent()
        .ok_or_else(|| UpdateError::Stage("pending update has no parent directory".into()))?;
    let detached = root.join(format!("{COMMITTED_SET_PREFIX}{}", unique_nonce()?));
    std::fs::rename(pending, &detached)?;
    cleanup_committed_entries(journal);
    let _ = std::fs::remove_dir_all(detached);
    Ok(())
}

fn cleanup_committed_entries(journal: &UpdateJournal) {
    for entry in &journal.entries {
        let _ = std::fs::remove_file(&entry.prepared);
        let _ = std::fs::remove_file(&entry.backup);
    }
}

fn cleanup_committed_sets(root: &Path) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let is_committed_set = entry
            .file_name()
            .to_str()
            .is_some_and(|name| name.starts_with(COMMITTED_SET_PREFIX));
        if !is_committed_set || !path.is_dir() {
            continue;
        }
        if let Ok(journal) = read_json::<UpdateJournal>(&path.join(JOURNAL_FILE))
            && journal.state == JournalState::Committed
        {
            cleanup_committed_entries(&journal);
        }
        let _ = std::fs::remove_dir_all(path);
    }
}

fn process_installation_locks() -> &'static (Mutex<HashSet<String>>, Condvar) {
    PROCESS_INSTALLATION_LOCKS.get_or_init(|| (Mutex::new(HashSet::new()), Condvar::new()))
}

fn installation_staging_dir(
    binary_name: &str,
    canonical_executable: &Path,
) -> Result<PathBuf, UpdateError> {
    if super::dirs_data_dir().is_none() {
        return Err(UpdateError::Stage(
            "cannot determine the current user's update staging directory".into(),
        ));
    }
    let installation_id = sha256_bytes(path_key(canonical_executable).as_bytes());
    Ok(staging_dir(binary_name)?
        .join("installations")
        .join(installation_id))
}

fn acquire_installation_lock(
    root: &Path,
    canonical_executable: &Path,
    timeout: Duration,
) -> Result<InstallationLock, UpdateError> {
    let started = Instant::now();
    let process =
        acquire_process_installation_lock(path_key(canonical_executable), started, timeout)?;
    std::fs::create_dir_all(root)?;
    let lock_path = root.join(TRANSACTION_LOCK_FILE);
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&lock_path)?;
    loop {
        match FileExt::try_lock(&file) {
            Ok(()) => {
                return Ok(InstallationLock {
                    file,
                    _process: process,
                });
            }
            Err(TryLockError::WouldBlock) => {
                wait_for_installation_lock(started, timeout, "update transaction file lock")?;
            }
            Err(TryLockError::Error(error)) => return Err(error.into()),
        }
    }
}

fn acquire_process_installation_lock(
    key: String,
    started: Instant,
    timeout: Duration,
) -> Result<ProcessInstallationGuard, UpdateError> {
    let (locks, wake) = process_installation_locks();
    let mut held = locks.lock().unwrap_or_else(|error| error.into_inner());
    loop {
        if held.insert(key.clone()) {
            return Ok(ProcessInstallationGuard { key });
        }
        let remaining = remaining_lock_time(started, timeout, "in-process update mutex")?;
        let (next, _) = wake
            .wait_timeout(held, remaining)
            .unwrap_or_else(|error| error.into_inner());
        held = next;
    }
}

fn wait_for_installation_lock(
    started: Instant,
    timeout: Duration,
    target: &str,
) -> Result<(), UpdateError> {
    let remaining = remaining_lock_time(started, timeout, target)?;
    std::thread::sleep(UPDATE_LOCK_BACKOFF.min(remaining));
    Ok(())
}

fn remaining_lock_time(
    started: Instant,
    timeout: Duration,
    target: &str,
) -> Result<Duration, UpdateError> {
    timeout.checked_sub(started.elapsed()).ok_or_else(|| {
        UpdateError::Stage(format!(
            "timed out waiting for {target} after {}ms",
            timeout.as_millis()
        ))
    })
}

fn unique_nonce() -> Result<String, UpdateError> {
    Ok(format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|error| UpdateError::Stage(error.to_string()))?
            .as_nanos()
    ))
}

fn validate_target(target: &UpdateTarget) -> Result<(), UpdateError> {
    if let UpdateTarget::Sibling { file_name } = target {
        validate_sibling_name(file_name)?;
    }
    Ok(())
}

fn validate_sibling_name(file_name: &str) -> Result<(), UpdateError> {
    let has_only_portable_filename_bytes = file_name
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'));
    if file_name.is_empty()
        || file_name.eq_ignore_ascii_case(REMOVED_CAPTURE_HELPER)
        || !has_only_portable_filename_bytes
        || file_name.ends_with('.')
        || file_name.contains(['/', '\\'])
        || matches!(file_name, "." | "..")
        || Path::new(file_name).file_name().and_then(|v| v.to_str()) != Some(file_name)
    {
        return Err(UpdateError::Stage(format!(
            "invalid sibling update target: {file_name:?}"
        )));
    }
    Ok(())
}

fn validate_expected_sha(value: &str) -> Result<(), UpdateError> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(UpdateError::Stage(
            "update component requires a 64-character SHA-256 digest".into(),
        ));
    }
    Ok(())
}

fn canonical_existing(path: &Path) -> Result<PathBuf, UpdateError> {
    if !path.is_file() {
        return Err(UpdateError::Stage(format!(
            "bound executable does not exist: {}",
            path.display()
        )));
    }
    Ok(std::fs::canonicalize(path)?)
}

fn target_key(target: &UpdateTarget) -> String {
    match target {
        UpdateTarget::CurrentExecutable => "current".into(),
        UpdateTarget::Sibling { file_name } => {
            format!("sibling:{}", file_name.to_ascii_lowercase())
        }
    }
}

fn path_key(path: &Path) -> String {
    let value = path.to_string_lossy();
    if cfg!(windows) {
        value.to_ascii_lowercase()
    } else {
        value.into_owned()
    }
}

fn paths_equal(left: &Path, right: &Path) -> bool {
    path_key(left) == path_key(right)
}

fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> Result<(), UpdateError> {
    use std::io::Write as _;

    let parent = path
        .parent()
        .ok_or_else(|| UpdateError::Stage("update journal has no parent directory".into()))?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    let bytes = serde_json::to_vec_pretty(value)?;
    temp.write_all(&bytes)?;
    temp.as_file().sync_all()?;
    temp.persist(path)
        .map_err(|error| UpdateError::Io(error.error))?;
    Ok(())
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, UpdateError> {
    let bytes = std::fs::read(path)?;
    Ok(serde_json::from_slice(&bytes)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unique_name(label: &str) -> String {
        format!(
            "dcc-mcp-updater-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        )
    }

    fn write_fixture(root: &Path, name: &str, bytes: &[u8]) -> PathBuf {
        let path = root.join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }

    fn installation_root(binary_name: &str, current_exe: &Path) -> PathBuf {
        let current_exe = canonical_existing(current_exe).unwrap();
        installation_staging_dir(binary_name, &current_exe).unwrap()
    }

    #[test]
    fn stage_set_rejects_missing_and_hash_mismatch_without_marker() {
        let temp = tempfile::tempdir().unwrap();
        let current = write_fixture(temp.path(), "dcc-mcp-server.exe", b"old-server");
        let missing = temp.path().join("missing.exe");
        let target = UpdateTarget::CurrentExecutable;
        let binary_name = unique_name("invalid");
        let error = stage_update_set_for(
            &binary_name,
            &current,
            &[UpdateSetSource {
                downloaded: &missing,
                target: &target,
                expected_sha256: &"0".repeat(64),
            }],
        )
        .unwrap_err();
        assert!(error.to_string().contains("missing"));
        assert!(
            !installation_root(&binary_name, &current)
                .join(PENDING_SET_DIR)
                .exists()
        );

        let downloaded = write_fixture(temp.path(), "new.exe", b"new-server");
        let error = stage_update_set_for(
            &binary_name,
            &current,
            &[UpdateSetSource {
                downloaded: &downloaded,
                target: &target,
                expected_sha256: &"f".repeat(64),
            }],
        )
        .unwrap_err();
        assert!(matches!(error, UpdateError::ChecksumMismatch { .. }));
        assert!(
            !installation_root(&binary_name, &current)
                .join(PENDING_SET_DIR)
                .exists()
        );
    }

    #[test]
    fn stage_set_rejects_removed_capture_helper() {
        let temp = tempfile::tempdir().unwrap();
        let current = write_fixture(temp.path(), "dcc-mcp-server.exe", b"old-server");
        let downloaded = write_fixture(temp.path(), "new.exe", b"new-server");
        let sha = sha256_file(&downloaded).unwrap();
        let current_target = UpdateTarget::CurrentExecutable;
        let removed_target = UpdateTarget::Sibling {
            file_name: REMOVED_CAPTURE_HELPER.into(),
        };
        let binary_name = unique_name("removed-helper");
        let error = stage_update_set_for(
            &binary_name,
            &current,
            &[
                UpdateSetSource {
                    downloaded: &downloaded,
                    target: &current_target,
                    expected_sha256: &sha,
                },
                UpdateSetSource {
                    downloaded: &downloaded,
                    target: &removed_target,
                    expected_sha256: &sha,
                },
            ],
        )
        .unwrap_err();
        assert!(error.to_string().contains("invalid sibling"));
    }

    #[test]
    fn sibling_target_rejects_windows_ads_and_ambiguous_names() {
        for file_name in [
            "host.exe:stream",
            "host.exe.",
            "host.exe ",
            "host name.exe",
            "host\n.exe",
            "..\\host.exe",
        ] {
            assert!(
                validate_sibling_name(file_name).is_err(),
                "unsafe sibling target was accepted: {file_name:?}"
            );
        }
        validate_sibling_name("dcc-mcp-ui-control-host.exe").unwrap();
    }

    #[test]
    fn update_set_applies_server_and_host_together() {
        let temp = tempfile::tempdir().unwrap();
        let current = write_fixture(temp.path(), "dcc-mcp-server.exe", b"old-server");
        let host = write_fixture(temp.path(), "dcc-mcp-ui-control-host.exe", b"old-host");
        let server_download = write_fixture(temp.path(), "server.download", b"new-server");
        let host_download = write_fixture(temp.path(), "host.download", b"new-host");
        let server_sha = sha256_file(&server_download).unwrap();
        let host_sha = sha256_file(&host_download).unwrap();
        let current_target = UpdateTarget::CurrentExecutable;
        let host_target = UpdateTarget::Sibling {
            file_name: "dcc-mcp-ui-control-host.exe".into(),
        };
        let binary_name = unique_name("apply");
        stage_update_set_for(
            &binary_name,
            &current,
            &[
                UpdateSetSource {
                    downloaded: &server_download,
                    target: &current_target,
                    expected_sha256: &server_sha,
                },
                UpdateSetSource {
                    downloaded: &host_download,
                    target: &host_target,
                    expected_sha256: &host_sha,
                },
            ],
        )
        .unwrap();
        assert!(apply_staged_update_set_for(&binary_name, &current).unwrap());
        assert_eq!(std::fs::read(&current).unwrap(), b"new-server");
        assert_eq!(std::fs::read(&host).unwrap(), b"new-host");
        assert!(
            !installation_root(&binary_name, &current)
                .join(PENDING_SET_DIR)
                .exists()
        );
    }

    #[test]
    fn update_set_rolls_back_first_component_when_second_replace_fails() {
        let temp = tempfile::tempdir().unwrap();
        let current = write_fixture(temp.path(), "dcc-mcp-server.exe", b"old-server");
        let host = write_fixture(temp.path(), "dcc-mcp-ui-control-host.exe", b"old-host");
        let server_download = write_fixture(temp.path(), "server.download", b"new-server");
        let host_download = write_fixture(temp.path(), "host.download", b"new-host");
        let server_sha = sha256_file(&server_download).unwrap();
        let host_sha = sha256_file(&host_download).unwrap();
        let current_target = UpdateTarget::CurrentExecutable;
        let host_target = UpdateTarget::Sibling {
            file_name: "dcc-mcp-ui-control-host.exe".into(),
        };
        let binary_name = unique_name("rollback");
        stage_update_set_for(
            &binary_name,
            &current,
            &[
                UpdateSetSource {
                    downloaded: &server_download,
                    target: &current_target,
                    expected_sha256: &server_sha,
                },
                UpdateSetSource {
                    downloaded: &host_download,
                    target: &host_target,
                    expected_sha256: &host_sha,
                },
            ],
        )
        .unwrap();

        // A directory at the deterministic server backup path forces the
        // second replacement to fail after the sibling host was installed.
        // The transaction must restore the already-replaced host.
        let blocking_backup = current.with_file_name("dcc-mcp-server.exe.dcc-mcp-backup");
        std::fs::create_dir(&blocking_backup).unwrap();
        let error = apply_staged_update_set_for(&binary_name, &current).unwrap_err();
        assert!(matches!(error, UpdateError::Stage(_)));
        assert!(error.to_string().contains("not a regular file"));
        assert_eq!(std::fs::read(&current).unwrap(), b"old-server");
        assert_eq!(std::fs::read(&host).unwrap(), b"old-host");
        assert!(
            installation_root(&binary_name, &current)
                .join(PENDING_SET_DIR)
                .is_dir()
        );
        std::fs::remove_dir(&blocking_backup).unwrap();
        std::fs::remove_dir_all(installation_root(&binary_name, &current)).unwrap();
    }

    #[test]
    fn rollback_preserves_original_before_first_rename() {
        let temp = tempfile::tempdir().unwrap();
        let target = write_fixture(temp.path(), "dcc-mcp-ui-control-host.exe", b"old-host");
        let prepared = write_fixture(
            temp.path(),
            "dcc-mcp-ui-control-host.exe.dcc-mcp-new",
            b"new-host",
        );
        let backup = temp
            .path()
            .join("dcc-mcp-ui-control-host.exe.dcc-mcp-backup");
        let journal = UpdateJournal {
            state: JournalState::Applying,
            entries: vec![JournalEntry {
                target: target.clone(),
                prepared: prepared.clone(),
                backup,
                had_original: true,
                backed_up: false,
                installed: false,
            }],
        };

        rollback(&journal).unwrap();

        assert_eq!(std::fs::read(target).unwrap(), b"old-host");
        assert!(!prepared.exists());
    }

    #[test]
    fn rollback_recovers_rename_before_backup_state_is_journaled() {
        let temp = tempfile::tempdir().unwrap();
        let target = write_fixture(temp.path(), "dcc-mcp-ui-control-host.exe", b"old-host");
        let prepared = write_fixture(
            temp.path(),
            "dcc-mcp-ui-control-host.exe.dcc-mcp-new",
            b"new-host",
        );
        let backup = temp
            .path()
            .join("dcc-mcp-ui-control-host.exe.dcc-mcp-backup");
        std::fs::rename(&target, &backup).unwrap();
        let journal = UpdateJournal {
            state: JournalState::Applying,
            entries: vec![JournalEntry {
                target: target.clone(),
                prepared: prepared.clone(),
                backup: backup.clone(),
                had_original: true,
                backed_up: false,
                installed: false,
            }],
        };

        rollback(&journal).unwrap();

        assert_eq!(std::fs::read(target).unwrap(), b"old-host");
        assert!(!backup.exists());
        assert!(!prepared.exists());
    }

    #[test]
    fn staged_sets_are_isolated_by_exact_server_installation() {
        let temp = tempfile::tempdir().unwrap();
        let first_dir = temp.path().join("first");
        let second_dir = temp.path().join("second");
        std::fs::create_dir_all(&first_dir).unwrap();
        std::fs::create_dir_all(&second_dir).unwrap();
        let first = write_fixture(&first_dir, "dcc-mcp-server.exe", b"old-first");
        let second = write_fixture(&second_dir, "dcc-mcp-server.exe", b"old-second");
        let downloaded = write_fixture(temp.path(), "server.download", b"new-server");
        let sha = sha256_file(&downloaded).unwrap();
        let target = UpdateTarget::CurrentExecutable;
        let binary_name = unique_name("bound");
        stage_update_set_for(
            &binary_name,
            &first,
            &[UpdateSetSource {
                downloaded: &downloaded,
                target: &target,
                expected_sha256: &sha,
            }],
        )
        .unwrap();
        stage_update_set_for(
            &binary_name,
            &second,
            &[UpdateSetSource {
                downloaded: &downloaded,
                target: &target,
                expected_sha256: &sha,
            }],
        )
        .unwrap();

        let first_root = installation_root(&binary_name, &first);
        let second_root = installation_root(&binary_name, &second);
        assert_ne!(first_root, second_root);
        assert!(first_root.join(PENDING_SET_DIR).is_dir());
        assert!(second_root.join(PENDING_SET_DIR).is_dir());
        assert!(apply_staged_update_set_for(&binary_name, &second).unwrap());
        assert!(apply_staged_update_set_for(&binary_name, &first).unwrap());
        assert_eq!(std::fs::read(&first).unwrap(), b"new-server");
        assert_eq!(std::fs::read(&second).unwrap(), b"new-server");
    }

    #[test]
    fn installation_lock_times_out_without_blocking_another_installation() {
        let temp = tempfile::tempdir().unwrap();
        let first_dir = temp.path().join("first");
        let second_dir = temp.path().join("second");
        std::fs::create_dir_all(&first_dir).unwrap();
        std::fs::create_dir_all(&second_dir).unwrap();
        let first = write_fixture(&first_dir, "dcc-mcp-server.exe", b"old-first");
        let second = write_fixture(&second_dir, "dcc-mcp-server.exe", b"old-second");
        let downloaded = write_fixture(temp.path(), "server.download", b"new-server");
        let sha = sha256_file(&downloaded).unwrap();
        let target = UpdateTarget::CurrentExecutable;
        let source = UpdateSetSource {
            downloaded: &downloaded,
            target: &target,
            expected_sha256: &sha,
        };
        let binary_name = unique_name("file-lock");
        let first_root = installation_root(&binary_name, &first);
        std::fs::create_dir_all(&first_root).unwrap();
        let external_lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(first_root.join(TRANSACTION_LOCK_FILE))
            .unwrap();
        FileExt::lock(&external_lock).unwrap();

        stage_update_set_for_with_timeout(
            &binary_name,
            &second,
            &[source],
            Duration::from_millis(25),
        )
        .unwrap();
        let error = stage_update_set_for_with_timeout(
            &binary_name,
            &first,
            &[source],
            Duration::from_millis(25),
        )
        .unwrap_err();
        assert!(error.to_string().contains("timed out waiting"));
        FileExt::unlock(&external_lock).unwrap();
    }

    #[test]
    fn in_process_installation_lock_times_out_fail_closed() {
        let temp = tempfile::tempdir().unwrap();
        let current = write_fixture(temp.path(), "dcc-mcp-server.exe", b"old-server");
        let downloaded = write_fixture(temp.path(), "server.download", b"new-server");
        let sha = sha256_file(&downloaded).unwrap();
        let target = UpdateTarget::CurrentExecutable;
        let binary_name = unique_name("process-lock");
        let canonical = canonical_existing(&current).unwrap();
        let root = installation_staging_dir(&binary_name, &canonical).unwrap();
        let _held =
            acquire_installation_lock(&root, &canonical, Duration::from_millis(25)).unwrap();

        let error = stage_update_set_for_with_timeout(
            &binary_name,
            &current,
            &[UpdateSetSource {
                downloaded: &downloaded,
                target: &target,
                expected_sha256: &sha,
            }],
            Duration::from_millis(25),
        )
        .unwrap_err();
        assert!(error.to_string().contains("in-process update mutex"));
    }

    #[test]
    fn committed_recovery_clears_marker_without_requesting_reexec() {
        let temp = tempfile::tempdir().unwrap();
        let current = write_fixture(temp.path(), "dcc-mcp-server.exe", b"new-server");
        let downloaded = write_fixture(temp.path(), "server.download", b"new-server");
        let sha = sha256_file(&downloaded).unwrap();
        let target = UpdateTarget::CurrentExecutable;
        let binary_name = unique_name("committed-recovery");
        stage_update_set_for(
            &binary_name,
            &current,
            &[UpdateSetSource {
                downloaded: &downloaded,
                target: &target,
                expected_sha256: &sha,
            }],
        )
        .unwrap();
        let prepared = write_fixture(temp.path(), "server.dcc-mcp-new", b"leftover");
        let backup = write_fixture(temp.path(), "server.dcc-mcp-backup", b"leftover");
        let journal = UpdateJournal {
            state: JournalState::Committed,
            entries: vec![JournalEntry {
                target: current.clone(),
                prepared: prepared.clone(),
                backup: backup.clone(),
                had_original: true,
                backed_up: true,
                installed: true,
            }],
        };
        let pending = installation_root(&binary_name, &current).join(PENDING_SET_DIR);
        write_json_atomic(&pending.join(JOURNAL_FILE), &journal).unwrap();

        assert!(!apply_staged_update_set_for(&binary_name, &current).unwrap());
        assert!(!pending.exists());
        assert!(!prepared.exists());
        assert!(!backup.exists());
        assert!(!apply_staged_update_set_for(&binary_name, &current).unwrap());
    }

    #[test]
    fn bootstrap_replaces_missing_or_stale_sibling_after_hash_check() {
        let temp = tempfile::tempdir().unwrap();
        let current = write_fixture(temp.path(), "dcc-mcp-server.exe", b"server");
        let host_download = write_fixture(temp.path(), "host.download", b"new-host");
        let host_sha = sha256_file(&host_download).unwrap();
        let binary_name = unique_name("bootstrap");
        install_verified_sibling(
            &binary_name,
            &current,
            &host_download,
            "dcc-mcp-ui-control-host.exe",
            &host_sha,
        )
        .unwrap();
        let host = temp.path().join("dcc-mcp-ui-control-host.exe");
        assert_eq!(std::fs::read(&host).unwrap(), b"new-host");
        std::fs::write(&host, b"stale-host").unwrap();
        install_verified_sibling(
            &binary_name,
            &current,
            &host_download,
            "dcc-mcp-ui-control-host.exe",
            &host_sha,
        )
        .unwrap();
        assert_eq!(std::fs::read(host).unwrap(), b"new-host");
    }
}
