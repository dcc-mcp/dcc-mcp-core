//! UI Control command routing and timeout precedence.

use super::{DEFAULT_CALL_TIMEOUT_SECS, UiControlAction, UiControlArgs};

impl UiControlAction {
    pub(super) fn effective_timeout_secs(&self, global: Option<u64>) -> u64 {
        global.or(self.timeout_secs()).unwrap_or({
            if matches!(self, Self::Act(_)) {
                // Match the tool budget around the 120s native confirmation wait.
                130
            } else {
                DEFAULT_CALL_TIMEOUT_SECS
            }
        })
    }

    pub(super) fn timeout_secs(&self) -> Option<u64> {
        match self {
            Self::Snapshot(args)
            | Self::Find(args)
            | Self::Act(args)
            | Self::RecordingStart(args)
            | Self::RecordingState(args)
            | Self::RecordingStop(args)
            | Self::Wait(args)
            | Self::Stop(args) => args.timeout_secs,
        }
    }

    pub(super) fn into_call(self) -> (&'static str, UiControlArgs) {
        match self {
            Self::Snapshot(args) => ("ui_control__snapshot", args),
            Self::Find(args) => ("ui_control__find", args),
            Self::Act(args) => ("ui_control__act", args),
            Self::RecordingStart(args) => ("ui_control__recording_start", args),
            Self::RecordingState(args) => ("ui_control__recording_state", args),
            Self::RecordingStop(args) => ("ui_control__recording_stop", args),
            Self::Wait(args) => ("ui_control__wait_for", args),
            Self::Stop(args) => ("ui_control__stop_computer_use", args),
        }
    }
}
