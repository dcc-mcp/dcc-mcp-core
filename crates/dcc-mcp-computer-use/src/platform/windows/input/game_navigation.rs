use super::*;

pub(super) fn game_navigation(
    window_handle: u64,
    process_id: u32,
    keys: &[String],
    duration_ms: Option<u64>,
    guard: &ActionGuard<'_>,
    pre_input_fence: &mut Option<&mut PreInputFence<'_>>,
) -> ComputerUseResult<()> {
    let (press, release) = game_navigation_key_inputs(keys)?;
    let duration_ms = duration_ms.unwrap_or(0);
    if duration_ms > crate::MAX_GAME_NAVIGATION_HOLD_MS {
        return Err(ComputerUseError::new(
            ComputerUseErrorCode::InvalidAction,
            "game_navigation duration_ms exceeds the 500 ms safety limit",
        ));
    }
    guard.synchronize()?;
    let _focus_elevation = focus_target(window_handle, process_id)?;
    guard.check()?;
    run_pre_input_fence(pre_input_fence)?;
    ensure_target_foreground(window_handle, process_id)?;

    let mut held_key =
        HeldGameNavigationKey::press_with(press, release, send_inputs, defer_input_releases)?;
    let hold_result = hold_key(
        window_handle,
        process_id,
        Duration::from_millis(duration_ms),
        guard,
    );
    let foreground_result = ensure_target_foreground(window_handle, process_id);
    let release_result = held_key.release();
    hold_result?;
    foreground_result?;
    release_result?;
    guard.check()
}

fn hold_key(
    window_handle: u64,
    process_id: u32,
    duration: Duration,
    guard: &ActionGuard<'_>,
) -> ComputerUseResult<()> {
    let deadline = Instant::now() + duration;
    loop {
        guard.check()?;
        ensure_target_foreground(window_handle, process_id)?;
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Ok(());
        }
        thread::sleep(remaining.min(Duration::from_millis(10)));
    }
}

pub(super) fn game_navigation_key_inputs(keys: &[String]) -> ComputerUseResult<(INPUT, INPUT)> {
    let virtual_key = crate::game_navigation_virtual_key(keys)
        .map(VIRTUAL_KEY)
        .ok_or_else(|| {
            ComputerUseError::new(
                ComputerUseErrorCode::InvalidAction,
                "game_navigation requires exactly one unmodified W, A, S, or D key",
            )
        })?;
    Ok((
        keyboard_vk(virtual_key, false),
        keyboard_vk(virtual_key, true),
    ))
}

/// One bounded game-navigation key held between separate native input calls.
/// Drop always attempts key-up and defers it through the session input-owner
/// fence if Windows cannot accept the release immediately.
pub(super) struct HeldGameNavigationKey<Send, Defer>
where
    Send: FnMut(&[INPUT]) -> ComputerUseResult<()>,
    Defer: FnMut(&[INPUT]),
{
    release: Option<INPUT>,
    send: Send,
    defer: Defer,
}

impl<Send, Defer> HeldGameNavigationKey<Send, Defer>
where
    Send: FnMut(&[INPUT]) -> ComputerUseResult<()>,
    Defer: FnMut(&[INPUT]),
{
    pub(super) fn press_with(
        press: INPUT,
        release: INPUT,
        mut send: Send,
        defer: Defer,
    ) -> ComputerUseResult<Self> {
        send(&[press])?;
        Ok(Self {
            release: Some(release),
            send,
            defer,
        })
    }

    pub(super) fn release(&mut self) -> ComputerUseResult<()> {
        let Some(release) = self.release.take() else {
            return Ok(());
        };
        if let Err(error) = (self.send)(&[release]) {
            (self.defer)(&[release]);
            return Err(error);
        }
        Ok(())
    }
}

impl<Send, Defer> Drop for HeldGameNavigationKey<Send, Defer>
where
    Send: FnMut(&[INPUT]) -> ComputerUseResult<()>,
    Defer: FnMut(&[INPUT]),
{
    fn drop(&mut self) {
        let Some(release) = self.release.take() else {
            return;
        };
        if (self.send)(&[release]).is_err() {
            (self.defer)(&[release]);
        }
    }
}
