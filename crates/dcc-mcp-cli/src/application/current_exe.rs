//! Real location of the running `dcc-mcp-cli` executable.
//!
//! Several features treat "the directory next to the executable" as the
//! install boundary: the `dcc-cua` companion component lands beside it, the
//! package-manager marker that disables self-update is read from beside it,
//! and the gateway binary is looked up beside it.
//!
//! Package managers are free to expose the binary through a symlink instead of
//! putting it where `PATH` points. WinGet portable installs are the common
//! case: the real binary lives in
//! `%LOCALAPPDATA%\Microsoft\WinGet\Packages\<Id>_<hash>\dcc-mcp-cli.exe`
//! while `PATH` only carries
//! `%LOCALAPPDATA%\Microsoft\WinGet\Links\dcc-mcp-cli.exe`.
//!
//! `std::env::current_exe()` reports the path the process was started from,
//! i.e. the symlink, so every `parent()` derived from it lands in the Links
//! directory: `dcc-cua` is reported `missing` even when it is installed next
//! to the real binary, and the package-manager marker is never found — which
//! silently re-enables self-update for a package-managed install.
//!
//! [`current_exe`] resolves the link first so all of those agree on one
//! directory. This mirrors what the updater crate already does when it stages
//! an install (`dcc_mcp_updater` canonicalizes before picking a sibling
//! target), so resolving here makes reads agree with where writes actually go.

use std::path::{Path, PathBuf};

use anyhow::Context;

/// Return the real path of the running executable, with symlinks resolved.
///
/// Falls back to the path reported by `std::env::current_exe()` when the link
/// target cannot be resolved, so callers keep their previous behaviour and
/// error semantics instead of failing on an unreadable path.
pub fn current_exe() -> anyhow::Result<PathBuf> {
    let reported = std::env::current_exe().context("cannot resolve the dcc-mcp-cli executable")?;
    Ok(resolve(&reported))
}

/// Resolve `path` to the file it points at, returning `path` unchanged when
/// the target cannot be resolved.
///
/// Resolution never fails: an unresolvable path is returned as-is so that
/// callers which only need a directory keep working. On Windows this goes
/// through `GetFinalPathNameByHandleW` (via `std::fs::canonicalize`), whose
/// verbatim `\\?\` prefix is stripped again — see [`without_verbatim_prefix`].
pub fn resolve(path: &Path) -> PathBuf {
    match std::fs::canonicalize(path) {
        Ok(resolved) => without_verbatim_prefix(&resolved),
        Err(_) => path.to_path_buf(),
    }
}

/// Drop the Windows verbatim (`\\?\`) prefix so resolved paths stay readable
/// in command output and comparable with the paths users typed.
///
/// Both forms address the same file; the verbatim form only additionally
/// bypasses the legacy 260-character limit, which install directories do not
/// reach. `\\?\UNC\server\share` is turned back into `\\server\share`.
///
/// A path that is not valid UTF-8 is returned untouched: `to_string_lossy()`
/// would swap ill-formed UTF-16 (lone surrogates) for U+FFFD and hand back a
/// path that does not exist, which is the same silent misplacement this
/// module exists to prevent. Keeping the `\\?\` prefix costs nothing there.
#[cfg(windows)]
fn without_verbatim_prefix(path: &Path) -> PathBuf {
    const VERBATIM: &str = r"\\?\";
    const VERBATIM_UNC: &str = r"\\?\UNC\";

    let Some(raw) = path.to_str() else {
        return path.to_path_buf();
    };
    if let Some(rest) = raw.strip_prefix(VERBATIM_UNC) {
        return PathBuf::from(format!(r"\\{rest}"));
    }
    if let Some(rest) = raw.strip_prefix(VERBATIM) {
        return PathBuf::from(rest);
    }
    path.to_path_buf()
}

/// Nothing to strip outside Windows: `canonicalize` returns plain paths.
#[cfg(not(windows))]
fn without_verbatim_prefix(path: &Path) -> PathBuf {
    path.to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;

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

    /// Build the WinGet portable layout: a real package directory plus a
    /// `Links` directory holding a symlink to the executable.
    ///
    /// Returns `None` when this host forbids symlink creation (Windows without
    /// Developer Mode / admin), so callers skip instead of failing the suite.
    fn winget_style_layout(root: &Path) -> Option<(PathBuf, PathBuf)> {
        let package_dir = root.join("Packages").join("DccMcp.DccMcpCli_abc123");
        let links_dir = root.join("Links");
        std::fs::create_dir_all(&package_dir).unwrap();
        std::fs::create_dir_all(&links_dir).unwrap();

        let package_exe = package_dir.join("dcc-mcp-cli.exe");
        std::fs::write(&package_exe, b"real binary").unwrap();

        let link_exe = links_dir.join("dcc-mcp-cli.exe");
        symlink_file(&package_exe, &link_exe).ok()?;
        Some((package_exe, link_exe))
    }

    #[test]
    fn symlinked_executable_resolves_to_the_real_directory() {
        let root = tempfile::tempdir().unwrap();
        let Some((package_exe, link_exe)) = winget_style_layout(root.path()) else {
            return;
        };

        let resolved = resolve(&link_exe);
        assert_ne!(resolved, link_exe, "the symlink must not resolve to itself");
        assert!(
            !resolved.to_string_lossy().contains("Links"),
            "resolved path escaped the WinGet Links directory: {}",
            resolved.display()
        );
        assert_eq!(
            std::fs::canonicalize(resolved.parent().unwrap()).unwrap(),
            std::fs::canonicalize(package_exe.parent().unwrap()).unwrap(),
            "the symlink and the real binary must share one install directory"
        );
    }

    #[test]
    fn symlinked_and_real_launch_paths_agree() {
        let root = tempfile::tempdir().unwrap();
        let Some((package_exe, link_exe)) = winget_style_layout(root.path()) else {
            return;
        };

        assert_eq!(
            resolve(&link_exe).parent().unwrap(),
            resolve(&package_exe).parent().unwrap(),
            "both launch paths must place companion files in the same directory"
        );
    }

    #[test]
    fn unresolved_paths_are_returned_unchanged() {
        let root = tempfile::tempdir().unwrap();
        let missing = root.path().join("dcc-mcp-cli.exe");

        assert_eq!(resolve(&missing), missing);
    }

    #[cfg(windows)]
    #[test]
    fn windows_verbatim_prefix_is_stripped() {
        assert_eq!(
            without_verbatim_prefix(Path::new(r"\\?\C:\Tools\dcc-mcp-cli.exe")),
            PathBuf::from(r"C:\Tools\dcc-mcp-cli.exe")
        );
        assert_eq!(
            without_verbatim_prefix(Path::new(r"\\?\UNC\server\share\dcc-mcp-cli.exe")),
            PathBuf::from(r"\\server\share\dcc-mcp-cli.exe")
        );
        assert_eq!(
            without_verbatim_prefix(Path::new(r"C:\Tools\dcc-mcp-cli.exe")),
            PathBuf::from(r"C:\Tools\dcc-mcp-cli.exe")
        );
    }

    #[cfg(windows)]
    #[test]
    fn non_utf8_paths_keep_the_canonicalized_value() {
        // Regression: `to_string_lossy()` would replace the lone surrogate
        // with U+FFFD, and `strip_prefix` would still match — producing a
        // path that does not exist, i.e. the very misplacement this module
        // prevents. A path that cannot be read losslessly is left alone.
        use std::os::windows::ffi::OsStringExt;

        // "C:\" + lone surrogate + "dcc-mcp-cli.exe"
        let mut wide: Vec<u16> = "C:\\".encode_utf16().collect();
        wide.push(0xD800);
        wide.extend("dcc-mcp-cli.exe".encode_utf16());
        let path = PathBuf::from(std::ffi::OsString::from_wide(&wide));

        assert!(path.to_str().is_none(), "test needs a non-UTF-8 path");
        assert_eq!(without_verbatim_prefix(&path), path);
    }
}
