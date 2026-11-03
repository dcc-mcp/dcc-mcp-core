//! Path-safe I/O diagnostics for staged-update filesystem operations.
//!
//! The staged-update swap is a chain of filesystem operations, and a failure
//! on Windows can surface as a bare `os error 5` with no indication of which
//! step failed. These helpers attach static, path-free context to that error
//! and to a structured log event. Absolute paths are deliberately kept out of
//! the public error/log: installation paths commonly contain user names and
//! other sensitive layout. Redacted paths are available only on an opt-in
//! local diagnostic channel for byte-level debugging on the affected host.

use std::path::{Path, PathBuf};

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
    redact_path_with_lookup(path, |key| std::env::var(key).ok())
}

/// Testable seam for [`redact_path`]: collect the configured prefixes through
/// an injectable environment lookup, then rewrite the path. The lookup is
/// injectable so prefix collection can be verified without mutating the
/// process-wide environment, which parallel tests also read when resolving
/// staging directories.
pub(super) fn redact_path_with_lookup(
    path: &Path,
    lookup: impl Fn(&str) -> Option<String>,
) -> String {
    redact_path_with_sources(path, lookup, |prefix| std::fs::canonicalize(prefix).ok())
}

/// Testable seam for [`redact_path_with_lookup`]: collect raw environment
/// prefixes plus their canonical spellings through injectable sources, then
/// rewrite `path`. `canonicalize` is injectable so the symlink/junction case
/// can be verified deterministically, without mutating the process-wide
/// environment or needing symlink-creation privileges on every CI host.
pub(super) fn redact_path_with_sources(
    path: &Path,
    lookup: impl Fn(&str) -> Option<String>,
    canonicalize: impl Fn(&Path) -> Option<PathBuf>,
) -> String {
    let mut prefixes = Vec::new();
    // `XDG_DATA_HOME` is the Linux staging root (`dirs_data_dir`), and it may
    // be configured outside `HOME` (for example a private cache directory), so
    // it needs its own entry. The `HOME/.local/share` fallback is already
    // covered by `HOME`.
    for (key, label) in [
        ("USERPROFILE", "<home>"),
        ("HOME", "<home>"),
        ("APPDATA", "<appdata>"),
        ("LOCALAPPDATA", "<localappdata>"),
        ("XDG_DATA_HOME", "<xdg-data-home>"),
    ] {
        if let Some(prefix) = lookup(key)
            && !prefix.is_empty()
        {
            prefixes.push((prefix.clone(), label));
            // `canonical_existing()` resolves `current_exe` with
            // `std::fs::canonicalize()`. When an environment prefix is a
            // symlink or junction, the failing path carries the resolved
            // spelling, which the raw prefix no longer matches. Collect that
            // canonical spelling under the same label too, so the opt-in
            // debug channel stays path-safe. `prefix_variants` already covers
            // case, separator and device-prefix differences between the two.
            if let Some(canonical) = canonicalize(Path::new(&prefix)) {
                let canonical = canonical.to_string_lossy().into_owned();
                if !canonical.is_empty() && !canonical.eq_ignore_ascii_case(&prefix) {
                    prefixes.push((canonical, label));
                }
            }
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
///
/// A descendant junction/symlink below a protected prefix can resolve the
/// failing path onto another drive while still carrying the profile identity.
/// Neither the raw prefix nor the prefix's own canonical spelling matches such
/// a path, so when no prefix matched at all the protected identity itself is
/// redacted. This stays inside the opt-in `debug` channel only; paths that do
/// not carry a protected identity remain fully visible for diagnostics.
pub(super) fn redact_path_with_prefixes(
    path: &Path,
    prefixes: &[(String, &'static str)],
) -> String {
    let mut text = path.to_string_lossy().into_owned();
    let mut matched = false;
    for (prefix, label) in prefixes {
        for variant in prefix_variants(prefix) {
            let replaced = replace_ignoring_ascii_case(&text, &variant, label);
            if replaced != text {
                matched = true;
                text = replaced;
            }
        }
    }
    if !matched {
        for (prefix, label) in prefixes {
            let identity = protected_identity(prefix);
            if identity.is_empty() {
                continue;
            }
            if let Some(redacted) = redact_identity_run(&text, &identity, label) {
                text = redacted;
                break;
            }
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

/// Static, non-user-specific directory names. They never count as a profile
/// identity, so a single generic directory (for example `AppData`) cannot
/// trigger identity redaction of an unrelated path.
const STRUCTURAL_COMPONENTS: &[&str] = &[
    "users",
    "user",
    "home",
    "documents and settings",
    "appdata",
    "local",
    "locallow",
    "roaming",
    ".local",
    ".config",
    ".cache",
    "cache",
    "share",
    "data",
    "programdata",
    "windows",
    "system32",
    "program files",
    "program files (x86)",
    "var",
    "usr",
    "mnt",
    "opt",
    "tmp",
    "temp",
];

/// Container components whose following component is the user name.
const PROFILE_CONTAINERS: &[&str] = &["users", "user", "home", "documents and settings"];

fn is_structural(component: &str) -> bool {
    STRUCTURAL_COMPONENTS
        .iter()
        .any(|known| component.eq_ignore_ascii_case(known))
}

fn is_root_component(component: &str) -> bool {
    let bytes = component.as_bytes();
    bytes.len() == 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

/// Byte spans of every whole path component, split on either separator. The
/// separator bytes are not part of any span, so cutting through a span's end
/// keeps the original separator spelling of the diagnostic tail.
fn component_spans(text: &str) -> Vec<(usize, usize, &str)> {
    let bytes = text.as_bytes();
    let mut spans = Vec::new();
    let mut start = 0;
    for (index, byte) in bytes.iter().enumerate() {
        if *byte == b'\\' || *byte == b'/' {
            if start < index {
                spans.push((start, index, &text[start..index]));
            }
            start = index + 1;
        }
    }
    if start < bytes.len() {
        spans.push((start, bytes.len(), &text[start..]));
    }
    spans
}

/// Whole components of a configured prefix, with device prefixes, drive
/// letters and `.` / `..` removed. Both separator conventions are normalized
/// so a resolved path on another drive stays comparable with the prefix.
fn prefix_components(prefix: &str) -> Vec<String> {
    let plain = strip_device_prefix(prefix);
    component_spans(&plain)
        .into_iter()
        .map(|(_, _, component)| component.to_string())
        .filter(|component| !is_root_component(component) && component != "." && component != "..")
        .collect()
}

/// The identity a descendant junction can preserve somewhere in a resolved
/// path. For a profile/data prefix this is the user name (one component); for
/// a custom data root with no profile container it is the final two
/// components, so one generic directory name cannot redact unrelated paths.
fn protected_identity(prefix: &str) -> Vec<String> {
    let components = prefix_components(prefix);
    for (index, component) in components.iter().enumerate() {
        let is_container = PROFILE_CONTAINERS
            .iter()
            .any(|known| component.eq_ignore_ascii_case(known));
        if !is_container {
            continue;
        }
        if let Some(name) = components.get(index + 1)
            && !is_structural(name)
            && name.chars().count() >= 3
        {
            return vec![name.clone()];
        }
    }
    match components.as_slice() {
        [.., parent, leaf] if !is_structural(leaf) && leaf.chars().count() >= 3 => {
            vec![parent.clone(), leaf.clone()]
        }
        _ => Vec::new(),
    }
}

/// Replace the head of `text` through the last whole-component occurrence of
/// `identity` with `label`, keeping the remaining diagnostic tail verbatim.
/// Whole-component matching means a similar but unrelated directory (for
/// example `hallong-backup`) is never redacted.
fn redact_identity_run(text: &str, identity: &[String], label: &str) -> Option<String> {
    if identity.is_empty() {
        return None;
    }
    let spans = component_spans(text);
    if spans.len() < identity.len() {
        return None;
    }
    let mut last_end = None;
    for window in spans.windows(identity.len()) {
        let matches = window
            .iter()
            .zip(identity)
            .all(|(span, expected)| span.2.eq_ignore_ascii_case(expected));
        if matches {
            last_end = window.last().map(|span| span.1);
        }
    }
    last_end.map(|end| format!("{label}{}", &text[end..]))
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
