use super::*;

pub(super) fn send_inputs(inputs: &[INPUT]) -> ComputerUseResult<()> {
    send_inputs_with(
        inputs,
        |batch| unsafe { SendInput(batch, size_of::<INPUT>() as i32) },
        desktop_interactive,
        defer_input_releases,
    )
}

pub(crate) fn flush_pending_input_releases() -> ComputerUseResult<()> {
    let _input_guard = INPUT_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    flush_pending_input_releases_locked()
}

pub(crate) fn flush_pending_input_releases_locked() -> ComputerUseResult<()> {
    let mut pending = PENDING_INPUT_RELEASES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    flush_pending_input_releases_with(
        &mut pending,
        |batch| unsafe { SendInput(batch, size_of::<INPUT>() as i32) },
        desktop_interactive,
    )
}

pub(super) fn defer_input_releases(releases: &[INPUT]) {
    if releases.is_empty() {
        return;
    }
    PENDING_INPUT_RELEASES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .extend_from_slice(releases);
}

pub(super) fn flush_pending_input_releases_with(
    pending: &mut Vec<INPUT>,
    mut inject: impl FnMut(&[INPUT]) -> u32,
    mut is_desktop_interactive: impl FnMut() -> bool,
) -> ComputerUseResult<()> {
    if pending.is_empty() {
        return Ok(());
    }
    require_interactive_desktop(is_desktop_interactive())?;
    let original_count = pending.len();
    while !pending.is_empty() {
        if !is_desktop_interactive() {
            return Err(ComputerUseError::new(
                ComputerUseErrorCode::DesktopUnavailable,
                format!(
                    "the Windows desktop is unavailable with {} pending input-release events; no new input may run until they are confirmed",
                    pending.len()
                ),
            ));
        }
        let released = (inject(pending) as usize).min(pending.len());
        if released == 0 {
            let code = if is_desktop_interactive() {
                ComputerUseErrorCode::InputFailed
            } else {
                ComputerUseErrorCode::DesktopUnavailable
            };
            return Err(ComputerUseError::new(
                code,
                format!(
                    "Windows could not confirm {} of {original_count} pending input-release events; no new input was sent",
                    pending.len()
                ),
            ));
        }
        pending.drain(..released);
    }
    Ok(())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PressedInput {
    LeftMouse,
    RightMouse,
    MiddleMouse,
    Keyboard {
        virtual_key: u16,
        scan: u16,
        unicode: bool,
    },
}

fn update_pressed_inputs(pressed: &mut Vec<PressedInput>, item: PressedInput, released: bool) {
    if released {
        if let Some(index) = pressed.iter().rposition(|candidate| *candidate == item) {
            pressed.remove(index);
        }
    } else {
        pressed.push(item);
    }
}

pub(super) fn compensating_releases(inserted: &[INPUT]) -> Vec<INPUT> {
    let mut pressed = Vec::new();
    for input in inserted {
        if input.r#type == INPUT_KEYBOARD {
            let keyboard = unsafe { input.Anonymous.ki };
            update_pressed_inputs(
                &mut pressed,
                PressedInput::Keyboard {
                    virtual_key: keyboard.wVk.0,
                    scan: keyboard.wScan,
                    unicode: keyboard.dwFlags.contains(KEYEVENTF_UNICODE),
                },
                keyboard.dwFlags.contains(KEYEVENTF_KEYUP),
            );
        } else if input.r#type == INPUT_MOUSE {
            let flags = unsafe { input.Anonymous.mi.dwFlags };
            for (down, up, item) in [
                (
                    MOUSEEVENTF_LEFTDOWN,
                    MOUSEEVENTF_LEFTUP,
                    PressedInput::LeftMouse,
                ),
                (
                    MOUSEEVENTF_RIGHTDOWN,
                    MOUSEEVENTF_RIGHTUP,
                    PressedInput::RightMouse,
                ),
                (
                    MOUSEEVENTF_MIDDLEDOWN,
                    MOUSEEVENTF_MIDDLEUP,
                    PressedInput::MiddleMouse,
                ),
            ] {
                if flags.contains(down) {
                    update_pressed_inputs(&mut pressed, item, false);
                }
                if flags.contains(up) {
                    update_pressed_inputs(&mut pressed, item, true);
                }
            }
        }
    }
    pressed
        .into_iter()
        .rev()
        .map(|item| match item {
            PressedInput::LeftMouse => mouse_input(MOUSEEVENTF_LEFTUP, 0, 0, 0),
            PressedInput::RightMouse => mouse_input(MOUSEEVENTF_RIGHTUP, 0, 0, 0),
            PressedInput::MiddleMouse => mouse_input(MOUSEEVENTF_MIDDLEUP, 0, 0, 0),
            PressedInput::Keyboard {
                virtual_key,
                scan,
                unicode,
            } => {
                if unicode {
                    keyboard_unicode(scan, true)
                } else {
                    keyboard_vk(VIRTUAL_KEY(virtual_key), true)
                }
            }
        })
        .collect()
}

pub(super) fn send_inputs_with(
    inputs: &[INPUT],
    mut inject: impl FnMut(&[INPUT]) -> u32,
    mut is_desktop_interactive: impl FnMut() -> bool,
    mut defer_releases: impl FnMut(&[INPUT]),
) -> ComputerUseResult<()> {
    require_interactive_desktop(is_desktop_interactive())?;
    let sent = inject(inputs);
    if sent == inputs.len() as u32 {
        return Ok(());
    }

    let inserted_count = (sent as usize).min(inputs.len());
    let releases = compensating_releases(&inputs[..inserted_count]);
    if !is_desktop_interactive() {
        defer_releases(&releases);
        return Err(ComputerUseError::new(
            ComputerUseErrorCode::DesktopUnavailable,
            format!(
                "the Windows desktop became unavailable after SendInput inserted {inserted_count} of {} events; {} release events are pending and block new input until confirmed",
                inputs.len(),
                releases.len()
            ),
        ));
    }

    let mut released_count = 0_usize;
    while released_count < releases.len() {
        if !is_desktop_interactive() {
            defer_releases(&releases[released_count..]);
            return Err(ComputerUseError::new(
                ComputerUseErrorCode::DesktopUnavailable,
                format!(
                    "the Windows desktop became unavailable after SendInput inserted {inserted_count} of {} events; only {released_count} of {} compensating releases were confirmed and the remainder block new input",
                    inputs.len(),
                    releases.len()
                ),
            ));
        }
        let released =
            (inject(&releases[released_count..]) as usize).min(releases.len() - released_count);
        if released == 0 {
            break;
        }
        released_count += released;
    }
    let cleanup_confirmed = released_count == releases.len();
    if !cleanup_confirmed {
        defer_releases(&releases[released_count..]);
    }
    if !is_desktop_interactive() {
        return Err(ComputerUseError::new(
            ComputerUseErrorCode::DesktopUnavailable,
            format!(
                "the Windows desktop became unavailable after SendInput inserted {inserted_count} of {} events; unconfirmed compensating releases block new input",
                inputs.len()
            ),
        ));
    }

    let cleanup = if releases.is_empty() {
        "no pressed input required compensation"
    } else if cleanup_confirmed {
        "compensating release events were sent"
    } else {
        "compensating release events could not be confirmed"
    };
    Err(ComputerUseError::new(
        ComputerUseErrorCode::InputFailed,
        format!(
            "SendInput inserted {inserted_count} of {} events; {cleanup}. Windows does not identify whether a short write was caused by UIPI, a desktop transition, or another input policy; take a fresh screenshot before retrying",
            inputs.len()
        ),
    ))
}
