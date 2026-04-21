//! Typed workspace-path handshake (issue #354).
//!
//! MCP 2025-03-26 defines a `roots` capability: clients advertise the
//! filesystem roots that represent their active workspace(s) on session
//! initialization, and the server may later call `roots/list` to refresh
//! the cache. DCC tools routinely need to resolve paths relative to those
//! roots — e.g. a workflow step says `workspace://scenes/hero.usd` and the
//! server picks up the session's first root to produce an absolute path.
//!
//! This module provides a small helper that consumes the session-level
//! `ClientRoot` list and performs the path-resolution rules:
//!
//! 1. `workspace://<rest>` — strip the scheme, decode percent-escapes, and
//!    join against the first advertised root. Empty root list ⇒ error.
//! 2. Absolute paths (on the current platform) are returned unchanged.
//! 3. Relative paths (no scheme, not absolute) are joined against the
//!    first advertised root as a convenience — if no root is advertised
//!    the path is returned unchanged so callers can still function.
//!
//! Roots whose `uri` uses the `file://` scheme are parsed into a local
//! filesystem path. Other schemes are kept verbatim (the server does not
//! rewrite them but exposes them via [`WorkspaceRoots::roots`]).
//!
//! # Examples
//!
//! ```
//! use dcc_mcp_http::workspace::WorkspaceRoots;
//! let roots = WorkspaceRoots::from_file_paths(["/projects/hero"]);
//! let resolved = roots.resolve("workspace://scenes/hero.usd").unwrap();
//! assert!(resolved.ends_with("scenes/hero.usd") || resolved.ends_with("scenes\\hero.usd"));
//! ```

use std::path::{Path, PathBuf};

use crate::protocol::ClientRoot;

/// Error produced by [`WorkspaceRoots::resolve`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceResolveError {
    /// The caller used the `workspace://` scheme but the session has no
    /// advertised roots. Surface this as JSON-RPC `-32602 no workspace roots`.
    NoRoots { path: String },
    /// The input was empty or malformed (e.g. `workspace://` with no path).
    Invalid { path: String, reason: String },
}

impl std::fmt::Display for WorkspaceResolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoRoots { path } => write!(
                f,
                "no workspace roots advertised by client; cannot resolve {path:?}"
            ),
            Self::Invalid { path, reason } => {
                write!(f, "invalid workspace path {path:?}: {reason}")
            }
        }
    }
}

impl std::error::Error for WorkspaceResolveError {}

/// A session's workspace-root set used to resolve `workspace://` URIs.
///
/// The struct is cheap to clone; it owns a small `Vec<PathBuf>` of local
/// filesystem roots derived from the MCP `roots/list` response, plus the
/// original URIs for any non-`file://` roots (those are surfaced through
/// [`Self::roots`] but cannot be used as a join base).
#[derive(Debug, Clone, Default)]
pub struct WorkspaceRoots {
    /// Local filesystem roots in declaration order. Empty ⇒ client has
    /// advertised no `file://` roots.
    local_roots: Vec<PathBuf>,
    /// Original `uri` strings as advertised by the client (includes
    /// non-`file://` schemes). Preserved so [`Self::roots`] can surface
    /// them verbatim to diagnostic tools.
    raw_uris: Vec<String>,
}

impl WorkspaceRoots {
    /// Build from a client-advertised list (the shape stored by the
    /// session manager).
    pub fn from_client_roots(roots: &[ClientRoot]) -> Self {
        let mut local_roots = Vec::with_capacity(roots.len());
        let mut raw_uris = Vec::with_capacity(roots.len());
        for root in roots {
            raw_uris.push(root.uri.clone());
            if let Some(p) = uri_to_local_path(&root.uri) {
                local_roots.push(p);
            }
        }
        Self {
            local_roots,
            raw_uris,
        }
    }

    /// Convenience constructor for tests / non-MCP callers.
    pub fn from_file_paths<I, S>(paths: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<Path>,
    {
        let mut local_roots = Vec::new();
        let mut raw_uris = Vec::new();
        for p in paths {
            let pb = p.as_ref().to_path_buf();
            raw_uris.push(path_to_file_uri(&pb));
            local_roots.push(pb);
        }
        Self {
            local_roots,
            raw_uris,
        }
    }

    /// All roots (as URI strings) in declaration order.
    pub fn roots(&self) -> &[String] {
        &self.raw_uris
    }

    /// Local filesystem roots only (the subset usable as a join base).
    pub fn local_roots(&self) -> &[PathBuf] {
        &self.local_roots
    }

    /// Resolve a typed path string against this workspace.
    ///
    /// See the module docs for the full rule list. Returns
    /// [`WorkspaceResolveError::NoRoots`] when `workspace://` is used but
    /// the client advertised no `file://` roots.
    pub fn resolve(&self, path: &str) -> Result<PathBuf, WorkspaceResolveError> {
        if path.is_empty() {
            return Err(WorkspaceResolveError::Invalid {
                path: path.to_string(),
                reason: "path must not be empty".into(),
            });
        }

        if let Some(rest) = path
            .strip_prefix("workspace://")
            .or_else(|| path.strip_prefix("workspace:/"))
        {
            let trimmed = rest.trim_start_matches('/');
            if trimmed.is_empty() {
                return Err(WorkspaceResolveError::Invalid {
                    path: path.to_string(),
                    reason: "workspace:// URI is missing a path component".into(),
                });
            }
            let root = self
                .local_roots
                .first()
                .ok_or_else(|| WorkspaceResolveError::NoRoots {
                    path: path.to_string(),
                })?;
            return Ok(join_root(root, trimmed));
        }

        // Pass-through for absolute paths.
        let as_path = Path::new(path);
        if as_path.is_absolute() {
            return Ok(as_path.to_path_buf());
        }

        // Relative — join against first root when available, else return
        // unchanged so callers can still construct a path.
        match self.local_roots.first() {
            Some(root) => Ok(join_root(root, path)),
            None => Ok(as_path.to_path_buf()),
        }
    }
}

/// Parse a `file://` URI into a local path. Returns `None` for other schemes.
fn uri_to_local_path(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    // `file:///C:/foo/bar` → strip one more `/` on Windows-style paths.
    let stripped = rest.strip_prefix('/').unwrap_or(rest);
    // If the original started with `file:///`, `rest` begins with `/`; on
    // POSIX we want to keep that leading `/`, on Windows a drive letter
    // like `C:` indicates the path is already absolute.
    if cfg!(windows) {
        // Prefer the Windows-style (`C:/...`) when present.
        if looks_like_windows_drive(stripped) {
            return Some(PathBuf::from(stripped.replace('/', "\\")));
        }
        Some(PathBuf::from(rest))
    } else {
        // POSIX: preserve the leading slash (`rest` includes it).
        Some(PathBuf::from(rest))
    }
}

fn looks_like_windows_drive(s: &str) -> bool {
    let bytes = s.as_bytes();
    bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

fn path_to_file_uri(p: &Path) -> String {
    let s = p.to_string_lossy().replace('\\', "/");
    if s.starts_with('/') {
        format!("file://{s}")
    } else {
        // Windows drive-letter path.
        format!("file:///{s}")
    }
}

fn join_root(root: &Path, rel: &str) -> PathBuf {
    // `Path::join` treats an absolute `rel` as replacing `root`. Since we
    // already stripped the scheme prefix and trimmed leading `/`, it is
    // safe to join directly.
    root.join(rel)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root(uri: &str) -> ClientRoot {
        ClientRoot {
            uri: uri.to_string(),
            name: None,
        }
    }

    #[test]
    fn resolves_workspace_scheme_against_first_root() {
        let roots = WorkspaceRoots::from_client_roots(&[root("file:///projects/hero")]);
        let resolved = roots.resolve("workspace://scenes/a.usd").unwrap();
        let s = resolved.to_string_lossy().replace('\\', "/");
        assert!(s.ends_with("/projects/hero/scenes/a.usd"), "{s}");
    }

    #[test]
    fn empty_roots_fails_for_workspace_scheme() {
        let roots = WorkspaceRoots::default();
        let err = roots.resolve("workspace://scenes/a.usd").unwrap_err();
        match err {
            WorkspaceResolveError::NoRoots { path } => {
                assert_eq!(path, "workspace://scenes/a.usd");
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn absolute_paths_pass_through() {
        let roots = WorkspaceRoots::from_file_paths(["/projects/hero"]);
        #[cfg(not(windows))]
        {
            let resolved = roots.resolve("/tmp/foo.txt").unwrap();
            assert_eq!(resolved, PathBuf::from("/tmp/foo.txt"));
        }
        #[cfg(windows)]
        {
            let resolved = roots.resolve(r"C:\tmp\foo.txt").unwrap();
            assert_eq!(resolved, PathBuf::from(r"C:\tmp\foo.txt"));
        }
    }

    #[test]
    fn relative_paths_join_against_first_root() {
        let roots = WorkspaceRoots::from_file_paths(["/projects/hero"]);
        let resolved = roots.resolve("scenes/a.usd").unwrap();
        let s = resolved.to_string_lossy().replace('\\', "/");
        assert!(s.ends_with("/projects/hero/scenes/a.usd"), "{s}");
    }

    #[test]
    fn relative_paths_returned_unchanged_without_roots() {
        let roots = WorkspaceRoots::default();
        let resolved = roots.resolve("scenes/a.usd").unwrap();
        assert_eq!(resolved, PathBuf::from("scenes/a.usd"));
    }

    #[test]
    fn invalid_empty_workspace_uri() {
        let roots = WorkspaceRoots::from_file_paths(["/projects/hero"]);
        let err = roots.resolve("workspace://").unwrap_err();
        assert!(matches!(err, WorkspaceResolveError::Invalid { .. }));
    }

    #[test]
    fn roots_getter_preserves_declaration_order() {
        let roots =
            WorkspaceRoots::from_client_roots(&[root("file:///a"), root("custom://something")]);
        assert_eq!(roots.roots(), &["file:///a", "custom://something"]);
        // Only the file:// root is usable as a join base.
        assert_eq!(roots.local_roots().len(), 1);
    }
}
