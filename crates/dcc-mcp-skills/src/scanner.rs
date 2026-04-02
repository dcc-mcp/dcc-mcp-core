//! SkillScanner — scan directories for SKILL.md files.

#[cfg(feature = "python-bindings")]
use pyo3::prelude::*;

use dcc_mcp_utils::constants::{MTIME_EPSILON_SECS, SKILL_METADATA_FILE};
use dcc_mcp_utils::filesystem::{self, path_to_string};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// Scanner for discovering Skill packages in directories.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "python-bindings", pyclass(name = "SkillScanner"))]
pub struct SkillScanner {
    cache: HashMap<String, f64>,
    skill_dirs: Vec<String>,
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

    /// Scan all configured paths for Skill packages.
    pub fn scan(
        &mut self,
        extra_paths: Option<&[String]>,
        dcc_name: Option<&str>,
        force_refresh: bool,
    ) -> Vec<String> {
        let unique_paths = Self::collect_search_paths(extra_paths, dcc_name);

        // Scan each path
        let mut discovered = Vec::new();
        for search_path in &unique_paths {
            discovered.extend(self.scan_directory(search_path, force_refresh));
        }

        tracing::debug!(
            "Discovered {} skill(s) across {} search path(s)",
            discovered.len(),
            unique_paths.len()
        );
        self.skill_dirs = discovered;
        self.skill_dirs.to_vec()
    }

    /// Collect and deduplicate all skill search paths from various sources.
    ///
    /// Priority order: extra_paths > env var > platform-specific > global.
    fn collect_search_paths(extra_paths: Option<&[String]>, dcc_name: Option<&str>) -> Vec<String> {
        let mut search_paths = Vec::new();

        // 1. Extra paths (highest priority)
        if let Some(extra) = extra_paths {
            search_paths.extend(extra.iter().cloned());
        }

        // 2. Environment variable paths
        search_paths.extend(filesystem::get_skill_paths_from_env());

        // 3. Platform-specific skills directory
        if let Ok(platform_dir) = filesystem::get_skills_dir(dcc_name) {
            if Path::new(&platform_dir).is_dir() {
                search_paths.push(platform_dir);
            }
        }

        // Also check global skills dir if dcc_name was specified
        if dcc_name.is_some() {
            if let Ok(global_dir) = filesystem::get_skills_dir(None) {
                if Path::new(&global_dir).is_dir() {
                    search_paths.push(global_dir);
                }
            }
        }

        // Deduplicate while preserving order
        let mut seen = HashSet::new();
        search_paths
            .into_iter()
            .filter(|p| {
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
            if !force_refresh {
                if let (Some(&cached_mtime), Some(mtime)) =
                    (self.cache.get(&abs_path), current_mtime)
                {
                    if (mtime - cached_mtime).abs() < MTIME_EPSILON_SECS {
                        results.push(abs_path);
                        continue;
                    }
                }
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

#[cfg(feature = "python-bindings")]
#[pymethods]
impl SkillScanner {
    #[new]
    fn py_new() -> Self {
        Self::new()
    }

    #[pyo3(name = "scan")]
    #[pyo3(signature = (extra_paths=None, dcc_name=None, force_refresh=false))]
    fn py_scan(
        &mut self,
        extra_paths: Option<Vec<String>>,
        dcc_name: Option<&str>,
        force_refresh: bool,
    ) -> Vec<String> {
        self.scan(extra_paths.as_deref(), dcc_name, force_refresh)
    }

    #[pyo3(name = "clear_cache")]
    fn py_clear_cache(&mut self) {
        self.clear_cache()
    }

    #[getter]
    fn discovered_skills(&self) -> Vec<String> {
        self.skill_dirs.to_vec()
    }

    fn __repr__(&self) -> String {
        format!(
            "SkillScanner(cached={}, discovered={})",
            self.cache.len(),
            self.skill_dirs.len()
        )
    }
}

/// Convenience function: scan with a fresh scanner.
#[cfg(feature = "python-bindings")]
#[pyfunction]
#[pyo3(name = "scan_skill_paths")]
#[pyo3(signature = (extra_paths=None, dcc_name=None))]
pub fn py_scan_skill_paths(
    extra_paths: Option<Vec<String>>,
    dcc_name: Option<&str>,
) -> Vec<String> {
    let mut scanner = SkillScanner::new();
    scanner.scan(extra_paths.as_deref(), dcc_name, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scanner_empty() {
        let mut scanner = SkillScanner::new();
        let result = scanner.scan(Some(&["/nonexistent".to_string()]), None, false);
        assert!(result.is_empty());
    }
}
