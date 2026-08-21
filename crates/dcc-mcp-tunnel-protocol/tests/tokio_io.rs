#![cfg(feature = "tokio-io")]

use dcc_mcp_tunnel_protocol::{
    Frame, RegisterRequest,
    frame::PROTOCOL_VERSION,
    tokio_io::{read_frame, write_frame},
};

#[tokio::test]
async fn round_trips_frame_over_duplex_stream() {
    let (mut writer, mut reader) = tokio::io::duplex(1024);
    let frame = Frame::Register(RegisterRequest {
        protocol_version: PROTOCOL_VERSION,
        token: "token".into(),
        dcc: "maya".into(),
        capabilities: vec![],
        agent_version: "test/0.0".into(),
        instance_id: None,
        capabilities_fingerprint: None,
        adapter_version: None,
        scene: None,
    });

    write_frame(&mut writer, &frame).await.unwrap();
    let decoded = read_frame(&mut reader).await.unwrap().expect("frame");

    assert_eq!(decoded, frame);
}

#[tokio::test]
async fn returns_none_for_clean_eof_between_frames() {
    let (writer, mut reader) = tokio::io::duplex(1024);
    drop(writer);

    assert!(read_frame(&mut reader).await.unwrap().is_none());
}
