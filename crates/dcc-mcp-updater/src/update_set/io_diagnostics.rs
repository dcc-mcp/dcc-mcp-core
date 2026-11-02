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
    let mut prefixes = Vec::new();
    for (key, label) in [
        ("USERPROFILE", "<home>"),
        ("HOME", "<home>"),
        ("APPDATA", "<appdata>"),
        ("LOCALAPPDATA", "<localappdata>"),
    ] {
        if let Ok(prefix) = std::env::var(key)
            && !prefix.is_empty()
        {
            prefixes.push((prefix, label));
        }
    }
    redact_path_with_prefixes(path, &prefixes)
}

/// Testable core of [`redact_path`]: redact each `(prefix, label)` pair from
/// `path`. Windows paths are case-insensitive and may carry an extended-length
/// / UNC device prefix (`\\?\`, `\\?\UNC\`), so a byte-exact `str::replace`
/// would leave the profile directory in the opt-in diagnostic channel. The
/// remainder of the path (including the diagnostically useful file name) is
/// preserved verbatim.
pub(super) fn redact_path_with_prefixes(
    path: &Path,
    prefixes: &[(String, &'static str)],
) -> String {
    let mut text = path.to_string_lossy().into_owned();
    for (prefix, label) in prefixes {
        for variant in prefix_variants(prefix) {
            text = replace_ignoring_ascii_case(&text, &variant, label);
        }
    }
    sanitize(&text, 256)
}

/// Every Windows spelling of one environment prefix that must be redacted.
/// The failing path and the environment variable can disagree about case,
/// separator spelling, or an extended-length / UNC device prefix.
fn prefix_variants(prefix: &str) -> Vec<String> {
    fn add(list: &mut Vec<String>, value: String) {
        if !value.is_empty()
            && !list
                .iter()
                .any(|existing| existing.eq_ignore_ascii_case(&value))
        {
            list.push(value);
        }
    }

    let plain = prefix.replace('/', "\\");
    let stripped = strip_device_prefix(&plain);
    let mut variants = Vec::new();

    // A UNC prefix may appear on the failing path in its extended-length
    // form; try that spelling first so the device prefix is consumed too.
    if let Some(rest) = stripped.strip_prefix("\\\\") {
        add(&mut variants, format!("\\\\?\\UNC\\{rest}"));
        add(
            &mut variants,
            format!("//?/UNC/{}", rest.replace('\\', "/")),
        );
    }
    // Plain and device-stripped spellings, with both separator conventions.
    add(&mut variants, plain.clone());
    add(&mut variants, plain.replace('\\', "/"));
    add(&mut variants, stripped.clone());
    add(&mut variants, stripped.replace('\\', "/"));
    variants
}

/// Map an extended-length Windows device prefix back to its plain form:
/// `\\?\C:\...` -> `C:\...` and `\\?\UNC\server\...` -> `\\server\...`.
fn strip_device_prefix(value: &str) -> String {
    let unified = value.replace('/', "\\");
    let lower = unified.to_ascii_lowercase();
    if lower.starts_with("\\\\?\\unc\\") {
        // "\\?\UNC\" is eight ASCII bytes.
        return format!("\\\\{}", &unified[8..]);
    }
    if lower.starts_with("\\\\?\\") {
        return unified[4..].to_string();
    }
    unified
}

/// Replace every occurrence of `needle` in `haystack` with `replacement`,
/// comparing ASCII case-insensitively. Working on bytes keeps every
/// non-matching byte of the original path intact instead of case-folding the
/// diagnostically useful tail.
fn replace_ignoring_ascii_case(haystack: &str, needle: &str, replacement: &str) -> String {
    if needle.is_empty() {
        return haystack.to_owned();
    }
    let hay = haystack.as_bytes();
    let needle_lower: Vec<u8> = needle
        .bytes()
        .map(|byte| byte.to_ascii_lowercase())
        .collect();
    let mut out: Vec<u8> = Vec::with_capacity(hay.len());
    let mut index = 0;
    while index < hay.len() {
        if index + needle_lower.len() <= hay.len()
            && hay[index..index + needle_lower.len()]
                .iter()
                .zip(&needle_lower)
                .all(|(byte, expected)| byte.to_ascii_lowercase() == *expected)
        {
            out.extend_from_slice(replacement.as_bytes());
            index += needle_lower.len();
        } else {
            out.push(hay[index]);
            index += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
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
