//! Provenance for package-manager-owned installations.
//!
//! Package managers that install `dcc-mcp-cli` (the PyPI wrapper in
//! `pkg/dcc-mcp-cli-bin`, WinGet, Homebrew taps, ...) drop a marker file next
//! to the binary they unpack. It records *who* installed this copy and *which*
//! file in the directory is the real binary: on Windows the pip-generated
//! console script stays the running image, so the wrapper unpacks the
//! executable as `dcc-mcp-cli-bin.exe` beside it.
//!
//! The marker is provenance, not a lock. `dcc-mcp-cli update apply` stays
//! enabled for **every** install, package-managed ones included, because
//! updating through the CLI is the supported flow everywhere. Self-update
//! replaces the running binary in place, so afterwards the version the manager
//! records no longer matches the file on disk; the CLI reports that as an
//! advisory and names the manager's own upgrade command. Re-running that
//! command (or reinstalling the wheel) restores the recorded version.
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

use anyhow::{Context, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;

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
    /// Human-readable way to bring the manager's own record back in sync.
    ///
    /// Self-update is never refused, so this is not an alternative to
    /// `dcc-mcp-cli update apply`: it is the optional follow-up that makes
    /// the manager's metadata agree with the binary that is now on disk.
    pub fn upgrade_hint(&self) -> String {
        match self.manager.as_str() {
            "pypi" => "upgrade it with your Python package manager, for example \
                       `uv tool upgrade dcc-mcp-cli`, `pipx upgrade dcc-mcp-cli`, or \
                       `pip install --upgrade dcc-mcp-cli`"
                .to_string(),
            other => format!("re-install or upgrade this package through {other}"),
        }
    }

    /// Advisory returned by `dcc-mcp-cli update apply` after staging an update.
    ///
    /// Package-managed installs self-update too, so this does not refuse
    /// anything: it tells the caller which version the manager still records,
    /// because the staged update will move the binary past that record.
    pub fn advisory_message(&self) -> String {
        format!(
            "the update will replace this binary in place on the next launch; the {} package manager \
             will still record version {}. To re-sync its metadata afterwards, {}",
            self.manager,
            self.version,
            self.upgrade_hint()
        )
    }
}

/// Re-record the binary fingerprint after a staged self-update replaced it.
///
/// `dcc-mcp-cli update apply` stages a newer binary and the next launch
/// replaces `current_exe` with it. The PyPI wrapper installed beside it
/// fingerprints the file it unpacked by size, so a marker left untouched
/// reads as a damaged install: the next wrapper launch re-unpacks the
/// wheel's older binary and silently undoes the update, with no error for
/// the user. Re-record the size of the file that is now on disk instead.
///
/// `version` is deliberately left at the version the package manager
/// recorded. The wrapper matches a marker against the wheel payload by
/// version, so rewriting it to the self-updated version would make the
/// marker stop matching and trigger exactly the re-unpack this prevents.
/// The drift stays visible through [`PackageManagerMarker::advisory_message`].
///
/// Unknown marker keys are preserved, so a marker written by a newer wrapper
/// keeps any field this build does not know about.
///
/// `current_exe` is resolved first, so a package manager that exposes the
/// binary through a symlink updates the marker beside the real binary.
pub fn record_self_updated_size(current_exe: &Path) -> anyhow::Result<()> {
    let resolved = crate::application::current_exe::resolve(current_exe);
    let path = marker_path_for(&resolved).context("cannot place a package manager marker")?;
    if !path.is_file() {
        // Direct install: nothing fingerprints the binary, so there is
        // nothing to keep in sync.
        return Ok(());
    }

    let raw =
        fs::read_to_string(&path).with_context(|| format!("cannot read {}", path.display()))?;
    let mut marker: serde_json::Map<String, Value> = serde_json::from_str(&raw)
        .with_context(|| format!("{} is not a JSON object", path.display()))?;

    let size = fs::metadata(&resolved)
        .with_context(|| format!("cannot stat {}", resolved.display()))?
        .len();
    marker.insert("binary_size".into(), Value::from(size));
    write_marker(&path, &marker)
}

/// Write `marker` to `path`, replacing it in one step.
///
/// The temporary file sits beside the target so the replace is atomic within
/// one filesystem, matching how the PyPI wrapper writes the same marker.
fn write_marker(path: &Path, marker: &serde_json::Map<String, Value>) -> anyhow::Result<()> {
    let serialized = serde_json::to_string_pretty(marker).context("cannot serialize the marker")?;
    let temporary = path.with_extension("json.tmp");
    fs::write(&temporary, format!("{serialized}\n"))
        .with_context(|| format!("cannot write {}", temporary.display()))?;
    if let Err(error) = fs::rename(&temporary, path) {
        let _ = fs::remove_file(&temporary);
        bail!("cannot replace {}: {error}", path.display());
    }
    Ok(())
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
/// to the real binary. Without this the provenance marker next to the real
/// binary would never be found.
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
    fn pypi_marker_explains_how_to_re_sync() {
        let marker: PackageManagerMarker = serde_json::from_str(&marker_json("pypi")).unwrap();
        let hint = marker.upgrade_hint();
        assert!(hint.contains("uv tool upgrade dcc-mcp-cli"), "{hint}");

        let advisory = marker.advisory_message();
        assert!(advisory.contains("pypi"), "{advisory}");
        assert!(advisory.contains("0.20.34"), "{advisory}");
        // The advisory must not read like a refusal.
        assert!(!advisory.contains("blocked"), "{advisory}");
    }

    #[test]
    fn unknown_manager_still_names_itself() {
        let marker: PackageManagerMarker = serde_json::from_str(&marker_json("winget")).unwrap();
        assert!(marker.upgrade_hint().contains("winget"));
    }

    #[test]
    fn advisory_points_at_re_syncing_not_at_another_command() {
        // Self-update is the supported flow, so the advisory must not send
        // the caller to a different command instead of `update apply`.
        for manager in ["winget", "homebrew", "pypi"] {
            let marker: PackageManagerMarker = serde_json::from_str(&marker_json(manager)).unwrap();
            let hint = marker.upgrade_hint();
            assert!(!hint.contains("instead of"), "{hint}");
            let advisory = marker.advisory_message();
            assert!(!advisory.contains("applied in place;"), "{advisory}");
            assert!(advisory.contains("will replace"), "{advisory}");
        }
    }

    /// Write `marker_json` into `dir` and return the marker path.
    fn write_marker_file(dir: &Path, extra: serde_json::Value) -> PathBuf {
        let mut marker: serde_json::Map<String, serde_json::Value> =
            serde_json::from_str(&marker_json("pypi")).unwrap();
        let extra = extra.as_object().expect("extra fields must be an object");
        for (key, value) in extra {
            marker.insert(key.clone(), value.clone());
        }
        let path = dir.join(MARKER_FILE_NAME);
        fs::write(&path, serde_json::to_string_pretty(&marker).unwrap()).unwrap();
        path
    }

    #[test]
    fn self_update_refreshes_the_recorded_binary_size() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("dcc-mcp-cli");
        // 51 is the size the wheel unpacked, before the updater replaced it.
        let marker_path = write_marker_file(dir.path(), serde_json::json!({"binary_size": 51}));
        fs::write(&exe, b"unpacked binary").unwrap();

        // The staged update lands a differently sized binary in place.
        fs::write(&exe, b"a newer release binary, never the same length").unwrap();

        record_self_updated_size(&exe).unwrap();

        let marker: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&marker_path).unwrap()).unwrap();
        let expected = fs::metadata(&exe).unwrap().len();
        assert_eq!(marker["binary_size"], serde_json::json!(expected));
        assert_ne!(marker["binary_size"], serde_json::json!(51));
    }

    #[test]
    fn self_update_keeps_the_version_the_wrapper_matches_on() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("dcc-mcp-cli");
        let marker_path = write_marker_file(
            dir.path(),
            serde_json::json!({"binary_size": 51, "version": "9.9.9"}),
        );
        fs::write(&exe, b"a newer release binary").unwrap();

        record_self_updated_size(&exe).unwrap();

        let marker: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&marker_path).unwrap()).unwrap();
        // The wrapper matches the marker against the wheel payload by version,
        // so rewriting it would trigger the re-unpack this prevents.
        assert_eq!(marker["version"], serde_json::json!("9.9.9"));
        assert_eq!(marker["platform"], serde_json::json!("windows-x86_64"));
    }

    #[test]
    fn self_update_preserves_unknown_marker_keys() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("dcc-mcp-cli");
        let marker_path = write_marker_file(
            dir.path(),
            serde_json::json!({"binary_size": 51, "from_a_newer_wrapper": true}),
        );
        fs::write(&exe, b"a newer release binary").unwrap();

        record_self_updated_size(&exe).unwrap();

        let marker: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&marker_path).unwrap()).unwrap();
        assert_eq!(marker["from_a_newer_wrapper"], serde_json::json!(true));
    }

    #[test]
    fn direct_install_has_nothing_to_refresh() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("dcc-mcp-cli");
        fs::write(&exe, b"binary").unwrap();

        // No marker beside the binary: nothing fingerprints it.
        record_self_updated_size(&exe).unwrap();
        assert!(!dir.path().join(MARKER_FILE_NAME).exists());
    }

    #[test]
    fn refreshing_a_marker_never_leaves_a_temporary_file() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("dcc-mcp-cli");
        write_marker_file(dir.path(), serde_json::json!({"binary_size": 51}));
        fs::write(&exe, b"a newer release binary").unwrap();

        record_self_updated_size(&exe).unwrap();

        let leftovers: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name())
            .filter(|name| name.to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "left behind {leftovers:?}");
    }
}
