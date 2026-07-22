use dcc_mcp_ui_control::host_protocol::{
    UI_CONTROL_HOST_CAPABILITIES, UiControlClipArtifact, UiControlClipFormat, UiControlHostRequest,
    UiControlHostResponse, UiControlTarget,
};

#[test]
fn exact_window_recording_is_an_explicit_host_capability() {
    assert!(UI_CONTROL_HOST_CAPABILITIES.contains(&"exact_window_recording"));
}

#[test]
fn record_clip_request_is_bounded_and_cannot_choose_a_host_path() {
    let request = UiControlHostRequest::RecordClip {
        session_id: "pv-session".to_owned(),
        task_grant_id: "grant-1".to_owned(),
        window_capability: "window-1".to_owned(),
        duration_ms: 12_000,
        frames_per_second: 30,
        format: UiControlClipFormat::JpegSequence,
        jpeg_quality: 92,
    };

    let value = serde_json::to_value(request).unwrap();
    assert_eq!(value["method"], "record_clip");
    assert_eq!(value["params"]["duration_ms"], 12_000);
    assert_eq!(value["params"]["frames_per_second"], 30);
    assert_eq!(value["params"]["format"], "jpeg_sequence");
    assert!(value["params"].get("output_path").is_none());

    let mut injected = value;
    injected["params"]["output_path"] = serde_json::json!("C:/outside-scope.mp4");
    assert!(serde_json::from_value::<UiControlHostRequest>(injected).is_err());
}

#[test]
fn recorded_clip_response_carries_exact_target_and_hash_bearing_artifact() {
    let response = UiControlHostResponse::ClipRecorded {
        target: UiControlTarget {
            process_id: 4242,
            window_handle: 0xCAFE,
            window_title: "Packaged Game".to_owned(),
        },
        artifact: UiControlClipArtifact {
            recording_id: "clip-1".to_owned(),
            directory: "host-owned-recording/clip-1".to_owned(),
            manifest_path: "host-owned-recording/clip-1/manifest.json".to_owned(),
            frame_pattern: "frame-%06d.jpg".to_owned(),
            frame_count: 360,
            width: 1280,
            height: 720,
            frames_per_second: 30,
            started_at_ms: 1_000,
            ended_at_ms: 13_000,
            manifest_sha256: "a".repeat(64),
        },
    };

    let value = serde_json::to_value(response).unwrap();
    assert_eq!(value["type"], "clip_recorded");
    assert_eq!(value["target"]["process_id"], 4242);
    assert_eq!(value["artifact"]["frame_count"], 360);
    assert_eq!(
        value["artifact"]["manifest_sha256"].as_str().unwrap().len(),
        64
    );
}
