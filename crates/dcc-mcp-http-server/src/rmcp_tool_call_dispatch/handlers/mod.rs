mod discovery;
mod dynamic;
mod jobs;
mod skills;

pub(super) use discovery::*;
pub(super) use dynamic::*;
pub(super) use jobs::*;
pub(super) use skills::*;

// `_tests.rs` suffix: these are test-only modules, so the file-size gate treats
// them as test files (2000 lines) rather than production Rust (1500).
#[cfg(test)]
mod skills_fallback_tests;
