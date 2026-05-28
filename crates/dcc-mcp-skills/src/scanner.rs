//! SkillScanner — scan directories for SKILL.md files.
//!
//! PyO3 bindings live in `crate::python::scanner`.

#[cfg(feature = "stub-gen")]
use pyo3_stub_gen_derive::gen_stub_pyclass;

use crate::catalog::scoring::SkillPathSource;
use crate::constants::{MTIME_EPSILON_SECS, SKILL_METADATA_FILE};
use dcc_mcp_paths::path_to_string;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// Scanner for discovering Skill packages in directories.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "stub-gen", gen_stub_pyclass)]
#[cfg_attr(
    feature = "python-bindings",
    pyo3::pyclass(name = "SkillScanner", from_py_object)
)]
pub struct SkillScanner {
    pub(crate) cache: HashMap<String, f64>,
    pub(crate) skill_dirs: Vec<String>,
}

impl Default for SkillScanner {
    fn default() -> Self {
        Self::new()
    }
}

impl SkillScanner {
    #[must_use]
    pub fn new() -> Self {
        Self {
            cache: HashMap::new(),
            skill_dirs: Vec::new(),
        }
    }

    /// Scan all configured paths for Skill packages, taking a typed
    /// [`dcc_mcp_models::DccName`] (#491).
    ///
    /// Thin wrapper around [`Self::scan`] that converts the typed value
    /// to its canonical lowercase string form before delegating. New
    /// callers should prefer this entry point so the DCC identifier is
    /// validated and normalised at the boundary.
    pub fn scan_for_dcc(
        &mut self,
        extra_paths: Option<&[String]>,
        dcc: Option<&dcc_mcp_models::DccName>,
        force_refresh: bool,
    ) -> Vec<String> {
        let dcc_str = dcc.map(|d| d.as_str());
        self.scan(extra_paths, dcc_str, force_refresh)
    }

    /// Scan all configured paths for Skill packages.
    pub fn scan(
        &mut self,
        extra_paths: Option<&[String]>,
        dcc_name: Option<&str>,
        force_refresh: bool,
    ) -> Vec<String> {
        self.scan_with_sources(extra_paths, dcc_name, force_refresh)
            .into_iter()
            .map(|(dir, _)| dir)
            .collect()
    }

    /// Variant of [`Self::scan`] that returns each discovered skill
    /// directory paired with the [`SkillPathSource`] of the search root it
    /// was found under. Used by [`crate::catalog::SkillCatalog`] to tag
    /// entries for the path-source rank penalty (issue #1403).
    ///
    /// The mapping is derived from the priority order documented on
    /// [`Self::collect_search_paths_with_sources`]: when a skill directory
    /// appears under multiple roots (e.g. an env-var path that is also the
    /// local-dev directory), the source of the highest-priority root that
    /// canonicalises to the same on-disk location wins.
    pub fn scan_with_sources(
        &mut self,
        extra_paths: Option<&[String]>,
        dcc_name: Option<&str>,
        force_refresh: bool,
    ) -> Vec<(String, SkillPathSource)> {
        let unique_paths = Self::collect_search_paths_with_sources(extra_paths, dcc_name);

        // Scan each path while preserving source attribution. The first
        // search root that produces a given skill_dir wins — this matches
        // the priority order from `collect_search_paths_with_sources`.
        let mut discovered: Vec<(String, SkillPathSource)> = Vec::new();
        let mut seen = HashSet::new();
        for (search_path, source) in &unique_paths {
            for dir in self.scan_directory(search_path, force_refresh) {
                if seen.insert(dir.clone()) {
                    discovered.push((dir, *source));
                }
            }
        }

        tracing::debug!(
            "Discovered {} skill(s) across {} search path(s)",
            discovered.len(),
            unique_paths.len()
        );
        self.skill_dirs = discovered.iter().map(|(d, _)| d.clone()).collect();
        discovered
    }

    /// Find child directories under explicit search paths that cannot be
    /// loaded because they do not contain `SKILL.md`.
    pub(crate) fn scan_explicit_directories_missing_skill_md(
        extra_paths: Option<&[String]>,
    ) -> Vec<String> {
        let Some(extra_paths) = extra_paths else {
            return Vec::new();
        };
        let mut seen = HashSet::new();
        let unique_paths: Vec<String> = extra_paths
            .iter()
            .filter(|p| {
                let abs = std::fs::canonicalize(p).unwrap_or_else(|e| {
                    tracing::debug!("canonicalize({p:?}) failed ({e}), using raw path for dedup");
                    PathBuf::from(p)
                });
                seen.insert(path_to_string(&abs))
            })
            .cloned()
            .collect();
        let mut missing = Vec::new();

        for search_path in &unique_paths {
            let path = Path::new(search_path);
            if !path.is_dir() || path.join(SKILL_METADATA_FILE).is_file() {
                continue;
            }

            let entries = match std::fs::read_dir(path) {
                Ok(e) => e,
                Err(e) => {
                    tracing::warn!("Error scanning directory {}: {}", search_path, e);
                    continue;
                }
            };

            for entry in entries.filter_map(|e| match e {
                Ok(entry) => Some(entry),
                Err(err) => {
                    tracing::warn!("Skipping unreadable entry in {search_path}: {err}");
                    None
                }
            }) {
                let ft = match entry.file_type() {
                    Ok(ft) => ft,
                    Err(_) => continue,
                };
                if !ft.is_dir() {
                    continue;
                }

                let entry_path = entry.path();
                if !entry_path.join(SKILL_METADATA_FILE).is_file() {
                    missing.push(path_to_string(&entry_path));
                }
            }
        }

        missing
    }

    /// Collect and deduplicate all skill search paths from various sources.
    ///
    /// Thin wrapper around
    /// [`Self::collect_search_paths_with_sources`] that drops the source
    /// labels. New code should prefer the `_with_sources` variant so the
    /// origin can be propagated into the catalog for ranking.
    pub fn collect_search_paths(
        extra_paths: Option<&[String]>,
        dcc_name: Option<&str>,
    ) -> Vec<String> {
        Self::collect_search_paths_with_sources(extra_paths, dcc_name)
            .into_iter()
            .map(|(p, _)| p)
            .collect()
    }

    /// Collect and deduplicate all skill search paths, paired with the
    /// [`SkillPathSource`] each path was sourced from.
    ///
    /// Priority order (highest → lowest):
    /// 1. `extra_paths` — caller-provided explicit paths ([`SkillPathSource::ExplicitArg`])
    /// 2. `DCC_MCP_{APP}_SKILL_PATHS` / `DCC_MCP_SKILL_PATHS` ([`SkillPathSource::EnvVar`])
    /// 3. Local developer skills directory under `~/.dcc-mcp/{dcc}/skills` ([`SkillPathSource::LocalDev`])
    /// 4. Platform-specific skills directory for this DCC ([`SkillPathSource::Platform`])
    /// 5. Global skills directory ([`SkillPathSource::Platform`])
    ///
    /// Deduplication preserves the highest-priority source for any path
    /// that appears in more than one bucket (e.g. an env-var path that
    /// happens to be `~/.dcc-mcp/<dcc>/skills` keeps the `EnvVar` tag).
    #[must_use]
    pub fn collect_search_paths_with_sources(
        extra_paths: Option<&[String]>,
        dcc_name: Option<&str>,
    ) -> Vec<(String, SkillPathSource)> {
        let mut search_paths: Vec<(String, SkillPathSource)> = Vec::new();

        // 1. Extra paths (highest priority) — explicit caller input.
        if let Some(extra) = extra_paths {
            for p in extra {
                search_paths.push((p.clone(), SkillPathSource::ExplicitArg));
            }
        }

        // 2. Env-var paths.
        if let Some(dcc) = dcc_name {
            for p in crate::paths::get_app_skill_paths_from_env(dcc) {
                search_paths.push((p, SkillPathSource::EnvVar));
            }
        } else {
            for p in crate::paths::get_skill_paths_from_env() {
                search_paths.push((p, SkillPathSource::EnvVar));
            }
        }

        // 3. Local developer skills directory.
        if let Ok(local_dir) = crate::paths::get_local_skills_dir(dcc_name)
            && Path::new(&local_dir).is_dir()
        {
            search_paths.push((local_dir, SkillPathSource::LocalDev));
        }

        // 4. Platform-specific skills directory.
        if let Ok(platform_dir) = crate::paths::get_skills_dir(dcc_name)
            && Path::new(&platform_dir).is_dir()
        {
            search_paths.push((platform_dir, SkillPathSource::Platform));
        }

        // 5. Global (dcc-less) skills dir is also platform-installed.
        if dcc_name.is_some()
            && let Ok(global_dir) = crate::paths::get_skills_dir(None)
            && Path::new(&global_dir).is_dir()
        {
            search_paths.push((global_dir, SkillPathSource::Platform));
        }

        // Deduplicate while preserving order. The first occurrence wins,
        // which encodes "highest priority source for the same on-disk
        // path".
        let mut seen = HashSet::new();
        search_paths
            .into_iter()
            .filter(|(p, _)| {
                let abs = std::fs::canonicalize(p).unwrap_or_else(|e| {
                    tracing::debug!("canonicalize({p:?}) failed ({e}), using raw path for dedup");
                    PathBuf::from(p)
                });
                seen.insert(path_to_string(&abs))
            })
            .collect()
    }

    fn scan_directory(&mut self, search_path: &str, force_refresh: bool) -> Vec<String> {
        let mut results = Vec::new();
        let path = Path::new(search_path);
        if !path.is_dir() {
            return results;
        }

        // OpenClaw / single-skill layout: the search_path itself is a skill directory
        // (contains SKILL.md directly, with or without a scripts/ subdirectory).
        let self_skill_md = path.join(SKILL_METADATA_FILE);
        if self_skill_md.is_file() {
            let abs_path = path_to_string(path);
            let current_mtime = Self::file_mtime_secs(&self_skill_md);
            if !force_refresh
                && let (Some(&cached_mtime), Some(mtime)) =
                    (self.cache.get(&abs_path), current_mtime)
                && (mtime - cached_mtime).abs() < MTIME_EPSILON_SECS
            {
                results.push(abs_path);
                return results;
            }
            if let Some(mtime) = current_mtime {
                self.cache.insert(abs_path.clone(), mtime);
            }
            results.push(abs_path);
            return results;
        }

        let entries = match std::fs::read_dir(path) {
            Ok(e) => e,
            Err(e) => {
                tracing::warn!("Error scanning directory {}: {}", search_path, e);
                return results;
            }
        };

        for entry in entries.filter_map(|e| match e {
            Ok(entry) => Some(entry),
            Err(err) => {
                tracing::warn!("Skipping unreadable entry in {search_path}: {err}");
                None
            }
        }) {
            // Use entry.file_type() (single stat) instead of entry_path.is_dir() + is_file().
            let ft = match entry.file_type() {
                Ok(ft) => ft,
                Err(_) => continue,
            };
            if !ft.is_dir() {
                continue;
            }

            let entry_path = entry.path();

            let skill_md_path = entry_path.join(SKILL_METADATA_FILE);
            if !skill_md_path.is_file() {
                continue;
            }

            let abs_path = path_to_string(&entry_path);
            let current_mtime = Self::file_mtime_secs(&skill_md_path);

            // Check cache — skip re-processing if mtime unchanged
            if !force_refresh
                && let (Some(&cached_mtime), Some(mtime)) =
                    (self.cache.get(&abs_path), current_mtime)
                && (mtime - cached_mtime).abs() < MTIME_EPSILON_SECS
            {
                results.push(abs_path);
                continue;
            }

            // Update cache with current mtime (before moving abs_path)
            if let Some(mtime) = current_mtime {
                self.cache.insert(abs_path.clone(), mtime);
            }

            results.push(abs_path);
        }

        results
    }

    /// Get file modification time as seconds since UNIX epoch.
    fn file_mtime_secs(path: &Path) -> Option<f64> {
        std::fs::metadata(path).ok()?.modified().ok().map(|mtime| {
            mtime
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs_f64()
        })
    }

    /// Clear the file modification time cache and discovered skill directories.
    pub fn clear_cache(&mut self) {
        self.cache.clear();
        self.skill_dirs.clear();
    }
}

// PyO3 bindings live in `crate::python::scanner`.
#[cfg(feature = "python-bindings")]
pub use crate::python::scanner::py_scan_skill_paths;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scanner_empty() {
        let mut scanner = SkillScanner::new();
        let result = scanner.scan(Some(&["/nonexistent".to_string()]), None, false);
        assert!(result.is_empty());
    }

    #[test]
    fn test_scan_for_dcc_uses_non_maya_typed_env_paths() {
        use dcc_mcp_models::DccName;
        use std::fs;

        fn write_skill_dir(root: &Path, name: &str) -> String {
            let dir = root.join(name);
            fs::create_dir_all(&dir).unwrap();
            fs::write(
                dir.join(crate::constants::SKILL_METADATA_FILE),
                format!("name: {name}\nversion: 1.0.0\n"),
            )
            .unwrap();
            path_to_string(&dir)
        }

        let tmp = tempfile::tempdir().unwrap();
        let photoshop_skill = write_skill_dir(tmp.path(), "photoshop-retouch");
        let krita_skill = write_skill_dir(tmp.path(), "krita-paintover");

        let saved_photoshop = std::env::var("DCC_MCP_PHOTOSHOP_SKILL_PATHS").ok();
        let saved_krita = std::env::var("DCC_MCP_KRITA_SKILL_PATHS").ok();
        unsafe {
            std::env::set_var("DCC_MCP_PHOTOSHOP_SKILL_PATHS", &photoshop_skill);
            std::env::set_var("DCC_MCP_KRITA_SKILL_PATHS", &krita_skill);
        }

        let mut scanner = SkillScanner::new();
        let photoshop = scanner.scan_for_dcc(None, Some(&DccName::Photoshop), true);
        scanner.clear_cache();
        let custom = scanner.scan_for_dcc(None, Some(&DccName::Other("krita".into())), true);

        unsafe {
            match saved_photoshop {
                Some(value) => std::env::set_var("DCC_MCP_PHOTOSHOP_SKILL_PATHS", value),
                None => std::env::remove_var("DCC_MCP_PHOTOSHOP_SKILL_PATHS"),
            }
            match saved_krita {
                Some(value) => std::env::set_var("DCC_MCP_KRITA_SKILL_PATHS", value),
                None => std::env::remove_var("DCC_MCP_KRITA_SKILL_PATHS"),
            }
        }

        assert!(
            photoshop.contains(&photoshop_skill),
            "photoshop scan returned {photoshop:?}"
        );
        assert!(
            custom.contains(&krita_skill),
            "custom DCC scan returned {custom:?}"
        );
    }

    #[test]
    fn test_scan_with_sources_tags_explicit_arg() {
        // Skills discovered from caller-supplied `extra_paths` should be
        // labelled `ExplicitArg` so the catalog can rank them above
        // bundled material. Issue #1403.
        use std::fs;

        fn write_skill_dir(root: &Path, name: &str) -> String {
            let dir = root.join(name);
            fs::create_dir_all(&dir).unwrap();
            fs::write(
                dir.join(crate::constants::SKILL_METADATA_FILE),
                format!("name: {name}\nversion: 1.0.0\n"),
            )
            .unwrap();
            path_to_string(&dir)
        }

        let tmp = tempfile::tempdir().unwrap();
        let explicit_root = path_to_string(tmp.path());
        let skill_dir = write_skill_dir(tmp.path(), "explicit-render");

        let mut scanner = SkillScanner::new();
        let with_sources = scanner.scan_with_sources(Some(&[explicit_root]), Some("maya"), true);

        let explicit_entry = with_sources
            .iter()
            .find(|(d, _)| d == &skill_dir)
            .expect("explicit-root skill must be discovered");
        assert_eq!(
            explicit_entry.1,
            SkillPathSource::ExplicitArg,
            "skill under extra_paths must be tagged ExplicitArg; got {with_sources:?}"
        );
    }

    #[test]
    fn test_collect_search_paths_priority_order() {
        // When the same on-disk path appears as both an extra_paths entry
        // and an env-var entry, the higher-priority `ExplicitArg` tag
        // wins (the dedupe keeps the first occurrence).
        use std::fs;

        let tmp = tempfile::tempdir().unwrap();
        let shared = tmp.path().join("shared");
        fs::create_dir_all(&shared).unwrap();
        let shared_str = path_to_string(&shared);

        let saved = std::env::var("DCC_MCP_MAYA_SKILL_PATHS").ok();
        unsafe {
            std::env::set_var("DCC_MCP_MAYA_SKILL_PATHS", &shared_str);
        }
        let collected = SkillScanner::collect_search_paths_with_sources(
            Some(std::slice::from_ref(&shared_str)),
            Some("maya"),
        );
        unsafe {
            match saved {
                Some(v) => std::env::set_var("DCC_MCP_MAYA_SKILL_PATHS", v),
                None => std::env::remove_var("DCC_MCP_MAYA_SKILL_PATHS"),
            }
        }

        let shared_entries: Vec<_> = collected
            .iter()
            .filter(|(p, _)| {
                let abs = std::fs::canonicalize(p).map(|x| path_to_string(&x)).ok();
                abs.as_deref() == Some(shared_str.as_str())
                    || abs.as_ref()
                        == std::fs::canonicalize(&shared_str)
                            .map(|x| path_to_string(&x))
                            .ok()
                            .as_ref()
            })
            .collect();
        assert_eq!(
            shared_entries.len(),
            1,
            "dedupe must keep exactly one entry per on-disk path; got {collected:?}"
        );
        assert_eq!(
            shared_entries[0].1,
            SkillPathSource::ExplicitArg,
            "extra_paths must win over env-var for the same on-disk path"
        );
    }
}
