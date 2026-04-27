//! Shared inner state for [`SkillWatcher`](super::SkillWatcher).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use dcc_mcp_models::SkillMetadata;
use parking_lot::RwLock;
use tracing::info;

use crate::loader::parse_skill_md;
use crate::scanner::SkillScanner;

// ── Error type ──

/// Errors that can occur during skill watching.
#[derive(Debug, thiserror::Error)]
pub enum WatcherError {
    /// A path could not be watched.
    #[error("Failed to watch path '{path}': {source}")]
    Watch {
        path: PathBuf,
        #[source]
        source: notify::Error,
    },

    /// The underlying notify watcher failed to initialise.
    #[error("Failed to create filesystem watcher: {0}")]
    Init(#[from] notify::Error),
}

// ── WatcherInner ──

/// Inner state shared between the watcher struct and the notify callback.
pub(crate) struct WatcherInner {
    /// Last-seen skill snapshot.
    pub(crate) skills: RwLock<Vec<SkillMetadata>>,
    /// Directories currently being watched (for full rescan on change).
    pub(crate) watched_paths: RwLock<Vec<PathBuf>>,
    /// Epoch-millisecond timestamp of the most recent skill-related FS event.
    ///
    /// Written atomically by the notify callback; read by the single background
    /// debounce thread.  Using u64 milliseconds fits in an atomic without any
    /// locking, and the debounce window is large enough that minor clock jitter
    /// is irrelevant.
    pub(crate) last_event_ms: AtomicU64,
    /// Callbacks to invoke after every successful reload.
    ///
    /// Registered via [`SkillWatcher::on_reload`](super::SkillWatcher::on_reload).
    /// Each callback is called **synchronously** from the reload thread after
    /// the skill snapshot has been updated, so it must be fast (typically just
    /// clearing a cache flag).
    pub(crate) on_reload_callbacks: RwLock<Vec<Box<dyn Fn() + Send + Sync>>>,
}

impl WatcherInner {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self {
            skills: RwLock::new(Vec::new()),
            watched_paths: RwLock::new(Vec::new()),
            last_event_ms: AtomicU64::new(0),
            on_reload_callbacks: RwLock::new(Vec::new()),
        })
    }

    /// Record a skill-related filesystem event for the debounce thread.
    pub(crate) fn mark_event(&self) {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        self.last_event_ms.store(now_ms, Ordering::Release);
    }

    /// Reload skills from all watched directories and notify listeners.
    pub(crate) fn reload(&self) {
        let paths: Vec<_> = self.watched_paths.read().clone();
        let extra_paths: Vec<String> = paths
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect();

        let mut scanner = SkillScanner::new();
        let dirs = scanner.scan(Some(&extra_paths), None, false);

        let mut new_skills = Vec::new();
        for dir_str in &dirs {
            let dir = Path::new(dir_str);
            if let Some(meta) = parse_skill_md(dir) {
                new_skills.push(meta);
            }
        }

        let count = new_skills.len();
        *self.skills.write() = new_skills;
        info!("SkillWatcher: reloaded {count} skill(s)");

        // Notify all registered listeners (e.g. SkillsManager cache invalidation).
        for cb in self.on_reload_callbacks.read().iter() {
            cb();
        }
    }

    /// Register a callback invoked after every reload.
    pub(crate) fn add_on_reload_callback(&self, cb: Box<dyn Fn() + Send + Sync>) {
        self.on_reload_callbacks.write().push(cb);
    }
}
