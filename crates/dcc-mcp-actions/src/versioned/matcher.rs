//! Strategy trait + per-shape matchers backing [`VersionConstraint`] (#493).
//!
//! Every constraint shape (`*`, `=X.Y.Z`, `>=X.Y.Z`, `^X.Y.Z`, …) is
//! represented by a small struct implementing [`VersionMatcher`].
//! [`VersionConstraint::matches`](super::VersionConstraint::matches) and
//! `Display` both go through [`VersionConstraint::with_matcher`], so the
//! constraint's *behaviour* lives next to the data it operates on.
//! Adding a new shape then requires:
//!
//! 1. a new enum variant on [`VersionConstraint`](super::VersionConstraint),
//! 2. a new matcher struct + `impl VersionMatcher`,
//! 3. one extra arm in `with_matcher`.
//!
//! `matches` and `Display` need no edits at all.

use std::fmt;

use super::SemVer;

/// Strategy trait for constraint matching + rendering.
///
/// Implemented by every per-shape matcher in this module. Public so
/// downstream crates can wrap a custom matcher in
/// [`VersionConstraint::Custom`](super::VersionConstraint) without
/// touching upstream code.
pub trait VersionMatcher: fmt::Debug + fmt::Display + Send + Sync {
    /// Does `version` satisfy the constraint?
    fn matches(&self, version: SemVer) -> bool;
}

// ── Per-shape matchers ────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnyMatcher;
impl fmt::Display for AnyMatcher {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("*")
    }
}
impl VersionMatcher for AnyMatcher {
    fn matches(&self, _: SemVer) -> bool {
        true
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExactMatcher(pub SemVer);
impl fmt::Display for ExactMatcher {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "={}", self.0)
    }
}
impl VersionMatcher for ExactMatcher {
    fn matches(&self, v: SemVer) -> bool {
        v == self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AtLeastMatcher(pub SemVer);
impl fmt::Display for AtLeastMatcher {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, ">={}", self.0)
    }
}
impl VersionMatcher for AtLeastMatcher {
    fn matches(&self, v: SemVer) -> bool {
        v >= self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GreaterThanMatcher(pub SemVer);
impl fmt::Display for GreaterThanMatcher {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, ">{}", self.0)
    }
}
impl VersionMatcher for GreaterThanMatcher {
    fn matches(&self, v: SemVer) -> bool {
        v > self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AtMostMatcher(pub SemVer);
impl fmt::Display for AtMostMatcher {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "<={}", self.0)
    }
}
impl VersionMatcher for AtMostMatcher {
    fn matches(&self, v: SemVer) -> bool {
        v <= self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LessThanMatcher(pub SemVer);
impl fmt::Display for LessThanMatcher {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "<{}", self.0)
    }
}
impl VersionMatcher for LessThanMatcher {
    fn matches(&self, v: SemVer) -> bool {
        v < self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CaretMatcher(pub SemVer);
impl fmt::Display for CaretMatcher {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "^{}", self.0)
    }
}
impl VersionMatcher for CaretMatcher {
    fn matches(&self, v: SemVer) -> bool {
        v.major == self.0.major && v >= self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TildeMatcher(pub SemVer);
impl fmt::Display for TildeMatcher {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "~{}", self.0)
    }
}
impl VersionMatcher for TildeMatcher {
    fn matches(&self, v: SemVer) -> bool {
        v.major == self.0.major && v.minor == self.0.minor && v.patch >= self.0.patch
    }
}
