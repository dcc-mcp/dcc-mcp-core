//! Path-safe I/O diagnostics for staged-update filesystem operations.
//!
//! The staged-update swap is a chain of filesystem operations, and a failure
//! on Windows can surface as a bare `os error 5` with no indication of which
//! step failed. These helpers attach static, path-free context to that error
//! and to a structured log event. Absolute paths are deliberately kept out of
//! the public error/log: installation paths commonly contain user names and
//! other sensitive layout. Redacted paths are available only on an opt-in
//! local diagnostic channel for byte-level debugging on the affected host.

use std::path::Path;

use crate::UpdateError;

/// Static, non-sensitive description of one filesystem operation in the
/// staged-update transaction. Every field is a fixed category, never a path.
#[derive(Clone, Copy)]
pub(super) struct IoOp {
    pub(super) stage: &'static str,
    pub(super) operation: &'static str,
    pub(super) source: &'static str,
    pub(super) target: &'static str,
}

/// Opt-in gate for the controlled local path diagnostic. Unset by default, so
/// no path (redacted or otherwise) is emitted unless the host explicitly asks
/// for byte-level diagnostics.
const UPDATE_DIAG_PATHS_ENV: &str = "DCC_MCP_UPDATE_DIAG_PATHS";

/// Bound and strip control characters before text reaches a log or warning.
pub(super) fn sanitize(text: &str, max_chars: usize) -> String {
    text.chars()
        .filter(|c| !c.is_control())
        .take(max_chars)
        .collect()
}

/// Redact a path for the controlled local diagnostic channel. User profile
/// and data-directory prefixes are replaced and the result is bounded. This
/// is never used by the propagated error or the always-on warning.
pub(super) fn redact_path(path: &Path) -> String {
    let mut text = path.to_string_lossy().into_owned();
    for (key, label) in [
        ("USERPROFILE", "<home>"),
        ("HOME", "<home>"),
        ("APPDATA", "<appdata>"),
        ("LOCALAPPDATA", "<localappdata>"),
    ] {
        if let Ok(prefix) = std::env::var(key)
            && !prefix.is_empty()
        {
            text = text.replace(&prefix, label);
        }
    }
    sanitize(&text, 256)
}

/// Run a filesystem operation and, on failure, attach diagnosable but
/// path-safe context to both the error and a structured log event.
///
/// The original [`std::io::Error`] is preserved as the error source, so its
/// raw OS code and `ErrorKind` remain available and the caller's rollback
/// semantics are unchanged. Public output carries only static categories;
/// redacted paths are emitted solely on the opt-in `debug` diagnostic channel.
pub(super) fn record_io_failure<T>(
    op: IoOp,
    source_path: &Path,
    target_path: &Path,
    result: std::io::Result<T>,
) -> Result<T, UpdateError> {
    result.map_err(|error| {
        let os_error = error.raw_os_error();
        tracing::warn!(
            stage = op.stage,
            operation = op.operation,
            source = op.source,
            target = op.target,
            os_error = ?os_error,
            error_kind = ?error.kind(),
            error = %sanitize(&error.to_string(), 240),
            "staged update filesystem operation failed"
        );
        if std::env::var_os(UPDATE_DIAG_PATHS_ENV).is_some() {
            tracing::debug!(
                stage = op.stage,
                operation = op.operation,
                source_path = %redact_path(source_path),
                target_path = %redact_path(target_path),
                os_error = ?os_error,
                "staged update filesystem operation failed (controlled local path diagnostic)"
            );
        }
        UpdateError::IoWithContext {
            stage: op.stage,
            operation: op.operation,
            source: op.source,
            target: op.target,
            error,
        }
    })
}
