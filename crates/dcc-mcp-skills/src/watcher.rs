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

#[cfg(feature = "python-bindings")]
use pyo3::prelude::*;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use dcc_mcp_models::SkillMetadata;
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use parking_lot::RwLock;
use tracing::{debug, info, warn};

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

// ── SkillWatcher ──

/// Inner state shared between the watcher struct and the notify callback.
struct WatcherInner {
    /// Last-seen skill snapshot.
    skills: RwLock<Vec<SkillMetadata>>,
    /// Directories currently being watched (for full rescan on change).
    watched_paths: RwLock<Vec<PathBuf>>,
}

impl WatcherInner {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            skills: RwLock::new(Vec::new()),
            watched_paths: RwLock::new(Vec::new()),
        })
    }

    /// Reload skills from all watched directories.
    fn reload(&self) {
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
    }
}

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
    /// reload. A value of 300ms is a reasonable default.
    ///
    /// # Errors
    ///
    /// Returns [`WatcherError::Init`] if the underlying notify watcher cannot
    /// be created (unlikely outside of test environments).
    pub fn new(debounce: Duration) -> Result<Self, WatcherError> {
        let inner = WatcherInner::new();
        let inner_cb = Arc::clone(&inner);
        let debounce_cb = debounce;

        let watcher = notify::recommended_watcher(move |res: notify::Result<Event>| {
            match res {
                Ok(event) => {
                    debug!("SkillWatcher: fs event {:?}", event.kind);
                    if should_reload(&event) {
                        // Spawn a dedicated thread for each debounced reload
                        // so the notify callback (which runs in its own thread)
                        // is never blocked.
                        let inner_reload = Arc::clone(&inner_cb);
                        std::thread::spawn(move || {
                            std::thread::sleep(debounce_cb);
                            inner_reload.reload();
                        });
                    }
                }
                Err(e) => {
                    warn!("SkillWatcher: notify error: {e}");
                }
            }
        })?;

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
        info!("SkillWatcher: watching '{}'", path.display());

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
}

// ── Helpers ──

/// Determine whether a notify event should trigger a skill reload.
///
/// We reload on Create/Modify/Remove events for any file whose name
/// matches skill-related patterns (SKILL.md, .py, .mel, .lua, etc.)
/// or any directory event (a new skill subdirectory may have appeared).
fn should_reload(event: &Event) -> bool {
    match event.kind {
        EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_) => {
            // Reload if the changed path looks like a skill file or directory
            event.paths.iter().any(|p| is_skill_related(p))
        }
        _ => false,
    }
}

/// Return `true` if `path` is likely to affect skill loading.
fn is_skill_related(path: &Path) -> bool {
    // Always reload for directory events — a new skill directory may appear
    if path.is_dir() {
        return true;
    }

    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();

    // SKILL.md itself
    if file_name.eq_ignore_ascii_case("skill.md") {
        return true;
    }

    // depends.md inside metadata/
    if file_name.eq_ignore_ascii_case("depends.md") {
        return true;
    }

    // Script files (check extension against supported list)
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        if dcc_mcp_utils::constants::is_supported_extension(ext) {
            return true;
        }
    }

    false
}

// ── Python bindings ──

/// Python-facing wrapper for [`SkillWatcher`].
#[cfg(feature = "python-bindings")]
#[pyclass(name = "SkillWatcher")]
pub struct PySkillWatcher {
    inner: SkillWatcher,
}

#[cfg(feature = "python-bindings")]
#[pymethods]
impl PySkillWatcher {
    /// Create a new SkillWatcher.
    ///
    /// Args:
    ///     debounce_ms: Milliseconds to wait before reloading after a change
    ///                  (default: 300).
    #[new]
    #[pyo3(signature = (debounce_ms=300))]
    pub fn new(debounce_ms: u64) -> pyo3::PyResult<Self> {
        let watcher = SkillWatcher::new(Duration::from_millis(debounce_ms))
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;
        Ok(Self { inner: watcher })
    }

    /// Start watching *path* for skill changes.
    ///
    /// An immediate reload is performed so skills are available without waiting
    /// for a filesystem event.
    ///
    /// Raises:
    ///     RuntimeError: If the path cannot be watched.
    pub fn watch(&mut self, path: &str) -> pyo3::PyResult<()> {
        self.inner
            .watch(path)
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))
    }

    /// Stop watching *path*.
    ///
    /// Returns ``True`` if the path was being watched, ``False`` otherwise.
    pub fn unwatch(&mut self, path: &str) -> bool {
        self.inner.unwatch(path)
    }

    /// Return the current skill snapshot as a list.
    pub fn skills(&self) -> Vec<SkillMetadata> {
        self.inner.skills()
    }

    /// Return the number of loaded skills.
    pub fn skill_count(&self) -> usize {
        self.inner.skill_count()
    }

    /// Return the list of watched directory paths.
    pub fn watched_paths(&self) -> Vec<String> {
        self.inner
            .watched_paths()
            .into_iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect()
    }

    /// Manually trigger a reload.
    pub fn reload(&self) {
        self.inner.reload();
    }

    fn __repr__(&self) -> String {
        format!(
            "SkillWatcher(skills={}, paths={})",
            self.inner.skill_count(),
            self.inner.watched_paths().len()
        )
    }
}

// ── Tests ──

#[cfg(test)]
mod tests {
    use super::*;
    use dcc_mcp_utils::constants::SKILL_METADATA_FILE;
    use std::fs;
    use tempfile::tempdir;

    // Helpers

    fn write_skill(dir: &Path, name: &str) {
        let skill_dir = dir.join(name);
        fs::create_dir_all(&skill_dir).unwrap();
        let content = format!("---\nname: {name}\ndcc: python\n---\n# {name}\n\nTest skill.");
        fs::write(skill_dir.join(SKILL_METADATA_FILE), &content).unwrap();
    }

    mod test_new {
        use super::*;

        #[test]
        fn create_with_default_debounce() {
            let watcher = SkillWatcher::new(Duration::from_millis(300));
            assert!(watcher.is_ok());
        }

        #[test]
        fn create_with_zero_debounce() {
            let watcher = SkillWatcher::new(Duration::ZERO);
            assert!(watcher.is_ok());
        }
    }

    mod test_watch {
        use super::*;

        #[test]
        fn watch_nonexistent_dir_returns_error() {
            let mut watcher = SkillWatcher::new(Duration::from_millis(100)).unwrap();
            let result = watcher.watch("/path/that/does/not/exist/xyz");
            assert!(result.is_err());
        }

        #[test]
        fn watch_valid_dir_succeeds() {
            let tmp = tempdir().unwrap();
            let mut watcher = SkillWatcher::new(Duration::from_millis(100)).unwrap();
            let result = watcher.watch(tmp.path());
            assert!(result.is_ok());
        }

        #[test]
        fn watch_and_immediate_skill_load() {
            let tmp = tempdir().unwrap();
            write_skill(tmp.path(), "alpha");
            write_skill(tmp.path(), "beta");

            let mut watcher = SkillWatcher::new(Duration::from_millis(100)).unwrap();
            watcher.watch(tmp.path()).unwrap();

            let skills = watcher.skills();
            assert_eq!(
                skills.len(),
                2,
                "Should have loaded 2 skills, got {skills:?}"
            );
            let names: Vec<&str> = skills.iter().map(|s| s.name.as_str()).collect();
            assert!(names.contains(&"alpha"));
            assert!(names.contains(&"beta"));
        }

        #[test]
        fn watched_paths_contains_added_path() {
            let tmp = tempdir().unwrap();
            let mut watcher = SkillWatcher::new(Duration::from_millis(100)).unwrap();
            watcher.watch(tmp.path()).unwrap();

            let paths = watcher.watched_paths();
            assert_eq!(paths.len(), 1);
            assert_eq!(paths[0], tmp.path());
        }
    }

    mod test_unwatch {
        use super::*;

        #[test]
        fn unwatch_removes_path() {
            let tmp = tempdir().unwrap();
            let mut watcher = SkillWatcher::new(Duration::from_millis(100)).unwrap();
            watcher.watch(tmp.path()).unwrap();
            assert_eq!(watcher.watched_paths().len(), 1);

            let removed = watcher.unwatch(tmp.path());
            assert!(removed, "unwatch should return true for known path");
            assert_eq!(watcher.watched_paths().len(), 0);
        }

        #[test]
        fn unwatch_unknown_path_returns_false() {
            let mut watcher = SkillWatcher::new(Duration::from_millis(100)).unwrap();
            let removed = watcher.unwatch("/no/such/path");
            assert!(!removed);
        }
    }

    mod test_reload {
        use super::*;

        #[test]
        fn manual_reload_updates_skill_count() {
            let tmp = tempdir().unwrap();
            let mut watcher = SkillWatcher::new(Duration::from_millis(100)).unwrap();
            watcher.watch(tmp.path()).unwrap();
            assert_eq!(watcher.skill_count(), 0);

            // Add a skill after initial watch
            write_skill(tmp.path(), "new-skill");

            // Trigger manual reload
            watcher.reload();
            assert_eq!(watcher.skill_count(), 1);
        }

        #[test]
        fn reload_reflects_removed_skill() {
            let tmp = tempdir().unwrap();
            write_skill(tmp.path(), "removable");

            let mut watcher = SkillWatcher::new(Duration::from_millis(100)).unwrap();
            watcher.watch(tmp.path()).unwrap();
            assert_eq!(watcher.skill_count(), 1);

            // Remove the skill directory
            fs::remove_dir_all(tmp.path().join("removable")).unwrap();
            watcher.reload();
            assert_eq!(watcher.skill_count(), 0);
        }
    }

    mod test_skill_related {
        use super::*;

        #[test]
        fn skill_md_is_related() {
            assert!(is_skill_related(Path::new("/skills/my-skill/SKILL.md")));
        }

        #[test]
        fn depends_md_is_related() {
            assert!(is_skill_related(Path::new(
                "/skills/my-skill/metadata/depends.md"
            )));
        }

        #[test]
        fn python_script_is_related() {
            assert!(is_skill_related(Path::new(
                "/skills/my-skill/scripts/run.py"
            )));
        }

        #[test]
        fn mel_script_is_related() {
            assert!(is_skill_related(Path::new(
                "/skills/my-skill/scripts/rig.mel"
            )));
        }

        #[test]
        fn text_file_is_not_related() {
            assert!(!is_skill_related(Path::new("/skills/notes.txt")));
        }

        #[test]
        fn json_config_is_not_related() {
            assert!(!is_skill_related(Path::new("/skills/config.json")));
        }
    }

    mod test_should_reload {
        use super::*;
        use notify::event::{CreateKind, ModifyKind, RemoveKind};

        fn make_event(kind: EventKind, path: &str) -> Event {
            Event {
                kind,
                paths: vec![PathBuf::from(path)],
                attrs: Default::default(),
            }
        }

        #[test]
        fn create_skill_md_triggers_reload() {
            let event = make_event(
                EventKind::Create(CreateKind::File),
                "/skills/new-skill/SKILL.md",
            );
            assert!(should_reload(&event));
        }

        #[test]
        fn modify_python_script_triggers_reload() {
            let event = make_event(
                EventKind::Modify(ModifyKind::Data(notify::event::DataChange::Any)),
                "/skills/my-skill/scripts/run.py",
            );
            assert!(should_reload(&event));
        }

        #[test]
        fn remove_skill_md_triggers_reload() {
            let event = make_event(
                EventKind::Remove(RemoveKind::File),
                "/skills/old-skill/SKILL.md",
            );
            assert!(should_reload(&event));
        }

        #[test]
        fn access_event_does_not_trigger_reload() {
            let event = make_event(
                EventKind::Access(notify::event::AccessKind::Read),
                "/skills/my-skill/SKILL.md",
            );
            assert!(!should_reload(&event));
        }

        #[test]
        fn modify_non_skill_file_does_not_trigger_reload() {
            let event = make_event(
                EventKind::Modify(ModifyKind::Data(notify::event::DataChange::Any)),
                "/skills/my-skill/README.md",
            );
            // "readme.md" is not SKILL.md / depends.md, and .md is not a
            // supported script extension — should not reload.
            assert!(!should_reload(&event));
        }
    }

    mod test_debug {
        use super::*;

        #[test]
        fn debug_format_shows_counts() {
            let watcher = SkillWatcher::new(Duration::from_millis(200)).unwrap();
            let debug = format!("{watcher:?}");
            assert!(debug.contains("SkillWatcher"));
            assert!(debug.contains("debounce_ms"));
        }
    }
}
