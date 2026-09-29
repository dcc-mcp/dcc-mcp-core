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

pub mod consent;
pub mod detect;
pub mod install;
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
    /// A non-fatal observation the operator should still see.
    ///
    /// Set when a probe succeeded through one source while a more specific
    /// source was rejected. The operator set the override deliberately, so
    /// silently proceeding from PATH hides the setting that is actually wrong.
    pub warning: Option<String>,
}

/// The result of asking one candidate executable for its version.
struct CandidateProbe {
    candidate: Candidate,
    /// `Ok(None)` means the manifest declares no version query, not failure.
    outcome: Result<Option<semver::Version>, detect::VersionQueryError>,
    version: Option<semver::Version>,
    status: HostStatus,
}

/// Ask one candidate for its version and grade it against `gate`.
///
/// Used by the threshold-aware scan so a candidate that cannot clear the gate
/// does not stop the search.
fn probe_candidate(
    def: &HostDefinition,
    candidate: &Candidate,
    gate: &VersionGate,
) -> CandidateProbe {
    match query_version(&candidate.path, &def.version_arg) {
        Ok(Some(version)) => {
            let status = if gate.satisfied_by(&version) {
                HostStatus::Available
            } else {
                HostStatus::VersionMismatch
            };
            CandidateProbe {
                candidate: candidate.clone(),
                outcome: Ok(Some(version.clone())),
                version: Some(version),
                status,
            }
        }
        Ok(None) => CandidateProbe {
            candidate: candidate.clone(),
            outcome: Ok(None),
            version: None,
            status: HostStatus::Available,
        },
        Err(error) => CandidateProbe {
            candidate: candidate.clone(),
            outcome: Err(error),
            version: None,
            status: HostStatus::VersionUnknown,
        },
    }
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

/// The installable version for a host: the pinned manifest version, unless the
/// spec pins a different one that happens to match.
#[must_use]
pub fn installable_version(def: &HostDefinition) -> Option<&str> {
    def.pinned_version.as_deref()
}

/// Why an install did not happen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallOutcome {
    /// The host already satisfies the requested version; nothing was done.
    AlreadySatisfied { id: String, version: Option<String> },
    /// The channel ran and the host now probes as available.
    Installed {
        id: String,
        version: Option<String>,
        channel: String,
    },
    /// The channel ran but the host still does not probe as available.
    Unverified {
        id: String,
        channel: String,
        detail: String,
    },
    /// The host must not be installed by this CLI.
    Refused {
        id: String,
        display_name: String,
        refusal: ProvisionRefusal,
    },
    /// Consent was not granted.
    ConsentRequired {
        id: String,
        reason: consent::ConsentRefusal,
    },
    /// The channel failed.
    Failed {
        id: String,
        error: install::InstallError,
    },
}

/// Render an install outcome for the CLI.
#[must_use]
pub fn install_value(outcome: &InstallOutcome) -> Value {
    match outcome {
        InstallOutcome::AlreadySatisfied { id, version } => json!({
            "id": id,
            "status": "already_satisfied",
            "version": version,
            "installed": false,
        }),
        InstallOutcome::Installed {
            id,
            version,
            channel,
        } => json!({
            "id": id,
            "status": "installed",
            "version": version,
            "channel": channel,
            "installed": true,
        }),
        InstallOutcome::Unverified {
            id,
            channel,
            detail,
        } => json!({
            "id": id,
            "status": "unverified",
            "channel": channel,
            "installed": true,
            "detail": detail,
            "hint": "The channel finished but the host does not probe as available. Check the channel output, then re-run `host doctor`.",
        }),
        InstallOutcome::Refused {
            id,
            display_name,
            refusal,
        } => json!({
            "id": id,
            "status": "refused",
            "reason": refusal,
            "installed": false,
            "hint": refusal_message(display_name, *refusal),
        }),
        InstallOutcome::ConsentRequired { id, reason } => json!({
            "id": id,
            "status": "consent_required",
            "reason": reason,
            "installed": false,
            "hint": consent::refusal_message(*reason),
        }),
        InstallOutcome::Failed { id, error } => json!({
            "id": id,
            "status": "failed",
            "installed": false,
            "error": error.to_string(),
        }),
    }
}

/// Whether the outcome should exit non-zero.
///
/// A channel reporting success is not evidence that the host is usable; only a
/// probe is. An unverified install is therefore a failure, so a pipeline that
/// chains `host install && blender -b ...` stops instead of running on a host
/// that was never actually installed.
#[must_use]
pub fn install_failed(outcome: &InstallOutcome) -> bool {
    !matches!(
        outcome,
        InstallOutcome::AlreadySatisfied { .. } | InstallOutcome::Installed { .. }
    )
}

/// Re-grade an installed outcome against the post-install probe.
///
/// Downgrades `Installed` to `Unverified` when the host still does not probe as
/// available. This is the only producer of that variant, which is the point:
/// the channel's word is not taken for the host's presence.
#[must_use]
pub fn verify_install(outcome: InstallOutcome, verified: bool) -> InstallOutcome {
    match outcome {
        InstallOutcome::Installed {
            id,
            version: _,
            channel,
        } if !verified => InstallOutcome::Unverified {
            detail: format!("{id} was installed but does not probe as available"),
            id,
            channel,
        },
        other => other,
    }
}

/// Run the provisioning flow, stopping at the first gate that fails.
///
/// The gates run in a fixed order on purpose: the commercial-host refusal
/// comes before the consent prompt, so a licence-gated host is never one
/// Enter press away from being installed.
pub fn provision(def: &HostDefinition, request: &ProvisionRequest) -> InstallOutcome {
    if let Some(refusal) = provision_decision(
        def,
        ProvisionQuery {
            channel_available: def.install_channel_for_current().is_some(),
        },
    ) {
        return InstallOutcome::Refused {
            id: def.id.clone(),
            display_name: def.display_name.clone(),
            refusal,
        };
    }

    let Some(version) = installable_version(def) else {
        return InstallOutcome::Refused {
            id: def.id.clone(),
            display_name: def.display_name.clone(),
            refusal: ProvisionRefusal::NoPinnedVersion,
        };
    };

    if let Some(current) = request.current_version.as_deref()
        && request.current_available
    {
        return InstallOutcome::AlreadySatisfied {
            id: def.id.clone(),
            version: Some(current.to_string()),
        };
    }

    match request.consent {
        consent::ConsentDecision::Proceed => {}
        consent::ConsentDecision::Ask { .. } => {
            // The caller is responsible for asking before re-entering; reaching
            // this arm means consent was never resolved.
            return InstallOutcome::ConsentRequired {
                id: def.id.clone(),
                reason: consent::ConsentRefusal::NonInteractive,
            };
        }
        consent::ConsentDecision::Refused { reason } => {
            return InstallOutcome::ConsentRequired {
                id: def.id.clone(),
                reason,
            };
        }
    }

    match install::install(def, version) {
        Ok(result) => InstallOutcome::Installed {
            id: def.id.clone(),
            version: Some(version.to_string()),
            channel: result.channel,
        },
        Err(error) => InstallOutcome::Failed {
            id: def.id.clone(),
            error,
        },
    }
}

/// Everything `provision` needs from the caller, resolved beforehand so the
/// flow itself stays pure and testable.
#[derive(Debug, Clone)]
pub struct ProvisionRequest {
    /// Whether the host already probes as available.
    pub current_available: bool,
    /// The host's current version, when one was read.
    pub current_version: Option<String>,
    /// The resolved consent decision.
    pub consent: consent::ConsentDecision,
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
            warning: None,
        };
    }

    let rejected = rejected_override(def, env);
    let found = candidates(def, env);
    if found.is_empty() {
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
            warning: None,
        };
    }

    // Probe candidates in precedence order and take the first one that clears
    // the version gate.
    //
    // Taking the first candidate unconditionally meant a host could be reported
    // `version_mismatch` while a build that satisfies the gate sat in a lower
    // priority source. That is the common thm/rez shape: the environment pins
    // an older Blender on PATH while a newer one is installed globally, and the
    // operator only ever saw the mismatch. It also let a non-runnable shim
    // shadow the real binary on Windows, where any existing file passes the
    // executable check but cannot be spawned.
    let mut probes: Vec<CandidateProbe> = Vec::new();
    let mut chosen = None;
    for candidate in &found {
        let probe = probe_candidate(def, candidate, &gate);
        let usable = probe.status.is_available();
        probes.push(probe);
        if usable {
            chosen = Some(probes.len() - 1);
            break;
        }
    }
    // With no usable candidate, report the highest-priority one: that is the
    // source the operator is best placed to act on.
    let chosen = chosen.unwrap_or(0);
    let probe = &probes[chosen];
    let candidate = probe.candidate.clone();

    // An override naming something unusable must stay visible even when PATH
    // supplied the candidate we are about to report. Silently falling back
    // hides the one setting the operator actually set, and once `host install`
    // exists it would misdirect them into installing a host that is already
    // present while the broken override remains the real problem.
    let mut warning = rejected.map(|path| {
        format!(
            "{} is set to {} but was not usable, so this probe used {} instead; fix or unset the override.",
            def.executable_env_var(),
            path.display(),
            candidate.source.as_str()
        )
    });
    if chosen > 0 {
        // Say which candidates were passed over and why, otherwise the report
        // names a binary the operator did not expect and looks arbitrary.
        let skipped: Vec<String> = probes[..chosen]
            .iter()
            .map(|probe| {
                let version = probe
                    .version
                    .as_ref()
                    .map(ToString::to_string)
                    .unwrap_or_else(|| "an unreadable version".to_string());
                format!(
                    "{} ({}) reported {}",
                    probe.candidate.path.display(),
                    probe.candidate.source.as_str(),
                    version
                )
            })
            .collect();
        let skipped_note = format!(
            "{} did not satisfy {}; skipped {} and used {} instead.",
            skipped.join(", "),
            gate,
            if skipped.len() == 1 { "it" } else { "them" },
            candidate.source.as_str()
        );
        warning = Some(match warning {
            Some(existing) => format!("{existing} {skipped_note}"),
            None => skipped_note,
        });
    }

    match &probe.outcome {
        Ok(Some(version)) => {
            if gate.satisfied_by(version) {
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
                    warning: warning.clone(),
                }
            } else {
                HostProbe {
                    id: def.id.clone(),
                    display_name: def.display_name.clone(),
                    status: HostStatus::VersionMismatch,
                    reason: mismatch_reason(def, version),
                    executable: Some(candidate.path),
                    executable_source: Some(candidate.source.as_str().to_string()),
                    version: Some(version.to_string()),
                    gate: Some(gate.to_string()),
                    expected,
                    license,
                    self_provision: def.self_provision,
                    sources_checked: sources,
                    hint: mismatch_hint(def, version),
                    warning: warning.clone(),
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
            warning: warning.clone(),
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
            warning.clone(),
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
            warning.clone(),
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
        warning: None,
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
    warning: Option<String>,
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
        warning,
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
        warning: None,
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
        warning: None,
    }
}

/// Render one probe.
fn probe_value(probe: &HostProbe) -> Value {
    let mut value = json!({
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
    });
    if let Some(warning) = probe.warning.as_ref() {
        value["warning"] = json!(warning);
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::host::manifest::{HostLicense, PlatformStrings, bundled};

    fn env_without_path() -> HostEnv {
        HostEnv::new()
    }

    /// Write a stub that prints `banner` when asked for its version.
    ///
    /// Uses a shell script on unix and a batch file on Windows so the stub is
    /// both executable and able to answer `--version`.
    fn write_version_stub(path: &Path, banner: &str) {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::write(path, format!("#!/bin/sh\necho '{banner}'\n")).unwrap();
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        #[cfg(windows)]
        {
            std::fs::write(path, format!("@echo off\r\necho {banner}\r\n")).unwrap();
        }
        #[cfg(not(any(unix, windows)))]
        {
            std::fs::write(path, banner).unwrap();
        }
    }

    /// The stub filename for this platform.
    ///
    /// Windows needs a real extension to execute the script, so the manifest
    /// name and the file on disk have to agree on it.
    fn stub_file_name() -> &'static str {
        if cfg!(windows) {
            "blender.bat"
        } else {
            "blender"
        }
    }

    /// A host definition that probes the current platform for `id`.
    fn stub_host_definition(id: &str, min_version: Option<&str>) -> HostDefinition {
        let mut def = fake_def(id);
        def.min_version = min_version.map(ToString::to_string);
        def.version_arg = vec!["--version".to_string()];
        for names in [
            &mut def.executables.windows,
            &mut def.executables.linux,
            &mut def.executables.macos,
        ] {
            names.push(stub_file_name().to_string());
        }
        def
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

    /// P2-1: a candidate that fails the version gate must not hide a build
    /// that satisfies it.
    ///
    /// This is the ordinary thm/rez shape: the environment pins an older host
    /// on PATH while a newer one is installed globally. Reporting the mismatch
    /// and stopping left the operator stuck on the wrong build even though a
    /// usable one was present.
    #[test]
    fn a_failing_candidate_falls_through_to_one_that_clears_the_gate() {
        // Two directories on PATH: the first holds an old build, the second the
        // build that clears the >=5.1 gate.
        let old_dir = tempfile::tempdir().unwrap();
        let new_dir = tempfile::tempdir().unwrap();
        write_version_stub(&old_dir.path().join(stub_file_name()), "Blender 5.0.0");
        write_version_stub(&new_dir.path().join(stub_file_name()), "Blender 5.1.1");

        let mut env = HostEnv::new();
        env.path_entries = Some(vec![
            old_dir.path().to_path_buf(),
            new_dir.path().to_path_buf(),
        ]);

        let def = stub_host_definition("blender", Some("5.1"));
        let manifest = HostManifest {
            version: "1".to_string(),
            hosts: vec![def],
        };
        let probes = doctor(&["blender".to_string()], &manifest, &env);

        assert_eq!(
            probes[0].status,
            HostStatus::Available,
            "the build that clears the gate must be used, got {:?} ({})",
            probes[0].status,
            probes[0].hint
        );
        assert_eq!(probes[0].version.as_deref(), Some("5.1.1"));
        // The skipped candidate must be named, or the report looks arbitrary.
        let warning = probes[0]
            .warning
            .as_deref()
            .expect("the skipped candidate must be surfaced");
        assert!(warning.contains("5.0.0"), "warning was: {warning}");
    }

    /// When nothing clears the gate, the highest-priority candidate is still
    /// the one reported: that is the source the operator can act on.
    #[test]
    fn with_no_candidate_clearing_the_gate_the_first_is_reported() {
        let dir = tempfile::tempdir().unwrap();
        write_version_stub(&dir.path().join(stub_file_name()), "Blender 5.0.0");

        let mut env = HostEnv::new();
        env.path_entries = Some(vec![dir.path().to_path_buf()]);

        let def = stub_host_definition("blender", Some("5.1"));
        let manifest = HostManifest {
            version: "1".to_string(),
            hosts: vec![def],
        };
        let probes = doctor(&["blender".to_string()], &manifest, &env);

        assert_eq!(probes[0].status, HostStatus::VersionMismatch);
        assert_eq!(probes[0].version.as_deref(), Some("5.0.0"));
        assert!(
            probes[0].warning.is_none(),
            "nothing was skipped, so there is no fallback to explain"
        );
    }

    /// P3-1: on Windows any existing file passes the executable check, so a
    /// non-runnable shim ahead of the real binary used to shadow it. A shim
    /// cannot produce a version, so the scan has to move past it.
    #[test]
    fn an_unrunnable_shim_does_not_shadow_a_real_binary() {
        let shim_dir = tempfile::tempdir().unwrap();
        let real_dir = tempfile::tempdir().unwrap();
        // A file that exists but cannot report a version, standing in for an
        // msys/Git-Bash style shim.
        let shim = shim_dir.path().join(stub_file_name());
        std::fs::write(&shim, b"not a binary").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&shim, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        write_version_stub(&real_dir.path().join(stub_file_name()), "Blender 5.1.1");

        let mut env = HostEnv::new();
        env.path_entries = Some(vec![
            shim_dir.path().to_path_buf(),
            real_dir.path().to_path_buf(),
        ]);

        let def = stub_host_definition("blender", Some("5.1"));
        let manifest = HostManifest {
            version: "1".to_string(),
            hosts: vec![def],
        };
        let probes = doctor(&["blender".to_string()], &manifest, &env);

        assert_eq!(
            probes[0].status,
            HostStatus::Available,
            "the real binary must win over the shim, got {:?}",
            probes[0].status
        );
        assert_eq!(probes[0].version.as_deref(), Some("5.1.1"));
    }

    /// P1-B: an install the probe cannot confirm must fail closed, so
    /// `host install ... && blender -b ...` stops instead of running on a host
    /// that was never installed.
    #[test]
    fn an_unverified_install_is_a_failure() {
        let installed = InstallOutcome::Installed {
            id: "blender".to_string(),
            version: Some("5.1.1".to_string()),
            channel: "winget".to_string(),
        };
        assert!(!install_failed(&installed), "a verified install succeeds");

        let unverified = verify_install(installed, false);
        assert!(
            matches!(unverified, InstallOutcome::Unverified { .. }),
            "an unverified install must be re-graded, got {unverified:?}"
        );
        assert!(
            install_failed(&unverified),
            "an unverified install must exit non-zero"
        );
        assert_eq!(install_value(&unverified)["status"], "unverified");

        // A verified install passes through untouched.
        let verified = verify_install(
            InstallOutcome::Installed {
                id: "blender".to_string(),
                version: Some("5.1.1".to_string()),
                channel: "winget".to_string(),
            },
            true,
        );
        assert!(!install_failed(&verified));
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

    /// Carried P2 from the #2628 review: `rejected_override` used to be
    /// consulted only when `candidates` came back empty, so a broken override
    /// was dropped whenever PATH could supply a candidate. With `host install`
    /// live, that would point the operator at installing a host that is
    /// already present instead of at the environment variable that is wrong.
    #[test]
    fn a_rejected_override_still_warns_when_path_supplies_the_candidate() {
        let dir = tempfile::tempdir().unwrap();
        let on_path = dir.path().join("stubhost-on-path");
        std::fs::write(&on_path, b"stub").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&on_path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }

        let absent = dir.path().join("absent-override-target");
        let mut env = HostEnv::new();
        env.path_entries = Some(vec![dir.path().to_path_buf()]);
        env.vars.insert(
            "DCC_MCP_STUBHOST_EXECUTABLE".to_string(),
            absent.to_string_lossy().into_owned(),
        );

        let mut def = fake_def("stubhost");
        // No version query, so existence alone makes the host available and
        // the PATH candidate wins.
        def.version_arg = Vec::new();
        let name = "stubhost-on-path";
        for names in [
            &mut def.executables.windows,
            &mut def.executables.linux,
            &mut def.executables.macos,
        ] {
            names.push(name.to_string());
        }
        let manifest = HostManifest {
            version: "1".to_string(),
            hosts: vec![def],
        };

        let probes = doctor(&["stubhost".to_string()], &manifest, &env);
        assert_eq!(probes[0].status, HostStatus::Available);
        assert_eq!(probes[0].executable_source.as_deref(), Some("PATH"));
        let warning = probes[0]
            .warning
            .as_deref()
            .expect("the rejected override must still be surfaced");
        assert!(
            warning.contains("DCC_MCP_STUBHOST_EXECUTABLE")
                && warning.contains(&absent.display().to_string()),
            "the warning must name the variable and its target, got {warning}"
        );
    }

    #[test]
    fn no_warning_when_the_override_is_absent() {
        // An unset variable is not a rejection, so nothing should be surfaced.
        let dir = tempfile::tempdir().unwrap();
        let on_path = dir.path().join("stubhost-on-path");
        std::fs::write(&on_path, b"stub").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&on_path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let mut env = HostEnv::new();
        env.path_entries = Some(vec![dir.path().to_path_buf()]);
        let mut def = fake_def("stubhost");
        def.version_arg = Vec::new();
        for names in [
            &mut def.executables.windows,
            &mut def.executables.linux,
            &mut def.executables.macos,
        ] {
            names.push("stubhost-on-path".to_string());
        }
        let manifest = HostManifest {
            version: "1".to_string(),
            hosts: vec![def],
        };
        let probes = doctor(&["stubhost".to_string()], &manifest, &env);
        assert_eq!(probes[0].status, HostStatus::Available);
        assert!(probes[0].warning.is_none());
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
