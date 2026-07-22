use super::*;

#[test]
fn one_pipe_cannot_address_another_pipes_session() {
    let (mut host, mut owner) = negotiated();
    let opened = owner.handle(
        &mut host,
        UiControlHostRequest::OpenSession {
            session_id: "owned".to_owned(),
            grant: grant(false),
        },
    );
    let UiControlHostResponse::SessionOpened {
        window_capability, ..
    } = opened
    else {
        panic!("session not opened: {opened:?}");
    };
    let (_, mut other) = negotiated();
    let response = other.handle(
        &mut host,
        UiControlHostRequest::GetWindowState {
            session_id: "owned".to_owned(),
            task_grant_id: "grant-1".to_owned(),
            window_capability,
        },
    );
    assert!(matches!(
        response,
        UiControlHostResponse::Error {
            code: UiControlHostErrorCode::SessionNotFound,
            ..
        }
    ));
}

#[test]
fn separate_pipes_can_reuse_a_logical_session_id_without_collision() {
    let (mut host, mut first) = negotiated();
    let mut second = UiControlHostConnection::default();
    assert!(matches!(
        second.handle(
            &mut host,
            UiControlHostRequest::Hello(UiControlHostHello {
                protocol_version: UI_CONTROL_HOST_PROTOCOL_VERSION,
                client_name: "second-adapter".to_owned(),
            })
        ),
        UiControlHostResponse::Hello { .. }
    ));

    let first_opened = first.handle(
        &mut host,
        UiControlHostRequest::OpenSession {
            session_id: "default".to_owned(),
            grant: grant(false),
        },
    );
    let UiControlHostResponse::SessionOpened {
        session_id: first_session_id,
        ..
    } = first_opened
    else {
        panic!("first logical session not opened: {first_opened:?}");
    };
    let mut second_grant = grant(false);
    second_grant.task_grant_id = "grant-2".to_owned();
    second_grant.process_id = Some(84);
    second_grant.window_handle = Some(0x5678);
    let second_opened = second.handle(
        &mut host,
        UiControlHostRequest::OpenSession {
            session_id: "default".to_owned(),
            grant: second_grant,
        },
    );
    let UiControlHostResponse::SessionOpened {
        session_id: second_session_id,
        target: second_target,
        window_capability: second_capability,
    } = second_opened
    else {
        panic!("second logical session not opened: {second_opened:?}");
    };

    assert_eq!(first_session_id, "default");
    assert_eq!(second_session_id, "default");
    assert_eq!(second_target.process_id, 84);
    assert_eq!(second_target.window_handle, 0x5678);
    assert_eq!(host.sessions.len(), 2);

    first.disconnect(&mut host);
    assert_eq!(host.sessions.len(), 1);
    let second_state = second.handle(
        &mut host,
        UiControlHostRequest::GetWindowState {
            session_id: "default".to_owned(),
            task_grant_id: "grant-2".to_owned(),
            window_capability: second_capability,
        },
    );
    let UiControlHostResponse::WindowState { session_id, state } = second_state else {
        panic!("remaining session was not independently routable: {second_state:?}");
    };
    assert_eq!(session_id, "default");
    assert_eq!(state.process_id, 84);
    assert_eq!(state.window_handle, 0x5678);
}
