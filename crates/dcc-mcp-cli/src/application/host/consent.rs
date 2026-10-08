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
//! 4. `always` — the default. With no explicit choice, an allowlisted host is
//!    installed directly, without waiting on a terminal that may not exist.
//!    The caller prints a notice first, so this is a tell, not a silence.
//!    Commercial hosts are refused in `provision_decision`, which runs before
//!    consent is consulted, so the default never lets one through.

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
            _ => ask_or_default(input.interactive, question),
        };
    }
    match input.stored {
        Some(ConsentMode::Always) => ConsentDecision::Proceed,
        Some(ConsentMode::Never) => ConsentDecision::Refused {
            reason: ConsentRefusal::PolicyNever,
        },
        _ => ask_or_default(input.interactive, question),
    }
}

/// An explicit `ask` prompts when it can, and falls through to the default
/// when it cannot: a question nobody can answer must not become a refusal.
///
/// Reaching here already means the host cleared the allowlist, because the
/// commercial-host refusal runs in `provision_decision`, ahead of consent.
fn ask_or_default(interactive: bool, question: String) -> ConsentDecision {
    match (interactive, DEFAULT_MODE) {
        (true, _) => ConsentDecision::Ask { question },
        (false, ConsentMode::Always) => ConsentDecision::Proceed,
        (false, ConsentMode::Never | ConsentMode::Ask) => ConsentDecision::Refused {
            reason: ConsentRefusal::NonInteractive,
        },
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

    #[test]
    fn the_default_proceeds_without_a_terminal() {
        // An allowlisted host must install unattended rather than deadlock on
        // a question nobody can answer. The caller prints a notice first, so
        // this is a tell, not a silence.
        let unattended = ConsentInput::default();
        assert_eq!(
            decide(unattended, "install Blender?"),
            ConsentDecision::Proceed
        );
    }

    #[test]
    fn an_explicit_ask_still_prompts_when_a_terminal_exists() {
        let attended = ConsentInput {
            interactive: true,
            stored: Some(ConsentMode::Ask),
            ..ConsentInput::default()
        };
        assert_eq!(
            decide(attended, "install Blender 5.1.1?"),
            ConsentDecision::Ask {
                question: "install Blender 5.1.1?".to_string()
            }
        );
    }

    #[test]
    fn an_explicit_ask_falls_through_to_the_default_without_a_terminal() {
        // A question with no one to answer it must not become a refusal.
        let unattended = ConsentInput {
            interactive: false,
            stored: Some(ConsentMode::Ask),
            ..ConsentInput::default()
        };
        assert_eq!(decide(unattended, "q"), ConsentDecision::Proceed);
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
