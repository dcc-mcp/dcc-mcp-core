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
const PREVIOUS_SET_DIR: &str = "pending-set.previous";
const COMMITTED_SET_PREFIX: &str = "pending-set.committed-";
const MANIFEST_FILE: &str = "manifest.json";
const JOURNAL_FILE: &str = "journal.json";
const APPLIED_GENERATION_FILE: &str = "applied-generation.json";
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
    source_build_version: String,
    source_server_sha256: String,
    components: Vec<StagedComponent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AppliedGeneration {
    format_version: u8,
    bound_executable: PathBuf,
    source_build_version: String,
    source_server_sha256: String,
    target_server_sha256: String,
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
    original_sha256: Option<String>,
    expected_sha256: String,
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
    stage_update_set_for_build_with_timeout(
        binary_name,
        current_exe,
        env!("CARGO_PKG_VERSION"),
        sources,
        lock_timeout,
    )
}

fn stage_update_set_for_build_with_timeout(
    binary_name: &str,
    current_exe: &Path,
    source_build_version: &str,
    sources: &[UpdateSetSource<'_>],
    lock_timeout: Duration,
) -> Result<(), UpdateError> {
    let bound_executable = canonical_existing(current_exe)?;
    let root = installation_staging_dir(binary_name, &bound_executable)?;
    let _lock = acquire_installation_lock(&root, &bound_executable, lock_timeout)?;
    recover_stage_swap(&root)?;
    cleanup_committed_sets(&root)?;

    let pending = root.join(PENDING_SET_DIR);
    if pending.join(JOURNAL_FILE).exists() {
        return Err(UpdateError::Stage(
            "recover the pending update transaction before staging another set".into(),
        ));
    }

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
    if source_build_version.is_empty() || source_build_version.len() > 128 {
        return Err(UpdateError::Stage(
            "source build version must contain 1 to 128 characters".into(),
        ));
    }
    let source_server_sha256 = sha256_file(&bound_executable)?;

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
            format_version: 2,
            bound_executable,
            source_build_version: source_build_version.to_owned(),
            source_server_sha256,
            components,
        };
        write_json_atomic(&temp_dir.join(MANIFEST_FILE), &manifest)?;
        Ok::<(), UpdateError>(())
    })();
    if let Err(error) = stage_result {
        let _ = std::fs::remove_dir_all(&temp_dir);
        return Err(error);
    }

    let previous = root.join(PREVIOUS_SET_DIR);
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
    if previous.exists() {
        std::fs::remove_dir_all(previous)?;
    }
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
    apply_staged_update_set_for_build_with_timeout(
        binary_name,
        current_exe,
        env!("CARGO_PKG_VERSION"),
        lock_timeout,
    )
}

fn apply_staged_update_set_for_build_with_timeout(
    binary_name: &str,
    current_exe: &Path,
    current_build_version: &str,
    lock_timeout: Duration,
) -> Result<bool, UpdateError> {
    let current_exe = canonical_existing(current_exe)?;
    let root = installation_staging_dir(binary_name, &current_exe)?;
    let _lock = acquire_installation_lock(&root, &current_exe, lock_timeout)?;
    recover_stage_swap(&root)?;
    cleanup_committed_sets(&root)?;

    let pending = root.join(PENDING_SET_DIR);
    if !pending.is_dir() {
        return applied_generation_requires_reexec(&root, &current_exe, current_build_version);
    }
    let manifest: UpdateSetManifest = read_json(&pending.join(MANIFEST_FILE))?;
    if manifest.format_version != 2 {
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
    if super::is_newer_version(&manifest.source_build_version, current_build_version) {
        if applied_generation_requires_reexec(&root, &current_exe, current_build_version)? {
            return Ok(true);
        }
        return Err(UpdateError::Stage(format!(
            "staged update source build {} is newer than running build {current_build_version}",
            manifest.source_build_version
        )));
    }

    let journal_path = pending.join(JOURNAL_FILE);
    if journal_path.is_file() {
        let journal: UpdateJournal = read_json(&journal_path)?;
        if journal.state == JournalState::Committed {
            persist_applied_generation(&root, &manifest)?;
            finish_committed(&pending, &journal)?;
            return applied_generation_requires_reexec(&root, &current_exe, current_build_version);
        }
        rollback(&journal)?;
        std::fs::remove_file(&journal_path)?;
        cleanup_committed_entries(&journal)?;
    }

    let actual_source_sha = sha256_file(&current_exe)?;
    if !actual_source_sha.eq_ignore_ascii_case(&manifest.source_server_sha256) {
        return Err(UpdateError::Stage(format!(
            "staged update source changed: expected {}, got {actual_source_sha}",
            manifest.source_server_sha256
        )));
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
        let had_original = ensure_reserved_file_path(&target, "update target")?;
        let original_sha256 = had_original.then(|| sha256_file(&target)).transpose()?;
        prepare_verified_copy(
            &source,
            &prepared,
            "prepared update path",
            &component.sha256,
        )?;
        entries.push(JournalEntry {
            target,
            prepared,
            backup,
            // Persist the pre-transaction state before the first rename. If
            // the process exits after the initial journal write, recovery
            // must not mistake an untouched original for a newly installed
            // file and delete it.
            had_original,
            original_sha256,
            expected_sha256: component.sha256.clone(),
        });
    }
    entries.sort_by_key(|entry| paths_equal(&entry.target, &current_exe));

    let mut journal = UpdateJournal {
        state: JournalState::Applying,
        entries,
    };
    write_json_atomic(&journal_path, &journal)?;
    let result = apply_entries(&journal);
    if let Err(error) = result {
        if let Err(rollback_error) = rollback(&journal) {
            return Err(UpdateError::Stage(format!(
                "update failed ({error}); rollback also failed ({rollback_error})"
            )));
        }
        std::fs::remove_file(&journal_path)?;
        cleanup_committed_entries(&journal)?;
        return Err(error);
    }
    journal.state = JournalState::Committed;
    write_json_atomic(&journal_path, &journal)?;
    persist_applied_generation(&root, &manifest)?;
    finish_committed(&pending, &journal)?;
    tracing::info!(path = %current_exe.display(), "staged update set applied successfully");
    applied_generation_requires_reexec(&root, &current_exe, current_build_version)
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
    cleanup_committed_sets(&root)?;

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

fn apply_entries(journal: &UpdateJournal) -> Result<(), UpdateError> {
    for index in 0..journal.entries.len() {
        let target = journal.entries[index].target.clone();
        let prepared = journal.entries[index].prepared.clone();
        let backup = journal.entries[index].backup.clone();
        let expected_sha256 = journal.entries[index].expected_sha256.clone();
        validate_expected_sha(&expected_sha256)?;
        if ensure_reserved_file_path(&backup, "update backup path")? {
            let original_sha256 = journal.entries[index]
                .original_sha256
                .as_deref()
                .ok_or_else(|| {
                    UpdateError::Stage(format!(
                        "unexpected backup without an original digest: {}",
                        backup.display()
                    ))
                })?;
            require_file_hash(&backup, original_sha256, "stale update backup")?;
            require_file_hash(&target, original_sha256, "restored update target")?;
            std::fs::remove_file(&backup)?;
        }
        if journal.entries[index].had_original {
            let original_sha256 = journal.entries[index]
                .original_sha256
                .as_deref()
                .ok_or_else(|| {
                    UpdateError::Stage(format!(
                        "update target has no recorded original digest: {}",
                        target.display()
                    ))
                })?;
            require_file_hash(&target, original_sha256, "update target")?;
            std::fs::rename(&target, &backup)?;
        } else if ensure_reserved_file_path(&target, "new update target")? {
            return Err(UpdateError::Stage(format!(
                "new update target appeared during the transaction: {}",
                target.display()
            )));
        }
        require_file_hash(&prepared, &expected_sha256, "prepared update component")?;
        std::fs::rename(&prepared, &target)?;
        require_file_hash(&target, &expected_sha256, "installed update component")?;
    }
    Ok(())
}

fn rollback(journal: &UpdateJournal) -> Result<(), UpdateError> {
    let mut first_error = None;
    for entry in journal.entries.iter().rev() {
        if let Err(error) = rollback_entry(entry)
            && first_error.is_none()
        {
            first_error = Some(error);
        }
    }
    if let Some(error) = first_error {
        return Err(error);
    }
    Ok(())
}

fn rollback_entry(entry: &JournalEntry) -> Result<(), UpdateError> {
    validate_expected_sha(&entry.expected_sha256)?;
    if entry.had_original {
        let original_sha256 = entry.original_sha256.as_deref().ok_or_else(|| {
            UpdateError::Stage(format!(
                "cannot roll back update without the original digest: {}",
                entry.target.display()
            ))
        })?;
        validate_expected_sha(original_sha256)?;
        let target_is_file = ensure_reserved_file_path(&entry.target, "rollback update target")?;

        if target_is_file && sha256_file(&entry.target)?.eq_ignore_ascii_case(original_sha256) {
            if ensure_reserved_file_path(&entry.backup, "update backup")? {
                require_file_hash(&entry.backup, original_sha256, "update backup")?;
            }
            remove_reserved_file(&entry.prepared, "rollback prepared path")?;
            return Ok(());
        }
        require_file_hash(&entry.backup, original_sha256, "update backup")?;
        if target_is_file {
            require_file_hash(
                &entry.target,
                &entry.expected_sha256,
                "installed update component",
            )?;
        }
        prepare_verified_copy(
            &entry.backup,
            &entry.prepared,
            "rollback prepared path",
            original_sha256,
        )?;
        if target_is_file {
            std::fs::remove_file(&entry.target)?;
        }
        std::fs::rename(&entry.prepared, &entry.target)?;
        require_file_hash(&entry.target, original_sha256, "restored update target")?;
    } else {
        if entry.original_sha256.is_some()
            || ensure_reserved_file_path(&entry.backup, "unexpected update backup")?
        {
            return Err(UpdateError::Stage(format!(
                "cannot roll back a new target with an unexpected backup: {}",
                entry.backup.display()
            )));
        }
        if ensure_reserved_file_path(&entry.target, "new update target")? {
            require_file_hash(&entry.target, &entry.expected_sha256, "new update target")?;
            std::fs::remove_file(&entry.target)?;
        }
        if ensure_reserved_file_path(&entry.prepared, "rollback prepared path")? {
            remove_reserved_file(&entry.prepared, "rollback prepared path")?;
        }
    }
    Ok(())
}

fn prepare_verified_copy(
    source: &Path,
    target: &Path,
    label: &str,
    expected_sha256: &str,
) -> Result<(), UpdateError> {
    ensure_reserved_file_path(target, label)?;
    std::fs::copy(source, target)?;
    require_file_hash(target, expected_sha256, label)
}

fn remove_reserved_file(path: &Path, label: &str) -> Result<(), UpdateError> {
    if ensure_reserved_file_path(path, label)? {
        std::fs::remove_file(path)?;
    }
    Ok(())
}

fn ensure_reserved_file_path(path: &Path, label: &str) -> Result<bool, UpdateError> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_file() => Ok(true),
        Ok(_) => Err(UpdateError::Stage(format!(
            "{label} is not a regular file: {}",
            path.display()
        ))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

fn remove_known_file(
    path: &Path,
    label: &str,
    expected_sha256: &str,
    alternate_sha256: Option<&str>,
) -> Result<(), UpdateError> {
    if !ensure_reserved_file_path(path, label)? {
        return Ok(());
    }
    let digest = sha256_file(path)?;
    if !digest.eq_ignore_ascii_case(expected_sha256)
        && !alternate_sha256.is_some_and(|value| digest.eq_ignore_ascii_case(value))
    {
        return Err(UpdateError::Stage(format!(
            "refusing to remove an unrecognized {label}: {}",
            path.display()
        )));
    }
    std::fs::remove_file(path)?;
    Ok(())
}

fn require_file_hash(path: &Path, expected: &str, label: &str) -> Result<(), UpdateError> {
    if !ensure_reserved_file_path(path, label)? {
        return Err(UpdateError::Stage(format!(
            "{label} is missing or not a regular file: {}",
            path.display()
        )));
    }
    let actual = sha256_file(path)?;
    if !actual.eq_ignore_ascii_case(expected) {
        return Err(UpdateError::ChecksumMismatch {
            expected: expected.to_ascii_lowercase(),
            actual,
        });
    }
    Ok(())
}

fn finish_committed(pending: &Path, journal: &UpdateJournal) -> Result<(), UpdateError> {
    let root = pending
        .parent()
        .ok_or_else(|| UpdateError::Stage("pending update has no parent directory".into()))?;
    let detached = root.join(format!("{COMMITTED_SET_PREFIX}{}", unique_nonce()?));
    std::fs::rename(pending, &detached)?;
    cleanup_committed_entries(journal)?;
    std::fs::remove_dir_all(detached)?;
    Ok(())
}

fn cleanup_committed_entries(journal: &UpdateJournal) -> Result<(), UpdateError> {
    for entry in &journal.entries {
        remove_known_file(
            &entry.prepared,
            "transaction prepared file",
            &entry.expected_sha256,
            entry.original_sha256.as_deref(),
        )?;
        if ensure_reserved_file_path(&entry.backup, "transaction backup")? {
            let original_sha256 = entry.original_sha256.as_deref().ok_or_else(|| {
                UpdateError::Stage(format!(
                    "unexpected backup for a newly created update target: {}",
                    entry.backup.display()
                ))
            })?;
            remove_known_file(&entry.backup, "transaction backup", original_sha256, None)?;
        }
    }
    Ok(())
}

fn cleanup_committed_sets(root: &Path) -> Result<(), UpdateError> {
    for entry in std::fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        let is_committed_set = entry
            .file_name()
            .to_str()
            .is_some_and(|name| name.starts_with(COMMITTED_SET_PREFIX));
        if !is_committed_set || !path.is_dir() {
            continue;
        }
        let journal: UpdateJournal = read_json(&path.join(JOURNAL_FILE))?;
        if journal.state != JournalState::Committed {
            return Err(UpdateError::Stage(format!(
                "detached update set is not committed: {}",
                path.display()
            )));
        }
        cleanup_committed_entries(&journal)?;
        std::fs::remove_dir_all(path)?;
    }
    Ok(())
}

fn recover_stage_swap(root: &Path) -> Result<(), UpdateError> {
    let previous = root.join(PREVIOUS_SET_DIR);
    if !previous.exists() {
        return Ok(());
    }
    if !previous.is_dir() {
        return Err(UpdateError::Stage(format!(
            "previous staged update is not a directory: {}",
            previous.display()
        )));
    }
    let pending = root.join(PENDING_SET_DIR);
    if pending.exists() {
        if !pending.is_dir() || !pending.join(MANIFEST_FILE).is_file() {
            return Err(UpdateError::Stage(format!(
                "pending update is incomplete while a previous set exists: {}",
                pending.display()
            )));
        }
        std::fs::remove_dir_all(previous)?;
    } else {
        std::fs::rename(previous, pending)?;
    }
    Ok(())
}

fn persist_applied_generation(
    root: &Path,
    manifest: &UpdateSetManifest,
) -> Result<(), UpdateError> {
    validate_expected_sha(&manifest.source_server_sha256)?;
    if manifest.source_build_version.is_empty() || manifest.source_build_version.len() > 128 {
        return Err(UpdateError::Stage(
            "staged update has an invalid source build version".into(),
        ));
    }
    let mut server_components = manifest
        .components
        .iter()
        .filter(|component| matches!(component.target, UpdateTarget::CurrentExecutable));
    let target_server_sha256 = server_components
        .next()
        .ok_or_else(|| UpdateError::Stage("staged update has no server component".into()))?
        .sha256
        .clone();
    if server_components.next().is_some() {
        return Err(UpdateError::Stage(
            "staged update has duplicate server components".into(),
        ));
    }
    validate_expected_sha(&target_server_sha256)?;
    require_file_hash(
        &manifest.bound_executable,
        &target_server_sha256,
        "committed server executable",
    )?;
    let generation = AppliedGeneration {
        format_version: 1,
        bound_executable: manifest.bound_executable.clone(),
        source_build_version: manifest.source_build_version.clone(),
        source_server_sha256: manifest.source_server_sha256.clone(),
        target_server_sha256,
    };
    write_json_atomic(&root.join(APPLIED_GENERATION_FILE), &generation)
}

fn applied_generation_requires_reexec(
    root: &Path,
    current_exe: &Path,
    current_build_version: &str,
) -> Result<bool, UpdateError> {
    let path = root.join(APPLIED_GENERATION_FILE);
    if !ensure_reserved_file_path(&path, "applied update generation")? {
        return Ok(false);
    }
    let generation: AppliedGeneration = read_json(&path)?;
    if generation.format_version != 1 || !paths_equal(current_exe, &generation.bound_executable) {
        return Err(UpdateError::Stage(
            "applied update generation does not match this installation".into(),
        ));
    }
    validate_expected_sha(&generation.source_server_sha256)?;
    validate_expected_sha(&generation.target_server_sha256)?;
    if generation
        .source_server_sha256
        .eq_ignore_ascii_case(&generation.target_server_sha256)
        || super::is_newer_version(current_build_version, &generation.source_build_version)
    {
        return Ok(false);
    }
    Ok(sha256_file(current_exe)?.eq_ignore_ascii_case(&generation.target_server_sha256))
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
mod tests;
