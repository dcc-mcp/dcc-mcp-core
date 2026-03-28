//! SkillScanner — scan directories for SKILL.md files.

#[cfg(feature = "python-bindings")]
use pyo3::prelude::*;

use dcc_mcp_utils::constants::SKILL_METADATA_FILE;
use dcc_mcp_utils::filesystem;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// Scanner for discovering Skill packages in directories.
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
        let unique_paths: Vec<String> = search_paths
            .into_iter()
            .filter(|p| {
                let abs = std::fs::canonicalize(p)
                    .unwrap_or_else(|_| PathBuf::from(p))
                    .to_string_lossy()
                    .to_string();
                seen.insert(abs)
            })
            .collect();

        // Scan each path
        let mut discovered = Vec::new();
        for search_path in &unique_paths {
            discovered.extend(self.scan_directory(search_path, force_refresh));
        }

        self.skill_dirs = discovered.clone();
        tracing::debug!(
            "Discovered {} skill(s) across {} search path(s)",
            discovered.len(),
            unique_paths.len()
        );
        discovered
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

        for entry in entries.flatten() {
            let entry_path = entry.path();
            if !entry_path.is_dir() {
                continue;
            }

            let skill_md_path = entry_path.join(SKILL_METADATA_FILE);
            if !skill_md_path.is_file() {
                continue;
            }

            let abs_path = entry_path.to_string_lossy().to_string();

            // Check cache
            if !force_refresh {
                if let Some(&cached_mtime) = self.cache.get(&abs_path) {
                    if let Ok(metadata) = std::fs::metadata(&skill_md_path) {
                        if let Ok(mtime) = metadata.modified() {
                            let mtime_secs = mtime
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_secs_f64();
                            if (mtime_secs - cached_mtime).abs() < 0.001 {
                                results.push(abs_path);
                                continue;
                            }
                        }
                    }
                }
            }

            // Update cache
            if let Ok(metadata) = std::fs::metadata(&skill_md_path) {
                if let Ok(mtime) = metadata.modified() {
                    let mtime_secs = mtime
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs_f64();
                    self.cache.insert(abs_path.clone(), mtime_secs);
                }
            }

            results.push(abs_path);
        }

        results
    }

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
        self.skill_dirs.clone()
    }
}

/// Convenience function: scan with a fresh scanner.
#[cfg(feature = "python-bindings")]
#[pyfunction]
#[pyo3(name = "scan_skill_paths")]
#[pyo3(signature = (extra_paths=None, dcc_name=None))]
pub fn py_scan_skill_paths(extra_paths: Option<Vec<String>>, dcc_name: Option<&str>) -> Vec<String> {
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
