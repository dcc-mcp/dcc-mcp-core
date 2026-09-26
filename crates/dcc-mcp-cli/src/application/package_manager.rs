//! Detection for package-manager-owned installations.
//!
//! `dcc-mcp-cli update apply` replaces `current_exe` on the next launch. When a
//! package manager owns that file, replacing it fights the manager's version
//! authority: `pip install --upgrade dcc-mcp-cli` and a self-applied binary
//! update would keep overwriting each other. Package-managed installations must
//! therefore upgrade through the package manager, which is what Homebrew's
//! packaging policy requires as well.
//!
//! The GitHub Release build keeps self-update. The PyPI wrapper
//! (`pkg/dcc-mcp-cli-bin`) writes a marker file next to the binary it unpacks,
//! and that marker is the contract between the two:
//!
//! ```json
//! {
//!   "schema_version": 1,
//!   "distribution": "dcc-mcp-cli",
//!   "manager": "pypi",
//!   "version": "0.20.34",
//!   "platform": "windows-x86_64",
//!   "binary": "dcc-mcp-cli-bin.exe"
//! }
//! ```
//!
//! The marker lives beside `current_exe` on purpose: the same directory is the
//! one the component service uses to place `dcc-cua`, so "who owns this
//! directory" has exactly one answer.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Marker file written by package managers that install this CLI.
pub const MARKER_FILE_NAME: &str = "dcc-mcp-cli.package-manager.json";

/// Marker schema this build understands.
pub const MARKER_SCHEMA_VERSION: u64 = 1;

/// Contents of [`MARKER_FILE_NAME`].
///
/// Unknown keys are ignored and numeric fields default to `0` so a marker
/// written by a newer wrapper stays readable by an older CLI.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PackageManagerMarker {
    pub schema_version: u64,
    pub distribution: String,
    pub manager: String,
    pub version: String,
    #[serde(default)]
    pub platform: String,
    #[serde(default)]
    pub binary: String,
}

impl PackageManagerMarker {
    /// Human-readable instruction shown when self-update is refused.
    pub fn upgrade_hint(&self) -> String {
        match self.manager.as_str() {
            "pypi" => "upgrade it with your Python package manager, for example \
                       `uv tool upgrade dcc-mcp-cli`, `pipx upgrade dcc-mcp-cli`, or \
                       `pip install --upgrade dcc-mcp-cli`"
                .to_string(),
            other => format!("upgrade it through {other} instead of `dcc-mcp-cli update apply`"),
        }
    }

    /// Message returned by `dcc-mcp-cli update apply`.
    pub fn blocked_message(&self) -> String {
        format!(
            "dcc-mcp-cli {} is managed by the {} package manager; {}",
            self.version,
            self.manager,
            self.upgrade_hint()
        )
    }
}

/// Return the marker path that governs the executable at `current_exe`.
///
/// `current_exe` is used as given; pass a resolved path (see
/// [`crate::application::current_exe::resolve`]) when it may be a symlink.
pub fn marker_path_for(current_exe: &Path) -> Option<PathBuf> {
    Some(current_exe.parent()?.join(MARKER_FILE_NAME))
}

/// Read and validate the marker beside `current_exe`, if it exists.
///
/// `current_exe` is resolved first, so a package manager that exposes the
/// binary through a symlink (WinGet portable links it from
/// `Microsoft\WinGet\Links`) is still detected from the marker it wrote next
/// to the real binary. Without this the "package-managed installs must not
/// self-update" rule silently stops applying.
pub fn read_marker_for(current_exe: &Path) -> Option<PackageManagerMarker> {
    let resolved = crate::application::current_exe::resolve(current_exe);
    let path = marker_path_for(&resolved)?;
    let raw = fs::read_to_string(&path).ok()?;
    let marker: PackageManagerMarker = serde_json::from_str(&raw).ok()?;
    (marker.schema_version == MARKER_SCHEMA_VERSION).then_some(marker)
}

/// Return the marker governing this process, or `None` for a direct install.
pub fn detect() -> Option<PackageManagerMarker> {
    read_marker_for(&std::env::current_exe().ok()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn marker_json(manager: &str) -> String {
        serde_json::json!({
            "schema_version": MARKER_SCHEMA_VERSION,
            "distribution": "dcc-mcp-cli",
            "manager": manager,
            "version": "0.20.34",
            "platform": "windows-x86_64",
            "binary": "dcc-mcp-cli-bin.exe",
        })
        .to_string()
    }

    #[test]
    fn missing_marker_is_not_package_managed() {
        let dir = tempfile::tempdir().unwrap();
        assert!(read_marker_for(&dir.path().join("dcc-mcp-cli")).is_none());
    }

    #[test]
    fn marker_beside_the_executable_is_detected() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("dcc-mcp-cli");
        fs::write(dir.path().join(MARKER_FILE_NAME), marker_json("pypi")).unwrap();

        let marker = read_marker_for(&exe).expect("marker should be detected");
        assert_eq!(marker.manager, "pypi");
        assert_eq!(marker.version, "0.20.34");
        assert_eq!(marker.binary, "dcc-mcp-cli-bin.exe");
    }

    /// Create `link` as a file symlink to `target`.
    fn symlink_file(target: &Path, link: &Path) -> std::io::Result<()> {
        #[cfg(windows)]
        {
            std::os::windows::fs::symlink_file(target, link)
        }
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(target, link)
        }
    }

    #[test]
    fn marker_is_detected_through_a_symlinked_executable() {
        // WinGet portable: PATH carries `...\WinGet\Links\dcc-mcp-cli.exe`,
        // the marker is written beside the real binary under `Packages`.
        let root = tempfile::tempdir().unwrap();
        let package_dir = root.path().join("Packages").join("DccMcp.DccMcpCli_abc123");
        let links_dir = root.path().join("Links");
        fs::create_dir_all(&package_dir).unwrap();
        fs::create_dir_all(&links_dir).unwrap();

        let package_exe = package_dir.join("dcc-mcp-cli.exe");
        fs::write(&package_exe, b"real binary").unwrap();
        fs::write(package_dir.join(MARKER_FILE_NAME), marker_json("winget")).unwrap();

        let link_exe = links_dir.join("dcc-mcp-cli.exe");
        if symlink_file(&package_exe, &link_exe).is_err() {
            // Symlinks need Developer Mode or admin rights on Windows.
            return;
        }

        let marker = read_marker_for(&link_exe).expect("marker must survive the symlink");
        assert_eq!(marker.manager, "winget");
    }

    #[test]
    fn malformed_marker_is_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("dcc-mcp-cli");
        fs::write(dir.path().join(MARKER_FILE_NAME), "{not json").unwrap();
        assert!(read_marker_for(&exe).is_none());
    }

    #[test]
    fn marker_from_a_future_schema_is_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("dcc-mcp-cli");
        fs::write(
            dir.path().join(MARKER_FILE_NAME),
            serde_json::json!({
                "schema_version": MARKER_SCHEMA_VERSION + 1,
                "distribution": "dcc-mcp-cli",
                "manager": "pypi",
                "version": "9.9.9",
            })
            .to_string(),
        )
        .unwrap();
        assert!(read_marker_for(&exe).is_none());
    }

    #[test]
    fn pypi_marker_explains_how_to_upgrade() {
        let marker: PackageManagerMarker = serde_json::from_str(&marker_json("pypi")).unwrap();
        let hint = marker.upgrade_hint();
        assert!(hint.contains("uv tool upgrade dcc-mcp-cli"), "{hint}");
        assert!(
            marker
                .blocked_message()
                .contains("managed by the pypi package manager")
        );
    }

    #[test]
    fn unknown_manager_still_names_itself() {
        let marker: PackageManagerMarker = serde_json::from_str(&marker_json("winget")).unwrap();
        assert!(marker.upgrade_hint().contains("winget"));
    }
}
