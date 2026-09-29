//! Host probing and provisioning decisions for `dcc-mcp-cli host`.
//!
//! Two surfaces live here:
//!
//! * [`doctor`] — read-only probing. Reports `available` / `missing` /
//!   `version_mismatch` and never installs anything.
//! * [`provision_decision`] — the pure yes/no that `host install` acts on.
//!   Keeping it separate from the install execution is what makes the
//!   commercial-host redline unit-testable without an installer in the loop.
//!
//! The probe is deliberately shallow: it locates the binary and asks it for a
//! version. It does not launch the host GUI, load an adapter, or touch a
//! project. A proposition runner needs "can I start?" not "does it work?", and
//! a probe that opened Blender would cost seconds per proposition.

pub mod detect;
pub mod manifest;

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use self::detect::{
    Candidate, CandidateSource, HostEnv, candidates, query_version, sources_checked,
};
use self::manifest::{HostDefinition, HostManifest};
use crate::domain::host::{
    HostReason, HostSpec, HostStatus, ProvisionRefusal, VersionGate, parse_manifest_version,
};

/// Outcome of probing one host.
#[derive(Debug, Clone)]
pub struct HostProbe {
    /// Host id from the manifest.
    pub id: String,
    /// Display name from the manifest.
    pub display_name: String,
    /// Resolved status.
    pub status: HostStatus,
    /// Machine-readable reason.
    pub reason: HostReason,
    /// Executable that was found, if any.
    pub executable: Option<PathBuf>,
    /// Where the executable came from.
    pub executable_source: Option<String>,
    /// Reported version, when one could be read.
    pub version: Option<String>,
    /// The gate that was applied.
    pub gate: Option<String>,
    /// Expected versions from the manifest.
    pub expected: Value,
    /// Licence class.
    pub license: String,
    /// Whether the CLI may provision this host.
    pub self_provision: bool,
    /// Where the probe looked.
    pub sources_checked: Vec<String>,
    /// Operator-facing next step.
    pub hint: String,
}

/// Probe every requested spec, or every known host when `specs` is empty.
///
/// Read-only by construction: the only process spawned is `<host> --version`
/// through [`detect::query_version`].
#[must_use]
pub fn doctor(specs: &[String], manifest: &HostManifest, env: &HostEnv) -> Vec<HostProbe> {
    if specs.is_empty() {
        return manifest
            .hosts
            .iter()
            .map(|def| probe_definition(def, None, env))
            .collect();
    }
    specs
        .iter()
        .map(|spec| match HostSpec::parse(spec) {
            Ok(parsed) => match manifest.find(&parsed.id) {
                Some(def) => probe_definition(def, Some(&parsed), env),
                None => unknown_host_probe(&parsed.id, spec),
            },
            Err(_) => invalid_spec_probe(spec),
        })
        .collect()
}

/// Render probes as the `host doctor` payload.
#[must_use]
pub fn doctor_value(probes: &[HostProbe]) -> Value {
    let hosts: Vec<Value> = probes.iter().map(probe_value).collect();
    let failures = probes
        .iter()
        .filter(|probe| !probe.status.is_available())
        .count();
    json!({
        "hosts": hosts,
        "summary": {
            "total": probes.len(),
            "available": probes.len() - failures,
            "unavailable": failures,
        },
        "read_only": true,
    })
}

/// Whether any probe is not usable, which drives the exit code.
#[must_use]
pub fn has_unavailable(probes: &[HostProbe]) -> bool {
    probes.iter().any(|probe| !probe.status.is_available())
}

/// Why provisioning would refuse a host, if it would.
///
/// `None` means the CLI is allowed to install it. Commercial hosts always
/// refuse: they need a licence and installing one is outside our authority.
#[must_use]
pub fn provision_decision(def: &HostDefinition, query: ProvisionQuery) -> Option<ProvisionRefusal> {
    if def.license == manifest::HostLicense::Commercial {
        return Some(ProvisionRefusal::Commercial);
    }
    if !def.self_provision {
        return Some(ProvisionRefusal::NoInstallChannel);
    }
    if def.pinned_version.is_none() {
        return Some(ProvisionRefusal::NoPinnedVersion);
    }
    if !query.channel_available {
        return Some(ProvisionRefusal::NoInstallChannel);
    }
    None
}

/// Platform facts `provision_decision` needs but should not look up itself.
#[derive(Debug, Clone, Copy)]
pub struct ProvisionQuery {
    /// Whether the manifest declares an install channel for this platform.
    pub channel_available: bool,
}

/// Human-readable refusal message for a host id.
#[must_use]
pub fn refusal_message(display_name: &str, refusal: ProvisionRefusal) -> String {
    match refusal {
        ProvisionRefusal::Commercial => format!(
            "{display_name} is licence-gated, so DCC MCP will not install it. Install {display_name} yourself, or set the executable override to an existing install."
        ),
        ProvisionRefusal::NoInstallChannel => format!(
            "{display_name} has no scripted install channel on this platform yet. Install it manually, or set the executable override to an existing install."
        ),
        ProvisionRefusal::NoPinnedVersion => format!(
            "{display_name} has no pinned version in the host manifest, so there is nothing to install."
        ),
    }
}

/// Probe one manifest entry against `spec` (a bare id when `None`).
fn probe_definition(def: &HostDefinition, spec: Option<&HostSpec>, env: &HostEnv) -> HostProbe {
    let gate = spec
        .map(|spec| spec.gate(def.min_version.as_deref()))
        .unwrap_or_else(|| {
            HostSpec::parse(&def.id)
                .map(|parsed| parsed.gate(def.min_version.as_deref()))
                .unwrap_or_else(|_| VersionGate {
                    op: crate::domain::host::VersionOp::Gte,
                    version: semver::Version::new(0, 0, 0),
                    components: 0,
                })
        });
    let spec_label = spec.map(ToString::to_string).unwrap_or_else(|| {
        if def.min_version.is_some() {
            format!("{}{}", def.id, gate)
        } else {
            def.id.clone()
        }
    });
    let expected = json!({
        "pinned_version": def.pinned_version,
        "min_version": def.min_version,
    });
    let sources = sources_checked(def, env);
    let license = serde_json::to_value(def.license)
        .ok()
        .and_then(|value| value.as_str().map(ToString::to_string))
        .unwrap_or_else(|| "open_source".to_string());

    // A host with no entry for this platform cannot be probed; saying
    // `missing` would imply "install it" where the real answer is "this
    // platform is not supported".
    if !def.executables.covers_current() && def.search_roots.for_current().is_empty() {
        return HostProbe {
            id: def.id.clone(),
            display_name: def.display_name.clone(),
            status: HostStatus::UnsupportedPlatform,
            reason: HostReason::UnsupportedPlatform,
            executable: None,
            executable_source: None,
            version: None,
            gate: Some(gate.to_string()),
            expected,
            license,
            self_provision: def.self_provision,
            sources_checked: sources,
            hint: format!(
                "{} declares no executable for this platform; probe it on a supported platform.",
                def.display_name
            ),
        };
    }

    let found = candidates(def, env);
    let Some(candidate) = found.into_iter().next() else {
        // An override is an explicit operator declaration, so a target that
        // was rejected is reported rather than folded into "not found" —
        // silently ignoring it hides the one setting the operator did set.
        if let Some(path) = rejected_override(def, env) {
            return rejected_override_probe(
                def,
                &path,
                sources,
                gate.to_string(),
                expected,
                license,
            );
        }
        return HostProbe {
            id: def.id.clone(),
            display_name: def.display_name.clone(),
            status: HostStatus::Missing,
            reason: HostReason::ExecutableNotFound,
            executable: None,
            executable_source: None,
            version: None,
            gate: Some(gate.to_string()),
            expected,
            license,
            self_provision: def.self_provision,
            sources_checked: sources,
            hint: missing_hint(def),
        };
    };

    match query_version(&candidate.path, &def.version_arg) {
        Ok(Some(version)) => {
            if gate.satisfied_by(&version) {
                HostProbe {
                    id: def.id.clone(),
                    display_name: def.display_name.clone(),
                    status: HostStatus::Available,
                    reason: HostReason::Ok,
                    executable: Some(candidate.path),
                    executable_source: Some(candidate.source.as_str().to_string()),
                    version: Some(version.to_string()),
                    gate: Some(gate.to_string()),
                    expected,
                    license,
                    self_provision: def.self_provision,
                    sources_checked: sources,
                    hint: String::new(),
                }
            } else {
                HostProbe {
                    id: def.id.clone(),
                    display_name: def.display_name.clone(),
                    status: HostStatus::VersionMismatch,
                    reason: mismatch_reason(def, &version),
                    executable: Some(candidate.path),
                    executable_source: Some(candidate.source.as_str().to_string()),
                    version: Some(version.to_string()),
                    gate: Some(gate.to_string()),
                    expected,
                    license,
                    self_provision: def.self_provision,
                    sources_checked: sources,
                    hint: mismatch_hint(def, &version),
                }
            }
        }
        Ok(None) => HostProbe {
            // The manifest declares no version query, so existence is the whole
            // contract. Commercial hosts land here because their binaries do not
            // answer a portable `--version`.
            id: def.id.clone(),
            display_name: def.display_name.clone(),
            status: HostStatus::Available,
            reason: HostReason::Ok,
            executable: Some(candidate.path),
            executable_source: Some(candidate.source.as_str().to_string()),
            version: None,
            gate: Some(gate.to_string()),
            expected,
            license,
            self_provision: def.self_provision,
            sources_checked: sources,
            hint: String::new(),
        },
        Err(detect::VersionQueryError::Unparsable { .. }) => version_unknown_probe(
            def,
            &candidate,
            HostReason::VersionUnparsable,
            sources,
            gate.to_string(),
            expected,
            license,
            spec_label,
        ),
        Err(_) => version_unknown_probe(
            def,
            &candidate,
            HostReason::VersionQueryFailed,
            sources,
            gate.to_string(),
            expected,
            license,
            spec_label,
        ),
    }
}

/// The override target, when the variable is set but produced no candidate.
///
/// `DCC_MCP_<ID>_EXECUTABLE` is a deliberate operator declaration, so a path
/// that is absent or lacks the executable bit deserves its own report instead
/// of the generic "executable not found" one.
fn rejected_override(def: &HostDefinition, env: &HostEnv) -> Option<PathBuf> {
    let path = PathBuf::from(env.var(&def.executable_env_var())?.trim());
    (!path.is_file() || !detect::is_executable_file(&path)).then_some(path)
}

/// Report an override that named something we cannot run.
///
/// `executable_source` is set even though `executable` is not, so the report
/// says "your override was seen and rejected here" rather than going `null`.
#[allow(clippy::too_many_arguments)]
fn rejected_override_probe(
    def: &HostDefinition,
    path: &Path,
    sources: Vec<String>,
    gate: String,
    expected: Value,
    license: String,
) -> HostProbe {
    let variable = def.executable_env_var();
    let hint = if path.is_file() {
        format!(
            "{variable} points at {}, which is not an executable file; run `chmod +x {}` or point {variable} at a runnable {} binary.",
            path.display(),
            path.display(),
            def.display_name
        )
    } else {
        format!(
            "{variable} points at {}, which does not exist; point {variable} at an existing {} binary or unset it.",
            path.display(),
            def.display_name
        )
    };
    HostProbe {
        id: def.id.clone(),
        display_name: def.display_name.clone(),
        status: HostStatus::Missing,
        reason: HostReason::OverrideNotRunnable,
        executable: None,
        executable_source: Some(CandidateSource::EnvOverride.as_str().to_string()),
        version: None,
        gate: Some(gate),
        expected,
        license,
        self_provision: def.self_provision,
        sources_checked: sources,
        hint,
    }
}

/// Distinguish "below the ecosystem minimum" from "not what you asked for".
fn mismatch_reason(def: &HostDefinition, version: &semver::Version) -> HostReason {
    match def.min_version.as_deref().and_then(parse_manifest_version) {
        Some(minimum) if version < &minimum => HostReason::VersionBelowMinimum,
        _ => HostReason::SpecNotSatisfied,
    }
}

fn mismatch_hint(def: &HostDefinition, version: &semver::Version) -> String {
    match def.pinned_version.as_deref() {
        Some(pinned) => format!(
            "{} {} does not satisfy the required baseline; run `dcc-mcp-cli host install {}=={}`.",
            def.display_name, version, def.id, pinned
        ),
        None => format!(
            "{} {} does not satisfy the requested version.",
            def.display_name, version
        ),
    }
}

fn missing_hint(def: &HostDefinition) -> String {
    let refusal = provision_decision(
        def,
        ProvisionQuery {
            channel_available: def.install_channel_for_current().is_some(),
        },
    );
    match (refusal, def.pinned_version.as_deref()) {
        (None, Some(pinned)) => format!(
            "Run `dcc-mcp-cli host install {}=={}` to let the CLI provision this host, or set {} to an existing install.",
            def.id,
            pinned,
            def.executable_env_var()
        ),
        (None, None) => format!("Set {} to an existing install.", def.executable_env_var()),
        (Some(refusal), _) => refusal_message(&def.display_name, refusal),
    }
}

#[allow(clippy::too_many_arguments)]
fn version_unknown_probe(
    def: &HostDefinition,
    candidate: &Candidate,
    reason: HostReason,
    sources: Vec<String>,
    gate: String,
    expected: Value,
    license: String,
    spec_label: String,
) -> HostProbe {
    HostProbe {
        id: def.id.clone(),
        display_name: def.display_name.clone(),
        status: HostStatus::VersionUnknown,
        reason,
        executable: Some(candidate.path.clone()),
        executable_source: Some(candidate.source.as_str().to_string()),
        version: None,
        gate: Some(gate),
        expected,
        license,
        self_provision: def.self_provision,
        sources_checked: sources,
        hint: format!(
            "Found {} at {} but could not read its version, so `{spec_label}` is unverified. Set {} to skip detection, or reinstall the host.",
            def.display_name,
            candidate.path.display(),
            def.executable_env_var()
        ),
    }
}

fn unknown_host_probe(id: &str, raw: &str) -> HostProbe {
    HostProbe {
        id: id.to_string(),
        display_name: id.to_string(),
        status: HostStatus::UnknownHost,
        reason: HostReason::UnknownHost,
        executable: None,
        executable_source: None,
        version: None,
        gate: None,
        expected: Value::Null,
        license: "unknown".to_string(),
        self_provision: false,
        sources_checked: Vec::new(),
        hint: format!(
            "'{raw}' is not in the host manifest; run `dcc-mcp-cli host list` to see the known hosts."
        ),
    }
}

fn invalid_spec_probe(raw: &str) -> HostProbe {
    let id = raw.split(['=', '>', '~', '<', '!']).next().unwrap_or(raw);
    HostProbe {
        id: id.trim().to_string(),
        display_name: id.trim().to_string(),
        status: HostStatus::InvalidSpec,
        reason: HostReason::InvalidSpec,
        executable: None,
        executable_source: None,
        version: None,
        gate: None,
        expected: Value::Null,
        license: "unknown".to_string(),
        self_provision: false,
        sources_checked: Vec::new(),
        hint: format!(
            "'{raw}' is not a valid host spec; use `host`, `host==5.1`, `host>=5.1` or `host~=5`."
        ),
    }
}

/// Render one probe.
fn probe_value(probe: &HostProbe) -> Value {
    json!({
        "id": probe.id,
        "display_name": probe.display_name,
        "status": probe.status,
        "reason": probe.reason,
        "executable": probe.executable,
        "executable_source": probe.executable_source,
        "version": probe.version,
        "gate": probe.gate,
        "expected": probe.expected,
        "license": probe.license,
        "self_provision": probe.self_provision,
        "sources_checked": probe.sources_checked,
        "hint": probe.hint,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::host::manifest::{HostLicense, PlatformStrings, bundled};

    fn env_without_path() -> HostEnv {
        HostEnv::new()
    }

    fn fake_def(id: &str) -> HostDefinition {
        HostDefinition {
            id: id.to_string(),
            display_name: id.to_string(),
            license: HostLicense::OpenSource,
            self_provision: false,
            min_version: None,
            pinned_version: None,
            executables: PlatformStrings::default(),
            version_arg: Vec::new(),
            search_roots: PlatformStrings::default(),
            install: std::collections::BTreeMap::new(),
        }
    }

    #[test]
    fn doctor_reports_missing_without_installing() {
        // Acceptance criterion 1 from the issue: probing an absent host returns
        // `missing` and performs no installation. The unit-level guarantee is
        // structural — `doctor` has no install call path at all — plus this
        // behavioural check on a host nobody has.
        let manifest = bundled();
        let probes = doctor(
            &["definitely-not-a-host".to_string()],
            &manifest,
            &env_without_path(),
        );
        assert_eq!(probes.len(), 1);
        assert_eq!(probes[0].status, HostStatus::UnknownHost);
        assert_eq!(probes[0].reason, HostReason::UnknownHost);
    }

    #[test]
    fn doctor_rejects_malformed_specs() {
        let manifest = bundled();
        let probes = doctor(
            &["blender>=five".to_string()],
            &manifest,
            &env_without_path(),
        );
        assert_eq!(probes[0].status, HostStatus::InvalidSpec);
        assert_eq!(probes[0].reason, HostReason::InvalidSpec);
        assert!(probes[0].hint.contains("host==5.1"));
    }

    #[test]
    fn doctor_without_specs_probes_every_known_host() {
        let manifest = bundled();
        let probes = doctor(&[], &manifest, &env_without_path());
        assert_eq!(probes.len(), manifest.hosts.len());
        assert!(probes.iter().any(|probe| probe.id == "blender"));
        assert!(probes.iter().any(|probe| probe.id == "maya"));
    }

    #[test]
    fn commercial_hosts_refuse_provisioning_every_time() {
        // Acceptance criterion 4: maya must never be installed by this CLI.
        let manifest = bundled();
        for id in [
            "maya",
            "3dsmax",
            "houdini",
            "nuke",
            "photoshop",
            "substance",
        ] {
            let def = manifest.find(id).expect("host in manifest");
            assert_eq!(
                provision_decision(
                    def,
                    ProvisionQuery {
                        channel_available: true
                    }
                ),
                Some(ProvisionRefusal::Commercial),
                "{id} must refuse"
            );
            assert!(
                refusal_message(&def.display_name, ProvisionRefusal::Commercial)
                    .contains("licence-gated")
            );
        }
    }

    #[test]
    fn blender_is_provisionable_when_a_channel_exists() {
        let manifest = bundled();
        let def = manifest.find("blender").expect("blender");
        assert_eq!(
            provision_decision(
                def,
                ProvisionQuery {
                    channel_available: true
                }
            ),
            None
        );
        assert_eq!(
            provision_decision(
                def,
                ProvisionQuery {
                    channel_available: false
                }
            ),
            Some(ProvisionRefusal::NoInstallChannel)
        );
    }

    #[test]
    fn open_source_host_without_channel_refuses_with_a_manual_hint() {
        let manifest = bundled();
        let def = manifest.find("kdenlive").expect("kdenlive");
        assert_eq!(
            provision_decision(
                def,
                ProvisionQuery {
                    channel_available: false
                }
            ),
            Some(ProvisionRefusal::NoInstallChannel)
        );
        assert!(
            refusal_message("Kdenlive", ProvisionRefusal::NoInstallChannel)
                .contains("no scripted install channel")
        );
    }

    #[test]
    fn missing_hints_point_at_install_for_blender_and_manual_for_maya() {
        let manifest = bundled();
        let blender = manifest.find("blender").expect("blender");
        let hint = missing_hint(blender);
        assert!(hint.contains("host install blender==5.1.1"), "{hint}");
        assert!(hint.contains("DCC_MCP_BLENDER_EXECUTABLE"), "{hint}");

        let maya = manifest.find("maya").expect("maya");
        let hint = missing_hint(maya);
        assert!(hint.contains("will not install"), "{hint}");
        assert!(!hint.contains("host install"), "{hint}");
    }

    #[test]
    fn doctor_value_summarises_availability() {
        let manifest = bundled();
        let probes = doctor(&["blender".to_string()], &manifest, &env_without_path());
        let value = doctor_value(&probes);
        assert_eq!(value["read_only"], true);
        assert_eq!(value["summary"]["total"], 1);
        let host = &value["hosts"][0];
        assert!(host["gate"].is_string());
        assert!(host["sources_checked"].is_array());
        assert_eq!(host["expected"]["pinned_version"], "5.1.1");
        assert!(has_unavailable(&probes));
    }

    /// A host that declares an executable on every platform, so probing is
    /// never short-circuited as `unsupported_platform` in these tests.
    fn fake_def_for_current_platform(id: &str) -> HostDefinition {
        let mut def = fake_def(id);
        for names in [
            &mut def.executables.windows,
            &mut def.executables.linux,
            &mut def.executables.macos,
        ] {
            names.push(format!("{id}-never-on-path"));
        }
        def
    }

    #[test]
    fn a_rejected_override_is_reported_instead_of_dropped() {
        // The override is an explicit operator declaration. Pointing it at
        // something unusable must surface the variable and the path, not
        // degrade into a generic "not found" that hides the operator's input.
        let target = std::env::temp_dir().join(format!("absent-{}.bin", uuid::Uuid::new_v4()));
        let mut env = env_without_path();
        env.vars.insert(
            "DCC_MCP_STUBHOST_EXECUTABLE".to_string(),
            target.to_string_lossy().into_owned(),
        );
        let manifest = HostManifest {
            version: "1".to_string(),
            hosts: vec![fake_def_for_current_platform("stubhost")],
        };
        let probes = doctor(&["stubhost".to_string()], &manifest, &env);
        assert_eq!(probes[0].status, HostStatus::Missing);
        assert_eq!(probes[0].reason, HostReason::OverrideNotRunnable);
        assert_eq!(probes[0].executable_source.as_deref(), Some("env_override"));
        assert!(probes[0].executable.is_none());
        assert!(
            probes[0].hint.contains("DCC_MCP_STUBHOST_EXECUTABLE")
                && probes[0].hint.contains(&target.display().to_string()),
            "the hint must name the variable and its target, got {}",
            probes[0].hint
        );
    }

    #[test]
    fn an_unset_override_still_reports_not_found() {
        // Without the override there is nothing to reject, so the reason stays
        // the generic one this branch always produced.
        let manifest = HostManifest {
            version: "1".to_string(),
            hosts: vec![fake_def_for_current_platform("stubhost")],
        };
        let probes = doctor(&["stubhost".to_string()], &manifest, &env_without_path());
        assert_eq!(probes[0].status, HostStatus::Missing);
        assert_eq!(probes[0].reason, HostReason::ExecutableNotFound);
        assert_eq!(probes[0].executable_source, None);
    }

    #[test]
    fn host_without_platform_entry_is_unsupported_not_missing() {
        let mut def = fake_def("windows-only");
        def.executables.windows = vec!["only.exe".to_string()];
        if cfg!(windows) {
            // On Windows this host is probed normally, so the assertion is
            // inverted; the contract still holds for the other platforms.
            return;
        }
        let probes = doctor(
            &["windows-only".to_string()],
            &HostManifest {
                version: "1".to_string(),
                hosts: vec![def],
            },
            &env_without_path(),
        );
        assert_eq!(probes[0].status, HostStatus::UnsupportedPlatform);
        assert_eq!(probes[0].reason, HostReason::UnsupportedPlatform);
    }
}
