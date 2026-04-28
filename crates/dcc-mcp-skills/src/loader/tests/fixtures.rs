//! Shared test fixtures for loader tests.
//!
//! [`SkillTestFixture`] wraps a `tempfile::TempDir` and exposes a
//! `skill_path: PathBuf` pointing at the temporary directory.  Use
//! it wherever tests previously created a temp-dir and wrote a
//! `SKILL.md` by hand.

use crate::constants::SKILL_METADATA_FILE;
use std::path::{Path, PathBuf};

/// A temporary skill directory for use in loader tests.
///
/// The inner `TempDir` is kept alive for the lifetime of the fixture;
/// dropping the fixture deletes the directory.
pub struct SkillTestFixture {
    /// Keep the `TempDir` alive so the directory is not cleaned up early.
    pub _dir: tempfile::TempDir,
    /// Path to the root of the skill directory.
    pub skill_path: PathBuf,
}

impl SkillTestFixture {
    /// An empty temp dir — no files at all.
    pub fn empty() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let skill_path = dir.path().to_path_buf();
        Self {
            _dir: dir,
            skill_path,
        }
    }

    /// A temp dir containing a `SKILL.md` with the given raw body.
    pub fn with_body(body: &str) -> Self {
        let fx = Self::empty();
        std::fs::write(fx.skill_path.join(SKILL_METADATA_FILE), body).unwrap();
        fx
    }

    /// The path to the skill directory.
    pub fn path(&self) -> &Path {
        &self.skill_path
    }

    /// Write a file at `rel` (relative to the skill dir), creating
    /// intermediate directories as needed.
    pub fn write_file(&self, rel: &str, content: &str) {
        let dest = self.skill_path.join(rel);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(dest, content).unwrap();
    }
}

// ── Standalone helpers used by metadata-compat test files ──────────────────

/// Write a minimal `SKILL.md` into `skill_dir`, creating the directory if
/// it does not exist.  Used by `test_metadata_compat`, `test_next_tools`,
/// and `test_layer_field`.
pub fn write_skill(skill_dir: &Path, body: &str) {
    std::fs::create_dir_all(skill_dir).unwrap();
    std::fs::write(skill_dir.join(SKILL_METADATA_FILE), body).unwrap();
}
