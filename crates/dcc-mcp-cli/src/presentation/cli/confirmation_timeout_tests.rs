use super::{UiControlAction, UiControlArgs};

fn args(timeout_secs: Option<u64>) -> UiControlArgs {
    UiControlArgs {
        dcc_type: None,
        instance_id: None,
        arguments_json: "{}".to_owned(),
        json_file: None,
        meta_json: None,
        timeout_secs,
        full_output: false,
    }
}

#[test]
fn only_ui_actions_receive_the_confirmation_budget() {
    assert_eq!(
        UiControlAction::Act(args(None)).effective_timeout_secs(None),
        130
    );
    for action in [
        UiControlAction::Snapshot(args(None)),
        UiControlAction::Find(args(None)),
        UiControlAction::Wait(args(None)),
        UiControlAction::Stop(args(None)),
        UiControlAction::RecordingStart(args(None)),
        UiControlAction::RecordingState(args(None)),
        UiControlAction::RecordingStop(args(None)),
    ] {
        assert_eq!(action.effective_timeout_secs(None), 30);
    }
}

#[test]
fn explicit_global_and_command_deadlines_keep_their_precedence() {
    let action = UiControlAction::Act(args(Some(10)));
    assert_eq!(action.effective_timeout_secs(None), 10);
    assert_eq!(action.effective_timeout_secs(Some(5)), 5);
}
