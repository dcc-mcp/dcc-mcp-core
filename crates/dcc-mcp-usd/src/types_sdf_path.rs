//! `SdfPath` — USD scene description path (e.g. `/World/Cube`).

use serde::{Deserialize, Serialize};

/// A USD scene description path (e.g. `/World/Cube`, `/Root`).
///
/// USD paths use forward slashes and start with `/` for absolute paths.
/// Relative paths are also supported (no leading `/`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SdfPath(String);

impl SdfPath {
    /// The absolute root path (`/`).
    pub const ROOT: &'static str = "/";

    /// Create a new `SdfPath`.  Returns an error if the path is empty.
    pub fn new(path: impl Into<String>) -> crate::UsdResult<Self> {
        let s = path.into();
        if s.is_empty() {
            return Err(crate::UsdError::InvalidPath(
                "path must not be empty".to_string(),
            ));
        }
        Ok(Self(s))
    }

    /// Create the absolute root path `/`.
    pub fn root() -> Self {
        Self("/".to_string())
    }

    /// Return the string representation.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Whether this is an absolute path (starts with `/`).
    pub fn is_absolute(&self) -> bool {
        self.0.starts_with('/')
    }

    /// Return the parent path.  Returns `None` for the root path.
    pub fn parent(&self) -> Option<Self> {
        if self.0 == "/" {
            return None;
        }
        let idx = self.0.rfind('/')?;
        if idx == 0 {
            Some(Self("/".to_string()))
        } else {
            Some(Self(self.0[..idx].to_string()))
        }
    }

    /// Append a child segment to this path.
    ///
    /// ```
    /// use dcc_mcp_usd::types::SdfPath;
    /// let root = SdfPath::new("/World").unwrap();
    /// let child = root.child("Cube").unwrap();
    /// assert_eq!(child.as_str(), "/World/Cube");
    /// ```
    pub fn child(&self, name: &str) -> crate::UsdResult<Self> {
        if name.is_empty() {
            return Err(crate::UsdError::InvalidPath(
                "child name must not be empty".to_string(),
            ));
        }
        if self.0.ends_with('/') {
            Ok(Self(format!("{}{}", self.0, name)))
        } else {
            Ok(Self(format!("{}/{}", self.0, name)))
        }
    }

    /// Return the last path element name.
    ///
    /// For `/World/Cube` returns `"Cube"`.
    /// For `/` returns `""`.
    pub fn name(&self) -> &str {
        match self.0.rfind('/') {
            Some(idx) if idx < self.0.len() - 1 => &self.0[idx + 1..],
            _ => "",
        }
    }
}

impl std::fmt::Display for SdfPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
