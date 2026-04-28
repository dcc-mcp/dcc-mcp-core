//! Detection helpers for DCC GUI executables (issue #524).
//!
//! Many DCCs ship two binaries with similar names: a *GUI* one
//! (`maya.exe`, `houdini.exe`, `UnrealEditor.exe`) and a *headless Python
//! interpreter* (`mayapy.exe`, `hython.exe`, `UnrealEditor-Cmd.exe`).
//! Pointing `DCC_MCP_PYTHON_EXECUTABLE` at the GUI binary spawns a new
//! DCC window instead of running the skill script — see
//! `loonghao/dcc-mcp-maya#125`.
//!
//! `is_gui_executable` lets every DCC plugin (Maya, Houdini, Unreal …)
//! detect the misconfiguration *before* it reaches the skill executor
//! and offer a recommended replacement.

use std::path::{Path, PathBuf};

/// Outcome of a GUI-binary detection on a Python-executable path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuiExecutableHint {
    /// The path that was probed.
    pub gui_path: PathBuf,
    /// DCC family name (`"maya"`, `"houdini"`, `"unreal"`, …) suitable
    /// for routing keys; lower-case ASCII.
    pub dcc_kind: &'static str,
    /// Recommended replacement path resolved from a sibling binary
    /// (e.g. `mayapy.exe` next to `maya.exe`). `None` when the DCC has
    /// no headless Python equivalent (`blender`, `nuke`, `katana` etc.)
    /// or when the sibling cannot be located on disk.
    pub recommended_replacement: Option<PathBuf>,
}

/// One row in the GUI-binary lookup table.
struct GuiBinaryRow {
    /// File-stem matches (case-insensitive); compared after `to_lowercase`.
    stems: &'static [&'static str],
    dcc_kind: &'static str,
    /// Sibling stems to look for when suggesting a replacement.
    /// Probed in order; first existing sibling wins.
    sibling_python_stems: &'static [&'static str],
}

/// Lookup table — kept ordered so the most-common DCCs hit early.
const GUI_BINARIES: &[GuiBinaryRow] = &[
    GuiBinaryRow {
        stems: &["maya", "maya.bin"],
        dcc_kind: "maya",
        sibling_python_stems: &["mayapy"],
    },
    GuiBinaryRow {
        stems: &["houdini", "houdinifx", "houdinicore"],
        dcc_kind: "houdini",
        sibling_python_stems: &["hython"],
    },
    GuiBinaryRow {
        // Unreal Editor + the commandlet entry point are both GUI in the
        // sense that they are not Python interpreters; the commandlet
        // (`-Cmd`) is still the recommended replacement when available
        // because it ships the embedded Python.
        stems: &["unrealeditor"],
        dcc_kind: "unreal",
        sibling_python_stems: &["unrealeditor-cmd"],
    },
    GuiBinaryRow {
        stems: &["blender"],
        dcc_kind: "blender",
        sibling_python_stems: &[],
    },
    GuiBinaryRow {
        stems: &["3dsmax"],
        dcc_kind: "3dsmax",
        sibling_python_stems: &[],
    },
    GuiBinaryRow {
        stems: &["nuke", "nukestudio"],
        dcc_kind: "nuke",
        sibling_python_stems: &[],
    },
    GuiBinaryRow {
        stems: &["modo"],
        dcc_kind: "modo",
        sibling_python_stems: &[],
    },
    GuiBinaryRow {
        stems: &["motionbuilder"],
        dcc_kind: "motionbuilder",
        sibling_python_stems: &[],
    },
    GuiBinaryRow {
        stems: &["cinema4d", "c4d"],
        dcc_kind: "c4d",
        sibling_python_stems: &[],
    },
    GuiBinaryRow {
        stems: &["katana"],
        dcc_kind: "katana",
        sibling_python_stems: &[],
    },
];

fn lowercase_stem(path: &Path) -> Option<String> {
    path.file_stem()
        .and_then(|s| s.to_str())
        .map(|s| s.to_ascii_lowercase())
}

/// Return a [`GuiExecutableHint`] when `path` looks like a known DCC GUI
/// binary. `None` when the path is anything else (including
/// `python.exe`, `mayapy`, missing files, or unknown vendor binaries).
///
/// Detection is case-insensitive on the *file stem* only — the parent
/// directory and extension are not consulted, so cross-platform paths
/// (`/Applications/Autodesk/maya2024/Maya.app/Contents/bin/maya` vs
/// `C:\Program Files\Autodesk\Maya2024\bin\maya.exe`) match identically.
pub fn is_gui_executable(path: &Path) -> Option<GuiExecutableHint> {
    let stem = lowercase_stem(path)?;
    for row in GUI_BINARIES {
        if row.stems.contains(&stem.as_str()) {
            let recommended = locate_sibling(path, row.sibling_python_stems);
            return Some(GuiExecutableHint {
                gui_path: path.to_path_buf(),
                dcc_kind: row.dcc_kind,
                recommended_replacement: recommended,
            });
        }
    }
    None
}

/// If `path` is a GUI binary with a known headless-Python sibling that
/// exists on disk, return the sibling path. Otherwise return `path`
/// unchanged (so callers can use this as a one-shot env-var fixer).
pub fn correct_python_executable(path: &Path) -> PathBuf {
    is_gui_executable(path)
        .and_then(|hint| hint.recommended_replacement)
        .unwrap_or_else(|| path.to_path_buf())
}

/// Locate the first existing sibling whose file-stem matches one of
/// `stems`, preserving the original extension (`.exe` on Windows, none
/// elsewhere). Returns `None` if no candidate file is found.
fn locate_sibling(gui_path: &Path, stems: &[&str]) -> Option<PathBuf> {
    if stems.is_empty() {
        return None;
    }
    let parent = gui_path.parent()?;
    let extension = gui_path.extension().and_then(|e| e.to_str()).unwrap_or("");
    for stem in stems {
        let mut candidate = parent.join(stem);
        if !extension.is_empty() {
            candidate.set_extension(extension);
        }
        if candidate.exists() {
            return Some(candidate);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn detects_maya_exe() {
        let hint = is_gui_executable(Path::new("C:/Program Files/Autodesk/Maya2024/bin/maya.exe"))
            .expect("maya.exe must be detected");
        assert_eq!(hint.dcc_kind, "maya");
    }

    #[test]
    fn detects_maya_app_macos() {
        let hint = is_gui_executable(Path::new(
            "/Applications/Autodesk/maya2024/Maya.app/Contents/bin/maya",
        ))
        .expect("maya (no extension, mixed case) must be detected");
        assert_eq!(hint.dcc_kind, "maya");
    }

    #[test]
    fn detects_houdini_variants() {
        for stem in ["houdini", "houdinifx.exe", "HoudiniCore"] {
            let hint = is_gui_executable(Path::new(stem))
                .unwrap_or_else(|| panic!("{stem} must be detected"));
            assert_eq!(hint.dcc_kind, "houdini");
        }
    }

    #[test]
    fn detects_unreal_editor() {
        let hint = is_gui_executable(Path::new("UnrealEditor.exe")).unwrap();
        assert_eq!(hint.dcc_kind, "unreal");
    }

    #[test]
    fn ignores_python_interpreter() {
        assert!(is_gui_executable(Path::new("python.exe")).is_none());
        assert!(is_gui_executable(Path::new("/usr/bin/python3")).is_none());
        assert!(is_gui_executable(Path::new("mayapy.exe")).is_none());
        assert!(is_gui_executable(Path::new("hython")).is_none());
    }

    #[test]
    fn ignores_unknown_binary() {
        assert!(is_gui_executable(Path::new("vscode.exe")).is_none());
        assert!(is_gui_executable(Path::new("/bin/ls")).is_none());
    }

    #[test]
    fn ignores_empty_path() {
        assert!(is_gui_executable(Path::new("")).is_none());
    }

    #[test]
    fn locates_existing_mayapy_sibling() {
        let dir = tempdir().unwrap();
        let maya = dir.path().join("maya.exe");
        let mayapy = dir.path().join("mayapy.exe");
        fs::write(&maya, b"").unwrap();
        fs::write(&mayapy, b"").unwrap();

        let hint = is_gui_executable(&maya).unwrap();
        assert_eq!(hint.recommended_replacement, Some(mayapy));
    }

    #[test]
    fn returns_none_when_sibling_missing() {
        let dir = tempdir().unwrap();
        let maya = dir.path().join("maya.exe");
        fs::write(&maya, b"").unwrap();
        let hint = is_gui_executable(&maya).unwrap();
        assert_eq!(hint.recommended_replacement, None);
    }

    #[test]
    fn correct_python_executable_returns_sibling_when_available() {
        let dir = tempdir().unwrap();
        let maya = dir.path().join("maya.exe");
        let mayapy = dir.path().join("mayapy.exe");
        fs::write(&maya, b"").unwrap();
        fs::write(&mayapy, b"").unwrap();

        assert_eq!(correct_python_executable(&maya), mayapy);
    }

    #[test]
    fn correct_python_executable_passes_through_for_python() {
        let p = Path::new("/usr/bin/python3");
        assert_eq!(correct_python_executable(p), p);
    }

    #[test]
    fn correct_python_executable_passes_through_when_sibling_missing() {
        // Maya detected, but no sibling on disk → return original.
        let p = Path::new("C:/nope/maya.exe");
        assert_eq!(correct_python_executable(p), p);
    }
}

// PyO3 bindings live in `crate::python::gui_executable`.
#[cfg(feature = "python-bindings")]
pub use crate::python::gui_executable::{
    PyGuiExecutableHint, py_correct_python_executable, py_is_gui_executable,
};
