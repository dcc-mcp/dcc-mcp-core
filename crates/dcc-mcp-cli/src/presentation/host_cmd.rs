//! `dcc-mcp-cli host` command surface.
//!
//! `list` and `doctor` are read-only: they report whether a host binary exists
//! and whether its version clears the manifest baseline, and they never write
//! to disk or spawn an installer. `install` and `pin` are the self-provisioning
//! half and are gated by the consent model.

use std::io::{BufRead, IsTerminal, Write};
use std::path::PathBuf;

use anyhow::Context;
use serde_json::Value;

use crate::application::host::consent::{self, ConsentDecision, ConsentInput, ConsentMode};
use crate::application::host::detect::HostEnv;
use crate::application::host::manifest::{self, HostDefinition, HostLock, HostManifest};
use crate::application::host::{
    ProvisionQuery, ProvisionRequest, doctor, doctor_value, has_unavailable, install_failed,
    install_value, manifest as host_manifest, provision, provision_decision,
};
use crate::domain::host::HostSpec;

#[derive(Debug, clap::Subcommand)]
pub(crate) enum HostAction {
    /// List every host in the manifest with its licence class and pinned version.
    List,
    /// Report host availability. Read-only: never installs anything.
    Doctor {
        /// Host specs such as `blender`, `blender==5.1`, `blender>=5.1`.
        /// Omit to probe every known host.
        #[arg(value_name = "SPEC")]
        specs: Vec<String>,
    },
    /// Provision an open-source host at the manifest-pinned version.
    Install {
        /// Host spec, e.g. `blender` or `blender==5.1.1`. The version must
        /// match the manifest pin; provisioning an arbitrary version would
        /// reintroduce version drift.
        #[arg(value_name = "SPEC")]
        spec: String,
        /// Skip the consent prompt and remember the choice.
        #[arg(long, short = 'y')]
        yes: bool,
    },
    /// Record a pinned host version in the user-level lock file.
    Pin {
        /// Host spec carrying the version to pin, e.g. `blender==5.1.1`.
        #[arg(value_name = "SPEC")]
        spec: String,
    },
}

/// Environment variable naming the user-level lock file.
const LOCK_ENV: &str = "DCC_MCP_HOSTS_LOCK";

/// Run the `host` command tree.
pub(crate) fn run(action: &HostAction) -> anyhow::Result<HostRun> {
    match action {
        HostAction::List => Ok(HostRun {
            value: load_manifest()?.to_list_value(),
            failed: false,
        }),
        HostAction::Doctor { specs } => {
            let manifest = load_manifest()?;
            let probes = doctor(specs, &manifest, &HostEnv::from_process());
            let failed = has_unavailable(&probes);
            Ok(HostRun {
                value: doctor_value(&probes),
                failed,
            })
        }
        HostAction::Install { spec, yes } => {
            let manifest = load_manifest()?;
            let parsed =
                HostSpec::parse(spec).with_context(|| format!("invalid host spec '{spec}'"))?;
            let Some(def) = manifest.find(&parsed.id) else {
                anyhow::bail!(
                    "'{}' is not in the host manifest; run `dcc-mcp-cli host list` to see the known hosts",
                    parsed.id
                );
            };
            // Reject a version the manifest pin cannot satisfy before probing,
            // consenting, or running a channel. Without this, `host install
            // blender==4.2` would install 5.1.1 and report success, which is
            // the version drift the manifest is supposed to prevent.
            if parsed.op.is_some() {
                let pin = def
                    .pinned_version
                    .as_deref()
                    .and_then(crate::domain::host::parse_manifest_version);
                let gate = parsed.gate(def.min_version.as_deref());
                if !pin.as_ref().is_some_and(|pin| gate.satisfied_by(pin)) {
                    anyhow::bail!(
                        "'{}' does not match the pinned version {}; run `host pin` first",
                        spec,
                        def.pinned_version.as_deref().unwrap_or("<none>")
                    );
                }
            }

            // Probe first: an already-usable host needs no install, and the
            // refusal check must run before any consent prompt.
            let env = HostEnv::from_process();
            let probes = doctor(std::slice::from_ref(&parsed.id), &manifest, &env);
            let probe = &probes[0];

            let question = format!(
                "Host '{}' is missing. Install {} {} now? The version comes from the host manifest.",
                def.id,
                def.display_name,
                def.pinned_version
                    .as_deref()
                    .unwrap_or("the pinned version")
            );
            let consent = resolve_consent(*yes, question);
            // "Stop asking" does not mean "stop telling": the default installs
            // unattended, so this notice is the only trace of what is about to
            // happen. It is gated so that it fires exactly when an install is
            // genuinely about to start, and never otherwise — which includes
            // consent: a host the operator has not agreed to install is not
            // about to be installed, so announcing it would claim an install
            // that `provision` is about to refuse.
            if consent_allows_install(&consent) && will_install(def, probe) {
                announce_install(def);
            }
            let outcome = provision(
                def,
                &ProvisionRequest {
                    current_available: probe.status.is_available(),
                    current_version: probe.version.clone(),
                    consent: consent.clone(),
                },
            );
            // Asking is the only branch that needs the terminal, and it happens
            // after the commercial-host refusal has already been enforced.
            let outcome = match (&outcome, &consent) {
                (_, ConsentDecision::Ask { question }) if needs_consent(&outcome) => {
                    match ask(question) {
                        true => {
                            // The operator just approved this specific install,
                            // so the notice is owed here too: the interactive
                            // path must not be the one path that installs
                            // without saying where or how to turn it off.
                            // Consent is `Proceed` by construction here, so
                            // this is the same gate as the one above and the
                            // notice is printed exactly once on this path.
                            if consent_allows_install(&ConsentDecision::Proceed)
                                && will_install(def, probe)
                            {
                                announce_install(def);
                            }
                            remember_consent(ConsentMode::Always);
                            provision(
                                def,
                                &ProvisionRequest {
                                    current_available: probe.status.is_available(),
                                    current_version: probe.version.clone(),
                                    consent: ConsentDecision::Proceed,
                                },
                            )
                        }
                        false => outcome,
                    }
                }
                _ => outcome,
            };

            // Re-probe after a successful install so the report states what is
            // actually on disk rather than what the channel claims, and so an
            // install that did not take effect fails closed instead of exiting 0.
            let channel = match &outcome {
                crate::application::host::InstallOutcome::Installed { channel, .. } => {
                    Some(channel.clone())
                }
                _ => None,
            };
            let outcome = if let Some(channel) = channel {
                let after = doctor(std::slice::from_ref(&parsed.id), &manifest, &env);
                let verified = after[0].status.is_available();
                let observed_version = after[0].version.clone();
                let executable = after[0]
                    .executable
                    .as_ref()
                    .map(|path| path.display().to_string());
                let outcome = crate::application::host::verify_install(outcome, verified);
                let mut value = install_value(&outcome);
                if let Some(object) = value.as_object_mut() {
                    object.insert("channel".to_string(), Value::from(channel));
                    object.insert("verified".to_string(), Value::from(verified));
                    // Report the version the probe observes, not the one we
                    // asked for: the channel is what decides what lands.
                    if let Some(observed) = observed_version {
                        object.insert("version".to_string(), Value::from(observed));
                    }
                    object.insert(
                        "executable".to_string(),
                        executable.map(Value::from).unwrap_or(Value::Null),
                    );
                }
                return Ok(HostRun {
                    failed: install_failed(&outcome),
                    value,
                });
            } else {
                outcome
            };
            Ok(HostRun {
                failed: install_failed(&outcome),
                value: install_value(&outcome),
            })
        }
        HostAction::Pin { spec } => {
            let manifest = load_manifest()?;
            let parsed =
                HostSpec::parse(spec).with_context(|| format!("invalid host spec '{spec}'"))?;
            let Some(def) = manifest.find(&parsed.id) else {
                anyhow::bail!(
                    "'{}' is not in the host manifest; run `dcc-mcp-cli host list` to see the known hosts",
                    parsed.id
                );
            };
            let version = parsed
                .version
                .as_ref()
                .map(|_| render_pinned(&parsed))
                .or_else(|| def.pinned_version.clone())
                .or_else(|| def.min_version.clone())
                .with_context(|| format!("'{}' has no version to pin", parsed.id))?;
            // The floor has to hold here, not only in `load_with_lock`: the
            // lock is written by this arm, so letting a low pin through would
            // put every later command in the position of refusing to load a
            // file this CLI itself produced.
            if def.is_below_minimum(&version) {
                anyhow::bail!(
                    "'{}' pins {} below the minimum version {} for {}; the manifest rules that build out",
                    spec,
                    version,
                    def.min_version.as_deref().unwrap_or("<none>"),
                    def.display_name
                );
            }

            let path =
                lock_path().context("no user config directory is available for the host lock")?;
            let mut lock = load_lock()?;
            lock.pins.insert(def.id.clone(), version.clone());
            lock.save(&path)
                .with_context(|| format!("failed to write {}", path.display()))?;
            Ok(HostRun {
                value: serde_json::json!({
                    "id": def.id,
                    "pinned_version": version,
                    "lock_path": path,
                }),
                failed: false,
            })
        }
    }
}

/// Result of one `host` invocation.
pub(crate) struct HostRun {
    pub value: Value,
    /// Whether the command found something unusable, which drives the exit code.
    pub failed: bool,
}

/// Whether an outcome reached the point where consent is the next gate.
fn needs_consent(outcome: &crate::application::host::InstallOutcome) -> bool {
    matches!(
        outcome,
        crate::application::host::InstallOutcome::ConsentRequired {
            reason: consent::ConsentRefusal::NonInteractive,
            ..
        }
    )
}

/// Resolve the consent decision from the flag, the environment, and the lock.
///
/// `--yes` outranks `DCC_MCP_HOST_INSTALL`, which outranks the remembered
/// choice, which falls back to `always` when no choice was ever expressed.
/// The default only ever applies to hosts the manifest allows the CLI to
/// provision: the commercial-host refusal runs in `provision_decision`,
/// ahead of consent.
fn resolve_consent(yes: bool, question: String) -> ConsentDecision {
    let env_mode = std::env::var(consent::CONSENT_ENV)
        .ok()
        .and_then(|value| ConsentMode::parse(&value));
    let stored = load_lock()
        .ok()
        .and_then(|lock| lock.consent.as_deref().and_then(ConsentMode::parse));
    let interactive = std::io::stdin().is_terminal() && std::io::stdout().is_terminal();
    let input = ConsentInput {
        yes_flag: yes,
        env: None,
        // An explicit env mode and a stored mode carry the same weight class,
        // so fold the env into the stored slot after the flag check.
        stored: env_mode.or(stored),
        interactive,
    };
    consent::decide(input, question)
}

/// Whether consent has already been given for an install to proceed.
///
/// The notice is a claim about what is about to happen, so an unresolved
/// `Ask` or a `Refused` decision means nothing is about to be installed yet.
/// Checking this alongside `will_install` keeps the notice from announcing an
/// install that `provision` refuses, and from printing a second time once an
/// interactive prompt is accepted.
fn consent_allows_install(consent: &ConsentDecision) -> bool {
    matches!(consent, ConsentDecision::Proceed)
}

/// Whether an install of `def` is genuinely about to start.
///
/// The notice is a claim about what is about to happen, so it may only fire
/// when the install will actually run. Two things about the host can stop
/// that, and both are checked here rather than in a second, drifting copy:
///
/// * the host is commercial or has no channel — `provision_decision`;
/// * the host is already usable — `provision` returns `AlreadySatisfied`
///   before it ever looks at consent;
///
/// so a host that will be refused, or one that needs no work, is never
/// announced as if it were being installed. Whether consent has been given is
/// a separate question, answered by `consent_allows_install`; callers that
/// announce must require both.
fn will_install(def: &HostDefinition, probe: &crate::application::host::HostProbe) -> bool {
    !probe.status.is_available()
        && provision_decision(
            def,
            ProvisionQuery {
                channel_available: def.install_channel_for_current().is_some(),
            },
        )
        .is_none()
}

/// Print what is about to be installed, before anything is downloaded or
/// written.
///
/// The default consent mode is `always`, so an allowlisted host installs
/// without asking. This notice is what keeps that from being silent: it names
/// the host, the pinned version, where it lands, and how to turn the behaviour
/// off. It is a statement, not a question, so it never waits on stdin.
fn announce_install(def: &HostDefinition) {
    eprintln!("{}", render_notice(def));
}

/// The notice text itself, kept separate so it can be asserted without
/// capturing stderr.
#[must_use]
fn render_notice(def: &HostDefinition) -> String {
    let version = def
        .pinned_version
        .as_deref()
        .unwrap_or("the pinned version");
    format!(
        "dcc-mcp: installing host '{}' ({} {version}) into {}. Set {}=never to disable host installs.",
        def.id,
        def.display_name,
        install_destination(def),
        consent::CONSENT_ENV,
    )
}

/// Where an install for `def` will land, as text an operator can act on.
///
/// A package manager owns its own location, which the manifest search roots
/// already cover; only an archive channel has a path this CLI chooses. Saying
/// the wrong one would send someone looking in a directory that is never used.
fn install_destination(def: &HostDefinition) -> String {
    match def.install_channel_for_current() {
        Some(manifest::InstallChannel::Tarball { .. }) => {
            crate::application::host::install::managed_host_dir(&def.id)
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "the dcc-mcp managed host directory".to_string())
        }
        Some(_) => "the location its package manager chooses".to_string(),
        None => "the location its install channel chooses".to_string(),
    }
}

/// Ask the operator once, on stderr so stdout stays parseable.
fn ask(question: &str) -> bool {
    eprintln!("{question}");
    eprint!("Type 'y' to continue, anything else to cancel: ");
    let _ = std::io::stdout().flush();
    let stdin = std::io::stdin();
    let mut line = String::new();
    match stdin.lock().read_line(&mut line) {
        Ok(_) => consent::read_answer(&line),
        Err(_) => false,
    }
}

/// Persist a consent choice in the lock file. Best effort: a read-only
/// filesystem must not turn an approved install into a failure.
fn remember_consent(mode: ConsentMode) {
    let Some(path) = lock_path() else {
        return;
    };
    if let Ok(mut lock) = load_lock() {
        lock.consent = Some(mode.as_str().to_string());
        let _ = lock.save(&path);
    }
}

/// Render a spec's version at the precision it was written with.
fn render_pinned(spec: &HostSpec) -> String {
    match spec.version.as_ref() {
        Some(version) => match spec.components {
            1 => format!("{}", version.major),
            2 => format!("{}.{}", version.major, version.minor),
            _ => version.to_string(),
        },
        None => spec.id.clone(),
    }
}

/// Load the manifest with the user-level lock applied.
fn load_manifest() -> anyhow::Result<HostManifest> {
    let lock = load_lock()?;
    host_manifest::load_with_lock(&lock).context("failed to load the host manifest")
}

/// Read the lock file named by `DCC_MCP_HOSTS_LOCK`, or the default location.
fn load_lock() -> anyhow::Result<HostLock> {
    manifest::load_lock(lock_path().as_deref()).context("failed to read the host version lock")
}

/// Path of the user-level lock file.
fn lock_path() -> Option<PathBuf> {
    std::env::var_os(LOCK_ENV)
        .map(PathBuf::from)
        .or_else(|| dirs::config_dir().map(|dir| dir.join("dcc-mcp").join("hosts.lock")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::host::HostStatus;

    fn blender() -> HostDefinition {
        crate::application::host::manifest::bundled()
            .find("blender")
            .expect("blender")
            .clone()
    }

    #[test]
    fn list_reports_the_manifest_without_probing() {
        let HostRun { value, failed } = run(&HostAction::List).unwrap();
        assert!(!failed);
        let hosts = value["hosts"].as_array().expect("hosts array");
        assert!(!hosts.is_empty());
        assert!(hosts.iter().any(|host| host["id"] == "blender"));
    }

    #[test]
    fn doctor_of_an_absent_host_is_missing() {
        let HostRun { value, failed } = run(&HostAction::Doctor {
            specs: vec!["definitely-not-a-host".to_string()],
        })
        .unwrap();
        assert!(failed);
        assert_eq!(value["hosts"][0]["status"], "unknown_host");
        assert_eq!(value["read_only"], true);
    }

    #[test]
    fn installing_a_commercial_host_is_refused_before_consent() {
        // Acceptance criterion 4. The refusal must not depend on consent or on
        // a terminal being present.
        let HostRun { value, failed } = run(&HostAction::Install {
            spec: "maya".to_string(),
            yes: true,
        })
        .unwrap();
        assert!(failed, "installing maya must fail");
        assert_eq!(value["status"], "refused");
        assert_eq!(value["reason"], "commercial");
        assert_eq!(value["installed"], false);
        assert!(
            value["hint"]
                .as_str()
                .is_some_and(|hint| hint.contains("licence-gated")),
            "the hint must tell the user to install it themselves"
        );
    }

    #[test]
    fn installing_an_unknown_host_fails_rather_than_no_op() {
        assert!(
            run(&HostAction::Install {
                spec: "not-a-host".to_string(),
                yes: true,
            })
            .is_err()
        );
    }

    #[test]
    fn pin_rejects_unknown_hosts_and_bad_specs() {
        assert!(
            run(&HostAction::Pin {
                spec: "not-a-host==1.0".to_string(),
            })
            .is_err()
        );
        assert!(
            run(&HostAction::Pin {
                spec: "blender>=five".to_string(),
            })
            .is_err()
        );
    }

    #[test]
    fn consent_defaults_to_proceeding_without_a_terminal() {
        // With no explicit choice an allowlisted host installs directly. Asking
        // is what used to happen, and with no terminal the question could never
        // be answered, so every unattended install was refused. The caller
        // prints a notice first: informed, not silent.
        let decision = consent::decide(
            ConsentInput {
                yes_flag: false,
                env: None,
                stored: None,
                interactive: false,
            },
            "q",
        );
        assert_eq!(decision, ConsentDecision::Proceed);
    }

    #[test]
    fn an_explicit_never_still_refuses_every_time() {
        // Acceptance criterion 5: the CI safety net is untouched by the
        // default, and it must hold with or without a terminal.
        for interactive in [true, false] {
            let decision = consent::decide(
                ConsentInput {
                    yes_flag: false,
                    env: Some("never"),
                    stored: None,
                    interactive,
                },
                "q",
            );
            assert_eq!(
                decision,
                ConsentDecision::Refused {
                    reason: consent::ConsentRefusal::PolicyNever
                },
                "interactive={interactive}"
            );
        }
    }

    /// A synthetic probe carrying only the status the notice gate reads.
    fn probe_with(status: crate::domain::host::HostStatus) -> crate::application::host::HostProbe {
        crate::application::host::HostProbe {
            id: String::new(),
            display_name: String::new(),
            status,
            reason: crate::domain::host::HostReason::Ok,
            executable: None,
            executable_source: None,
            version: None,
            gate: None,
            expected: Value::Null,
            license: String::new(),
            self_provision: false,
            sources_checked: Vec::new(),
            hint: String::new(),
            warning: None,
        }
    }

    /// The notice is a claim about what is about to happen. Showing it for a
    /// host that is about to be refused is worse than showing nothing.
    #[test]
    fn a_commercial_host_is_never_announced_as_being_installed() {
        let manifest = crate::application::host::manifest::bundled();
        let maya = manifest.find("maya").expect("maya").clone();
        assert!(
            !will_install(&maya, &probe_with(HostStatus::Missing)),
            "maya is refused, so it must never be announced"
        );
    }

    /// `provision` short-circuits to `AlreadySatisfied` before it looks at
    /// consent, so a host that needs no work must not be announced as if it
    /// were being installed.
    #[test]
    fn an_already_available_host_is_never_announced_as_being_installed() {
        let def = blender();
        assert!(
            !will_install(&def, &probe_with(HostStatus::Available)),
            "an already-available host is not about to be installed"
        );
    }

    /// The positive case: a missing, allowlisted host is announced.
    #[test]
    fn a_missing_allowlisted_host_is_announced() {
        let def = blender();
        assert!(
            will_install(&def, &probe_with(HostStatus::Missing)),
            "a missing allowlisted host is about to be installed"
        );
        // A version mismatch is still an install: the pinned build is absent.
        assert!(
            will_install(&def, &probe_with(HostStatus::VersionMismatch)),
            "a host below the baseline is still about to be installed"
        );
    }

    #[test]
    fn the_install_notice_names_the_host_version_and_the_off_switch() {
        // Acceptance criterion 3: a tell, not a question, and not a silence.
        let def = blender();
        let notice = render_notice(&def);
        assert!(notice.contains("blender"), "got {notice}");
        assert!(notice.contains("5.1.1"), "got {notice}");
        assert!(
            notice.contains(consent::CONSENT_ENV),
            "the notice must say how to disable installs, got {notice}"
        );
        assert!(
            !notice.contains('?'),
            "a notice must not read as a question, got {notice}"
        );
        // Criterion 3 asks for the destination too, and for how to switch the
        // behaviour off. Both have to be in the text an operator actually sees.
        assert!(
            notice.contains(&install_destination(&def)),
            "the notice must name where the host lands, got {notice}"
        );
        assert!(
            notice.contains("never"),
            "the notice must name the off switch, got {notice}"
        );
    }

    /// The interactive path accepts by re-entering `provision` with `Proceed`,
    /// so it is the one path that could install without ever printing the
    /// notice. Pin the condition that decides it, so that branch cannot drift
    /// back to silence: a missing, allowlisted host still owes the notice
    /// after the operator answers yes.
    #[test]
    fn an_accepted_interactive_prompt_still_owes_the_notice() {
        let def = blender();
        // `needs_consent` gates on `ConsentRequired { NonInteractive }`, which
        // is what `provision` returns for an unresolved `Ask`. Reaching the
        // accept branch therefore requires the host to be missing, and the
        // notice condition must hold under exactly those circumstances.
        let unresolved = provision(
            &def,
            &ProvisionRequest {
                current_available: false,
                current_version: None,
                consent: ConsentDecision::Ask {
                    question: "q".to_string(),
                },
            },
        );
        assert!(
            needs_consent(&unresolved),
            "an unresolved Ask must reach the prompt branch, got {unresolved:?}"
        );
        assert!(
            will_install(&def, &probe_with(HostStatus::Missing)),
            "the operator answered yes, so the install is about to start"
        );
        // The accept branch re-enters `provision` with `Proceed`, so consent
        // is satisfied by the time that copy of the gate runs.
        assert!(
            consent_allows_install(&ConsentDecision::Proceed),
            "an accepted prompt has consent, so the notice is owed"
        );
    }

    /// A host that is missing and allowlisted is still not about to be
    /// installed if consent was refused: `provision` will refuse it. The
    /// notice must therefore stay silent, or it claims an install the same
    /// run goes on to decline.
    #[test]
    fn a_refused_host_is_never_announced_as_being_installed() {
        let def = blender();
        assert!(
            will_install(&def, &probe_with(HostStatus::Missing)),
            "precondition: the host itself is installable"
        );
        assert!(
            !consent_allows_install(&ConsentDecision::Refused {
                reason: consent::ConsentRefusal::PolicyNever,
            }),
            "DCC_MCP_HOST_INSTALL=never is refused, so it must never be announced"
        );
    }

    /// An unresolved `Ask` is the interactive path. Consent has not been given
    /// yet, so the pre-prompt gate must not announce: either the operator
    /// declines, or the accept branch prints the notice itself. Announcing
    /// here would print it twice on the accept path.
    #[test]
    fn an_unresolved_ask_is_not_announced_before_the_prompt() {
        assert!(
            !consent_allows_install(&ConsentDecision::Ask {
                question: "q".to_string(),
            }),
            "consent is not granted until the operator answers, so no notice yet"
        );
    }

    #[test]
    fn provision_refuses_commercial_hosts_even_when_consent_is_granted() {
        let manifest = crate::application::host::manifest::bundled();
        let maya = manifest.find("maya").expect("maya").clone();
        let outcome = provision(
            &maya,
            &ProvisionRequest {
                current_available: false,
                current_version: None,
                consent: ConsentDecision::Proceed,
            },
        );
        assert!(matches!(
            outcome,
            crate::application::host::InstallOutcome::Refused { .. }
        ));
        assert!(install_failed(&outcome));
    }

    #[test]
    fn provision_skips_an_already_usable_host() {
        let def = blender();
        let outcome = provision(
            &def,
            &ProvisionRequest {
                current_available: true,
                current_version: Some("5.1.1".to_string()),
                consent: ConsentDecision::Proceed,
            },
        );
        assert!(matches!(
            outcome,
            crate::application::host::InstallOutcome::AlreadySatisfied { .. }
        ));
        assert!(
            !install_failed(&outcome),
            "a satisfied host is not a failure"
        );
    }

    #[test]
    fn provision_reports_consent_refusals() {
        let def = blender();
        let outcome = provision(
            &def,
            &ProvisionRequest {
                current_available: false,
                current_version: None,
                consent: ConsentDecision::Refused {
                    reason: consent::ConsentRefusal::PolicyNever,
                },
            },
        );
        assert!(matches!(
            outcome,
            crate::application::host::InstallOutcome::ConsentRequired { .. }
        ));
    }

    #[test]
    fn render_pinned_keeps_declared_precision() {
        assert_eq!(
            render_pinned(&HostSpec::parse("blender==5.1").unwrap()),
            "5.1"
        );
        assert_eq!(
            render_pinned(&HostSpec::parse("blender==5.1.1").unwrap()),
            "5.1.1"
        );
        assert_eq!(render_pinned(&HostSpec::parse("blender~=5").unwrap()), "5");
    }
}
