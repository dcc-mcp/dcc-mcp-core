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
//! This module is a **thin facade** over three focused siblings so the
//! public `SkillWatcher` surface stays compact:
//!
//! - [`watcher_inner`](super::watcher_inner) — `WatcherInner` shared state
//!   (snapshot, watched paths, debounce atomic, on-reload callbacks) and the
//!   public `WatcherError` type.
//! - [`watcher_filter`](super::watcher_filter) — `should_reload` /
//!   `is_skill_related` heuristics that decide which filesystem events matter.
//! - [`watcher_python`](super::watcher_python) — `PySkillWatcher` PyO3 wrapper
//!   (compiled only with the `python-bindings` feature).
//! - [`watcher_tests`](super::watcher_tests) — unit tests for the three
//!   modules above (gated on `#[cfg(test)]`).

#[path = "watcher_inner.rs"]
mod inner;

#[path = "watcher_filter.rs"]
mod filter;

#[cfg(feature = "python-bindings")]
#[path = "watcher_python.rs"]
mod python_impl;

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
    /// # Design note — single debounce thread
    ///
    /// The previous implementation spawned a new OS thread per filesystem event
    /// (`thread::sleep(debounce); reload()`).  A rapid burst of changes (e.g.
    /// `git checkout` touching 100 files) would spawn 100 threads, all sleeping
    /// concurrently, saturating the thread pool.
    ///
    /// The new design uses a single, long-lived background thread that polls an
    /// `AtomicU64` timestamp every 50 ms.  The notify callback only writes the
    /// current epoch-ms into the atomic — zero allocation, no spawn.  The poll
    /// thread fires a reload exactly once per quiet period (no new events for
    /// `debounce` ms), regardless of how many raw events arrived.
    ///
    /// # Errors
    ///
    /// Returns [`WatcherError::Init`] if the underlying notify watcher cannot
    /// be created (unlikely outside of test environments).
    pub fn new(debounce: Duration) -> Result<Self, WatcherError> {
        let inner = WatcherInner::new();

        // ── Notify callback: only stamps the atomic, never sleeps ──────────
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

        // ── Single background poll thread ───────────────────────────────────
        // Polls every 50 ms.  When the last event is older than `debounce` and
        // hasn't been fired yet, it triggers a reload and records that it did.
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
                        continue; // nothing new
                    }

                    let now_ms = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as u64;

                    if now_ms.saturating_sub(last_event) >= debounce_ms {
                        // Quiet window elapsed — fire exactly one reload.
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
    ///
    /// The directory is watched **recursively** so changes deep in a skill's
    /// `scripts/` or `metadata/` subdirectories are captured.
    ///
    /// # Errors
    ///
    /// Returns [`WatcherError::Watch`] if the directory cannot be watched
    /// (e.g. it does not exist or insufficient permissions).
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

        // Immediate scan so the caller sees skills without waiting for a change.
        self.inner.reload();

        Ok(())
    }

    /// Stop watching a previously added directory.
    ///
    /// Returns `true` if the directory was being watched and has now been
    /// removed; `false` if it was not in the watch list.
    pub fn unwatch<P: AsRef<Path>>(&mut self, path: P) -> bool {
        let path = path.as_ref();
        let _ = self._watcher.unwatch(path);

        let mut paths = self.inner.watched_paths.write();
        let before = paths.len();
        paths.retain(|p| p != path);
        let removed = paths.len() < before;

        if removed {
            drop(paths); // release write lock before reload
            self.inner.reload();
        }

        removed
    }

    /// Return a snapshot of all currently loaded skills.
    ///
    /// This is a cloned, immutable snapshot — it does not block the background
    /// reload thread.
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
    ///
    /// Useful in tests or when you know a change has occurred outside the
    /// normal watcher loop.
    pub fn reload(&self) {
        self.inner.reload();
    }

    /// Register a callback that is invoked **after every reload**.
    ///
    /// Use this to connect external caches to the watcher so they are
    /// automatically invalidated whenever skills change on disk.
    ///
    /// The callback runs synchronously on the debounce thread, so it must
    /// complete quickly (e.g. clearing a flag or calling `.clear_cache()`).
    ///
    /// # Example
    ///
    /// ```no_run
    /// use dcc_mcp_skills::watcher::SkillWatcher;
    /// use dcc_mcp_skills::manager::SkillsManager;
    /// use std::sync::Arc;
    /// use std::time::Duration;
    ///
    /// let manager = Arc::new(SkillsManager::new(/* ... */ todo!()));
    /// let mut watcher = SkillWatcher::new(Duration::from_millis(300)).unwrap();
    ///
    /// // Invalidate the manager's cache every time skills are reloaded.
    /// let mgr = manager.clone();
    /// watcher.on_reload(move || mgr.clear_cache());
    /// ```
    pub fn on_reload(&self, callback: impl Fn() + Send + Sync + 'static) {
        self.inner.add_on_reload_callback(Box::new(callback));
    }
}

#[cfg(feature = "python-bindings")]
pub use python_impl::PySkillWatcher;

#[cfg(test)]
#[path = "watcher_tests.rs"]
mod tests;
