//! Sandbox policy: API whitelist, path allowlist, execution constraints.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::SandboxError;

// ── Execution Mode ────────────────────────────────────────────────────────────

/// Whether the sandbox allows write operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionMode {
    /// Full read+write access (within other policy constraints).
    #[default]
    ReadWrite,
    /// Query-only mode; any operation tagged as a write is blocked.
    ReadOnly,
}

// ── SandboxPolicy ─────────────────────────────────────────────────────────────

/// Complete set of constraints applied to one sandbox context.
///
/// Use [`SandboxPolicy::builder`] to construct incrementally.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxPolicy {
    /// If `Some`, only actions explicitly listed here are allowed.
    /// If `None`, all actions are allowed (subject to other constraints).
    pub allowed_actions: Option<HashSet<String>>,

    /// Actions that are always denied, regardless of `allowed_actions`.
    pub denied_actions: HashSet<String>,

    /// Directories scripts may read/write. Empty = unrestricted.
    pub allowed_paths: Vec<PathBuf>,

    /// Maximum wall-clock time for a single execution (milliseconds).
    /// `None` means no time limit.
    pub timeout_ms: Option<u64>,

    /// Maximum number of actions callable in a single execution session.
    /// `None` means unlimited.
    pub max_actions: Option<u32>,

    /// Read-only flag: write operations are rejected at the policy level.
    pub mode: ExecutionMode,
}

impl Default for SandboxPolicy {
    fn default() -> Self {
        Self {
            allowed_actions: None,
            denied_actions: HashSet::new(),
            allowed_paths: Vec::new(),
            timeout_ms: None,
            max_actions: None,
            mode: ExecutionMode::ReadWrite,
        }
    }
}

// ── Path helpers ──────────────────────────────────────────────────────────────

/// Canonicalize `path`, falling back to the nearest existing ancestor when the
/// path does not exist (e.g. a planned output file that hasn't been created).
///
/// This ensures that Windows long/short path normalization is applied to the
/// portion of the path that does exist, avoiding false negatives when comparing
/// against `allowed_paths` that were themselves canonicalized.
fn canonicalize_best_effort(path: &Path) -> std::path::PathBuf {
    if let Ok(p) = path.canonicalize() {
        return p;
    }
    // Walk up the ancestor chain until we find something that exists.
    let mut ancestor = path.to_path_buf();
    let mut suffix = std::path::PathBuf::new();
    while let Some(parent) = ancestor.parent().map(|p| p.to_path_buf()) {
        let file_name = ancestor
            .file_name()
            .map(std::path::PathBuf::from)
            .unwrap_or_default();
        suffix = file_name.join(&suffix);
        ancestor = parent;
        if ancestor.exists() {
            if let Ok(canonical_ancestor) = ancestor.canonicalize() {
                return canonical_ancestor.join(suffix);
            }
            break;
        }
    }
    // Last resort: return the original path unchanged.
    path.to_path_buf()
}

impl SandboxPolicy {
    /// Return a builder for incremental construction.
    pub fn builder() -> SandboxPolicyBuilder {
        SandboxPolicyBuilder::new()
    }

    // ── Validation helpers ────────────────────────────────────────────────────

    /// Check whether `action` is permitted by this policy.
    ///
    /// Returns `Ok(())` when allowed, `Err(SandboxError)` when denied.
    pub fn check_action(&self, action: &str) -> Result<(), SandboxError> {
        if self.denied_actions.contains(action) {
            return Err(SandboxError::ActionNotAllowed {
                action: action.to_owned(),
            });
        }
        if let Some(ref allowed) = self.allowed_actions
            && !allowed.contains(action)
        {
            return Err(SandboxError::ActionNotAllowed {
                action: action.to_owned(),
            });
        }
        Ok(())
    }

    /// Check whether `path` is within one of the allowed directories.
    ///
    /// Returns `Ok(())` if `allowed_paths` is empty (unrestricted) or if
    /// `path` starts with at least one allowed directory.
    pub fn check_path(&self, path: &Path) -> Result<(), SandboxError> {
        if self.allowed_paths.is_empty() {
            return Ok(());
        }
        // Try to canonicalize the path.  For non-existent paths (e.g. a file
        // that hasn't been created yet), fall back to canonicalizing the
        // nearest existing ancestor so that Windows short-path / symlink
        // normalization still applies.
        let canonical = canonicalize_best_effort(path);
        for allowed in &self.allowed_paths {
            let allowed_canonical = match allowed.canonicalize() {
                Ok(p) => p,
                Err(_) => allowed.clone(),
            };
            if canonical.starts_with(&allowed_canonical) {
                return Ok(());
            }
        }
        Err(SandboxError::PathNotAllowed {
            path: path.display().to_string(),
        })
    }

    /// Check whether a write operation is permitted in the current mode.
    pub fn check_write(&self, operation: &str) -> Result<(), SandboxError> {
        if self.mode == ExecutionMode::ReadOnly {
            return Err(SandboxError::ReadOnlyViolation {
                operation: operation.to_owned(),
            });
        }
        Ok(())
    }
}

// ── Builder ───────────────────────────────────────────────────────────────────

/// Fluent builder for [`SandboxPolicy`].
#[derive(Debug, Default)]
pub struct SandboxPolicyBuilder {
    inner: SandboxPolicy,
}

impl SandboxPolicyBuilder {
    /// Create a new builder with default policy.
    pub fn new() -> Self {
        Self {
            inner: SandboxPolicy::default(),
        }
    }

    /// Restrict execution to only these actions.
    pub fn allow_actions<I, S>(mut self, actions: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let set: HashSet<String> = actions.into_iter().map(Into::into).collect();
        self.inner.allowed_actions = Some(set);
        self
    }

    /// Always deny these actions even if present in the allowlist.
    pub fn deny_actions<I, S>(mut self, actions: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.inner
            .denied_actions
            .extend(actions.into_iter().map(Into::into));
        self
    }

    /// Allow scripts to access files inside these directories.
    pub fn allow_paths<I, P>(mut self, paths: I) -> Self
    where
        I: IntoIterator<Item = P>,
        P: Into<PathBuf>,
    {
        self.inner
            .allowed_paths
            .extend(paths.into_iter().map(Into::into));
        self
    }

    /// Set execution timeout in milliseconds.
    pub fn timeout_ms(mut self, ms: u64) -> Self {
        self.inner.timeout_ms = Some(ms);
        self
    }

    /// Set maximum number of actions per session.
    pub fn max_actions(mut self, count: u32) -> Self {
        self.inner.max_actions = Some(count);
        self
    }

    /// Set execution mode (ReadWrite or ReadOnly).
    pub fn mode(mut self, mode: ExecutionMode) -> Self {
        self.inner.mode = mode;
        self
    }

    /// Build the final [`SandboxPolicy`].
    pub fn build(self) -> SandboxPolicy {
        self.inner
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    mod test_check_action {
        use super::*;

        #[test]
        fn allow_all_when_no_whitelist() {
            let policy = SandboxPolicy::builder().build();
            assert!(policy.check_action("anything").is_ok());
        }

        #[test]
        fn whitelist_permits_listed_action() {
            let policy = SandboxPolicy::builder()
                .allow_actions(["get_scene_info", "list_objects"])
                .build();
            assert!(policy.check_action("get_scene_info").is_ok());
            assert!(policy.check_action("list_objects").is_ok());
        }

        #[test]
        fn whitelist_blocks_unlisted_action() {
            let policy = SandboxPolicy::builder()
                .allow_actions(["get_scene_info"])
                .build();
            assert!(matches!(
                policy.check_action("delete_scene"),
                Err(SandboxError::ActionNotAllowed { .. })
            ));
        }

        #[test]
        fn deny_list_overrides_whitelist() {
            let policy = SandboxPolicy::builder()
                .allow_actions(["get_scene_info", "delete_scene"])
                .deny_actions(["delete_scene"])
                .build();
            assert!(policy.check_action("get_scene_info").is_ok());
            assert!(matches!(
                policy.check_action("delete_scene"),
                Err(SandboxError::ActionNotAllowed { .. })
            ));
        }

        #[test]
        fn deny_list_blocks_even_without_whitelist() {
            let policy = SandboxPolicy::builder()
                .deny_actions(["dangerous_op"])
                .build();
            assert!(matches!(
                policy.check_action("dangerous_op"),
                Err(SandboxError::ActionNotAllowed { .. })
            ));
        }
    }

    mod test_check_path {
        use super::*;
        use std::env;

        #[test]
        fn unrestricted_when_no_allowed_paths() {
            let policy = SandboxPolicy::builder().build();
            assert!(policy.check_path(Path::new("/any/path")).is_ok());
        }

        #[test]
        fn allows_path_within_allowed_dir() {
            let tmp = env::temp_dir();
            // Use a path that actually exists (the temp dir itself is always
            // present) so that canonicalize() succeeds on both Unix and Windows.
            let child = tmp.clone();
            let policy = SandboxPolicy::builder().allow_paths([&tmp]).build();
            assert!(policy.check_path(&child).is_ok());
        }

        #[test]
        fn blocks_path_outside_allowed_dirs() {
            let tmp = env::temp_dir();
            let policy = SandboxPolicy::builder().allow_paths([&tmp]).build();
            assert!(matches!(
                policy.check_path(Path::new("/etc/passwd")),
                Err(SandboxError::PathNotAllowed { .. })
            ));
        }
    }

    mod test_check_write {
        use super::*;

        #[test]
        fn write_allowed_in_readwrite_mode() {
            let policy = SandboxPolicy::builder()
                .mode(ExecutionMode::ReadWrite)
                .build();
            assert!(policy.check_write("create_mesh").is_ok());
        }

        #[test]
        fn write_blocked_in_readonly_mode() {
            let policy = SandboxPolicy::builder()
                .mode(ExecutionMode::ReadOnly)
                .build();
            assert!(matches!(
                policy.check_write("create_mesh"),
                Err(SandboxError::ReadOnlyViolation { .. })
            ));
        }
    }

    mod test_builder {
        use super::*;

        #[test]
        fn default_policy_has_no_restrictions() {
            let policy = SandboxPolicy::default();
            assert!(policy.allowed_actions.is_none());
            assert!(policy.denied_actions.is_empty());
            assert!(policy.allowed_paths.is_empty());
            assert!(policy.timeout_ms.is_none());
            assert!(policy.max_actions.is_none());
            assert_eq!(policy.mode, ExecutionMode::ReadWrite);
        }

        #[test]
        fn builder_sets_all_fields() {
            let tmp = std::env::temp_dir();
            let policy = SandboxPolicy::builder()
                .allow_actions(["op_a", "op_b"])
                .deny_actions(["op_c"])
                .allow_paths([tmp.clone()])
                .timeout_ms(3000)
                .max_actions(50)
                .mode(ExecutionMode::ReadOnly)
                .build();

            assert!(policy.allowed_actions.as_ref().unwrap().contains("op_a"));
            assert!(policy.denied_actions.contains("op_c"));
            assert_eq!(policy.allowed_paths, vec![tmp]);
            assert_eq!(policy.timeout_ms, Some(3000));
            assert_eq!(policy.max_actions, Some(50));
            assert_eq!(policy.mode, ExecutionMode::ReadOnly);
        }
    }
}
