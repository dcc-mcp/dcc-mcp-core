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
//! 4. `ask` — the default. With no TTY to ask on, that resolves to a refusal,
//!    never to a silent install.

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
            _ => ask_or_refuse(input.interactive, question),
        };
    }
    match input.stored {
        Some(ConsentMode::Always) => ConsentDecision::Proceed,
        Some(ConsentMode::Never) => ConsentDecision::Refused {
            reason: ConsentRefusal::PolicyNever,
        },
        _ => ask_or_refuse(input.interactive, question),
    }
}

/// `ask` only works with a terminal. Without one, refusing is the safe answer:
/// an agent running unattended must not install a host nobody approved.
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

    #[test]
    fn ask_becomes_a_refusal_without_a_terminal() {
        // The default must never install silently under an unattended agent.
        let unattended = ConsentInput::default();
        assert_eq!(
            decide(unattended, "install Blender?"),
            ConsentDecision::Refused {
                reason: ConsentRefusal::NonInteractive
            }
        );
    }

    #[test]
    fn ask_prompts_when_a_terminal_exists() {
        let attended = ConsentInput {
            interactive: true,
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
