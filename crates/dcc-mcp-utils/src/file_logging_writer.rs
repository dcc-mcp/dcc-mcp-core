//! Rolling writer implementation for [`crate::file_logging`].
//!
//! Contains the `RollingFileWriter` public type, its private `Inner`
//! rotation state, and the filesystem helpers (`open_append`,
//! `current_path`, `rotated_path`, `prune_old`).

use super::config::{FileLoggingConfig, FileLoggingError, RotationPolicy};

use parking_lot::Mutex;
use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use time::OffsetDateTime;
use time::macros::format_description;

// ── Rolling writer ───────────────────────────────────────────────────────────

/// Thread-safe rolling writer — size **and/or** calendar-date triggered.
///
/// Consumed by [`tracing_appender::non_blocking`] to get async flushing.
#[derive(Debug)]
pub struct RollingFileWriter {
    pub(crate) inner: Arc<Mutex<Inner>>,
}

#[derive(Debug)]
pub(crate) struct Inner {
    pub(crate) directory: PathBuf,
    pub(crate) prefix: String,
    pub(crate) max_size: u64,
    pub(crate) max_files: usize,
    pub(crate) rotation: RotationPolicy,
    pub(crate) current: File,
    pub(crate) current_size: u64,
    pub(crate) current_date: CalendarDate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CalendarDate {
    pub(crate) year: i32,
    pub(crate) month: u8,
    pub(crate) day: u8,
}

impl CalendarDate {
    pub(crate) fn today_local() -> Self {
        let now = OffsetDateTime::now_local().unwrap_or_else(|_| OffsetDateTime::now_utc());
        Self {
            year: now.year(),
            month: now.month() as u8,
            day: now.day(),
        }
    }

    pub(crate) fn as_basename(self) -> String {
        format!("{:04}{:02}{:02}", self.year, self.month, self.day)
    }
}

impl RollingFileWriter {
    /// Build a writer from a resolved configuration.
    ///
    /// # Errors
    /// Fails if the log directory cannot be created or the initial log
    /// file cannot be opened for append.
    pub fn new(config: &FileLoggingConfig) -> Result<Self, FileLoggingError> {
        let directory = config.resolved_directory()?;
        let current_date = CalendarDate::today_local();
        let current_path = current_path(&directory, &config.file_name_prefix, current_date);
        let current = open_append(&current_path).map_err(FileLoggingError::Io)?;
        let current_size = current
            .metadata()
            .map(|m| m.len())
            .map_err(FileLoggingError::Io)?;

        Ok(Self {
            inner: Arc::new(Mutex::new(Inner {
                directory,
                prefix: config.file_name_prefix.clone(),
                max_size: config.max_size_bytes.max(1),
                max_files: config.max_files,
                rotation: config.rotation,
                current,
                current_size,
                current_date,
            })),
        })
    }
}

impl Inner {
    /// Check whether the current file needs rotating *before* a write
    /// of `incoming` bytes. Rotates if needed.
    fn maybe_rotate(&mut self, incoming: usize) -> io::Result<()> {
        let today = CalendarDate::today_local();

        let size_trigger = self.rotation.rotates_on_size()
            && self.current_size.saturating_add(incoming as u64) > self.max_size
            && self.current_size > 0;
        let date_trigger = self.rotation.rotates_on_date() && today != self.current_date;

        if !size_trigger && !date_trigger {
            return Ok(());
        }

        // Best-effort flush before rename.
        let _ = self.current.flush();

        // Drop the old handle by replacing with a dummy, so the file
        // is closed on Windows (which forbids renaming an open file).
        let placeholder = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(self.directory.join(".dcc-mcp-rotate-tmp"))?;
        let old = std::mem::replace(&mut self.current, placeholder);
        drop(old);

        let old_current = current_path(&self.directory, &self.prefix, self.current_date);
        if old_current.exists() {
            let rotated = rotated_path(&self.directory, &self.prefix);
            // Ignore rotate-rename failures silently — we'd rather keep
            // logging to the current file than panic on EBUSY.
            let _ = std::fs::rename(&old_current, &rotated);
        }

        // Update date and open the new current file.
        self.current_date = today;
        let new_current = current_path(&self.directory, &self.prefix, self.current_date);
        self.current = open_append(&new_current)?;
        self.current_size = self.current.metadata().map(|m| m.len()).unwrap_or_default();

        // Clean the placeholder file after successful rotation.
        let _ = std::fs::remove_file(self.directory.join(".dcc-mcp-rotate-tmp"));

        // Retention pruning.
        prune_old(&self.directory, &self.prefix, self.max_files);

        Ok(())
    }
}

impl Write for RollingFileWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut inner = self.inner.lock();
        inner.maybe_rotate(buf.len())?;
        let n = inner.current.write(buf)?;
        inner.current_size = inner.current_size.saturating_add(n as u64);
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.lock().current.flush()
    }
}

// ── File helpers ─────────────────────────────────────────────────────────────

pub(crate) fn open_append(path: &Path) -> io::Result<File> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    OpenOptions::new()
        .create(true)
        .append(true)
        .read(false)
        .open(path)
}

/// `<directory>/<prefix>.<YYYYMMDD>.log`
pub(crate) fn current_path(directory: &Path, prefix: &str, date: CalendarDate) -> PathBuf {
    directory.join(format!("{prefix}.{}.log", date.as_basename()))
}

/// Filename used when rolling out an existing file — includes time-of-day
/// so size-triggered rotations within the same day remain sortable.
pub(crate) fn rotated_path(directory: &Path, prefix: &str) -> PathBuf {
    let now = OffsetDateTime::now_local().unwrap_or_else(|_| OffsetDateTime::now_utc());
    let fmt = format_description!("[year][month][day]T[hour][minute][second]");
    let stamp = now.format(&fmt).unwrap_or_else(|_| {
        format!(
            "{:04}{:02}{:02}T{:02}{:02}{:02}",
            now.year(),
            now.month() as u8,
            now.day(),
            now.hour(),
            now.minute(),
            now.second(),
        )
    });

    // Collision-safe: append a `.N` counter if the timestamp already exists
    // (possible on rapid successive rotations at sub-second resolution).
    let base = directory.join(format!("{prefix}.{stamp}.log"));
    if !base.exists() {
        return base;
    }
    for n in 1..1000 {
        let candidate = directory.join(format!("{prefix}.{stamp}.{n}.log"));
        if !candidate.exists() {
            return candidate;
        }
    }
    base
}

/// Keep the most recent `max_files` **rolled** files; delete older ones.
///
/// The "current" file (today's `<prefix>.<YYYYMMDD>.log` stem with no
/// hour/minute/second component) is never pruned.
pub(crate) fn prune_old(directory: &Path, prefix: &str, max_files: usize) {
    let Ok(read_dir) = std::fs::read_dir(directory) else {
        return;
    };
    let today_stem = format!("{prefix}.{}", CalendarDate::today_local().as_basename());

    let mut rolled: Vec<(std::time::SystemTime, PathBuf)> = Vec::new();
    for entry in read_dir.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        if !name.starts_with(&format!("{prefix}.")) || !name.ends_with(".log") {
            continue;
        }
        // Skip today's "plain" file (no `T` separator in the timestamp).
        let stem = name.trim_end_matches(".log");
        if stem == today_stem {
            continue;
        }
        let modified = entry
            .metadata()
            .and_then(|m| m.modified())
            .unwrap_or(std::time::UNIX_EPOCH);
        rolled.push((modified, path));
    }

    if rolled.len() <= max_files {
        return;
    }

    // Sort newest → oldest; drop the tail beyond `max_files`.
    rolled.sort_by_key(|entry| std::cmp::Reverse(entry.0));
    for (_, path) in rolled.into_iter().skip(max_files) {
        let _ = std::fs::remove_file(path);
    }
}
