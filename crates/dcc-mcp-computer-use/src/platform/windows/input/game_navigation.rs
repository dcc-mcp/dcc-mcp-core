use super::*;

pub(super) fn game_navigation(
    window_handle: u64,
    process_id: u32,
    keys: &[String],
    duration_ms: Option<u64>,
    guard: &ActionGuard<'_>,
    pre_input_fence: &mut Option<&mut PreInputFence<'_>>,
) -> ComputerUseResult<()> {
    let (presses, releases) = game_navigation_key_inputs(keys)?;
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

    let mut held_keys =
        HeldGameNavigationKeys::press_with(presses, releases, send_inputs, defer_input_releases)?;
    let hold_result = hold_key(
        window_handle,
        process_id,
        Duration::from_millis(duration_ms),
        guard,
    );
    let foreground_result = ensure_target_foreground(window_handle, process_id);
    let release_result = held_keys.release();
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

pub(super) fn game_navigation_key_inputs(
    keys: &[String],
) -> ComputerUseResult<(Vec<INPUT>, Vec<INPUT>)> {
    if crate::game_navigation_virtual_keys(keys).is_none() {
        return Err(ComputerUseError::new(
            ComputerUseErrorCode::InvalidAction,
            "game_navigation requires one to four distinct supported canvas keys",
        ));
    }
    held_key_inputs(keys)
}

/// Bounded canvas keys held between separate native input calls. Drop always
/// attempts every key-up and defers them through the session input-owner fence
/// if Windows cannot accept the release immediately.
pub(super) struct HeldGameNavigationKeys<Send, Defer>
where
    Send: FnMut(&[INPUT]) -> ComputerUseResult<()>,
    Defer: FnMut(&[INPUT]),
{
    releases: Vec<INPUT>,
    send: Send,
    defer: Defer,
}

impl<Send, Defer> HeldGameNavigationKeys<Send, Defer>
where
    Send: FnMut(&[INPUT]) -> ComputerUseResult<()>,
    Defer: FnMut(&[INPUT]),
{
    pub(super) fn press_with(
        presses: Vec<INPUT>,
        releases: Vec<INPUT>,
        mut send: Send,
        defer: Defer,
    ) -> ComputerUseResult<Self> {
        send(&presses)?;
        Ok(Self {
            releases,
            send,
            defer,
        })
    }

    pub(super) fn release(&mut self) -> ComputerUseResult<()> {
        let releases = std::mem::take(&mut self.releases);
        if releases.is_empty() {
            return Ok(());
        }
        if let Err(error) = (self.send)(&releases) {
            (self.defer)(&releases);
            return Err(error);
        }
        Ok(())
    }
}

impl<Send, Defer> Drop for HeldGameNavigationKeys<Send, Defer>
where
    Send: FnMut(&[INPUT]) -> ComputerUseResult<()>,
    Defer: FnMut(&[INPUT]),
{
    fn drop(&mut self) {
        let releases = std::mem::take(&mut self.releases);
        if releases.is_empty() {
            return;
        }
        if (self.send)(&releases).is_err() {
            (self.defer)(&releases);
        }
    }
}
