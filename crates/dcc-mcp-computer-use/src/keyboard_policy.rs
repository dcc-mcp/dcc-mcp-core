//! Shared keyboard alias normalization and scope-escape policy.

/// Canonical keys whose aliases affect the native-input safety boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CommonKey {
    Control(u16),
    Shift(u16),
    Alt(u16),
    Meta,
    Escape,
    Tab,
    Space,
    PrintScreen,
}

impl CommonKey {
    #[allow(dead_code)]
    pub(crate) const fn virtual_key(self) -> Option<u16> {
        match self {
            Self::Control(value) | Self::Shift(value) | Self::Alt(value) => Some(value),
            Self::Escape => Some(0x1B),
            Self::Tab => Some(0x09),
            Self::Space => Some(0x20),
            Self::PrintScreen => Some(0x2C),
            Self::Meta => None,
        }
    }
}

/// Consequence classification for a canonical keyboard chord.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ShortcutRisk {
    None,
    ActionConfirmation,
    HardDeny,
}

/// Resolve every accepted alias for policy-sensitive keys.
pub(crate) fn common_key(key: &str) -> Option<CommonKey> {
    let upper = key.trim().to_ascii_uppercase();
    Some(match upper.as_str() {
        "CTRL" | "CONTROL" => CommonKey::Control(0x11),
        "LCTRL" | "LEFTCTRL" | "LEFT_CTRL" | "CTRL_L" | "CONTROL_L" => CommonKey::Control(0xA2),
        "RCTRL" | "RIGHTCTRL" | "RIGHT_CTRL" | "CTRL_R" | "CONTROL_R" => CommonKey::Control(0xA3),
        "SHIFT" => CommonKey::Shift(0x10),
        "LSHIFT" | "LEFTSHIFT" | "LEFT_SHIFT" | "SHIFT_L" => CommonKey::Shift(0xA0),
        "RSHIFT" | "RIGHTSHIFT" | "RIGHT_SHIFT" | "SHIFT_R" => CommonKey::Shift(0xA1),
        "ALT" => CommonKey::Alt(0x12),
        "LALT" | "LEFTALT" | "LEFT_ALT" | "ALT_L" => CommonKey::Alt(0xA4),
        "RALT" | "RIGHTALT" | "RIGHT_ALT" | "ALT_R" | "ALTGR" => CommonKey::Alt(0xA5),
        "META" | "WIN" | "WINDOWS" | "SUPER" | "CMD" | "COMMAND" => CommonKey::Meta,
        "ESC" | "ESCAPE" => CommonKey::Escape,
        "TAB" => CommonKey::Tab,
        "SPACE" => CommonKey::Space,
        "PRINTSCREEN" | "PRINT_SCREEN" | "PRTSC" => CommonKey::PrintScreen,
        _ => return None,
    })
}

pub(crate) fn function_key_virtual_key(key: &str) -> Option<u16> {
    let upper = key.trim().to_ascii_uppercase();
    let number = upper.strip_prefix('F')?.parse::<u16>().ok()?;
    (1..=24).contains(&number).then_some(0x70 + number - 1)
}

/// Return true when pointer actions use only non-system keyboard modifiers.
pub(crate) fn are_pointer_modifiers(keys: &[String]) -> bool {
    keys.iter().all(|item| {
        let mut saw_token = false;
        let valid = item.split('+').all(|token| {
            let token = token.trim();
            if token.is_empty() {
                return false;
            }
            saw_token = true;
            matches!(
                common_key(token),
                Some(CommonKey::Control(_) | CommonKey::Shift(_) | CommonKey::Alt(_))
            )
        });
        saw_token && valid
    })
}

/// Return true when a keypress can synthesize ordinary text without Ctrl or Alt.
pub(crate) fn is_unmodified_text_entry(keys: &[String]) -> bool {
    let mut shortcut_modifier_active = false;
    let mut text_modifier_active = false;
    for token in keys.iter().flat_map(|item| item.split('+')).map(str::trim) {
        if token.is_empty() {
            continue;
        }
        match common_key(token) {
            Some(CommonKey::Alt(0xA5)) => text_modifier_active = true,
            Some(CommonKey::Control(_) | CommonKey::Alt(_) | CommonKey::Meta) => {
                shortcut_modifier_active = true;
            }
            Some(CommonKey::Space) if !shortcut_modifier_active || text_modifier_active => {
                return true;
            }
            Some(CommonKey::Space) => {}
            Some(_) => {}
            None => {
                let upper = token.to_ascii_uppercase();
                let printable = (upper.len() == 1
                    && upper
                        .as_bytes()
                        .first()
                        .is_some_and(u8::is_ascii_alphanumeric))
                    || upper.strip_prefix("KP_").is_some_and(|value| {
                        value.len() == 1 && value.as_bytes()[0].is_ascii_digit()
                    })
                    || matches!(
                        upper.as_str(),
                        "KP_DECIMAL"
                            | "KPDECIMAL"
                            | "NUMPAD_DECIMAL"
                            | ";"
                            | "SEMICOLON"
                            | "="
                            | "EQUAL"
                            | "EQUALS"
                            | ","
                            | "COMMA"
                            | "-"
                            | "MINUS"
                            | "."
                            | "PERIOD"
                            | "DOT"
                            | "/"
                            | "SLASH"
                            | "`"
                            | "GRAVE"
                            | "BACKTICK"
                            | "["
                            | "LEFTBRACKET"
                            | "BRACKETLEFT"
                            | "\\"
                            | "BACKSLASH"
                            | "]"
                            | "RIGHTBRACKET"
                            | "BRACKETRIGHT"
                            | "'"
                            | "APOSTROPHE"
                            | "QUOTE"
                    );
                if printable && (!shortcut_modifier_active || text_modifier_active) {
                    return true;
                }
            }
        }
    }
    false
}

/// Classify chords that can escape the bound DCC or invoke system UI.
pub(crate) fn shortcut_risk(keys: &[String]) -> ShortcutRisk {
    let mut control = false;
    let mut alt = false;
    let mut escape = false;
    let mut tab = false;
    let mut space = false;
    let mut delete = false;
    let mut f4 = false;
    let mut close_or_quit = false;
    let mut save = false;

    for token in keys.iter().flat_map(|item| item.split('+')).map(str::trim) {
        if token.is_empty() {
            continue;
        }
        match common_key(token) {
            Some(CommonKey::Control(_)) => control = true,
            Some(CommonKey::Shift(_)) => {}
            Some(CommonKey::Alt(_)) => alt = true,
            Some(CommonKey::Meta | CommonKey::PrintScreen) => return ShortcutRisk::HardDeny,
            Some(CommonKey::Escape) => escape = true,
            Some(CommonKey::Tab) => tab = true,
            Some(CommonKey::Space) => space = true,
            None => match token.to_ascii_uppercase().as_str() {
                "DELETE" | "DEL" => delete = true,
                "W" | "Q" => close_or_quit = true,
                "S" => save = true,
                value if function_key_virtual_key(value) == Some(0x73) => f4 = true,
                _ => {}
            },
        }
    }

    if (control && escape) || (alt && (tab || escape || space)) || (control && alt && delete) {
        ShortcutRisk::HardDeny
    } else if delete || (alt && f4) || (control && (f4 || close_or_quit || save)) {
        ShortcutRisk::ActionConfirmation
    } else {
        ShortcutRisk::None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aliases_cannot_bypass_scope_escape_policy() {
        for chord in [
            "CONTROL+SHIFT+ESCAPE",
            "CTRL_L+SHIFT+ESC",
            "ALT+TAB",
            "LEFT_ALT+ESCAPE",
            "ALT+SPACE",
            "CONTROL+ESCAPE",
            "CTRL+ALT+DELETE",
            "WIN+R",
            "PRINT_SCREEN",
        ] {
            assert_eq!(
                shortcut_risk(&[chord.to_owned()]),
                ShortcutRisk::HardDeny,
                "{chord}"
            );
        }
    }

    #[test]
    fn closing_the_bound_window_requires_confirmation() {
        for chord in [
            "ALT+F4",
            "CTRL+W",
            "CONTROL+F04",
            "CTRL+Q",
            "DELETE",
            "DEL",
            "SHIFT+DELETE",
            "CTRL+S",
            "CONTROL+SHIFT+S",
            "CTRL_L+S",
            "RIGHTCTRL+SHIFT+S",
        ] {
            assert_eq!(
                shortcut_risk(&[chord.to_owned()]),
                ShortcutRisk::ActionConfirmation,
                "{chord}"
            );
        }
    }

    #[test]
    fn printable_keypresses_cannot_bypass_raw_type_policy() {
        for chord in [
            "A",
            "1",
            "SHIFT+A",
            "SHIFT+SEMICOLON",
            "SPACE",
            "KP_1",
            "A+CTRL",
            "A+ALT",
            "ALTGR+Q",
            "RIGHTALT+Q",
        ] {
            assert!(is_unmodified_text_entry(&[chord.to_owned()]), "{chord}");
        }
        for chord in ["LEFT", "ENTER", "F5", "CTRL+A", "ALT+A"] {
            assert!(!is_unmodified_text_entry(&[chord.to_owned()]), "{chord}");
        }
    }

    #[test]
    fn pointer_actions_accept_only_ctrl_shift_and_alt_modifiers() {
        for modifiers in ["CTRL", "SHIFT+ALT", "CTRL_L+RIGHTSHIFT+RALT"] {
            assert!(
                are_pointer_modifiers(&[modifiers.to_owned()]),
                "{modifiers}"
            );
        }
        assert!(are_pointer_modifiers(&[]));
        for keys in ["A", "SHIFT+A", "CTRL+V", "WIN", "LEFT", "", "CTRL+"] {
            assert!(!are_pointer_modifiers(&[keys.to_owned()]), "{keys}");
        }
    }
}
