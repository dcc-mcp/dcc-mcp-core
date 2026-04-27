//! Skill hot-reload watcher.
//!
//! [`SkillWatcher`] monitors one or more directories for filesystem changes and
//! automatically re-loads skill metadata when SKILL.md files (or their
//! companion scripts) are created, modified, or deleted.
//!
//! # Design
//!
//! - Uses [`notify::RecommendedWatcher`] (platform-native: inotify on Linux,
//!   FSEvents on macOS, ReadDirectoryChangesW on Windows).
//! - Debounces rapid successive events with a configurable delay so that a
//!   single "save" that triggers multiple low-level events produces only one
//!   reload.
//! - Re-load runs on a dedicated background thread via [`std::thread::spawn`]
//!   so it never blocks the calling application.
//! - Thread-safe snapshot of the loaded skills is kept in an `Arc<RwLock<>>`.
//!
//! # Usage
//!
//! ```no_run
//! use dcc_mcp_skills::watcher::SkillWatcher;
//! use std::time::Duration;
//!
//! let mut watcher = SkillWatcher::new(Duration::from_millis(300)).unwrap();
//! watcher.watch("/path/to/skills").unwrap();
//!
//! // Wait a moment for the initial load…
//! std::thread::sleep(Duration::from_millis(100));
//!
//! // Inspect the current snapshot
//! for skill in watcher.skills() {
//!     println!("Loaded: {} ({})", skill.name, skill.dcc);
//! }
//! ```
//!
//! # Maintainer layout
//!
//! This module is a directory module split into focused sub-files:
//!
//! - [`inner`] — `WatcherInner` shared state (snapshot, watched paths,
//!   debounce atomic, on-reload callbacks) and the public `WatcherError` type.
//! - [`filter`] — `should_reload` / `is_skill_related` heuristics that decide
//!   which filesystem events matter.
//! - [`crate::python::watcher`] — `PySkillWatcher` PyO3 wrapper
//!   (compiled only with the `python-bindings` feature).
//! - [`tests`] — unit tests (gated on `#[cfg(test)]`).

mod filter;
mod inner;

#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use dcc_mcp_models::SkillMetadata;
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use tracing::{debug, warn};

pub use inner::WatcherError;

use filter::should_reload;
use inner::WatcherInner;

// ── SkillWatcher ──

/// Hot-reload watcher for skill directories.
///
/// Monitors filesystem events and re-scans skill directories whenever a
/// SKILL.md file or its adjacent assets change.
pub struct SkillWatcher {
    inner: Arc<WatcherInner>,
    _watcher: RecommendedWatcher,
    debounce: Duration,
}

impl std::fmt::Debug for SkillWatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let skills_count = self.inner.skills.read().len();
        let paths_count = self.inner.watched_paths.read().len();
        f.debug_struct("SkillWatcher")
            .field("skills_count", &skills_count)
            .field("watched_paths_count", &paths_count)
            .field("debounce_ms", &self.debounce.as_millis())
            .finish()
    }
}

impl SkillWatcher {
    /// Create a new watcher with the given debounce delay.
    ///
    /// Events within `debounce` of each other are coalesced into a single
    /// reload. A value of 300 ms is a reasonable default.
    ///
    /// # Errors
    ///
    /// Returns [`WatcherError::Init`] if the underlying notify watcher cannot
    /// be created.
    pub fn new(debounce: Duration) -> Result<Self, WatcherError> {
        let inner = WatcherInner::new();

        let inner_cb = Arc::clone(&inner);
        let watcher = notify::recommended_watcher(move |res: notify::Result<Event>| match res {
            Ok(event) => {
                debug!("SkillWatcher: fs event {:?}", event.kind);
                if should_reload(&event) {
                    inner_cb.mark_event();
                }
            }
            Err(e) => {
                warn!("SkillWatcher: notify error: {e}");
            }
        })?;

        let inner_poll = Arc::clone(&inner);
        let debounce_ms = debounce.as_millis() as u64;
        std::thread::Builder::new()
            .name("skill-watcher-debounce".into())
            .spawn(move || {
                let poll_interval = Duration::from_millis(50);
                let mut last_fired_ms: u64 = 0;

                loop {
                    std::thread::sleep(poll_interval);

                    let last_event = inner_poll.last_event_ms.load(Ordering::Acquire);
                    if last_event == 0 || last_event == last_fired_ms {
                        continue;
                    }

                    let now_ms = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as u64;

                    if now_ms.saturating_sub(last_event) >= debounce_ms {
                        inner_poll.reload();
                        last_fired_ms = last_event;
                    }
                }
            })
            .expect("failed to spawn skill-watcher-debounce thread");

        Ok(Self {
            inner,
            _watcher: watcher,
            debounce,
        })
    }

    /// Add a directory to the watch list and trigger an immediate reload.
    pub fn watch<P: AsRef<Path>>(&mut self, path: P) -> Result<(), WatcherError> {
        let path = path.as_ref().to_path_buf();

        self._watcher
            .watch(&path, RecursiveMode::Recursive)
            .map_err(|source| WatcherError::Watch {
                path: path.clone(),
                source,
            })?;

        self.inner.watched_paths.write().push(path.clone());
        tracing::info!("SkillWatcher: watching '{}'", path.display());

        self.inner.reload();

        Ok(())
    }

    /// Stop watching a previously added directory.
    pub fn unwatch<P: AsRef<Path>>(&mut self, path: P) -> bool {
        let path = path.as_ref();
        let _ = self._watcher.unwatch(path);

        let mut paths = self.inner.watched_paths.write();
        let before = paths.len();
        paths.retain(|p| p != path);
        let removed = paths.len() < before;

        if removed {
            drop(paths);
            self.inner.reload();
        }

        removed
    }

    /// Return a snapshot of all currently loaded skills.
    #[must_use]
    pub fn skills(&self) -> Vec<SkillMetadata> {
        self.inner.skills.read().clone()
    }

    /// Return the number of skills currently loaded.
    #[must_use]
    pub fn skill_count(&self) -> usize {
        self.inner.skills.read().len()
    }

    /// Return the list of directories currently being watched.
    #[must_use]
    pub fn watched_paths(&self) -> Vec<PathBuf> {
        self.inner.watched_paths.read().clone()
    }

    /// Manually trigger a reload without waiting for a filesystem event.
    pub fn reload(&self) {
        self.inner.reload();
    }

    /// Register a callback that is invoked **after every reload**.
    pub fn on_reload(&self, callback: impl Fn() + Send + Sync + 'static) {
        self.inner.add_on_reload_callback(Box::new(callback));
    }
}

#[cfg(feature = "python-bindings")]
pub use crate::python::watcher::PySkillWatcher;
