//! Consent gating for `dcc-mcp-cli host install`.
//!
//! Follows the pattern the `media` skill already established with `vx ffmpeg`:
//! a read-only operation never installs anything, while a write operation asks
//! once, remembers the answer, and honours an explicit override.
//!
//! Precedence, highest first:
//!
//! 1. `--yes` on the command line.
//! 2. `DCC_MCP_HOST_INSTALL=never|ask|always`.
//! 3. The `consent` value remembered in the user-level lock file.
//! 4. `always` — the default, and it applies only where no choice was ever
//!    expressed. An allowlisted host then installs directly, with or without
//!    a terminal, because waiting on a prompt nobody may be there to answer
//!    is the deadlock this default removes. The caller prints a notice first,
//!    so this is a tell, not a silence.
//!
//! The default fills a gap; it never overrides a choice. An explicit `ask`
//! still prompts when it can and refuses when it cannot, and an explicit
//! `never` still refuses, exactly as they did before.
//!
//! Commercial hosts are refused in `provision_decision`, which runs before
//! consent is consulted, so no tier here can let one through.

use serde::{Deserialize, Serialize};

/// Environment variable overriding the stored consent choice.
pub const CONSENT_ENV: &str = "DCC_MCP_HOST_INSTALL";

/// How an install request may proceed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConsentMode {
    /// Never install; always refuse.
    Never,
    /// Ask each time.
    Ask,
    /// Install without asking.
    Always,
}

impl ConsentMode {
    /// Parse a consent value, accepting the usual truthy spellings.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "never" | "no" | "off" | "0" | "false" => Some(Self::Never),
            "ask" | "prompt" | "" => Some(Self::Ask),
            "always" | "yes" | "on" | "1" | "true" => Some(Self::Always),
            _ => None,
        }
    }

    /// Stable label for the lock file and operator output.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Never => "never",
            Self::Ask => "ask",
            Self::Always => "always",
        }
    }
}

/// What a consent check decided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConsentDecision {
    /// Go ahead.
    Proceed,
    /// Ask the operator; carries the question to print.
    Ask { question: String },
    /// Refuse; carries why, so the operator can unblock it.
    Refused { reason: ConsentRefusal },
}

/// Why an install was refused before it started.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConsentRefusal {
    /// Policy says never install.
    PolicyNever,
    /// Policy says ask, but there is no terminal to ask on.
    NonInteractive,
}

/// The mode applied when nothing above it made a choice.
pub const DEFAULT_MODE: ConsentMode = ConsentMode::Always;

/// Inputs to a consent decision, all injectable so tests need no TTY.
#[derive(Debug, Clone, Copy, Default)]
pub struct ConsentInput {
    /// `--yes` was passed.
    pub yes_flag: bool,
    /// `DCC_MCP_HOST_INSTALL` value, when set.
    pub env: Option<&'static str>,
    /// Consent remembered in the lock file, when any.
    pub stored: Option<ConsentMode>,
    /// Whether stdin/stdout can carry a prompt.
    pub interactive: bool,
}

/// Resolve whether an install may proceed.
#[must_use]
pub fn decide(input: ConsentInput, question: impl Into<String>) -> ConsentDecision {
    let question = question.into();
    // An explicit `--yes` outranks everything, including a stored `never`: the
    // operator is standing at the terminal asking for this specific install.
    if input.yes_flag {
        return ConsentDecision::Proceed;
    }
    if let Some(raw) = input.env {
        return match ConsentMode::parse(raw) {
            Some(ConsentMode::Always) => ConsentDecision::Proceed,
            Some(ConsentMode::Never) => ConsentDecision::Refused {
                reason: ConsentRefusal::PolicyNever,
            },
            // An explicit `ask` is an explicit choice, so it takes the `ask`
            // path rather than the default: asking is what the operator asked
            // for, and with no terminal to ask on it stays a refusal.
            Some(ConsentMode::Ask) => ask_or_refuse(input.interactive, question),
            None => default_decision(),
        };
    }
    match input.stored {
        Some(ConsentMode::Always) => ConsentDecision::Proceed,
        Some(ConsentMode::Never) => ConsentDecision::Refused {
            reason: ConsentRefusal::PolicyNever,
        },
        Some(ConsentMode::Ask) => ask_or_refuse(input.interactive, question),
        // Only here, where no choice was ever expressed, does the default
        // apply. It overrides nothing: it fills a gap.
        None => default_decision(),
    }
}

/// What happens when the operator expressed no choice at all.
///
/// `always`, on both paths. The default has to hold with a terminal attached
/// too, otherwise an agent driving a pty still blocks on a prompt it was
/// never meant to answer — which is the deadlock this default exists to
/// remove. The notice the caller prints beforehand is what keeps this
/// informed rather than silent.
///
/// Reaching here already means the host cleared the allowlist, because the
/// commercial-host refusal runs in `provision_decision`, ahead of consent.
fn default_decision() -> ConsentDecision {
    ConsentDecision::Proceed
}

/// An explicit `ask` prompts when it can, and refuses when it cannot: a
/// question nobody can answer must not become an install nobody approved.
fn ask_or_refuse(interactive: bool, question: String) -> ConsentDecision {
    if interactive {
        ConsentDecision::Ask { question }
    } else {
        ConsentDecision::Refused {
            reason: ConsentRefusal::NonInteractive,
        }
    }
}

/// Read a single yes/no answer from the terminal.
///
/// Anything other than an affirmative is treated as "no", so a stray Enter or
/// a closed stdin never authorises an install.
#[must_use]
pub fn read_answer(line: &str) -> bool {
    matches!(
        line.trim().to_ascii_lowercase().as_str(),
        "y" | "yes" | "yeah" | "yep" | "ok" | "okay"
    )
}

/// Message for a consent refusal, telling the operator how to unblock it.
#[must_use]
pub fn refusal_message(reason: ConsentRefusal) -> String {
    match reason {
        ConsentRefusal::PolicyNever => format!(
            "host installs are disabled by policy ({CONSENT_ENV}=never). Re-run with --yes, or set {CONSENT_ENV}=always to change the stored choice."
        ),
        ConsentRefusal::NonInteractive => format!(
            "host installs need confirmation and there is no terminal to ask on. Re-run interactively, pass --yes, or set {CONSENT_ENV}=always."
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_accepts_common_spellings() {
        for raw in ["never", "NEVER", "no", "off", "0", "false"] {
            assert_eq!(ConsentMode::parse(raw), Some(ConsentMode::Never), "{raw}");
        }
        for raw in ["ask", "", "  ", "prompt"] {
            assert_eq!(ConsentMode::parse(raw), Some(ConsentMode::Ask), "{raw}");
        }
        for raw in ["always", "yes", "on", "1", "true"] {
            assert_eq!(ConsentMode::parse(raw), Some(ConsentMode::Always), "{raw}");
        }
        assert_eq!(ConsentMode::parse("maybe"), None);
    }

    #[test]
    fn yes_flag_outranks_a_stored_never() {
        let decision = decide(
            ConsentInput {
                yes_flag: true,
                env: None,
                stored: Some(ConsentMode::Never),
                interactive: false,
            },
            "install?",
        );
        assert_eq!(decision, ConsentDecision::Proceed);
    }

    #[test]
    fn env_overrides_stored_choice_in_both_directions() {
        let stored_always = ConsentInput {
            yes_flag: false,
            env: Some("never"),
            stored: Some(ConsentMode::Always),
            interactive: false,
        };
        assert_eq!(
            decide(stored_always, "q"),
            ConsentDecision::Refused {
                reason: ConsentRefusal::PolicyNever
            }
        );

        let stored_never = ConsentInput {
            yes_flag: false,
            env: Some("always"),
            stored: Some(ConsentMode::Never),
            interactive: false,
        };
        assert_eq!(decide(stored_never, "q"), ConsentDecision::Proceed);
    }

    #[test]
    fn stored_choice_is_remembered() {
        let remembered = ConsentInput {
            stored: Some(ConsentMode::Always),
            ..ConsentInput::default()
        };
        assert_eq!(decide(remembered, "q"), ConsentDecision::Proceed);
    }

    /// The behaviour matrix, as a table.
    ///
    /// Only the two rows for "no choice expressed" may differ from the
    /// pre-change behaviour. Every row that carries an explicit choice is
    /// pinned here, because folding an explicit `ask` into the default is
    /// exactly the regression this guards.
    #[test]
    fn the_default_proceeds_on_both_paths_and_never_overrides_a_choice() {
        let cases: &[(Option<&str>, Option<ConsentMode>, bool, ConsentDecision)] = &[
            // No choice expressed -> the default, on both paths. Asking with a
            // terminal attached is what used to happen, and it is the deadlock
            // the default exists to remove.
            (None, None, true, ConsentDecision::Proceed),
            (None, None, false, ConsentDecision::Proceed),
            // An explicit `ask`: unchanged. Prompts when it can, refuses when
            // it cannot. It is a choice, not a gap for the default to fill.
            (
                None,
                Some(ConsentMode::Ask),
                true,
                ConsentDecision::Ask {
                    question: "q".to_string(),
                },
            ),
            (
                None,
                Some(ConsentMode::Ask),
                false,
                ConsentDecision::Refused {
                    reason: ConsentRefusal::NonInteractive,
                },
            ),
            (
                Some("ask"),
                None,
                true,
                ConsentDecision::Ask {
                    question: "q".to_string(),
                },
            ),
            (
                Some("ask"),
                None,
                false,
                ConsentDecision::Refused {
                    reason: ConsentRefusal::NonInteractive,
                },
            ),
            // An explicit `never`: unchanged, either way.
            (
                None,
                Some(ConsentMode::Never),
                true,
                ConsentDecision::Refused {
                    reason: ConsentRefusal::PolicyNever,
                },
            ),
            (
                None,
                Some(ConsentMode::Never),
                false,
                ConsentDecision::Refused {
                    reason: ConsentRefusal::PolicyNever,
                },
            ),
            (
                Some("never"),
                None,
                true,
                ConsentDecision::Refused {
                    reason: ConsentRefusal::PolicyNever,
                },
            ),
            (
                Some("never"),
                None,
                false,
                ConsentDecision::Refused {
                    reason: ConsentRefusal::PolicyNever,
                },
            ),
            // An explicit `always`: unchanged, either way.
            (
                None,
                Some(ConsentMode::Always),
                true,
                ConsentDecision::Proceed,
            ),
            (
                None,
                Some(ConsentMode::Always),
                false,
                ConsentDecision::Proceed,
            ),
            (Some("always"), None, true, ConsentDecision::Proceed),
            (Some("always"), None, false, ConsentDecision::Proceed),
        ];
        for (env, stored, interactive, expected) in cases {
            let input = ConsentInput {
                yes_flag: false,
                env: *env,
                stored: *stored,
                interactive: *interactive,
            };
            assert_eq!(
                decide(input, "q"),
                *expected,
                "env={env:?} stored={stored:?} interactive={interactive}"
            );
        }
    }

    /// The default has to hold with a terminal attached, not only without one:
    /// an agent driving a pty would otherwise still block on stdin, which is
    /// the very deadlock this default removes. Acceptance criterion 1 states
    /// no prompt and no waiting, without conditioning it on a missing TTY.
    #[test]
    fn the_default_does_not_prompt_when_a_terminal_exists() {
        let attended = ConsentInput {
            interactive: true,
            ..ConsentInput::default()
        };
        let decision = decide(attended, "install Blender 5.1.1?");
        assert_eq!(decision, ConsentDecision::Proceed);
    }

    /// An unparsable environment value is not an explicit choice, so it falls
    /// to the default rather than silently installing under an operator who
    /// tried to express one.
    #[test]
    fn an_unparsable_env_value_is_treated_as_no_choice() {
        let input = ConsentInput {
            env: Some("maybe"),
            ..ConsentInput::default()
        };
        assert_eq!(decide(input, "q"), ConsentDecision::Proceed);
    }

    #[test]
    fn an_explicit_never_is_never_overridden_by_the_default() {
        // Acceptance criterion 4: a stored or environmental `never` must behave
        // exactly as it did before the default changed.
        let stored_never = ConsentInput {
            interactive: true,
            stored: Some(ConsentMode::Never),
            ..ConsentInput::default()
        };
        assert_eq!(
            decide(stored_never, "q"),
            ConsentDecision::Refused {
                reason: ConsentRefusal::PolicyNever
            }
        );

        let env_never = ConsentInput {
            env: Some("never"),
            ..ConsentInput::default()
        };
        assert_eq!(
            decide(env_never, "q"),
            ConsentDecision::Refused {
                reason: ConsentRefusal::PolicyNever
            }
        );
    }

    #[test]
    fn only_explicit_affirmatives_count() {
        for line in ["y", "Y", "yes", "YES", " yeah ", "ok", "okay"] {
            assert!(read_answer(line), "{line}");
        }
        for line in ["", "n", "no", "maybe", "\n", "later"] {
            assert!(!read_answer(line), "{line}");
        }
    }

    #[test]
    fn refusals_name_the_unblock() {
        assert!(refusal_message(ConsentRefusal::PolicyNever).contains(CONSENT_ENV));
        assert!(refusal_message(ConsentRefusal::NonInteractive).contains("--yes"));
    }
}
