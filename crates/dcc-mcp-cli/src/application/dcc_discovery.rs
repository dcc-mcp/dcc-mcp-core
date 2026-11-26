//! Read-only discovery of locally installed DCC applications.
//!
//! Discovery is intentionally bounded. It checks explicit environment
//! overrides, PATH entries, platform application roots, and known vendor
//! directories on mounted Windows drives. It never launches a DCC or executes
//! an installer.

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const SEARCH_PATHS_ENV: &str = "DCC_MCP_DCC_SEARCH_PATHS";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DccInstallation {
    pub dcc_type: String,
    pub display_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    pub executable: PathBuf,
    pub install_root: PathBuf,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub python: Option<PathBuf>,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DccDiscovery {
    pub platform: String,
    pub searched_roots: Vec<PathBuf>,
    pub installations: Vec<DccInstallation>,
}

#[derive(Debug, Clone, Copy)]
struct DccSpec {
    dcc_type: &'static str,
    display_name: &'static str,
    executables: &'static [&'static str],
    python_executables: &'static [&'static str],
    vendor_roots: &'static [&'static str],
}

const SPECS: &[DccSpec] = &[
    DccSpec {
        dcc_type: "maya",
        display_name: "Autodesk Maya",
        executables: &["maya", "maya.exe"],
        python_executables: &["mayapy", "mayapy.exe"],
        vendor_roots: &["Autodesk"],
    },
    DccSpec {
        dcc_type: "blender",
        display_name: "Blender",
        executables: &["blender", "blender.exe"],
        python_executables: &["python", "python.exe"],
        vendor_roots: &["Blender Foundation"],
    },
    DccSpec {
        dcc_type: "houdini",
        display_name: "SideFX Houdini",
        executables: &["houdini", "houdini.exe"],
        python_executables: &["hython", "hython.exe"],
        vendor_roots: &["Side Effects Software"],
    },
    DccSpec {
        dcc_type: "3dsmax",
        display_name: "Autodesk 3ds Max",
        executables: &["3dsmax", "3dsmax.exe"],
        python_executables: &["python", "python.exe"],
        vendor_roots: &["Autodesk"],
    },
    DccSpec {
        dcc_type: "nuke",
        display_name: "Foundry Nuke",
        executables: &["nuke", "nuke.exe"],
        python_executables: &["python", "python.exe"],
        vendor_roots: &["Foundry"],
    },
    DccSpec {
        dcc_type: "photoshop",
        display_name: "Adobe Photoshop",
        executables: &["Photoshop", "Photoshop.exe"],
        python_executables: &[],
        vendor_roots: &["Adobe"],
    },
    DccSpec {
        dcc_type: "premiere",
        display_name: "Adobe Premiere Pro",
        executables: &["Adobe Premiere Pro", "Adobe Premiere Pro.exe"],
        python_executables: &[],
        vendor_roots: &["Adobe"],
    },
    DccSpec {
        dcc_type: "aftereffects",
        display_name: "Adobe After Effects",
        executables: &["AfterFX", "AfterFX.exe"],
        python_executables: &[],
        vendor_roots: &["Adobe"],
    },
    DccSpec {
        dcc_type: "katana",
        display_name: "Foundry Katana",
        executables: &["katana", "katana.exe"],
        python_executables: &["katana", "katana.exe"],
        vendor_roots: &["Foundry", "Katana"],
    },
    DccSpec {
        dcc_type: "mobu",
        display_name: "Autodesk MotionBuilder",
        executables: &["motionbuilder", "motionbuilder.exe"],
        python_executables: &["python", "python.exe"],
        vendor_roots: &["Autodesk"],
    },
];

pub fn discover_dcc_installations(
    dcc_filter: Option<&str>,
    extra_roots: &[PathBuf],
) -> DccDiscovery {
    let specs = SPECS.iter().filter(|spec| {
        dcc_filter
            .map(|filter| normalize_key(filter) == spec.dcc_type)
            .unwrap_or(true)
    });
    let roots = candidate_roots(extra_roots);
    let mut found = BTreeMap::new();

    for spec in specs {
        let env_key = format!("DCC_MCP_{}_PATH", spec.dcc_type.to_ascii_uppercase());
        if let Ok(path) = env::var(&env_key)
            && let Some(installation) = installation_from_path(spec, Path::new(&path), "env")
        {
            found.insert(dedupe_key(&installation), installation);
        }

        for path_entry in path_entries() {
            for executable in spec.executables {
                let candidate = path_entry.join(executable);
                if let Some(installation) = installation_from_path(spec, &candidate, "PATH") {
                    found.insert(dedupe_key(&installation), installation);
                }
            }
        }

        for root in &roots {
            scan_root(spec, root, 0, &mut found);
        }
    }

    let mut installations: Vec<_> = found.into_values().collect();
    installations.sort_by(|left, right| {
        left.dcc_type
            .cmp(&right.dcc_type)
            .then_with(|| left.executable.cmp(&right.executable))
    });

    DccDiscovery {
        platform: env::consts::OS.to_string(),
        searched_roots: roots,
        installations,
    }
}

pub fn preferred_python(dcc_type: &str, extra_roots: &[PathBuf]) -> Option<PathBuf> {
    discover_dcc_installations(Some(dcc_type), extra_roots)
        .installations
        .into_iter()
        .find_map(|installation| installation.python)
}

fn candidate_roots(extra_roots: &[PathBuf]) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    roots.extend(extra_roots.iter().cloned());
    roots.extend(configured_search_paths());

    for variable in [
        "ProgramFiles",
        "ProgramFiles(x86)",
        "ProgramW6432",
        "LOCALAPPDATA",
        "APPDATA",
    ] {
        if let Ok(value) = env::var(variable) {
            let base = PathBuf::from(value);
            for vendor in [
                "Autodesk",
                "Blender Foundation",
                "Side Effects Software",
                "Foundry",
                "Adobe",
            ] {
                roots.push(base.join(vendor));
                roots.push(base.join("Programs").join(vendor));
            }
        }
    }

    if let Some(home) = dirs::home_dir() {
        roots.extend([home.join("Applications"), home.join(".local").join("bin")]);
    }

    if cfg!(windows) {
        for drive in b'A'..=b'Z' {
            let drive_root = PathBuf::from(format!("{}:\\", char::from(drive)));
            for program_dir in ["Program Files", "Program Files (x86)", "Apps"] {
                for vendor in [
                    "Autodesk",
                    "Blender Foundation",
                    "Side Effects Software",
                    "Foundry",
                    "Adobe",
                ] {
                    roots.push(drive_root.join(program_dir).join(vendor));
                }
            }
        }
    } else {
        roots.extend([
            PathBuf::from("/Applications"),
            PathBuf::from("/opt"),
            PathBuf::from("/usr/local/bin"),
            PathBuf::from("/usr/bin"),
        ]);
    }

    dedupe_paths(roots)
}

fn path_entries() -> Vec<PathBuf> {
    let separator = if cfg!(windows) { ';' } else { ':' };
    env::var_os("PATH")
        .into_iter()
        .flat_map(|value| {
            value
                .to_string_lossy()
                .split(separator)
                .map(PathBuf::from)
                .collect::<Vec<_>>()
        })
        .filter(|path| path.is_dir())
        .collect()
}

fn configured_search_paths() -> Vec<PathBuf> {
    let separator = if cfg!(windows) { ';' } else { ':' };
    env::var(SEARCH_PATHS_ENV)
        .ok()
        .into_iter()
        .flat_map(|value| {
            value
                .split(separator)
                .map(PathBuf::from)
                .collect::<Vec<_>>()
        })
        .filter(|path| path.is_dir())
        .collect()
}

fn scan_root(
    spec: &DccSpec,
    root: &Path,
    depth: usize,
    found: &mut BTreeMap<String, DccInstallation>,
) {
    if depth > 4 || !root.is_dir() {
        return;
    }

    for executable in spec.executables {
        let candidate = root.join(executable);
        if let Some(installation) = installation_from_path(spec, &candidate, "common_path") {
            found.insert(dedupe_key(&installation), installation);
        }
    }

    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false) {
            scan_root(spec, &path, depth + 1, found);
        }
    }
}

fn installation_from_path(spec: &DccSpec, path: &Path, source: &str) -> Option<DccInstallation> {
    if !path.is_file() || !is_known_executable(spec, path.file_name()?.to_string_lossy().as_ref()) {
        return None;
    }
    let executable = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let install_root = infer_install_root(&executable, spec);
    let python = find_python(spec, &executable);
    let version = extract_version(&install_root).or_else(|| extract_version(&executable));

    Some(DccInstallation {
        dcc_type: spec.dcc_type.to_string(),
        display_name: spec.display_name.to_string(),
        version,
        executable,
        install_root,
        python,
        source: source.to_string(),
    })
}

fn find_python(spec: &DccSpec, executable: &Path) -> Option<PathBuf> {
    let parent = executable.parent()?;
    for name in spec.python_executables {
        let candidate = parent.join(name);
        if candidate.is_file() {
            return Some(fs::canonicalize(&candidate).unwrap_or(candidate));
        }
    }
    for ancestor in parent.ancestors().take(3) {
        for name in spec.python_executables {
            let candidate = ancestor.join(name);
            if candidate.is_file() {
                return Some(fs::canonicalize(&candidate).unwrap_or(candidate));
            }
        }
    }
    None
}

fn infer_install_root(executable: &Path, spec: &DccSpec) -> PathBuf {
    let mut current = executable.parent();
    while let Some(ancestor) = current {
        let name = ancestor
            .file_name()
            .map(|value| value.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        if name.contains(spec.dcc_type)
            || spec
                .vendor_roots
                .iter()
                .any(|vendor| name.contains(&vendor.to_ascii_lowercase()))
        {
            return ancestor.to_path_buf();
        }
        current = ancestor.parent();
    }
    executable.parent().unwrap_or(executable).to_path_buf()
}

fn is_known_executable(spec: &DccSpec, name: &str) -> bool {
    spec.executables
        .iter()
        .any(|candidate| candidate.eq_ignore_ascii_case(name))
}

fn extract_version(path: &Path) -> Option<String> {
    let text = path.to_string_lossy();
    let mut current = String::new();
    let mut best = None;
    for character in text.chars() {
        if character.is_ascii_digit() || (character == '.' && !current.is_empty()) {
            current.push(character);
        } else {
            if current.chars().any(|value| value.is_ascii_digit()) {
                best = Some(current.trim_matches('.').to_string());
            }
            current.clear();
        }
    }
    if current.chars().any(|value| value.is_ascii_digit()) {
        best = Some(current.trim_matches('.').to_string());
    }
    best.filter(|value| !value.is_empty())
}

fn normalize_key(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn dedupe_key(installation: &DccInstallation) -> String {
    format!(
        "{}:{}",
        installation.dcc_type,
        installation
            .executable
            .to_string_lossy()
            .to_ascii_lowercase()
    )
}

fn dedupe_paths(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut seen = BTreeMap::new();
    for path in paths {
        if path.is_dir() {
            let canonical = fs::canonicalize(&path).unwrap_or(path);
            seen.insert(canonical.to_string_lossy().to_ascii_lowercase(), canonical);
        }
    }
    seen.into_values().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_known_executable_and_host_python() {
        let temp = tempfile::tempdir().unwrap();
        let bin = temp.path().join("Maya2026").join("bin");
        fs::create_dir_all(&bin).unwrap();
        let maya = bin.join(if cfg!(windows) { "maya.exe" } else { "maya" });
        let mayapy = bin.join(if cfg!(windows) {
            "mayapy.exe"
        } else {
            "mayapy"
        });
        fs::write(&maya, b"").unwrap();
        fs::write(&mayapy, b"").unwrap();

        let discovery = discover_dcc_installations(Some("Maya"), &[temp.path().to_path_buf()]);
        let installation = discovery
            .installations
            .iter()
            .find(|installation| installation.executable == fs::canonicalize(&maya).unwrap())
            .expect("temporary Maya installation should be discovered");
        assert_eq!(installation.dcc_type, "maya");
        assert_eq!(
            installation.install_root,
            fs::canonicalize(temp.path().join("Maya2026")).unwrap()
        );
        assert_eq!(
            installation
                .python
                .as_ref()
                .and_then(|path| path.file_name())
                .map(|name| name.to_string_lossy().to_ascii_lowercase()),
            Some(
                mayapy
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .to_ascii_lowercase()
            )
        );
    }

    #[test]
    fn ignores_unknown_dcc_filter() {
        let discovery = discover_dcc_installations(Some("custom"), &[]);
        assert!(discovery.installations.is_empty());
    }
}
