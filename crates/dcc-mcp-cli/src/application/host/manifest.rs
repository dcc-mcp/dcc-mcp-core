//! Host manifest: the single source of truth for host versions.
//!
//! The bundled `assets/hosts.yml` is compiled into the binary so probing works
//! offline and cannot be perturbed by a stale cache. A user-level lock file can
//! pin versions on top of it; nothing else in the process is allowed to supply
//! a version. That rule comes from PIP-2387, where a version carried in context
//! rather than read from the manifest shipped a mismatched artifact.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// Host manifest bundled with the CLI.
pub const BUNDLED_HOSTS_YAML: &str = include_str!("../../../assets/hosts.yml");

/// Current manifest schema version.
pub const MANIFEST_VERSION: &str = "1";

/// Manifest-level failures. Every one of them is fatal: a partially loaded
/// manifest would let `install` act on a host it should refuse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManifestError {
    /// YAML could not be parsed.
    Parse(String),
    /// The schema version is not one this build understands.
    UnsupportedVersion { found: String },
    /// Two entries claim the same id.
    DuplicateHost { id: String },
    /// A commercial host declared `self_provision: true`.
    ProvisionOnCommercial { id: String },
    /// An entry declares no executable for any platform.
    NoExecutables { id: String },
    /// A self-provisioning host has no pinned version to install.
    MissingPinnedVersion { id: String },
    /// A pinned or minimum version is not a dotted number.
    InvalidVersion { id: String, version: String },
    /// The lock file could not be read.
    LockUnreadable { path: String, detail: String },
}

impl std::fmt::Display for ManifestError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Parse(detail) => write!(formatter, "host manifest is not valid YAML: {detail}"),
            Self::UnsupportedVersion { found } => write!(
                formatter,
                "host manifest version '{found}' is unsupported; expected '{MANIFEST_VERSION}'"
            ),
            Self::DuplicateHost { id } => {
                write!(formatter, "host manifest declares host '{id}' twice")
            }
            Self::ProvisionOnCommercial { id } => write!(
                formatter,
                "host '{id}' is commercial and must not declare self_provision: true"
            ),
            Self::NoExecutables { id } => {
                write!(
                    formatter,
                    "host '{id}' declares no executable for any platform"
                )
            }
            Self::MissingPinnedVersion { id } => write!(
                formatter,
                "host '{id}' declares self_provision: true but has no pinned_version"
            ),
            Self::InvalidVersion { id, version } => write!(
                formatter,
                "host '{id}' declares invalid version '{version}'"
            ),
            Self::LockUnreadable { path, detail } => {
                write!(formatter, "host lock '{path}' is unreadable: {detail}")
            }
        }
    }
}

impl std::error::Error for ManifestError {}

/// Whether a host is licence-gated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostLicense {
    /// Free and open source; may be provisioned by the CLI.
    OpenSource,
    /// Licence required; the CLI reports but never installs.
    Commercial,
}

/// Per-platform string lists (executable names, search roots).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlatformStrings {
    #[serde(default)]
    pub windows: Vec<String>,
    #[serde(default)]
    pub macos: Vec<String>,
    #[serde(default)]
    pub linux: Vec<String>,
}

impl PlatformStrings {
    /// Entries for the platform this binary was built for.
    #[must_use]
    pub fn for_current(&self) -> &[String] {
        if cfg!(windows) {
            &self.windows
        } else if cfg!(target_os = "macos") {
            &self.macos
        } else {
            &self.linux
        }
    }

    /// Whether this platform has any entry at all.
    #[must_use]
    pub fn covers_current(&self) -> bool {
        !self.for_current().is_empty()
    }
}

/// Scripted install channel for one platform. Consumed by `host install`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum InstallChannel {
    /// Windows package manager.
    Winget {
        /// Package identifier, e.g. `Blender.Blender`.
        id: Option<String>,
        /// Extra argv appended after the package id.
        #[serde(default)]
        extra_args: Vec<String>,
    },
    /// macOS Homebrew.
    Brew {
        /// Cask name, e.g. `blender`.
        cask: Option<String>,
        /// Formula name, for hosts distributed as a formula.
        formula: Option<String>,
    },
    /// Official archive with a published checksum.
    Tarball {
        /// Archive URL.
        url: String,
        /// Inline lowercase hex SHA-256, when the manifest carries it.
        sha256: Option<String>,
        /// URL serving `<hex>  <filename>` when the checksum is published apart.
        sha256_url: Option<String>,
        /// Leading path components stripped when unpacking.
        #[serde(default)]
        strip_components: usize,
    },
}

/// One host known to the CLI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostDefinition {
    /// Canonical lowercase id.
    pub id: String,
    /// Human-readable name.
    pub display_name: String,
    /// Licence class driving the provisioning redline.
    pub license: HostLicense,
    /// Whether the CLI may install this host itself.
    ///
    /// Required, deliberately: a host that omits it would inherit a default,
    /// and the safe default for a licence-gated host is not something a
    /// deserializer should guess at.
    pub self_provision: bool,
    /// Lowest version the ecosystem accepts.
    #[serde(default)]
    pub min_version: Option<String>,
    /// Exact version `host install` provisions.
    #[serde(default)]
    pub pinned_version: Option<String>,
    /// Binary names searched on PATH.
    ///
    /// Optional at parse time so a host that omits it produces the domain
    /// error `NoExecutables` rather than a serde field error.
    #[serde(default)]
    pub executables: PlatformStrings,
    /// argv used to ask the binary for its version; empty means "do not query".
    #[serde(default)]
    pub version_arg: Vec<String>,
    /// Platform install locations, supporting `*` and `?` per segment.
    #[serde(default)]
    pub search_roots: PlatformStrings,
    /// Scripted install channels by platform key (`windows` / `macos` / `linux`).
    #[serde(default)]
    pub install: BTreeMap<String, InstallChannel>,
}

impl HostDefinition {
    /// Look up the host by id, case-insensitively.
    #[must_use]
    pub fn executable_env_var(&self) -> String {
        crate::domain::host::executable_env_var(&self.id)
    }

    /// Install channel for the current platform, if one is declared.
    #[must_use]
    pub fn install_channel_for_current(&self) -> Option<&InstallChannel> {
        let key = if cfg!(windows) {
            "windows"
        } else if cfg!(target_os = "macos") {
            "macos"
        } else {
            "linux"
        };
        self.install.get(key)
    }
}

/// The parsed host manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostManifest {
    pub version: String,
    #[serde(default)]
    pub hosts: Vec<HostDefinition>,
}

impl HostManifest {
    /// Look up a host by id. Matching is case-insensitive on the manifest ids,
    /// which are already lowercase, so `Maya` and `maya` both resolve.
    #[must_use]
    pub fn find(&self, id: &str) -> Option<&HostDefinition> {
        let needle = id.trim().to_ascii_lowercase();
        self.hosts.iter().find(|host| host.id == needle)
    }

    /// Render the manifest for `host list`.
    #[must_use]
    pub fn to_list_value(&self) -> Value {
        let hosts: Vec<Value> = self
            .hosts
            .iter()
            .map(|host| {
                json!({
                    "id": host.id,
                    "display_name": host.display_name,
                    "license": host.license,
                    "self_provision": host.self_provision,
                    "min_version": host.min_version,
                    "pinned_version": host.pinned_version,
                    "platforms": host.executables.platforms(),
                    "install_channels": host.install.keys().collect::<Vec<_>>(),
                })
            })
            .collect();
        json!({ "version": self.version, "hosts": hosts })
    }
}

impl PlatformStrings {
    /// Platform keys that carry at least one entry.
    fn platforms(&self) -> Vec<&'static str> {
        let mut names = Vec::new();
        if !self.windows.is_empty() {
            names.push("windows");
        }
        if !self.macos.is_empty() {
            names.push("macos");
        }
        if !self.linux.is_empty() {
            names.push("linux");
        }
        names
    }
}

/// User-level version lock. Only pins versions; it never adds a host.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostLock {
    /// Schema version.
    #[serde(default)]
    pub version: Option<String>,
    /// `host id -> exact version`.
    #[serde(default)]
    pub pins: BTreeMap<String, String>,
}

impl HostLock {
    /// Empty lock: bundled manifest versions apply unchanged.
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }
}

/// Parse and validate the bundled manifest.
///
/// # Panics
///
/// Panics only if `assets/hosts.yml` is malformed, which is a build-time
/// defect caught by the unit tests below.
#[must_use]
pub fn bundled() -> HostManifest {
    parse(BUNDLED_HOSTS_YAML).expect("bundled host manifest must parse and validate")
}

/// Load the manifest, applying `lock` pins on top of the bundled defaults.
///
/// A pin can only change `pinned_version`; it cannot create a host, flip a
/// licence, or make a commercial host installable. That keeps the redline
/// outside the reach of a user-editable file.
pub fn load_with_lock(lock: &HostLock) -> Result<HostManifest, ManifestError> {
    let mut manifest = bundled();
    for (id, version) in &lock.pins {
        if !is_dotted_version(version) {
            return Err(ManifestError::InvalidVersion {
                id: id.clone(),
                version: version.clone(),
            });
        }
        let Some(host) = manifest.hosts.iter_mut().find(|host| host.id == *id) else {
            // An unknown id in the lock is stale, not fatal: versions drift and
            // hosts get renamed. Report it through `host list` diagnostics
            // rather than refusing to probe every other host.
            continue;
        };
        host.pinned_version = Some(version.clone());
    }
    Ok(manifest)
}

/// Read a lock file from disk. A missing file means "no pins", which is the
/// default state on every fresh machine.
pub fn load_lock(path: Option<&Path>) -> Result<HostLock, ManifestError> {
    let Some(path) = path else {
        return Ok(HostLock::empty());
    };
    if !path.exists() {
        return Ok(HostLock::empty());
    }
    let text = std::fs::read_to_string(path).map_err(|err| ManifestError::LockUnreadable {
        path: path.display().to_string(),
        detail: err.to_string(),
    })?;
    serde_yaml_ng::from_str(&text).map_err(|err| ManifestError::LockUnreadable {
        path: path.display().to_string(),
        detail: err.to_string(),
    })
}

/// Parse and validate a manifest document.
///
/// # Errors
///
/// Returns [`ManifestError`] on malformed YAML, an unknown schema version, or
/// any entry that violates the provisioning redline.
pub fn parse(text: &str) -> Result<HostManifest, ManifestError> {
    let manifest: HostManifest =
        serde_yaml_ng::from_str(text).map_err(|err| ManifestError::Parse(err.to_string()))?;
    if manifest.version != MANIFEST_VERSION {
        return Err(ManifestError::UnsupportedVersion {
            found: manifest.version,
        });
    }
    validate(&manifest)?;
    Ok(manifest)
}

/// Enforce the invariants `host install` depends on.
fn validate(manifest: &HostManifest) -> Result<(), ManifestError> {
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    for host in &manifest.hosts {
        if host.id.is_empty() {
            return Err(ManifestError::DuplicateHost {
                id: "<empty>".to_string(),
            });
        }
        if !seen.insert(host.id.as_str()) {
            return Err(ManifestError::DuplicateHost {
                id: host.id.clone(),
            });
        }
        // The redline, enforced in code rather than left to the data file:
        // licence-gated hosts are never auto-installed.
        if host.self_provision && host.license == HostLicense::Commercial {
            return Err(ManifestError::ProvisionOnCommercial {
                id: host.id.clone(),
            });
        }
        if host.executables.windows.is_empty()
            && host.executables.macos.is_empty()
            && host.executables.linux.is_empty()
        {
            return Err(ManifestError::NoExecutables {
                id: host.id.clone(),
            });
        }
        if host.self_provision && host.pinned_version.is_none() {
            return Err(ManifestError::MissingPinnedVersion {
                id: host.id.clone(),
            });
        }
        for version in [host.min_version.as_deref(), host.pinned_version.as_deref()]
            .into_iter()
            .flatten()
        {
            if !is_dotted_version(version) {
                return Err(ManifestError::InvalidVersion {
                    id: host.id.clone(),
                    version: version.to_string(),
                });
            }
        }
    }
    Ok(())
}

/// Whether `value` is `N`, `N.N` or `N.N.N`.
fn is_dotted_version(value: &str) -> bool {
    let trimmed = value.trim().trim_start_matches('v');
    if trimmed.is_empty() {
        return false;
    }
    let parts: Vec<&str> = trimmed.split('.').collect();
    if parts.len() > 3 {
        return false;
    }
    parts
        .iter()
        .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest_with(yaml: &str) -> Result<HostManifest, ManifestError> {
        parse(yaml)
    }

    fn blender_yaml(extra: &str) -> String {
        format!(
            r#"
version: "1"
hosts:
  - id: blender
    display_name: Blender
    license: open_source
    self_provision: true
    min_version: "5.1"
    pinned_version: "5.1.1"
    executables:
      windows: ["blender.exe"]
{extra}
"#
        )
    }

    #[test]
    fn bundled_manifest_is_valid() {
        let manifest = bundled();
        assert_eq!(manifest.version, "1");
        assert!(manifest.find("blender").is_some());
        assert!(manifest.find("maya").is_some());
        assert!(manifest.find("3dsmax").is_some());
        assert!(manifest.find("Blender").is_some(), "lookup is lower-cased");
        assert!(manifest.find("nuke-from-space").is_none());
    }

    #[test]
    fn blender_is_the_self_provisioning_baseline() {
        let manifest = bundled();
        let blender = manifest
            .find("blender")
            .expect("blender is in the manifest");
        assert_eq!(blender.license, HostLicense::OpenSource);
        assert!(blender.self_provision);
        assert_eq!(blender.min_version.as_deref(), Some("5.1"));
        assert_eq!(blender.pinned_version.as_deref(), Some("5.1.1"));
        assert_eq!(blender.version_arg, vec!["--version".to_string()]);
    }

    #[test]
    fn commercial_hosts_are_never_self_provisioning() {
        let manifest = bundled();
        for id in [
            "maya",
            "3dsmax",
            "houdini",
            "nuke",
            "photoshop",
            "substance",
        ] {
            let host = manifest
                .find(id)
                .unwrap_or_else(|| panic!("{id} is in the manifest"));
            assert_eq!(host.license, HostLicense::Commercial, "{id} licence");
            assert!(!host.self_provision, "{id} must not self-provision");
        }
    }

    #[test]
    fn validation_rejects_commercial_self_provision() {
        let yaml = r#"
version: "1"
hosts:
  - id: maya
    display_name: Maya
    license: commercial
    self_provision: true
    pinned_version: "2026.1"
    executables:
      windows: ["maya.exe"]
"#;
        assert_eq!(
            manifest_with(yaml).unwrap_err(),
            ManifestError::ProvisionOnCommercial {
                id: "maya".to_string()
            }
        );
    }

    #[test]
    fn validation_rejects_duplicate_ids_and_missing_fields() {
        let duplicate = r#"
version: "1"
hosts:
  - id: blender
    display_name: Blender
    license: open_source
    self_provision: false
    executables:
      linux: ["blender"]
  - id: blender
    display_name: Blender Again
    license: open_source
    self_provision: false
    executables:
      linux: ["blender"]
"#;
        assert_eq!(
            manifest_with(duplicate).unwrap_err(),
            ManifestError::DuplicateHost {
                id: "blender".to_string()
            }
        );

        let no_executables = r#"
version: "1"
hosts:
  - id: ghost
    display_name: Ghost
    license: open_source
    self_provision: false
"#;
        assert_eq!(
            manifest_with(no_executables).unwrap_err(),
            ManifestError::NoExecutables {
                id: "ghost".to_string()
            }
        );

        let no_pin = r#"
version: "1"
hosts:
  - id: blender
    display_name: Blender
    license: open_source
    self_provision: true
    executables:
      linux: ["blender"]
"#;
        assert_eq!(
            manifest_with(no_pin).unwrap_err(),
            ManifestError::MissingPinnedVersion {
                id: "blender".to_string()
            }
        );
    }

    #[test]
    fn validation_rejects_bad_versions_and_schemas() {
        let bad_version = blender_yaml("");
        let bad_version = bad_version.replace("min_version: \"5.1\"", "min_version: \"5.1.x\"");
        assert!(matches!(
            manifest_with(&bad_version).unwrap_err(),
            ManifestError::InvalidVersion { .. }
        ));

        let bad_schema = blender_yaml("").replace("version: \"1\"", "version: \"2\"");
        assert_eq!(
            manifest_with(&bad_schema).unwrap_err(),
            ManifestError::UnsupportedVersion {
                found: "2".to_string()
            }
        );

        assert!(matches!(
            manifest_with("hosts: [").unwrap_err(),
            ManifestError::Parse(_)
        ));
    }

    #[test]
    fn self_provision_is_required_not_defaulted() {
        let yaml = r#"
version: "1"
hosts:
  - id: blender
    display_name: Blender
    license: open_source
    executables:
      linux: ["blender"]
"#;
        // A host must state its provisioning stance. Inheriting a default
        // would let a new commercial entry become installable by omission.
        assert!(matches!(
            manifest_with(yaml).unwrap_err(),
            ManifestError::Parse(_)
        ));
    }

    #[test]
    fn lock_pins_override_versions_only() {
        let mut lock = HostLock::empty();
        lock.pins.insert("blender".to_string(), "5.1.2".to_string());
        lock.pins.insert("maya".to_string(), "2026.1".to_string());
        let manifest = load_with_lock(&lock).unwrap();
        assert_eq!(
            manifest.find("blender").unwrap().pinned_version.as_deref(),
            Some("5.1.2")
        );
        // A pin cannot make a commercial host installable.
        let maya = manifest.find("maya").unwrap();
        assert_eq!(maya.pinned_version.as_deref(), Some("2026.1"));
        assert!(!maya.self_provision);
        assert_eq!(maya.license, HostLicense::Commercial);
    }

    #[test]
    fn lock_rejects_malformed_version_and_unknown_ids_are_ignored() {
        let mut lock = HostLock::empty();
        lock.pins.insert("blender".to_string(), "five".to_string());
        assert_eq!(
            load_with_lock(&lock).unwrap_err(),
            ManifestError::InvalidVersion {
                id: "blender".to_string(),
                version: "five".to_string()
            }
        );

        let mut stale = HostLock::empty();
        stale.pins.insert("shake".to_string(), "4.1".to_string());
        assert!(
            load_with_lock(&stale).is_ok(),
            "stale pin must not be fatal"
        );
    }

    #[test]
    fn missing_lock_file_means_no_pins() {
        let lock = load_lock(Some(Path::new("/__definitely_missing__/hosts.lock"))).unwrap();
        assert!(lock.pins.is_empty());
        assert!(load_lock(None).unwrap().pins.is_empty());
    }

    #[test]
    fn list_value_exposes_provisioning_surface() {
        let value = bundled().to_list_value();
        let hosts = value["hosts"].as_array().expect("hosts array");
        let blender = hosts
            .iter()
            .find(|host| host["id"] == "blender")
            .expect("blender row");
        assert_eq!(blender["self_provision"], true);
        assert_eq!(blender["license"], "open_source");
        assert_eq!(blender["pinned_version"], "5.1.1");
        let channels = blender["install_channels"].as_array().expect("channels");
        assert_eq!(channels.len(), 3, "blender ships all three channels");
    }

    #[test]
    fn platform_strings_select_the_built_target() {
        let strings = PlatformStrings {
            windows: vec!["a.exe".to_string()],
            macos: vec!["A".to_string()],
            linux: vec!["a".to_string()],
        };
        let current = strings.for_current();
        assert_eq!(current.len(), 1);
        if cfg!(windows) {
            assert_eq!(current[0], "a.exe");
        } else if cfg!(target_os = "macos") {
            assert_eq!(current[0], "A");
        } else {
            assert_eq!(current[0], "a");
        }
        assert!(strings.covers_current());
        assert!(!PlatformStrings::default().covers_current());
    }
}
