//! Windows-safe cleanup of committed staged-update transactions.
//!
//! A running executable image can be *renamed* on Windows, but deleting the
//! renamed old image fails with `ERROR_ACCESS_DENIED` (os error 5) until every
//! process that mapped it exits; a handle opened without `FILE_SHARE_DELETE`
//! fails with `ERROR_SHARING_VIOLATION` (os error 32) for the same reason.
//! The swap has already landed by that point, so a synchronous cleanup error
//! must not abort activation or turn every later launch into a permanent
//! failure. Instead the committed set is kept as a retry marker: each launch
//! retries the deletion and drops the marker only once it succeeds.
//!
//! See dcc-mcp-core#2864.

use std::path::{Path, PathBuf};

use crate::UpdateError;

use super::io_diagnostics::{IoOp, record_io_failure};
use super::{UpdateJournal, ensure_reserved_file_path, sha256_file, unique_nonce};

/// Whether a cleanup I/O failure means "the old image is still mapped; retry
/// on the next launch" rather than a real filesystem defect.
///
/// Unix can unlink a mapped executable, so this is deliberately Windows-only
/// and keeps the strict behavior everywhere else.
#[cfg(windows)]
pub(super) fn is_deferred_image_lock(error: &UpdateError) -> bool {
    let io_error = match error {
        UpdateError::IoWithContext { error, .. } => Some(error),
        UpdateError::Io(error) => Some(error),
        _ => None,
    };
    io_error.is_some_and(|error| matches!(error.raw_os_error(), Some(5) | Some(32)))
}

#[cfg(not(windows))]
pub(super) fn is_deferred_image_lock(_error: &UpdateError) -> bool {
    false
}

/// Remove one reserved file after verifying its digest.
///
/// Returns `true` when the file is gone (or was already absent) and `false`
/// when Windows still holds the old image and `defer_in_use` allows a later
/// launch to retry. Digest mismatches and all other I/O errors stay fatal.
/// `defer_in_use = false` preserves the strict behavior used by package
/// recovery, where no activation decision depends on the cleanup result.
pub(super) fn remove_reserved_file(
    path: &Path,
    label: &str,
    expected_sha256: &str,
    alternate_sha256: Option<&str>,
    defer_in_use: bool,
) -> Result<bool, UpdateError> {
    if !ensure_reserved_file_path(path, label)? {
        return Ok(true);
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
    match record_io_failure(
        IoOp {
            stage: "cleanup",
            operation: "remove_committed_reserved_file",
            source: "committed-reserved-file",
            target: "removed-reserved-file",
        },
        path,
        path,
        std::fs::remove_file(path),
    ) {
        Ok(()) => Ok(true),
        Err(error) if defer_in_use && is_deferred_image_lock(&error) => {
            tracing::info!(
                label,
                "committed reserved file is still in use; deferring cleanup"
            );
            Ok(false)
        }
        Err(error) => Err(error),
    }
}

/// Choose the backup path for a new transaction.
///
/// A regular file left at the canonical path is a committed backup whose
/// deletion was deferred because Windows still maps the old image. Reusing
/// it would make the next swap collide (Windows `rename` refuses an existing
/// destination) or fail the stale-backup digest check against a different
/// generation. Use a unique sibling for this transaction and let the retained
/// committed set clean up the old one once the image is released. Any other
/// path shape (directory, symlink, ...) is deliberately left to the
/// transaction's existing reserved-path validation.
pub(super) fn backup_path_for_transaction(
    target: &Path,
    file_name: &str,
) -> Result<PathBuf, UpdateError> {
    let canonical = target.with_file_name(format!("{file_name}.dcc-mcp-backup"));
    let deferred =
        std::fs::symlink_metadata(&canonical).is_ok_and(|metadata| metadata.file_type().is_file());
    if deferred {
        return Ok(target.with_file_name(format!("{file_name}.dcc-mcp-backup-{}", unique_nonce()?)));
    }
    Ok(canonical)
}

/// Clean up the reserved files of a committed transaction.
///
/// Returns `true` only when every file was removed, so the caller knows it
/// can discard the retry marker. A deferred Windows image lock is not an
/// error: the swap already landed and activation must proceed.
pub(super) fn cleanup_committed_entries(journal: &UpdateJournal) -> Result<bool, UpdateError> {
    let mut fully_cleaned = true;
    for entry in &journal.entries {
        fully_cleaned &= remove_reserved_file(
            &entry.prepared,
            "transaction prepared file",
            &entry.expected_sha256,
            entry.original_sha256.as_deref(),
            true,
        )?;
        if ensure_reserved_file_path(&entry.backup, "transaction backup")? {
            let original_sha256 = entry.original_sha256.as_deref().ok_or_else(|| {
                UpdateError::Stage(format!(
                    "unexpected backup for a newly created update target: {}",
                    entry.backup.display()
                ))
            })?;
            fully_cleaned &= remove_reserved_file(
                &entry.backup,
                "transaction backup",
                original_sha256,
                None,
                true,
            )?;
        }
    }
    Ok(fully_cleaned)
}
