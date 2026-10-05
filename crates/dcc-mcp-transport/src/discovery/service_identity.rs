//! Process identity and lineage envelope for registry rows (RFC-0007 §3.2).
//!
//! A [`ServiceEntry`](super::types::ServiceEntry) identifies a service by
//! `(dcc_type, instance_id)` and carries the owner `pid`, but nothing explains
//! where a row came from: whether two rows belong to the same launch, whether a
//! row is the real DCC host or a wrapper around it, and when the process
//! started as opposed to when it registered.
//!
//! The types here are the additive envelope that answers those questions. Every
//! part is optional and skipped when unset, so rows written by adapters that
//! predate the envelope keep their exact current wire shape, and readers built
//! before the envelope keep working unchanged.

use std::fmt;
use std::str::FromStr;
use std::sync::OnceLock;
use std::time::SystemTime;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use uuid::Uuid;

/// Environment variable a launcher sets so every process it spawns inherits the
/// same launch id.
///
/// Producers read it with [`launch_id_from_env`]. Children inherit it for free
/// because the environment carries across `CreateProcess` / `execve`, so a
/// launcher only sets it once for the whole process tree.
pub const LAUNCH_ID_ENV_VAR: &str = "DCC_MCP_LAUNCH_ID";

/// Role of the process that owns a registry row.
///
/// Serialised as a plain string (`"host"` / `"launcher"` / `"sidecar"`). The
/// three known roles match case-insensitively; any other value deserialises
/// into [`ServiceRole::Custom`] and round-trips verbatim — only surrounding
/// whitespace is trimmed. A newer producer publishing `"Gateway-Sidecar"`
/// therefore never makes an older reader fail or silently rewrite the row.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ServiceRole {
    /// The DCC application process itself (embedded adapter / plugin).
    Host,
    /// A wrapper that starts other DCC-MCP processes and may exit before them.
    Launcher,
    /// A helper process that watches a DCC host on its behalf.
    Sidecar,
    /// Any other role; preserved verbatim for forward compatibility.
    Custom(String),
}

impl ServiceRole {
    /// Wire value of this role.
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::Host => "host",
            Self::Launcher => "launcher",
            Self::Sidecar => "sidecar",
            Self::Custom(value) => value.as_str(),
        }
    }
}

/// Map a raw role string onto a variant.
///
/// Known roles are matched on the trimmed, lowercased value so producers can
/// spell them however they like. Everything else keeps its original spelling,
/// which is what makes a producer-defined role survive a `services.json`
/// round-trip unchanged.
fn from_raw(raw: &str) -> ServiceRole {
    let trimmed = raw.trim();
    match trimmed.to_ascii_lowercase().as_str() {
        "host" => ServiceRole::Host,
        "launcher" => ServiceRole::Launcher,
        "sidecar" => ServiceRole::Sidecar,
        _ => ServiceRole::Custom(trimmed.to_string()),
    }
}

impl From<&str> for ServiceRole {
    fn from(raw: &str) -> Self {
        from_raw(raw)
    }
}

impl From<String> for ServiceRole {
    fn from(raw: String) -> Self {
        from_raw(&raw)
    }
}

impl FromStr for ServiceRole {
    type Err = std::convert::Infallible;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        Ok(Self::from(raw))
    }
}

impl fmt::Display for ServiceRole {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for ServiceRole {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ServiceRole {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        Ok(Self::from(raw))
    }
}

/// Launch id published by the launching process, when there is one.
#[must_use]
pub fn launch_id_from_env() -> Option<String> {
    launch_id_from_env_value(std::env::var(LAUNCH_ID_ENV_VAR).ok().as_deref())
}

/// Normalise a raw [`LAUNCH_ID_ENV_VAR`] value; `None` when it carries no id.
///
/// Empty and whitespace-only values degrade to *unknown* so a launcher that
/// exports the variable as `""` cannot collapse unrelated rows into one launch.
#[must_use]
pub fn launch_id_from_env_value(raw: Option<&str>) -> Option<String> {
    let trimmed = raw?.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

/// Launch id for this process: the inherited one, or a generated id cached for
/// the lifetime of the process.
///
/// Every row this process registers shares the value, which is what makes
/// "same launch" decidable from the registry alone — no live process tree
/// inspection required.
#[must_use]
pub fn process_launch_id() -> String {
    static LAUNCH_ID: OnceLock<String> = OnceLock::new();
    LAUNCH_ID
        .get_or_init(|| launch_id_from_env().unwrap_or_else(|| Uuid::new_v4().to_string()))
        .clone()
}

/// Seconds since the Unix epoch, for JSON projections that must stay
/// platform-neutral (the std `SystemTime` serde shape is Rust-specific).
#[must_use]
pub fn system_time_to_unix_secs(time: SystemTime) -> Option<u64> {
    time.duration_since(SystemTime::UNIX_EPOCH)
        .ok()
        .map(|elapsed| elapsed.as_secs())
}

#[cfg(test)]
#[path = "service_identity_tests.rs"]
mod tests;
