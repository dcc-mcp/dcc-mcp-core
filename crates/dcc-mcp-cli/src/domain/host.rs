//! Host spec parsing and probe grading for `dcc-mcp-cli host`.
//!
//! A host spec is the argument form of "which host, in which version":
//! `blender`, `blender==5.1`, `blender>=5.1`, `blender~=5.1`. The domain layer
//! only parses and grades; locating binaries and asking them for a version is
//! the application layer's job.
//!
//! # Version precision
//!
//! Hosts rarely publish a three-component version in every channel, so
//! comparisons keep the precision the caller declared:
//!
//! * `==5.1` matches any `5.1.x` (prefix equality), not just `5.1.0`.
//! * `==5.1.1` requires that exact version.
//! * `~=5.1` means `>=5.1.0, <5.2.0`; `~=5` means `>=5.0.0, <6.0.0`.
//!
//! Treating `==5.1` as `==5.1.0` would make every spec in the issue's own
//! examples (`blender==5.1`) fail against a healthy `5.1.1` install.

use std::fmt;

use semver::Version;
use serde::Serialize;

/// Prefix of the per-host executable override, e.g. `DCC_MCP_BLENDER_EXECUTABLE`.
pub const HOST_EXECUTABLE_ENV_PREFIX: &str = "DCC_MCP_";

/// Suffix of the per-host executable override.
pub const HOST_EXECUTABLE_ENV_SUFFIX: &str = "_EXECUTABLE";

/// Environment variable naming the executable override for `host_id`.
#[must_use]
pub fn executable_env_var(host_id: &str) -> String {
    let mut name = String::with_capacity(host_id.len() + 20);
    name.push_str(HOST_EXECUTABLE_ENV_PREFIX);
    for ch in host_id.chars() {
        if ch.is_ascii_alphanumeric() {
            name.push(ch.to_ascii_uppercase());
        } else {
            name.push('_');
        }
    }
    name.push_str(HOST_EXECUTABLE_ENV_SUFFIX);
    name
}

/// Comparison operator carried by a host spec.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VersionOp {
    /// Exact match, or prefix match when fewer than three components were given.
    Eq,
    /// Greater than or equal.
    Gte,
    /// Compatible release: same precision-bound major/minor, no upper break.
    Compatible,
}

impl fmt::Display for VersionOp {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Eq => "==",
            Self::Gte => ">=",
            Self::Compatible => "~=",
        })
    }
}

/// A parsed `host[op version]` request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostSpec {
    /// Canonical host id, lower-cased.
    pub id: String,
    /// Operator carried by the spec, when one was written.
    pub op: Option<VersionOp>,
    /// Requested version, zero-padded to three components.
    pub version: Option<Version>,
    /// How many components the caller actually wrote (drives prefix matching).
    pub components: usize,
}

impl HostSpec {
    /// Parse a spec such as `blender`, `blender==5.1`, or `blender>=5.1`.
    ///
    /// # Errors
    ///
    /// Returns [`HostSpecError`] when the spec is empty, names an unsupported
    /// operator, or carries a version that is not a dotted number.
    pub fn parse(spec: &str) -> Result<Self, HostSpecError> {
        let spec = spec.trim();
        if spec.is_empty() {
            return Err(HostSpecError::Empty);
        }
        let (id, op, raw_version) = split_operator(spec)?;
        if id.is_empty() {
            return Err(HostSpecError::Empty);
        }
        let Some(op) = op else {
            return Ok(Self {
                id,
                op: None,
                version: None,
                components: 0,
            });
        };
        let Some(raw_version) = raw_version.filter(|value| !value.trim().is_empty()) else {
            return Err(HostSpecError::MissingVersion {
                spec: spec.to_string(),
                operator: op.to_string(),
            });
        };
        let (version, components) = parse_partial_version(raw_version.trim()).ok_or_else(|| {
            HostSpecError::InvalidVersion {
                spec: spec.to_string(),
                version: raw_version.trim().to_string(),
            }
        })?;
        Ok(Self {
            id,
            op: Some(op),
            version: Some(version),
            components,
        })
    }

    /// The version gate this spec imposes.
    ///
    /// A spec without an operator falls back to the manifest's `min_version`,
    /// so `host doctor blender` already enforces the ecosystem baseline instead
    /// of accepting any Blender it happens to find.
    #[must_use]
    pub fn gate(&self, min_version: Option<&str>) -> VersionGate {
        if let (Some(op), Some(version)) = (self.op, self.version.as_ref()) {
            return VersionGate {
                op,
                version: version.clone(),
                components: self.components,
            };
        }
        match min_version.and_then(parse_partial_version) {
            Some((version, components)) => VersionGate {
                op: VersionOp::Gte,
                version,
                components,
            },
            None => VersionGate {
                op: VersionOp::Gte,
                version: Version::new(0, 0, 0),
                components: 0,
            },
        }
    }
}

impl fmt::Display for HostSpec {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self.op, self.version.as_ref()) {
            (Some(op), Some(version)) => {
                write!(
                    formatter,
                    "{}{}{}",
                    self.id,
                    op,
                    format_partial(version, self.components)
                )
            }
            _ => formatter.write_str(&self.id),
        }
    }
}

/// A concrete version constraint resolved from a spec or a manifest baseline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionGate {
    pub op: VersionOp,
    /// Lower bound, always padded to three components.
    pub version: Version,
    /// Declared precision, used by `Eq` and `Compatible`.
    pub components: usize,
}

impl VersionGate {
    /// Whether `version` satisfies this gate.
    #[must_use]
    pub fn satisfied_by(&self, version: &Version) -> bool {
        match self.op {
            VersionOp::Eq => {
                if self.components >= 3 {
                    return version == &self.version;
                }
                prefix_eq(version, &self.version, self.components)
            }
            VersionOp::Gte => version >= &self.version,
            VersionOp::Compatible => {
                if version < &self.version {
                    return false;
                }
                // Bump the last declared component; a two-component `~=5.1`
                // allows 5.1.x but not 5.2.0, a one-component `~=5` allows 5.y.z.
                let upper = if self.components <= 1 {
                    Version::new(self.version.major + 1, 0, 0)
                } else {
                    Version::new(self.version.major, self.version.minor + 1, 0)
                };
                version < &upper
            }
        }
    }
}

impl fmt::Display for VersionGate {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{}{}",
            self.op,
            format_partial(&self.version, self.components)
        )
    }
}

/// Outcome of probing one host.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HostStatus {
    /// Executable found and its version satisfies the gate.
    Available,
    /// No executable found anywhere.
    Missing,
    /// Executable found, but the version does not satisfy the gate.
    VersionMismatch,
    /// Executable found, but its version could not be determined.
    VersionUnknown,
    /// The host declares no support for this platform.
    UnsupportedPlatform,
    /// The id is not in the host manifest.
    UnknownHost,
    /// The spec itself could not be parsed.
    InvalidSpec,
}

impl HostStatus {
    /// Whether a runner may proceed with this host.
    #[must_use]
    pub fn is_available(self) -> bool {
        matches!(self, Self::Available)
    }
}

/// Machine-readable reason accompanying a [`HostStatus`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HostReason {
    /// Executable located and version accepted.
    Ok,
    /// Nothing on PATH, in the search roots, or in the override variable.
    ExecutableNotFound,
    /// The override variable is set, but its target is not a runnable file.
    OverrideNotRunnable,
    /// The binary exists but `--version` never returned a readable value.
    VersionQueryFailed,
    /// The binary answered but no version token could be parsed from the output.
    VersionUnparsable,
    /// The version is below the host's declared minimum.
    VersionBelowMinimum,
    /// The version does not satisfy the requested spec.
    SpecNotSatisfied,
    /// The manifest has no entry for this platform.
    UnsupportedPlatform,
    /// The id is not in the manifest.
    UnknownHost,
    /// The spec string is malformed.
    InvalidSpec,
}

/// Why provisioning refuses to install a host.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProvisionRefusal {
    /// Commercial host: licence required, the CLI must never install it.
    Commercial,
    /// Open source, but no install channel is declared for this platform.
    NoInstallChannel,
    /// The manifest has no pinned version, so there is nothing to install.
    NoPinnedVersion,
}

/// Why a spec string could not be parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostSpecError {
    Empty,
    UnknownOperator { spec: String, operator: String },
    InvalidVersion { spec: String, version: String },
    MissingVersion { spec: String, operator: String },
}

impl fmt::Display for HostSpecError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => formatter.write_str("host spec is empty"),
            Self::UnknownOperator { spec, operator } => write!(
                formatter,
                "unsupported operator '{operator}' in host spec '{spec}'; use ==, >= or ~="
            ),
            Self::InvalidVersion { spec, version } => write!(
                formatter,
                "invalid version '{version}' in host spec '{spec}'; expected dotted numbers"
            ),
            Self::MissingVersion { spec, operator } => write!(
                formatter,
                "host spec '{spec}' uses '{operator}' without a version"
            ),
        }
    }
}

/// Split `id[op version]` into its three parts.
fn split_operator(
    spec: &str,
) -> Result<(String, Option<VersionOp>, Option<String>), HostSpecError> {
    // Two-character operators come first so `>=` is never read as `>`.
    for token in ["==", ">=", "~=", "<=", "!=", "<", ">"] {
        if let Some((id, version)) = spec.split_once(token) {
            let op = match token {
                "==" => VersionOp::Eq,
                ">=" => VersionOp::Gte,
                "~=" => VersionOp::Compatible,
                operator => {
                    return Err(HostSpecError::UnknownOperator {
                        spec: spec.to_string(),
                        operator: operator.to_string(),
                    });
                }
            };
            return Ok((
                id.trim().to_ascii_lowercase(),
                Some(op),
                Some(version.to_string()),
            ));
        }
    }
    Ok((spec.trim().to_ascii_lowercase(), None, None))
}

/// Parse `5`, `5.1` or `5.1.1` as a zero-padded version plus its precision.
fn parse_partial_version(value: &str) -> Option<(Version, usize)> {
    let trimmed = value.trim().trim_start_matches('v');
    if trimmed.is_empty() {
        return None;
    }
    let mut parts = trimmed.split('.');
    let mut numbers = [0_u64; 3];
    let mut count = 0_usize;
    for slot in numbers.iter_mut() {
        let Some(part) = parts.next() else {
            break;
        };
        let digits = part.trim();
        if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        *slot = digits.parse::<u64>().ok()?;
        count += 1;
    }
    if count == 0 || parts.next().is_some() {
        return None;
    }
    Some((Version::new(numbers[0], numbers[1], numbers[2]), count))
}

/// Render a version at the precision the caller declared.
fn format_partial(version: &Version, components: usize) -> String {
    match components {
        1 => format!("{}", version.major),
        2 => format!("{}.{}", version.major, version.minor),
        _ => version.to_string(),
    }
}

/// Compare only the first `components` components of two versions.
fn prefix_eq(version: &Version, expected: &Version, components: usize) -> bool {
    let matches = [
        version.major == expected.major,
        version.minor == expected.minor,
        version.patch == expected.patch,
    ];
    let depth = components.clamp(1, 3);
    matches[..depth].iter().all(|value| *value)
}

/// Parse a manifest-declared version such as `5` or `5.1`.
///
/// Manifest versions are written at whatever precision the host publishes, so
/// they are padded to three components before comparison.
#[must_use]
pub fn parse_manifest_version(value: &str) -> Option<Version> {
    parse_partial_version(value).map(|(version, _)| version)
}

/// Scan the first dotted-number token out of a `--version` banner.
///
/// Host version output is not a contract: Blender prints `Blender 5.1.1`
/// followed by a build banner, Godot appends a commit hash to the patch
/// position (`4.3.stable.official.77dcf97d3`), and some hosts lead with a
/// copyright line. Splitting on non-numeric characters and taking the first
/// token that parses handles all of them without a regex dependency.
#[must_use]
pub fn extract_version(text: &str) -> Option<Version> {
    for token in text.split(|ch: char| !ch.is_ascii_digit() && ch != '.') {
        let token = token.trim_matches('.');
        if token.is_empty() {
            continue;
        }
        if let Some((version, _)) = parse_partial_version(token) {
            return Some(version);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn executable_env_var_normalises_ids() {
        assert_eq!(executable_env_var("blender"), "DCC_MCP_BLENDER_EXECUTABLE");
        assert_eq!(executable_env_var("3dsmax"), "DCC_MCP_3DSMAX_EXECUTABLE");
        assert_eq!(
            executable_env_var("substance-painter"),
            "DCC_MCP_SUBSTANCE_PAINTER_EXECUTABLE"
        );
    }

    #[test]
    fn parse_accepts_bare_id_and_operators() {
        let spec = HostSpec::parse("Blender").unwrap();
        assert_eq!(spec.id, "blender");
        assert_eq!(spec.op, None);
        assert_eq!(spec.to_string(), "blender");

        let exact = HostSpec::parse("blender==5.1.1").unwrap();
        assert_eq!(exact.op, Some(VersionOp::Eq));
        assert_eq!(exact.components, 3);
        assert_eq!(exact.to_string(), "blender==5.1.1");

        let gte = HostSpec::parse("blender>=5.1").unwrap();
        assert_eq!(gte.op, Some(VersionOp::Gte));
        assert_eq!(gte.components, 2);

        let compat = HostSpec::parse("blender~=5").unwrap();
        assert_eq!(compat.op, Some(VersionOp::Compatible));
        assert_eq!(compat.components, 1);
    }

    #[test]
    fn parse_rejects_malformed_specs() {
        assert_eq!(HostSpec::parse("   ").unwrap_err(), HostSpecError::Empty);
        assert_eq!(HostSpec::parse("==5.1").unwrap_err(), HostSpecError::Empty);
        assert!(matches!(
            HostSpec::parse("blender<5.1").unwrap_err(),
            HostSpecError::UnknownOperator { .. }
        ));
        assert!(matches!(
            HostSpec::parse("blender>=five").unwrap_err(),
            HostSpecError::InvalidVersion { .. }
        ));
        assert!(matches!(
            HostSpec::parse("blender>=").unwrap_err(),
            HostSpecError::MissingVersion { .. }
        ));
        assert!(matches!(
            HostSpec::parse("blender==1.2.3.4").unwrap_err(),
            HostSpecError::InvalidVersion { .. }
        ));
    }

    #[test]
    fn eq_uses_declared_precision() {
        let spec = HostSpec::parse("blender==5.1").unwrap();
        let gate = spec.gate(None);
        // `==5.1` is a prefix match: 5.1.1 satisfies it, 5.2.0 does not.
        assert!(gate.satisfied_by(&Version::new(5, 1, 1)));
        assert!(gate.satisfied_by(&Version::new(5, 1, 0)));
        assert!(!gate.satisfied_by(&Version::new(5, 2, 0)));

        let exact = HostSpec::parse("blender==5.1.1").unwrap().gate(None);
        assert!(exact.satisfied_by(&Version::new(5, 1, 1)));
        assert!(!exact.satisfied_by(&Version::new(5, 1, 2)));
    }

    #[test]
    fn compatible_bounds_at_declared_precision() {
        let minor = HostSpec::parse("blender~=5.1").unwrap().gate(None);
        assert!(minor.satisfied_by(&Version::new(5, 1, 0)));
        assert!(minor.satisfied_by(&Version::new(5, 1, 9)));
        assert!(!minor.satisfied_by(&Version::new(5, 2, 0)));
        assert!(!minor.satisfied_by(&Version::new(5, 0, 9)));

        let major = HostSpec::parse("blender~=5").unwrap().gate(None);
        assert!(major.satisfied_by(&Version::new(5, 9, 9)));
        assert!(!major.satisfied_by(&Version::new(6, 0, 0)));
    }

    #[test]
    fn bare_spec_falls_back_to_manifest_minimum() {
        let spec = HostSpec::parse("blender").unwrap();
        let gate = spec.gate(Some("5.1"));
        assert_eq!(gate.to_string(), ">=5.1");
        assert!(!gate.satisfied_by(&Version::new(4, 5, 0)));
        assert!(gate.satisfied_by(&Version::new(5, 1, 0)));
        assert!(gate.satisfied_by(&Version::new(5, 1, 1)));

        // A host with no declared minimum accepts any version.
        let any = spec.gate(None);
        assert_eq!(any.to_string(), ">=0.0.0");
        assert!(any.satisfied_by(&Version::new(0, 0, 1)));
    }

    #[test]
    fn extract_version_reads_real_banners() {
        assert_eq!(
            extract_version("Blender 5.1.1\n\tbuild date: 2026-09-20"),
            Some(Version::new(5, 1, 1))
        );
        assert_eq!(
            extract_version("4.3.stable.official.77dcf97d3"),
            Some(Version::new(4, 3, 0))
        );
        assert_eq!(
            extract_version("kdenlive 25.08.0 (rev 1)"),
            Some(Version::new(25, 8, 0))
        );
        // A bare year-like number is still the first dotted-number token.
        assert_eq!(extract_version("v5"), Some(Version::new(5, 0, 0)));
        assert_eq!(extract_version("no version here"), None);
        assert_eq!(extract_version(""), None);
    }

    #[test]
    fn status_availability_is_strict() {
        assert!(HostStatus::Available.is_available());
        for status in [
            HostStatus::Missing,
            HostStatus::VersionMismatch,
            HostStatus::VersionUnknown,
            HostStatus::UnsupportedPlatform,
            HostStatus::UnknownHost,
            HostStatus::InvalidSpec,
        ] {
            assert!(!status.is_available(), "{status:?} must not be available");
        }
    }
}
